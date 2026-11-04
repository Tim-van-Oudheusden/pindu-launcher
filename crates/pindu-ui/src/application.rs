use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::game_dialog::GameDialog;
use crate::gamepad_service::GamepadService;
use crate::launch_service;
use crate::library_service::{ImportFailure, LibraryService};
use crate::manette_backend::ManetteBackend;
use crate::models::GameObject;
use crate::permission_service;
use crate::preferences::PreferencesDialog;
use crate::search_provider;
use crate::session_service::SessionService;
use crate::steamgriddb_service;
use crate::sync_service::SyncService;
use crate::watch_service::WatchService;
use crate::window::PinduWindow;
use pindu_core::backup::ImportSummary;

mod imp {
    use std::cell::{OnceCell, RefCell};
    use std::ops::ControlFlow;

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
        /// The GNOME Shell search provider's registration.
        pub search_provider: RefCell<Option<gio::RegistrationId>>,
        /// Whether SteamGridDB art is being fetched.
        pub fetching_art: std::cell::Cell<bool>,
        /// A game `--launch` asked for when this is the first instance.
        pub pending_launch: RefCell<Option<String>>,
        /// Whether `--fullscreen` asked for the window in fullscreen mode.
        pub pending_fullscreen: std::cell::Cell<bool>,
        /// Controller input for every shell; it opens controllers only when one listens.
        pub gamepad_service: OnceCell<GamepadService>,
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
            obj.set_accels_for_action("win.show-hidden", &["<primary>h"]);
            obj.set_accels_for_action("win.fullscreen-mode", &["F11"]);
            obj.add_main_option(
                "launch",
                glib::Char::from(b'l'),
                glib::OptionFlags::NONE,
                glib::OptionArg::String,
                &gettext("Start a game by its ID, without opening the window"),
                Some("GAME-ID"),
            );
            obj.add_main_option(
                "fullscreen",
                glib::Char::from(b'f'),
                glib::OptionFlags::NONE,
                glib::OptionArg::None,
                &gettext("Open the window in fullscreen mode"),
                None,
            );
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
                #[weak(rename_to = app)]
                self.obj(),
                #[weak]
                library,
                move |_| {
                    library.reload();
                    app.fetch_missing_art();
                }
            ));
            self.session_service
                .set(SessionService::new(&library))
                .expect("start-up runs once");
            self.watch_service
                .set(WatchService::new(&self.sync_service))
                .expect("start-up runs once");
            self.gamepad_service
                .set(GamepadService::new(Box::new(ManetteBackend::new())))
                .expect("start-up runs once");
            // Loaded here, not on the first window: Shell search and --launch need the
            // library when no window opens.
            library.reload();
            self.library_service
                .set(library)
                .expect("start-up runs once");
        }

        fn handle_local_options(&self, options: &glib::VariantDict) -> ControlFlow<glib::ExitCode> {
            let Some(id) = options.lookup::<String>("launch").ok().flatten() else {
                if options.contains("fullscreen") {
                    return self.request_fullscreen();
                }
                return self.parent_handle_local_options(options);
            };
            // Hand the ID to the running instance (or this one, if it is the first).
            let app = self.obj();
            if let Err(error) = app.register(gio::Cancellable::NONE) {
                glib::g_warning!("pindu", "Could not start: {error}");
                return ControlFlow::Break(glib::ExitCode::FAILURE);
            }
            if app.is_remote() {
                app.activate_action("launch-game", Some(&id.to_variant()));
                ControlFlow::Break(glib::ExitCode::SUCCESS)
            } else {
                // The first instance runs for the launch, and `activate` starts the game
                // instead of opening the window.
                self.pending_launch.replace(Some(id));
                ControlFlow::Continue(())
            }
        }

        fn dbus_register(
            &self,
            connection: &gio::DBusConnection,
            path: &str,
        ) -> Result<(), glib::Error> {
            self.parent_dbus_register(connection, path)?;
            let id = search_provider::register(&self.obj(), connection)?;
            self.search_provider.replace(Some(id));
            Ok(())
        }

        fn dbus_unregister(&self, connection: &gio::DBusConnection, path: &str) {
            if let Some(id) = self.search_provider.take() {
                let _ = connection.unregister_object(id);
            }
            self.parent_dbus_unregister(connection, path);
        }

        fn activate(&self) {
            let application = self.obj();
            if let Some(id) = self.pending_launch.take() {
                application.activate_action("launch-game", Some(&id.to_variant()));
                return;
            }
            let window = application.active_window().unwrap_or_else(|| {
                let window = PinduWindow::new(
                    application.upcast_ref::<adw::Application>(),
                    &self.sync_service,
                    &application.library_service().model(),
                );
                if window.settings().boolean("start-in-fullscreen") {
                    window.enter_fullscreen();
                }
                // The stored library is loaded at start-up; sync every source now.
                self.sync_service.sync();
                window.upcast()
            });
            if self.pending_fullscreen.take()
                && let Some(window) = window.downcast_ref::<PinduWindow>()
            {
                window.enter_fullscreen();
            }
            window.present();
        }
    }

    impl PinduApplication {
        /// Has the window open in fullscreen mode: this instance's, or the running one's.
        fn request_fullscreen(&self) -> ControlFlow<glib::ExitCode> {
            let app = self.obj();
            if let Err(error) = app.register(gio::Cancellable::NONE) {
                glib::g_warning!("pindu", "Could not start: {error}");
                return ControlFlow::Break(glib::ExitCode::FAILURE);
            }
            // The running instance gets the request first, then the activation that
            // presents its window.
            app.activate_action("fullscreen", None);
            ControlFlow::Continue(())
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

    /// Controller input, as intents.
    pub fn gamepad_service(&self) -> &GamepadService {
        self.imp()
            .gamepad_service
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
                PreferencesDialog::new(&app.imp().sync_service)
                    .present(app.active_window().as_ref());
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
        let export_library_action = gio::ActionEntry::builder("export-library")
            .activate(move |app: &Self, _, _| app.export_library())
            .build();
        let import_backup_action = gio::ActionEntry::builder("import-backup")
            .activate(move |app: &Self, _, _| app.import_backup())
            .build();
        let play_action = gio::ActionEntry::builder("play")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                let sessions = app.session_service().clone();
                let app_ref = app.clone();
                app.with_game(id, move |window, game, library, toast| {
                    launch_service::play(
                        app_ref.upcast_ref(),
                        Some(window.upcast_ref()),
                        game,
                        library,
                        &sessions,
                        toast,
                    );
                });
            })
            .build();
        // Starts a game without needing a window: `--launch <id>` and Shell search.
        let launch_action = gio::ActionEntry::builder("launch-game")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) else {
                    return;
                };
                let app = app.clone();
                let hold = app.hold();
                glib::spawn_future_local(async move {
                    let library = app.library_service().clone();
                    library.wait_loaded().await;
                    match library.model().game_object(&id) {
                        Some(game) => {
                            let window = app.active_window();
                            let toast_window = window.clone().and_downcast::<PinduWindow>();
                            launch_service::play(
                                app.upcast_ref(),
                                window.as_ref(),
                                &game,
                                &library,
                                app.session_service(),
                                move |toast| {
                                    if let Some(window) = &toast_window {
                                        window.add_toast(toast);
                                    }
                                },
                            );
                        }
                        None => glib::g_warning!("pindu", "No game {id} in the library"),
                    }
                    drop(hold);
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
        // `--fullscreen`: the activation that follows opens the window in fullscreen mode.
        let fullscreen_action = gio::ActionEntry::builder("fullscreen")
            .activate(move |app: &Self, _, _| app.imp().pending_fullscreen.set(true))
            .build();
        self.add_action_entries([
            quit_action,
            about_action,
            preferences_action,
            add_game_action,
            edit_game_action,
            import_action,
            export_library_action,
            import_backup_action,
            play_action,
            launch_action,
            stop_action,
            open_action,
            fullscreen_action,
        ]);
    }

    /// Runs `work` for the game an action names, in the active window, with a way to
    /// show a toast there.
    /// Fetches SteamGridDB art for games without their own, once at a time, and shows it.
    fn fetch_missing_art(&self) {
        if self.imp().fetching_art.replace(true) {
            return;
        }
        let prefer = gio::Settings::new(APP_ID).boolean("prefer-steamgriddb");
        let app = self.clone();
        glib::spawn_future_local(async move {
            let stored = steamgriddb_service::fetch_missing_art(prefer).await;
            app.imp().fetching_art.set(false);
            if stored > 0 {
                app.library_service().reload();
            }
        });
    }

    /// Saves a backup of collections, favourites, hidden games, games added by hand and
    /// tracked playtime to a file the user picks.
    fn export_library(&self) {
        let window = self.active_window().and_downcast::<PinduWindow>();
        let date = glib::DateTime::now_local()
            .and_then(|now| now.format("%F"))
            .map(|date| date.to_string())
            .unwrap_or_default();
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Export Library"))
            .initial_name(format!("pindu-launcher-backup-{date}.json"))
            .modal(true)
            .build();
        let library = self.library_service().clone();
        let parent = window.clone();
        chooser.save(parent.as_ref(), gio::Cancellable::NONE, move |file| {
            let Some(path) = file.ok().and_then(|file| file.path()) else {
                return;
            };
            glib::spawn_future_local(async move {
                let message = match library
                    .export_backup(path, glib::real_time() / 1_000_000)
                    .await
                {
                    Ok(()) => gettext("Library exported"),
                    Err(error) => {
                        gettext("Could not export the library: {error}").replace("{error}", &error)
                    }
                };
                if let Some(window) = window {
                    window.add_toast(adw::Toast::new(&message));
                }
            });
        });
    }

    /// Restores a backup the user picks; it adds to the library and never removes.
    fn import_backup(&self) {
        let window = self.active_window().and_downcast::<PinduWindow>();
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&gettext("Library Backups")));
        filter.add_suffix("json");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let chooser = gtk::FileDialog::builder()
            .title(gettext("Import Backup"))
            .filters(&filters)
            .modal(true)
            .build();
        let library = self.library_service().clone();
        let parent = window.clone();
        chooser.open(parent.as_ref(), gio::Cancellable::NONE, move |file| {
            let Some(path) = file.ok().and_then(|file| file.path()) else {
                return;
            };
            glib::spawn_future_local(async move {
                let message = match library.import_backup(path).await {
                    Ok(summary) => import_message(&summary),
                    Err(ImportFailure::NotABackup) => gettext("This file is not a library backup"),
                    Err(ImportFailure::TooNew) => {
                        gettext("This backup is from a newer version of Pindu launcher")
                    }
                    Err(ImportFailure::Failed(error)) => {
                        gettext("Could not import the backup: {error}").replace("{error}", &error)
                    }
                };
                if let Some(window) = window {
                    window.add_toast(adw::Toast::new(&message));
                }
            });
        });
    }

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

/// What an import restored, and how many of its games this library lacks.
fn import_message(summary: &ImportSummary) -> String {
    let restored = summary.manual_games
        + summary.favourites
        + summary.hidden
        + summary.collections
        + summary.sessions;
    let done = if restored == 0 {
        gettext("Nothing new to restore")
    } else {
        gettext("Backup restored")
    };
    match summary.missing_games {
        0 => done,
        missing => {
            let note = ngettext(
                "one game in it is not in this library",
                "{count} games in it are not in this library",
                missing as u32,
            )
            .replace("{count}", &missing.to_string());
            gettext("{result}; {note}")
                .replace("{result}", &done)
                .replace("{note}", &note)
        }
    }
}
