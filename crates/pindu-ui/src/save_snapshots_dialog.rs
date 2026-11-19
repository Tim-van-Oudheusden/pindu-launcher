//! Save Snapshots: a Windows game's save snapshots (`pindu_core::saves`), newest first,
//! each with its time, size and why it was taken. One leads to its page with the exact
//! paths it holds and the rules that left the rest out, and Restore puts it back after a
//! confirmation (`save_service::restore`). While the game runs a banner says to quit it
//! and Restore is off. The desktop game page's menu and fullscreen mode's options sheet
//! open it; a controller moves through it as through any dialog, A activates and B goes
//! back or closes.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::saves::{Reason, Snapshot};

use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::save_service;
use crate::session_service::SessionService;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/save_snapshots_dialog.ui"
    )]
    pub struct SaveSnapshotsDialog {
        #[template_child]
        pub toasts: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub navigation: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub running_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub empty_status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub snapshot_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub snapshot_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub summary_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub restore_row: TemplateChild<adw::ButtonRow>,
        #[template_child]
        pub included_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub excluded_list: TemplateChild<gtk::ListBox>,
        pub game: RefCell<Option<GameObject>>,
        pub library: RefCell<Option<LibraryService>>,
        pub sessions: RefCell<Option<SessionService>>,
        /// The snapshots listed, in the list's order.
        pub snapshots: RefCell<Vec<Snapshot>>,
        /// The snapshot whose page shows.
        pub shown: RefCell<Option<Snapshot>>,
        pub restoring: Cell<bool>,
        pub playing_handler: RefCell<Option<glib::SignalHandlerId>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SaveSnapshotsDialog {
        const NAME: &'static str = "PinduSaveSnapshotsDialog";
        type Type = super::SaveSnapshotsDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for SaveSnapshotsDialog {
        fn dispose(&self) {
            if let (Some(game), Some(handler)) =
                (self.game.borrow().as_ref(), self.playing_handler.take())
            {
                game.disconnect(handler);
            }
        }
    }

    impl WidgetImpl for SaveSnapshotsDialog {}
    impl AdwDialogImpl for SaveSnapshotsDialog {}

    #[gtk::template_callbacks]
    impl SaveSnapshotsDialog {
        #[template_callback]
        fn snapshot_activated(&self, row: &gtk::ListBoxRow) {
            let snapshot = usize::try_from(row.index())
                .ok()
                .and_then(|index| self.snapshots.borrow().get(index).cloned());
            if let Some(snapshot) = snapshot {
                self.obj().show_snapshot(snapshot);
            }
        }

        #[template_callback]
        fn restore_activated(&self) {
            self.obj().confirm_restore();
        }
    }
}

glib::wrapper! {
    /// A Windows game's save snapshots, to see what each holds and restore one.
    pub struct SaveSnapshotsDialog(ObjectSubclass<imp::SaveSnapshotsDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl SaveSnapshotsDialog {
    /// Opens the dialog for `game` over `parent` and lists its snapshots.
    pub fn show(
        parent: &impl IsA<gtk::Widget>,
        game: &GameObject,
        library: LibraryService,
        sessions: SessionService,
    ) {
        let dialog: Self = glib::Object::new();
        let imp = dialog.imp();
        imp.game.replace(Some(game.clone()));
        imp.library.replace(Some(library));
        imp.sessions.replace(Some(sessions));
        let handler = game.connect_notify_local(
            Some("playing"),
            glib::clone!(
                #[weak]
                dialog,
                move |_, _| dialog.show_playing()
            ),
        );
        imp.playing_handler.replace(Some(handler));
        // Back on the list, focus returns to the snapshot that was open.
        imp.navigation.connect_popped(glib::clone!(
            #[weak]
            dialog,
            move |_, _| dialog.focus_shown_row()
        ));
        dialog.show_playing();
        dialog.present(Some(parent));
        dialog.load();
    }

    /// Goes back from a snapshot's page to the list; `false` on the list, where Back
    /// closes the dialog.
    pub(crate) fn go_back(&self) -> bool {
        self.imp().navigation.pop()
    }

    /// Focuses the row of the snapshot whose page showed last.
    fn focus_shown_row(&self) {
        let imp = self.imp();
        let index = imp.shown.borrow().as_ref().and_then(|shown| {
            imp.snapshots
                .borrow()
                .iter()
                .position(|snapshot| snapshot.dir == shown.dir)
        });
        if let Some(row) = index.and_then(|index| imp.snapshot_list.row_at_index(index as i32)) {
            row.grab_focus();
        }
    }

    /// Reads the game's snapshots and lists them.
    fn load(&self) {
        let Some(id) = self.imp().game.borrow().as_ref().map(GameObject::id) else {
            return;
        };
        self.imp().stack.set_visible_child_name("loading");
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            async move {
                match save_service::snapshots(&id).await {
                    Ok(snapshots) => dialog.fill(snapshots),
                    Err(error) => {
                        glib::g_warning!("pindu", "Could not list the snapshots of {id}: {error}");
                        let imp = dialog.imp();
                        imp.empty_status
                            .set_title(&gettext("Could Not Read the Snapshots"));
                        imp.empty_status.set_description(None);
                        imp.stack.set_visible_child_name("empty");
                    }
                }
            }
        ));
    }

    /// Lists `snapshots`, newest first: the time each was taken, its size and why.
    fn fill(&self, snapshots: Vec<Snapshot>) {
        let imp = self.imp();
        imp.snapshot_list.remove_all();
        for snapshot in &snapshots {
            let row = adw::ActionRow::builder()
                .title(taken_at(snapshot))
                .subtitle(summary(snapshot))
                .activatable(true)
                .build();
            row.add_suffix(
                &gtk::Image::builder()
                    .icon_name("go-next-symbolic")
                    .accessible_role(gtk::AccessibleRole::Presentation)
                    .build(),
            );
            imp.snapshot_list.append(&row);
        }
        let empty = snapshots.is_empty();
        imp.snapshots.replace(snapshots);
        imp.stack
            .set_visible_child_name(if empty { "empty" } else { "list" });
        if let Some(first) = imp.snapshot_list.row_at_index(0) {
            first.grab_focus();
        }
    }

    /// Shows `snapshot`'s page: its time and size, Restore, the paths it holds and the
    /// rules that left the rest out.
    fn show_snapshot(&self, snapshot: Snapshot) {
        let imp = self.imp();
        imp.snapshot_page.set_title(&taken_at(&snapshot));
        imp.summary_group.set_description(Some(&summary(&snapshot)));
        imp.included_list.remove_all();
        for file in &snapshot.manifest.files {
            let row = adw::ActionRow::builder()
                .title(&file.path)
                .subtitle(glib::format_size(file.size))
                .use_markup(false)
                .build();
            row.add_css_class("property");
            imp.included_list.append(&row);
        }
        imp.excluded_list.remove_all();
        for rule in &snapshot.manifest.exclude {
            let row = adw::ActionRow::builder()
                .title(rule)
                .use_markup(false)
                .build();
            imp.excluded_list.append(&row);
        }
        imp.shown.replace(Some(snapshot));
        self.show_playing();
        imp.navigation.push(&*imp.snapshot_page);
        imp.restore_row.grab_focus();
    }

    /// Says whether the game runs, which turns Restore off.
    fn show_playing(&self) {
        let imp = self.imp();
        let playing = imp
            .game
            .borrow()
            .as_ref()
            .is_some_and(GameObject::is_playing);
        imp.running_banner.set_revealed(playing);
        imp.restore_row
            .set_sensitive(!playing && !imp.restoring.get());
    }

    /// Asks before restoring the snapshot shown.
    fn confirm_restore(&self) {
        let imp = self.imp();
        let (Some(game), Some(snapshot)) = (imp.game.borrow().clone(), imp.shown.borrow().clone())
        else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Restore These Saves?"))
            .body(
                // Translators: {title} is a game's title, {time} when the snapshot was taken.
                gettext(
                    "The saves of “{title}” go back to how they were on {time}. The saves they replace are kept in a new snapshot first.",
                )
                .replace("{title}", &game.game().title)
                .replace("{time}", &taken_at(&snapshot)),
            )
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("restore", &gettext("_Restore")),
        ]);
        dialog.set_response_appearance("restore", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            Some("restore"),
            glib::clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| this.restore(snapshot.clone())
            ),
        );
        dialog.present(Some(self));
    }

    /// Restores `snapshot`, then lists the snapshots again, the safety snapshot among
    /// them, and says how it went.
    fn restore(&self, snapshot: Snapshot) {
        let imp = self.imp();
        let (Some(game), Some(library), Some(sessions)) = (
            imp.game.borrow().clone(),
            imp.library.borrow().clone(),
            imp.sessions.borrow().clone(),
        ) else {
            return;
        };
        imp.restoring.set(true);
        self.show_playing();
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = dialog)]
            self,
            async move {
                let game = game.game().clone();
                let result = save_service::restore(&game, snapshot, &library, &sessions).await;
                let imp = dialog.imp();
                imp.restoring.set(false);
                let message = match result {
                    Ok(()) => {
                        imp.navigation.pop();
                        dialog.load();
                        // Translators: {title} is a game's title.
                        gettext("Restored the saves of “{title}”").replace("{title}", &game.title)
                    }
                    Err(message) => {
                        dialog.show_playing();
                        message
                    }
                };
                imp.toasts.add_toast(adw::Toast::new(&message));
            }
        ));
    }
}

/// When `snapshot` was taken, in the user's time zone and format.
fn taken_at(snapshot: &Snapshot) -> String {
    glib::DateTime::from_unix_local(snapshot.manifest.taken_at)
        .and_then(|time| time.format("%x %X"))
        .map_or_else(|_| snapshot.name.clone(), String::from)
}

/// `snapshot`'s size, how many files it holds and why it was taken.
fn summary(snapshot: &Snapshot) -> String {
    let files = snapshot.manifest.files.len();
    let count = gettextrs::ngettext(
        "{count} file",
        "{count} files",
        u32::try_from(files).unwrap_or(u32::MAX),
    )
    .replace("{count}", &files.to_string());
    let reason = match snapshot.manifest.reason {
        Reason::Exit => gettext("Taken when the game exited"),
        Reason::BeforeRestore => gettext("Taken before a restore"),
    };
    format!(
        "{} · {count} · {reason}",
        glib::format_size(snapshot.manifest.size())
    )
}
