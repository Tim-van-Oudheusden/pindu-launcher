//! The application: starts the shared services, handles `--launch`, `--fullscreen` and
//! `--session`, registers the Shell search provider and owns the app-wide actions.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::add_store_service::AddStoreService;
use crate::audio_output_service::AudioOutputService;
use crate::bluetooth_service::BluetoothService;
use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::diagnostics_service;
use crate::first_sync_summary;
use crate::game_dialog::GameDialog;
use crate::game_overlay::PinduGameOverlay;
use crate::gamepad_service::GamepadService;
use crate::gamescope_service::GamescopeService;
use crate::health_service::HealthService;
use crate::launch_service;
use crate::library_service::{ImportFailure, LibraryService};
use crate::manette_backend::ManetteBackend;
use crate::models::GameObject;
use crate::network_service::NetworkService;
use crate::notification_service::NotificationService;
use crate::permission_service;
use crate::power_service::PowerService;
use crate::preferences::PreferencesDialog;
use crate::proton_launch::UMU_INSTALL_URI;
use crate::search_provider;
use crate::session_service::SessionService;
use crate::setup_checklist;
use crate::sources::source_list;
use crate::steamgriddb_service;
use crate::style_service::StyleService;
use crate::sync_service::SyncService;
use crate::system_power_service::SystemPowerService;
use crate::system_update_service::SystemUpdateService;
use crate::task_service::TaskService;
use crate::watch_service::WatchService;
use crate::wikidata_service;
use crate::window::PinduWindow;
use pindu_core::accessibility::{LEGACY_LARGE_TEXT_KEY, TEXT_SIZE_KEY, migrated_text_size};
use pindu_core::backup::ImportSummary;
use pindu_core::gamescope::session::{Session, has_os_session_select, runs_as_own_session};
use pindu_core::health;
use pindu_core::keys;
use pindu_core::launch::{Start, WebPage};
use pindu_core::library::{Filters, View};
use pindu_core::model::GameId;
use pindu_core::notifications::{Kind, Target};
use pindu_core::panels::Playing;
use pindu_core::setup::SetupState;
use pindu_core::sources::location::HostDirs;
use pindu_core::steamgriddb::{self, RunNotice};
use pindu_core::wikidata::{self, RunNotice as WikidataNotice};

/// The signal saying that a launch of a game was recorded, with the game's ID.
const LAUNCH_RECORDED: &str = "launch-recorded";

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
        /// Another fetch asked for during a run, which follows it; `true` when the user
        /// asked for it.
        pub fetch_art_again: std::cell::Cell<Option<bool>>,
        /// Whether metadata is being refreshed from Wikidata.
        pub refreshing_metadata: std::cell::Cell<bool>,
        /// A game `--launch` asked for when this is the first instance.
        pub pending_launch: RefCell<Option<String>>,
        /// Whether `--fullscreen` asked for the window in fullscreen mode.
        pub pending_fullscreen: std::cell::Cell<bool>,
        /// Whether `--session` said this process is gamescope-session-plus's client.
        pub session_client: std::cell::Cell<bool>,
        /// What `--session` changes, read on first use.
        pub session: OnceCell<Session>,
        /// Whether the launcher runs as its own login session, read on first use.
        pub own_session: OnceCell<bool>,
        /// Controller input for every shell; it opens controllers only when one listens.
        pub gamepad_service: OnceCell<GamepadService>,
        /// gamescope's `--steam` focus protocol; inert outside such a gamescope.
        pub gamescope_service: OnceCell<GamescopeService>,
        /// The Library Health report, for both shells.
        pub health_service: OnceCell<HealthService>,
        /// Whether a finished sync owes the summary of what the library found
        /// (`health::summary_owed`), which waits for the report gathered after it.
        pub first_sync_pending: std::cell::Cell<bool>,
        /// The user's own stylesheet for fullscreen mode.
        pub style_service: OnceCell<StyleService>,
        /// The batteries, for Quick Access's header.
        pub power_service: OnceCell<PowerService>,
        /// Bluetooth, for Quick Access's Bluetooth.
        pub bluetooth_service: OnceCell<BluetoothService>,
        /// The audio outputs, for Quick Access's Sound Output.
        pub audio_output_service: OnceCell<AudioOutputService>,
        /// OS updates on Pindu OS, for Quick Access's System Update.
        pub system_update_service: OnceCell<SystemUpdateService>,
        /// The "add a store" flow, for Quick Access and the empty library.
        pub add_store_service: OnceCell<AddStoreService>,
        /// The network, for Quick Access's Network.
        pub network_service: OnceCell<NetworkService>,
        /// Suspend, restart and power off, for the drawer's Power.
        pub system_power_service: OnceCell<SystemPowerService>,
        /// What happened in the background this session, for Quick Access.
        pub notification_service: NotificationService,
        /// What runs in the background, for Quick Access's Activity.
        pub task_service: TaskService,
        /// Fullscreen mode's library filters, apart from the desktop window's filter row;
        /// they last until the app quits, through windows closed and opened again.
        pub fullscreen_filters: std::cell::RefCell<Filters>,
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
            obj.set_accels_for_action("win.search", &[&keys::SEARCH.accelerator()]);
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
            obj.add_main_option(
                "session",
                glib::Char::from(0),
                glib::OptionFlags::NONE,
                glib::OptionArg::None,
                &gettext("Run as the client of a gamescope-session-plus session"),
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

        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::LazyLock<Vec<glib::subclass::Signal>> =
                std::sync::LazyLock::new(|| {
                    vec![
                        glib::subclass::Signal::builder(LAUNCH_RECORDED)
                            .param_types([String::static_type()])
                            .build(),
                    ]
                });
            SIGNALS.as_ref()
        }
    }

    impl ApplicationImpl for PinduApplication {
        fn startup(&self) {
            self.parent_startup();
            // Before any window reads the text size.
            super::carry_over_large_text(&gio::Settings::new(APP_ID));
            let library = LibraryService::new();
            self.sync_service.connect_finished(glib::clone!(
                #[weak(rename_to = app)]
                self.obj(),
                #[weak]
                library,
                move |sync| {
                    // Before the reload: the games the library had before this sync.
                    let games_before = library.model().counts().get(View::All);
                    library.reload();
                    app.notify_sync(sync);
                    app.fetch_missing_art(false);
                    // Before Library Health gathers again, which it connected after this.
                    if health::summary_owed(sync.was_first(), games_before) {
                        app.imp().first_sync_pending.set(true);
                    }
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
            self.gamescope_service
                .set(GamescopeService::new(
                    self.session_service.get().expect("set above"),
                    self.gamepad_service.get().expect("set above"),
                ))
                .expect("start-up runs once");
            self.health_service
                .set(HealthService::new(
                    &self.sync_service,
                    &library,
                    self.gamescope_service.get().expect("set above").is_active(),
                ))
                .expect("start-up runs once");
            self.obj().health_service().connect_changed(glib::clone!(
                #[weak(rename_to = app)]
                self.obj(),
                move |health| app.show_first_sync_summary(health)
            ));
            self.style_service
                .set(StyleService::new())
                .expect("start-up runs once");
            self.power_service
                .set(PowerService::new())
                .expect("start-up runs once");
            self.bluetooth_service
                .set(BluetoothService::new(&self.notification_service))
                .expect("start-up runs once");
            self.audio_output_service
                .set(AudioOutputService::new(&self.notification_service))
                .expect("start-up runs once");
            self.system_update_service
                .set(SystemUpdateService::new(&self.notification_service))
                .expect("start-up runs once");
            self.add_store_service
                .set(AddStoreService::new(
                    &self.sync_service,
                    &self.task_service,
                    &self.notification_service,
                ))
                .expect("start-up runs once");
            self.network_service
                .set(NetworkService::new(&self.notification_service))
                .expect("start-up runs once");
            self.system_power_service
                .set(SystemPowerService::new(&self.notification_service))
                .expect("start-up runs once");
            // Loaded here, not on the first window: Shell search and --launch need the
            // library when no window opens.
            library.reload();
            self.library_service
                .set(library)
                .expect("start-up runs once");
        }

        fn handle_local_options(&self, options: &glib::VariantDict) -> ControlFlow<glib::ExitCode> {
            // Before anything registers: the session's own process is the first instance.
            self.session_client.set(options.contains("session"));
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
            let mut new_window = false;
            let window = application.active_window().unwrap_or_else(|| {
                new_window = true;
                let window = PinduWindow::new(
                    application.upcast_ref::<adw::Application>(),
                    &self.sync_service,
                    &application.library_service().model(),
                );
                application.gamescope_service().attach_window(&window);
                if window.settings().boolean("start-in-fullscreen") {
                    window.enter_fullscreen();
                }
                // The stored library is loaded at start-up; sync every source now.
                self.sync_service.sync();
                window.upcast()
            });
            let pending_fullscreen = self.pending_fullscreen.take();
            if let Some(window) = window.downcast_ref::<PinduWindow>() {
                if pending_fullscreen {
                    window.enter_fullscreen();
                }
                // Fullscreen mode is not interrupted: the offer waits for a start in the
                // desktop window.
                if new_window
                    && !pending_fullscreen
                    && !window.settings().boolean("start-in-fullscreen")
                {
                    application.offer_steamgriddb_key(window);
                }
            }
            window.present();
            // In a `--steam` gamescope the menu drawer and Quick Access also open over a
            // running game, in a window of their own; shown without focus, it stays
            // see-through until one opens.
            if new_window
                && application.gamescope_service().is_active()
                && let Some(window) = window.downcast_ref::<PinduWindow>()
            {
                let overlay = PinduGameOverlay::new(&application, window);
                application.gamescope_service().attach_overlay(&overlay);
                overlay.set_visible(true);
            }
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
    /// The Pindu application, holding the services every window shares.
    pub struct PinduApplication(ObjectSubclass<imp::PinduApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl PinduApplication {
    /// The application with Pindu's ID and resources.
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

    /// What `--session` changes: whether closing the window quits, and whether the Power
    /// level offers Switch to Desktop. Read once, after the options are handled.
    pub fn session(&self) -> Session {
        let imp = self.imp();
        *imp.session.get_or_init(|| Session {
            client: imp.session_client.get(),
            os_session_select: has_os_session_select(&HostDirs::from_env().host_usr),
            host_spawn: permission_service::host_spawn_allowed(),
        })
    }

    /// Whether the launcher runs as its own login session (`--session`, or a `--steam`
    /// gamescope the display manager started), where quitting leaves nothing behind, so the
    /// power dock has no Exit. Read once from the environment.
    pub fn runs_as_own_session(&self) -> bool {
        let imp = self.imp();
        *imp.own_session.get_or_init(|| {
            runs_as_own_session(
                imp.session_client.get(),
                self.gamescope_service().is_active(),
                |name| std::env::var(name).ok(),
            )
        })
    }

    /// Controller input, as intents.
    pub fn gamepad_service(&self) -> &GamepadService {
        self.imp()
            .gamepad_service
            .get()
            .expect("the application has started")
    }

    /// gamescope's `--steam` focus protocol: who is in front, and Return to a game.
    pub fn gamescope_service(&self) -> &GamescopeService {
        self.imp()
            .gamescope_service
            .get()
            .expect("the application has started")
    }

    /// Says that a launch of the game with `id` was recorded, as it began or ended (see
    /// `outcome_service`).
    pub fn emit_launch_recorded(&self, id: &GameId) {
        self.emit_by_name::<()>(LAUNCH_RECORDED, &[&id.as_str()]);
    }

    /// Calls `callback` with the game whose launch was recorded, as it began or ended.
    pub fn connect_launch_recorded<F: Fn(&GameId) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(LAUNCH_RECORDED, false, move |values| {
            let id = values
                .get(1)
                .and_then(|value| value.get::<String>().ok())
                .and_then(|id| id.parse().ok());
            if let Some(id) = id {
                callback(&id);
            }
            None
        })
    }

    /// The Library Health report, gathered again after every sync.
    pub fn health_service(&self) -> &HealthService {
        self.imp()
            .health_service
            .get()
            .expect("the application has started")
    }

    /// The user's own stylesheet for fullscreen mode.
    pub fn style_service(&self) -> &StyleService {
        self.imp()
            .style_service
            .get()
            .expect("the application has started")
    }

    /// The batteries the kernel reports.
    pub fn power_service(&self) -> &PowerService {
        self.imp()
            .power_service
            .get()
            .expect("the application has started")
    }

    /// Bluetooth.
    pub fn bluetooth_service(&self) -> &BluetoothService {
        self.imp()
            .bluetooth_service
            .get()
            .expect("the application has started")
    }

    /// The audio outputs and the one in use.
    pub fn audio_output_service(&self) -> &AudioOutputService {
        self.imp()
            .audio_output_service
            .get()
            .expect("the application has started")
    }

    /// OS updates on Pindu OS.
    pub fn system_update_service(&self) -> &SystemUpdateService {
        self.imp()
            .system_update_service
            .get()
            .expect("the application has started")
    }

    /// The "add a store" flow.
    pub fn add_store_service(&self) -> &AddStoreService {
        self.imp()
            .add_store_service
            .get()
            .expect("the application has started")
    }

    /// The network.
    pub fn network_service(&self) -> &NetworkService {
        self.imp()
            .network_service
            .get()
            .expect("the application has started")
    }

    /// Suspend, restart and power off.
    pub fn system_power_service(&self) -> &SystemPowerService {
        self.imp()
            .system_power_service
            .get()
            .expect("the application has started")
    }

    /// What happened in the background this session.
    pub fn notification_service(&self) -> &NotificationService {
        &self.imp().notification_service
    }

    /// What runs in the background: syncs, art fetches and Proton downloads.
    pub fn task_service(&self) -> &TaskService {
        &self.imp().task_service
    }

    /// The sync of every launcher's library, and what it found.
    pub fn sync_service(&self) -> &SyncService {
        &self.imp().sync_service
    }

    /// Fullscreen mode's library filters, as Quick Access set them this run.
    pub fn fullscreen_filters(&self) -> Filters {
        self.imp().fullscreen_filters.borrow().clone()
    }

    /// Keeps fullscreen mode's library filters until the app quits.
    pub fn set_fullscreen_filters(&self, filters: Filters) {
        self.imp().fullscreen_filters.replace(filters);
    }

    fn setup_actions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about(None))
            .build();
        // About, with the game's newest log after its debugging information.
        let about_with_log_action = gio::ActionEntry::builder("about-with-log")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                if let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) {
                    app.show_about(Some(id));
                }
            })
            .build();
        let preferences_action = gio::ActionEntry::builder("preferences")
            .activate(move |app: &Self, _, _| {
                app.show_preferences();
            })
            .build();
        // Preferences at its Permissions group: how to let games start on this computer.
        let permissions_action = gio::ActionEntry::builder("permissions")
            .activate(move |app: &Self, _, _| app.show_preferences().show_permissions())
            .build();
        // Preferences at its Sources page: each source's folder, last sync and problems.
        let source_preferences_action = gio::ActionEntry::builder("source-preferences")
            .activate(move |app: &Self, _, _| app.show_preferences().show_sources_page())
            .build();
        // umu-launcher's install instructions, in the browser.
        let install_umu_action = gio::ActionEntry::builder("install-umu")
            .activate(move |app: &Self, _, _| {
                gtk::UriLauncher::new(UMU_INSTALL_URI).launch(
                    app.active_window().as_ref(),
                    gio::Cancellable::NONE,
                    |result| {
                        if let Err(error) = result {
                            glib::g_warning!("pindu", "Could not open {UMU_INSTALL_URI}: {error}");
                        }
                    },
                );
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
                    let dialog = GameDialog::edit(library, &id, &title);
                    dialog.connect_closed(glib::clone!(
                        #[weak]
                        window,
                        move |_| window.refresh_game_page()
                    ));
                    dialog.present(Some(window));
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
            .activate(move |app: &Self, _, id| app.play(id, Start::Play))
            .build();
        // "Play with Logs": the game's output, and Proton's log, kept in a new log.
        let play_with_logs_action = gio::ActionEntry::builder("play-with-logs")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| app.play(id, Start::WithLogs))
            .build();
        // "Start Safely": once without wrappers, MangoHud, GameMode or the game's own
        // environment, with a log; its saved settings stay as they are.
        let start_safely_action = gio::ActionEntry::builder("start-safely")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| app.play(id, Start::Safely))
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
                                Start::Play,
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
        // Stop ends a game we started, and closes a store-launched game's windows.
        let stop_action = gio::ActionEntry::builder("stop")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) else {
                    return;
                };
                let in_store = app
                    .library_service()
                    .model()
                    .game_object(&id)
                    .is_some_and(|game| game.playing() == Playing::InStore);
                if in_store {
                    app.gamescope_service().exit_store_game(&id);
                } else {
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
        let protondb_action = gio::ActionEntry::builder("open-protondb")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                app.with_game(id, |window, game, _, toast| {
                    launch_service::open_web_page(window, game, WebPage::ProtonDb, toast)
                });
            })
            .build();
        let pcgamingwiki_action = gio::ActionEntry::builder("open-pcgamingwiki")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(move |app: &Self, _, id| {
                app.with_game(id, |window, game, _, toast| {
                    launch_service::open_web_page(window, game, WebPage::PcGamingWiki, toast)
                });
            })
            .build();
        // `--fullscreen`: the activation that follows opens the window in fullscreen mode.
        let fullscreen_action = gio::ActionEntry::builder("fullscreen")
            .activate(move |app: &Self, _, _| app.imp().pending_fullscreen.set(true))
            .build();
        // Fetches missing SteamGridDB art; `true` when the user asked (saved a key).
        let fetch_art_action = gio::ActionEntry::builder("fetch-art")
            .parameter_type(Some(glib::VariantTy::BOOLEAN))
            .activate(move |app: &Self, _, asked| {
                app.fetch_missing_art(asked.and_then(bool::from_variant).unwrap_or(false));
            })
            .build();
        // Refreshes metadata from Wikidata, which the user asked for.
        let refresh_metadata_action = gio::ActionEntry::builder("refresh-metadata")
            .activate(move |app: &Self, _, _| app.refresh_metadata())
            .build();
        // Leaves Steam Big Picture for the launcher, with Steam left running, for a
        // distribution's `steamos-session-select` behind Steam's Switch to Desktop over the
        // session bus. It succeeds and changes nothing outside a Big Picture session, and
        // its name is a promise to those scripts: keep it.
        let leave_big_picture_action = gio::ActionEntry::builder("leave-big-picture")
            .activate(move |app: &Self, _, _| app.gamescope_service().leave_big_picture())
            .build();
        self.add_action_entries([
            quit_action,
            about_action,
            about_with_log_action,
            preferences_action,
            permissions_action,
            source_preferences_action,
            install_umu_action,
            add_game_action,
            edit_game_action,
            import_action,
            export_library_action,
            import_backup_action,
            play_action,
            play_with_logs_action,
            start_safely_action,
            launch_action,
            stop_action,
            open_action,
            protondb_action,
            pcgamingwiki_action,
            fullscreen_action,
            fetch_art_action,
            refresh_metadata_action,
            leave_big_picture_action,
        ]);
        // Refresh Metadata is there only while Wikidata metadata is on.
        if let Some(action) = self
            .lookup_action("refresh-metadata")
            .and_downcast::<gio::SimpleAction>()
        {
            gio::Settings::new(APP_ID)
                .bind(wikidata_service::SETTINGS_KEY, &action, "enabled")
                .get_only()
                .build();
        }
    }

    /// Opens Preferences over the active window.
    fn show_preferences(&self) -> PreferencesDialog {
        let dialog = PreferencesDialog::new(&self.imp().sync_service);
        dialog.present(self.active_window().as_ref());
        dialog
    }

    /// Fetches SteamGridDB art for games without their own, one run at a time, and shows
    /// it. A run the user `asked` for (by saving a key) says it started and how it went;
    /// one after an import speaks only of new art or a refused key
    /// ([`steamgriddb::run_notice`]). A fetch asked for during a run follows it, so a key
    /// saved meanwhile is used.
    fn fetch_missing_art(&self, asked: bool) {
        let imp = self.imp();
        if imp.fetching_art.replace(true) {
            let again = imp.fetch_art_again.get().unwrap_or(false) || asked;
            imp.fetch_art_again.set(Some(again));
            return;
        }
        let prefer = gio::Settings::new(APP_ID).boolean("prefer-steamgriddb");
        let app = self.clone();
        glib::spawn_future_local(async move {
            let mut progress = None;
            let run = steamgriddb_service::fetch_missing_art(prefer, app.task_service(), |games| {
                if steamgriddb::announces_start(games, asked) {
                    // It stays up until the run ends.
                    let toast = adw::Toast::builder()
                        .title(gettext("Getting artwork from SteamGridDB…"))
                        .timeout(0)
                        .build();
                    app.show_art_toast(&toast);
                    progress = Some(toast);
                }
            })
            .await;
            if let Some(toast) = progress {
                toast.dismiss();
            }
            let imp = app.imp();
            imp.fetching_art.set(false);
            if let Some(run) = run {
                if run.stored > 0 {
                    app.library_service().reload();
                }
                if let Some(notice) = steamgriddb::run_notice(&run, asked) {
                    let message = art_notice_message(notice);
                    app.show_art_toast(&adw::Toast::new(&message));
                    let kind = match notice {
                        RunNotice::Added(_) | RunNotice::NoneFound => Kind::Artwork,
                        RunNotice::KeyRejected | RunNotice::Unreachable => Kind::ArtworkFailed,
                    };
                    // A name, so it is not translated.
                    app.notification_service()
                        .post(kind, &message, "SteamGridDB", None);
                }
            }
            if let Some(asked) = imp.fetch_art_again.take() {
                app.fetch_missing_art(asked);
            }
        });
    }

    /// Refreshes the library's metadata from Wikidata, while it is on in Preferences and
    /// one refresh at a time. It runs only when the user asks, never after an import; a
    /// toast with Cancel stays up while it runs, and it says how it went in a toast and a
    /// notification.
    fn refresh_metadata(&self) {
        let on = gio::Settings::new(APP_ID).boolean(wikidata_service::SETTINGS_KEY);
        if !on || self.imp().refreshing_metadata.replace(true) {
            return;
        }
        let app = self.clone();
        glib::spawn_future_local(async move {
            let mut progress = None;
            let run = wikidata_service::refresh_metadata(app.task_service(), |task| {
                let toast = adw::Toast::builder()
                    .title(gettext("Getting metadata from Wikidata…"))
                    .button_label(gettext("Cancel"))
                    .timeout(0)
                    .build();
                toast.connect_button_clicked(glib::clone!(
                    #[weak]
                    app,
                    move |_| app.task_service().cancel(task)
                ));
                app.add_window_toast(&toast);
                progress = Some(toast);
            })
            .await;
            if let Some(toast) = progress {
                toast.dismiss();
            }
            app.imp().refreshing_metadata.set(false);
            let Some(run) = run else { return };
            if run.updated > 0 {
                app.library_service().reload();
            }
            if let Some(notice) = wikidata::run_notice(&run) {
                let message = metadata_notice_message(notice);
                app.add_window_toast(&adw::Toast::new(&message));
                let kind = match notice {
                    WikidataNotice::Unreachable => Kind::MetadataFailed,
                    _ => Kind::Metadata,
                };
                // A name, so it is not translated.
                app.notification_service()
                    .post(kind, &message, "Wikidata", None);
            }
        });
    }

    /// Shows `toast` in the active window, if there is one.
    fn add_window_toast(&self, toast: &adw::Toast) {
        if let Some(window) = self.active_window().and_downcast::<PinduWindow>() {
            window.add_toast(toast.clone());
        }
    }

    /// Posts what the last sync changed: one notification for the games it added and one
    /// for the games it found missing, each saying how many and from which sources. A
    /// single game added opens its page.
    fn notify_sync(&self, sync: &SyncService) {
        let notifications = self.notification_service();
        let added = sync.added();
        if !added.games.is_empty() {
            let count = added.games.len();
            let title = ngettext("Added one game", "Added {count} games", count as u32)
                .replace("{count}", &count.to_string());
            // Translators: {sources} names launchers, such as "Steam and Lutris".
            let body = gettext("From {sources}").replace("{sources}", &source_list(&added.sources));
            let target = match added.games.as_slice() {
                [game] => Some(Target::Game(game.clone())),
                _ => None,
            };
            notifications.post(Kind::Library, &title, &body, target);
        }
        let missing = sync.missing();
        if !missing.games.is_empty() {
            let count = missing.games.len();
            let title = ngettext(
                "One game is missing",
                "{count} games are missing",
                count as u32,
            )
            .replace("{count}", &count.to_string());
            // Translators: {sources} names launchers, such as "Steam and Lutris".
            let body = gettext("No longer listed by {sources}")
                .replace("{sources}", &source_list(&missing.sources));
            notifications.post(Kind::Library, &title, &body, None);
        }
    }

    /// Shows a toast about SteamGridDB art in Preferences while it is open, as saving a
    /// key there starts a fetch, or else in the active window.
    fn show_art_toast(&self, toast: &adw::Toast) {
        let Some(window) = self.active_window().and_downcast::<PinduWindow>() else {
            return;
        };
        match window.visible_dialog().and_downcast::<PreferencesDialog>() {
            Some(preferences) => preferences.add_toast(toast.clone()),
            None => window.add_toast(toast.clone()),
        }
    }

    /// Once the report gathered after a sync that owes the summary is in
    /// (`health::summary_owed`), shows what the library found in the active window,
    /// unless the summary was dismissed before, the sync found nothing, or the set-up
    /// checklist shows and no game came in (`health::first_sync_summary`).
    fn show_first_sync_summary(&self, service: &HealthService) {
        let owed = self.imp().first_sync_pending.replace(false);
        let (Some(report), Some(window)) = (
            service.report(),
            self.active_window().and_downcast::<PinduWindow>(),
        ) else {
            return;
        };
        let shown = window.settings().boolean(first_sync_summary::SHOWN_KEY);
        let setup = SetupState::from_name(&window.settings().string(setup_checklist::STATE_KEY))
            .unwrap_or_default();
        if let Some(summary) = health::first_sync_summary(&report, owed, shown, setup) {
            window.show_first_sync_summary(&summary);
        }
    }

    /// Once, on a start in the desktop window without a stored key, explains SteamGridDB
    /// art and offers to open the key field. Either answer is the last time it is shown;
    /// a keyring that cannot be read leaves it for another start.
    fn offer_steamgriddb_key(&self, window: &PinduWindow) {
        let settings = window.settings().clone();
        if settings.boolean("steamgriddb-prompt-shown") {
            return;
        }
        let app = self.clone();
        let window = window.downgrade();
        glib::spawn_future_local(async move {
            let Ok(None) = steamgriddb_service::api_key().await else {
                return;
            };
            // The window may have gone fullscreen or closed meanwhile.
            let Some(window) = window.upgrade().filter(|window| !window.is_fullscreen()) else {
                return;
            };
            let dialog = adw::AlertDialog::builder()
                .heading(gettext("Game Art From SteamGridDB"))
                .body(gettext(
                    "SteamGridDB has covers, banners and logos for games that come without them. To use it, make a free account on steamgriddb.com and add your own API key in Preferences; it is kept in your keyring.",
                ))
                .close_response("not-now")
                .default_response("settings")
                .build();
            dialog.add_responses(&[
                ("not-now", &gettext("_Not Now")),
                ("settings", &gettext("_Open Settings")),
            ]);
            dialog.set_response_appearance("settings", adw::ResponseAppearance::Suggested);
            dialog.connect_response(None, move |_, response| {
                if let Err(error) = settings.set_boolean("steamgriddb-prompt-shown", true) {
                    glib::g_warning!("pindu", "Could not note the SteamGridDB prompt: {error}");
                }
                if response == "settings" {
                    app.show_preferences().show_key_field();
                }
            });
            dialog.present(Some(&window));
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
        let health = self.health_service().clone();
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
                    Ok(()) => {
                        // Library data's Last Backup changed.
                        health.refresh();
                        gettext("Library exported")
                    }
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

    /// Starts a game from the window as `start` says: plainly, keeping its log, or safely.
    /// A game its store launcher already runs is brought back instead of handed to the
    /// store again.
    fn play(&self, id: Option<&glib::Variant>, start: Start) {
        let sessions = self.session_service().clone();
        let app = self.clone();
        self.with_game(id, move |window, game, library, toast| {
            if game.playing() == Playing::InStore {
                ActionGroupExt::activate_action(
                    window,
                    "playing-return",
                    Some(&game.id().as_str().to_variant()),
                );
                return;
            }
            // In fullscreen mode the launch overlay follows the launch, which reports
            // back through its toast.
            window.begin_launch(game);
            let reported = window.downgrade();
            launch_service::play(
                app.upcast_ref(),
                Some(window.upcast_ref()),
                game,
                library,
                &sessions,
                start,
                move |message| {
                    toast(message);
                    if let Some(window) = reported.upgrade() {
                        window.launch_reported();
                    }
                },
            );
        });
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

    /// Opens About once its debugging information is gathered, which takes a moment. With
    /// `game`, the game's newest log follows the debugging information.
    fn show_about(&self, game: Option<GameId>) {
        let steam_gamescope = self.gamescope_service().is_active();
        let own_session = self.runs_as_own_session();
        let app = self.downgrade();
        glib::spawn_future_local(async move {
            let debug_info =
                diagnostics_service::debug_info(steam_gamescope, own_session, game).await;
            let Some(app) = app.upgrade() else {
                return;
            };
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
                .debug_info(debug_info)
                .debug_info_filename("pindu-launcher-debug-info.txt")
                .build();
            about.add_legal_section(
                &gettext("Launcher Icons"),
                None,
                gtk::License::Custom,
                Some(LAUNCHER_ICONS_LEGAL),
            );
            about.present(app.active_window().as_ref());
        });
    }
}

/// Where the launcher icons come from and under what terms (`data/icons/sources/LICENSE`),
/// for the About dialog. Not translated: the legal wording is the owners'.
const LAUNCHER_ICONS_LEGAL: &str = "Steam logo: from Simple Icons (CC0 1.0), used under \
    Valve's branding guidelines. Steam and the Steam logo are trademarks and/or registered \
    trademarks of Valve Corporation in the U.S. and/or other countries.\n\
    Lutris, Bottles and Legendary icons: from Cartridges, © 2023 kramo, GPL-3.0-or-later.\n\
    Faugus Launcher logo: © 2024 Faugus, MIT License.\n\
    itch.io logo: from the itch.io press kit, used under its guidelines.\n\
    Pindu launcher is not affiliated with, sponsored or endorsed by any of them.";

impl Default for PinduApplication {
    fn default() -> Self {
        Self::new()
    }
}

/// Carries the former Larger Text setting over to Text Size once, so an install that had
/// it on keeps the size it gave, and turns the former setting off.
fn carry_over_large_text(settings: &gio::Settings) {
    let large_text = settings.boolean(LEGACY_LARGE_TEXT_KEY);
    let chosen = settings.user_value(TEXT_SIZE_KEY).is_some();
    if let Some(size) = migrated_text_size(large_text, chosen) {
        // Best effort: a read-only settings backend keeps the default size.
        let _ = settings.set_string(TEXT_SIZE_KEY, size.setting());
    }
    if large_text {
        settings.reset(LEGACY_LARGE_TEXT_KEY);
    }
}

/// What a run fetching SteamGridDB art tells the user.
fn art_notice_message(notice: RunNotice) -> String {
    match notice {
        RunNotice::Added(games) => ngettext(
            "Added artwork for one game",
            "Added artwork for {count} games",
            games as u32,
        )
        .replace("{count}", &games.to_string()),
        RunNotice::NoneFound => gettext("No new artwork found"),
        RunNotice::KeyRejected => gettext("SteamGridDB did not accept the key"),
        RunNotice::Unreachable => gettext("Could not reach SteamGridDB"),
    }
}

/// What a Wikidata refresh tells the user.
fn metadata_notice_message(notice: WikidataNotice) -> String {
    match notice {
        WikidataNotice::Updated(games) => ngettext(
            "Updated metadata for one game",
            "Updated metadata for {count} games",
            games as u32,
        )
        .replace("{count}", &games.to_string()),
        WikidataNotice::NoneFound => gettext("Wikidata has no metadata for these games"),
        WikidataNotice::NothingToLookUp => {
            gettext("No game has a Steam app ID to look up on Wikidata")
        }
        WikidataNotice::Unreachable => gettext("Could not reach Wikidata"),
    }
}

/// What an import restored, how many of its games this library lacks and how many of its
/// groups could not be made again.
fn import_message(summary: &ImportSummary) -> String {
    let restored = summary.manual_games
        + summary.favourites
        + summary.hidden
        + summary.collections
        + summary.sessions
        + summary.groups
        + summary.splits
        + summary.metadata;
    let mut message = if restored == 0 {
        gettext("Nothing new to restore")
    } else {
        gettext("Backup restored")
    };
    let mut notes = Vec::new();
    if summary.missing_games > 0 {
        notes.push(
            ngettext(
                "one game in it is not in this library",
                "{count} games in it are not in this library",
                summary.missing_games as u32,
            )
            .replace("{count}", &summary.missing_games.to_string()),
        );
    }
    if summary.skipped_groups > 0 {
        notes.push(
            ngettext(
                "one group of copies in it was skipped",
                "{count} groups of copies in it were skipped",
                summary.skipped_groups as u32,
            )
            .replace("{count}", &summary.skipped_groups.to_string()),
        );
    }
    for note in notes {
        message = gettext("{result}; {note}")
            .replace("{result}", &message)
            .replace("{note}", &note);
    }
    message
}
