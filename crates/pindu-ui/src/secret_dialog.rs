//! The passphrase and PIN dialog, for a Wi-Fi passphrase or a Bluetooth PIN typed with a
//! controller: a title, a line saying what to type, an entry that shows its text as dots
//! unless the caller's keyboard shows it, and the on-screen keyboard with its lowercase,
//! capitals and symbols layers and its Show key. The entry has the focus, so a keyboard
//! types too. While the dialog is open the on-screen keyboard takes every button. The
//! grid and what each button does are `pindu_core::osk`; this draws the keys, marks the
//! cursor's, and hands the text to the caller on Done or Start, or nothing on Cancel or on
//! B with the entry empty. It opens on the window Quick Access is in, the main window or
//! Quick Access over a game (`game_overlay`), whose hint bar shows its buttons. The main
//! window also asks for a name with it, such as a collection's ([`PinduWindow::ask_text`]).

use std::cell::{Cell, RefCell};
use std::fmt;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use pindu_core::gamepad::Intent;
use pindu_core::hints::Typing;
use pindu_core::osk::{Keyboard, Outcome};

use crate::fullscreen::Sound;
use crate::game_overlay::PinduGameOverlay;
use crate::search_keyboard::{CURSOR_CLASS, key_label, key_name};
use crate::window::PinduWindow;

/// What the caller does with the text, or with `None` when the dialog is cancelled.
type Done = Box<dyn FnOnce(Option<String>)>;

/// What the dialog hands its text to, once.
#[derive(Default)]
pub struct Answer(RefCell<Option<Done>>);

impl Answer {
    /// Hands `text` to the caller, unless it already has its answer.
    fn give(&self, text: Option<String>) {
        if let Some(done) = self.0.take() {
            done(text);
        }
    }
}

impl fmt::Debug for Answer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Answer")
            .field("given", &self.0.borrow().is_none())
            .finish()
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/secret_dialog.ui")]
    pub struct SecretDialog {
        #[template_child]
        pub message: TemplateChild<gtk::Label>,
        #[template_child]
        pub entry: TemplateChild<gtk::Entry>,
        #[template_child]
        pub keyboard_grid: TemplateChild<gtk::Grid>,
        /// The keyboard's cursor, layer and whether it hides the text.
        pub keyboard: Cell<Keyboard>,
        /// The shown layer's keys, row by row.
        pub keys: RefCell<Vec<Vec<gtk::Label>>>,
        pub answer: Answer,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SecretDialog {
        const NAME: &'static str = "PinduSecretDialog";
        type Type = super::SecretDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl SecretDialog {
        #[template_callback]
        fn confirm(&self) {
            self.obj().confirm();
        }
    }

    impl ObjectImpl for SecretDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let dialog = self.obj();
            // Cancel, Esc and B on an empty entry all close the dialog: no text then.
            dialog.connect_closed(|dialog| dialog.imp().answer.give(None));
        }
    }
    impl WidgetImpl for SecretDialog {}
    impl AdwDialogImpl for SecretDialog {}
}

glib::wrapper! {
    /// The passphrase and PIN dialog.
    pub struct SecretDialog(ObjectSubclass<imp::SecretDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl SecretDialog {
    /// A dialog titled `title`, saying `message` above its entry, with `keyboard` (the
    /// passphrase keyboard or the code keyboard of `pindu_core::osk`), that hands the
    /// typed text to `done`, or `None` once it is cancelled.
    pub fn new(
        title: &str,
        message: &str,
        keyboard: Keyboard,
        done: impl FnOnce(Option<String>) + 'static,
    ) -> Self {
        let dialog: Self = glib::Object::builder().property("title", title).build();
        let imp = dialog.imp();
        imp.answer.0.replace(Some(Box::new(done)));
        imp.message.set_label(message);
        imp.entry
            .update_property(&[gtk::accessible::Property::Description(message)]);
        imp.keyboard.set(keyboard);
        dialog.show_keys();
        dialog
    }

    /// Closes the dialog without handing the caller anything, as what it asked for went
    /// away (BlueZ cancelled the request, or a new one replaced it).
    pub fn withdraw(&self) {
        self.imp().answer.0.take();
        self.force_close();
    }

    /// Starts the entry with `text`, a name to edit, shown in full: no password field.
    fn start_with(&self, text: &str) {
        let entry = &self.imp().entry;
        entry.set_input_purpose(gtk::InputPurpose::FreeForm);
        entry.set_text(text);
        entry.set_position(-1);
    }

    /// Acts on a controller intent, and says what the keyboard did.
    pub fn handle(&self, intent: Intent) -> Outcome {
        let imp = self.imp();
        let mut keyboard = imp.keyboard.get();
        let outcome = keyboard.handle(intent, &imp.entry.text());
        imp.keyboard.set(keyboard);
        match &outcome {
            Outcome::Moved => self.show_cursor(),
            Outcome::Layer | Outcome::Reveal => self.show_keys(),
            Outcome::Edit(text) => {
                imp.entry.set_text(text);
                imp.entry.set_position(-1);
            }
            Outcome::Close => {
                self.close();
            }
            Outcome::Confirm => self.confirm(),
            Outcome::Ignored => {}
        }
        outcome
    }

    /// The keyboard's cursor key and whether the entry is empty, for the hints.
    pub fn typing(&self) -> Typing {
        let imp = self.imp();
        Typing {
            key: imp.keyboard.get().key(),
            empty: imp.entry.text().is_empty(),
        }
    }

    /// Hands the entry's text to the caller and closes.
    fn confirm(&self) {
        let imp = self.imp();
        imp.answer.give(Some(imp.entry.text().into()));
        self.close();
    }

    /// Lays out the shown layer's keys, and shows or hides the entry's text.
    fn show_keys(&self) {
        let imp = self.imp();
        let keyboard = imp.keyboard.get();
        while let Some(child) = imp.keyboard_grid.first_child() {
            imp.keyboard_grid.remove(&child);
        }
        let mut keys = Vec::new();
        for (row, cells) in keyboard.layer().rows().enumerate() {
            let mut labels = Vec::new();
            for cell in cells {
                let label = gtk::Label::builder()
                    .label(key_label(cell.key, keyboard.hidden()))
                    .css_classes(["osk-key"])
                    .build();
                imp.keyboard_grid.attach(
                    &label,
                    cell.column as i32,
                    row as i32,
                    cell.width as i32,
                    1,
                );
                labels.push(label);
            }
            keys.push(labels);
        }
        imp.keys.replace(keys);
        imp.entry.set_visibility(!keyboard.hidden());
        self.show_cursor();
    }

    /// Marks the key under the cursor, and tells screen readers which key it is.
    fn show_cursor(&self) {
        let imp = self.imp();
        let keyboard = imp.keyboard.get();
        let cursor = keyboard.position();
        for (row, labels) in imp.keys.borrow().iter().enumerate() {
            for (index, label) in labels.iter().enumerate() {
                if (row, index) == cursor {
                    label.add_css_class(CURSOR_CLASS);
                } else {
                    label.remove_css_class(CURSOR_CLASS);
                }
            }
        }
        self.announce(
            &key_name(keyboard.key(), keyboard.hidden()),
            gtk::AccessibleAnnouncementPriority::Medium,
        );
    }

    /// Acts on a controller intent while the dialog is open on its host, which plays the
    /// returned sound and shows the hints again.
    pub(crate) fn controller_intent(&self, intent: Intent) -> Option<Sound> {
        match self.handle(intent) {
            Outcome::Moved | Outcome::Layer => Some(Sound::Move),
            Outcome::Edit(_) | Outcome::Reveal | Outcome::Confirm => Some(Sound::Select),
            Outcome::Close => Some(Sound::Back),
            Outcome::Ignored => None,
        }
    }

    /// Presents the dialog on `host`, calling `typed` whenever its entry changes: B's hint
    /// turns from Delete into Close as the entry empties, however it does.
    fn present_on(&self, host: &impl IsA<gtk::Widget>, typed: impl Fn() + 'static) {
        self.imp().entry.connect_changed(move |_| typed());
        self.present(Some(host));
    }
}

/// Asks for a passphrase or PIN on the window `widget` is in, the main window or Quick
/// Access over a game, as [`PinduWindow::ask_secret`] does; `None` when `widget` is in
/// neither.
pub(crate) fn ask_secret(
    widget: &impl IsA<gtk::Widget>,
    title: &str,
    message: &str,
    keyboard: Keyboard,
    done: impl FnOnce(Option<String>) + 'static,
) -> Option<SecretDialog> {
    let root = widget.root()?;
    if let Some(window) = root.downcast_ref::<PinduWindow>() {
        return Some(window.ask_secret(title, message, keyboard, done));
    }
    let overlay = root.downcast_ref::<PinduGameOverlay>()?;
    Some(overlay.ask_secret(title, message, keyboard, done))
}

impl PinduWindow {
    /// Asks for a passphrase or PIN in a dialog titled `title` that says `message`, typed
    /// with `keyboard` on screen or with a keyboard; `done` gets the text on Done or Start,
    /// or `None` once the dialog is cancelled. The dialog is returned to
    /// [`withdraw`](SecretDialog::withdraw) it once the question goes away.
    pub fn ask_secret(
        &self,
        title: &str,
        message: &str,
        keyboard: Keyboard,
        done: impl FnOnce(Option<String>) + 'static,
    ) -> SecretDialog {
        let dialog = SecretDialog::new(title, message, keyboard, done);
        dialog.present_on(
            self,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.update_hints()
            ),
        );
        dialog
    }

    /// Asks for a name, such as a collection's, in the same dialog: titled `title`, saying
    /// `message`, starting with `text` and typed with the passphrase keyboard showing its
    /// text ([`Keyboard::text`]); `done` gets the text on Done or Start, or `None` once the
    /// dialog is cancelled.
    pub fn ask_text(
        &self,
        title: &str,
        message: &str,
        text: &str,
        done: impl FnOnce(Option<String>) + 'static,
    ) {
        let dialog = SecretDialog::new(title, message, Keyboard::text(), done);
        dialog.start_with(text);
        dialog.present_on(
            self,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.update_hints()
            ),
        );
    }

    /// The open passphrase dialog, if any.
    fn secret_dialog(&self) -> Option<SecretDialog> {
        self.visible_dialog().and_downcast::<SecretDialog>()
    }

    /// Hands `intent` to the open passphrase dialog, which takes every button then;
    /// `false` when none is open.
    pub(crate) fn secret_dialog_intent(&self, intent: Intent) -> bool {
        let Some(dialog) = self.secret_dialog() else {
            return false;
        };
        if let Some(sound) = dialog.controller_intent(intent) {
            self.play_sound(sound);
        }
        self.update_hints();
        true
    }

    /// The open passphrase dialog's cursor key and whether its entry is empty, for the
    /// hints.
    pub(crate) fn secret_dialog_typing(&self) -> Option<Typing> {
        self.secret_dialog().map(|dialog| dialog.typing())
    }
}

impl PinduGameOverlay {
    /// Asks for a passphrase or PIN over the game, as [`PinduWindow::ask_secret`] does in
    /// the main window: the dialog's keyboard takes the controller and the overlay's hint
    /// bar shows its buttons.
    pub fn ask_secret(
        &self,
        title: &str,
        message: &str,
        keyboard: Keyboard,
        done: impl FnOnce(Option<String>) + 'static,
    ) -> SecretDialog {
        let dialog = SecretDialog::new(title, message, keyboard, done);
        dialog.present_on(
            self,
            glib::clone!(
                #[weak(rename_to = overlay)]
                self,
                move || overlay.update_hints()
            ),
        );
        dialog
    }

    /// The open passphrase dialog, if any.
    fn secret_dialog(&self) -> Option<SecretDialog> {
        self.visible_dialog().and_downcast::<SecretDialog>()
    }

    /// Hands `intent` to the open passphrase dialog, which takes every button then;
    /// `false` when none is open.
    pub(crate) fn secret_dialog_intent(&self, intent: Intent) -> bool {
        let Some(dialog) = self.secret_dialog() else {
            return false;
        };
        if let Some(sound) = dialog.controller_intent(intent) {
            self.play_sound(sound);
        }
        self.update_hints();
        true
    }

    /// The open passphrase dialog's cursor key and whether its entry is empty, for the
    /// hints.
    pub(crate) fn secret_dialog_typing(&self) -> Option<Typing> {
        self.secret_dialog().map(|dialog| dialog.typing())
    }
}
