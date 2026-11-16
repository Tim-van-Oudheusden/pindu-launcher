//! The passphrase and PIN dialog, for a Wi-Fi passphrase or a Bluetooth PIN typed with a
//! controller: a title, an entry that shows its text as dots, and the on-screen keyboard
//! with its lowercase, capitals and symbols layers and its Show key. While the dialog is
//! open the keyboard takes every button. The grid and what each button does are
//! `pindu_core::osk`; this draws the keys, marks the cursor's, and hands the text to the
//! caller on Done or Start, or nothing on Cancel or on B with the entry empty.

use std::cell::{Cell, RefCell};
use std::fmt;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::glib;
use pindu_core::gamepad::Intent;
use pindu_core::hints::Typing;
use pindu_core::osk::{Keyboard, Outcome};

use crate::fullscreen::Sound;
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
            self.keyboard.set(Keyboard::passphrase());
            // Cancel, Esc and B on an empty entry all close the dialog: no text then.
            dialog.connect_closed(|dialog| dialog.imp().answer.give(None));
            dialog.show_keys();
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
    /// A dialog titled `title` that hands the typed text to `done`, or `None` once it is
    /// cancelled.
    pub fn new(title: &str, done: impl FnOnce(Option<String>) + 'static) -> Self {
        let dialog: Self = glib::Object::builder().property("title", title).build();
        dialog.imp().answer.0.replace(Some(Box::new(done)));
        dialog
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
}

impl PinduWindow {
    /// Asks for a passphrase or PIN in a dialog titled `title`, typed with the on-screen
    /// keyboard or a keyboard; `done` gets the text on Done or Start, or `None` once the
    /// dialog is cancelled.
    pub fn ask_secret(&self, title: &str, done: impl FnOnce(Option<String>) + 'static) {
        let dialog = SecretDialog::new(title, done);
        // B's hint turns from Delete into Close as the entry empties, however it does.
        dialog.imp().entry.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_hints()
        ));
        dialog.present(Some(self));
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
        let sound = match dialog.handle(intent) {
            Outcome::Moved | Outcome::Layer => Some(Sound::Move),
            Outcome::Edit(_) | Outcome::Reveal | Outcome::Confirm => Some(Sound::Select),
            Outcome::Close => Some(Sound::Back),
            Outcome::Ignored => None,
        };
        if let Some(sound) = sound {
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
