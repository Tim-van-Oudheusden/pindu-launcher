use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::game_dialog::GameDialog;
use crate::launch_service;
use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::permission_service;
use crate::preferences::PreferencesDialog;
use crate::session_service::SessionService;
use crate::sync_service::SyncService;
use crate::watch_service::WatchService;
use crate::window::PinduWindow;

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default)]
    pub struct PinduApplication {
        /// Shared by every shell, so there is one import at a time.
        pub sync_service: SyncService,
        /// The library every shell shows; created at start-up, once GTK is initialised.
        pub library_service: OnceCell<LibraryService>,
        /// The games we started that are running.
        pub session_service: OnceCell<SessionService>,
        /// Resyncs when a launcher changes its files.
        pub watch_service: OnceCell<WatchService>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduApplication {
        const NAME: &'static str = "PinduApplication";
        type Type = super::PinduApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for PinduApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_actions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
            obj.set_accels_for_action("win.show-sidebar", &["F9"]);
            obj.set_accels_for_action("win.search", &["<primary>f"]);
            obj.set_accels_for_action("win.back", &["<alt>Left", "Back"]);
            obj.set_accels_for_action("app.preferences", &["<primary>comma"]);
            obj.set_accels_for_action("app.add-game", &["<primary>n"]);
            glib::g_info!(
                "pindu",
                "Host spawn {}",
                if permission_service::host_spawn_allowed() {
                    "allowed"
                } else {
                    "not allowed"
                }
            );
        }
    }

    impl ApplicationImpl for PinduApplication {
        fn startup(&self) {
            self.parent_startup();
            let library = LibraryService::new();
            self.sync_service.connect_finished(glib::clone!(
                #[weak]
                library,
                move |_| library.reload()
            ));
            self.session_service
                .set(SessionService::new(&library))
                .expect("start-up runs once");
            self.watch_service
                .set(WatchService::new(&self.sync_service))
                .expect("start-up runs once");
            self.library_service
                .set(library)
                .expect("start-up runs once");
        }

        fn activate(&self) {
            let application = self.obj();
            let window = application.active_window().unwrap_or_else(|| {
                let window = PinduWindow::new(
                    application.upcast_ref::<adw::Application>(),
                    &self.sync_service,
                    &application.library_service().model(),
                );
                // Show the stored library at once, then sync every source.
                application.library_service().reload();
                self.sync_service.sync();
                window.upcast()
            });
            window.present();
        }
    }

    impl GtkApplicationImpl for PinduApplication {}
    impl AdwApplicationImpl for PinduApplication {}
}

glib::wrapper! {
    pub struct PinduApplication(ObjectSubclass<imp::PinduApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl PinduApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("resource-base-path", RESOURCE_BASE_PATH)
            .build()
    }

    /// The library every shell shows.
    pub fn library_service(&self) -> &LibraryService {
        self.imp()
            .library_service
            .get()
            .expect("the application has started")
    }

    /// The games we started that are running.
    pub fn session_service(&self) -> &SessionService {
        self.imp()
            .session_service
            .get()
            .expect("the application has started")
    }

    fn setup_actions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| {
                PreferencesDialog::new().present(app.active_window().as_ref());
            })
            .build();
        let add_game_action = gio::ActionEntry::builder("add-game")
            .activate(move |app: &Self, _, _| {
                GameDialog::add(app.library_service()).present(app.active_window().as_ref());
            })
            .build();
        let edit_game_action = gio::ActionEntry::builder("edit-game")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                app.with_game(id, |window, game, library, _| {
                    let (id, title) = (game.id(), game.game().title.clone());
                    GameDialog::edit(library, &id, &title).present(Some(window));
                });
            })
            .build();
        let import_action = gio::ActionEntry::builder("import")
            .activate(move |app: &Self, _, _| app.imp().sync_service.sync())
            .build();
        let play_action = gio::ActionEntry::builder("play")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                let sessions = app.session_service().clone();
                app.with_game(id, move |window, game, library, toast| {
                    launch_service::play(window, game, library, &sessions, toast);
                });
            })
            .build();
        let stop_action = gio::ActionEntry::builder("stop")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                if let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) {
                    app.session_service().stop(&id);
                }
            })
            .build();
        let open_action = gio::ActionEntry::builder("open-in-launcher")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                app.with_game(id, |window, game, _, toast| {
                    launch_service::open_in_launcher(window, game, toast)
                });
            })
            .build();
        self.add_action_entries([
            quit_action,
            about_action,
            preferences_action,
            add_game_action,
            edit_game_action,
            import_action,
            play_action,
            stop_action,
            open_action,
        ]);
    }

    /// Runs `work` for the game an action names, in the active window, with a way to
    /// show a toast there.
    fn with_game(
        &self,
        id: Option<&glib::Variant>,
        work: impl FnOnce(&PinduWindow, &GameObject, &LibraryService, Box<dyn Fn(adw::Toast)>),
    ) {
        let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) else {
            return;
        };
        let Some(window) = self.active_window().and_downcast::<PinduWindow>() else {
            return;
        };
        let library = self.library_service();
        let Some(game) = library.model().game_object(&id) else {
            return;
        };
        let toast_window = window.downgrade();
        let toast = Box::new(move |toast: adw::Toast| {
            if let Some(window) = toast_window.upgrade() {
                window.add_toast(toast);
            }
        });
        work(&window, &game, library, toast);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name(gettext("Pindu launcher"))
            .application_icon(APP_ID)
            .developer_name("Tim van Oudheusden")
            .version(VERSION)
            .website("https://github.com/Tim-van-Oudheusden/pindu-launcher")
            .issue_url("https://github.com/Tim-van-Oudheusden/pindu-launcher/issues")
            .developers(["Tim van Oudheusden"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(gettext("translator-credits"))
            .license_type(gtk::License::Gpl30)
            .build();
        about.present(self.active_window().as_ref());
    }
}

impl Default for PinduApplication {
    fn default() -> Self {
        Self::new()
    }
}
