//! Starts games and opens their launchers. Store games go to their launcher through a URI,
//! which `GtkUriLauncher` hands to the OpenURI portal inside the sandbox, or, in a
//! `gamescope --steam` session, a Steam game goes to Steam as `steam -silent <uri>` on the
//! host; games we start ourselves run on the host as our child, from an argument list, and
//! Windows games and native games in the Steam Linux Runtime run through umu in the
//! sandbox (see `proton_launch`).

use std::collections::BTreeMap;
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::add_store::{self, StoreApp};
use pindu_core::launch::{self, Command, Route, Start, SteamSession, WebPage};
use pindu_core::launch_outcome::{Ending, SIGTERM};
use pindu_core::model::{Game, LaunchRoute, Source};
use pindu_core::notifications::{Kind, Target};
use pindu_core::sources::location::HostDirs;

use crate::add_store_service::store_name;
use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::library_service::LibraryService;
use crate::log_service::{self, LaunchLog};
use crate::models::GameObject;
use crate::outcome_service::{self, Recording};
use crate::permission_service;
use crate::proton_launch;
use crate::save_service;
use crate::session_service::{self, SessionService};
use crate::window::PinduWindow;

/// The app that owns a store game's source, as named in toasts and buttons.
pub fn launcher_name(source: Source) -> Option<&'static str> {
    match source {
        Source::Steam => Some("Steam"),
        Source::HeroicEpic | Source::HeroicGog | Source::HeroicAmazon | Source::HeroicSideload => {
            Some("Heroic")
        }
        Source::Lutris => Some("Lutris"),
        Source::Bottles => Some("Bottles"),
        Source::Itch => Some("itch"),
        Source::Faugus | Source::Legendary | Source::Desktop | Source::Manual => None,
    }
}

/// Starts a game. A store launch hands the game to its launcher; a host launch runs the
/// game as our child, tracking its session unless its launcher tracks playtime itself; a
/// Windows game, or a native game in the Steam Linux Runtime, runs through umu, its
/// session tracked from the moment umu starts it. Either saves the last-played time, and
/// the toast says what happened, or why the game could not start. Without a window
/// (`--launch`, Shell search) nothing is shown, unless the game needs a permission: then
/// the window opens to explain it, and a notification keeps the explanation for Quick
/// Access. Every launch but one refused for a permission records how it ended (see
/// `outcome_service`). With `start` keeping a log ("Play with Logs", "Start Safely"), a
/// game we start keeps its output, and a Windows game Proton's log too, in a new log of
/// the game (see `log_service`); a safe start runs it without its wrappers, MangoHud,
/// GameMode or its own environment variables, this once. Any other start of a game whose
/// session we track keeps the end of its output in a short log.
pub fn play(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &GameObject,
    library: &LibraryService,
    sessions: &SessionService,
    start: Start,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let game = game.game().clone();
    if permission_service::needs_permission(&game) {
        if let Some(application) = app.downcast_ref::<PinduApplication>() {
            application.notification_service().post(
                Kind::Permission,
                &permission_needed(),
                &permission_explanation(&game.title),
                Some(Target::Permissions),
            );
        }
        if window.is_none() {
            app.activate();
        }
        if let Some(window) = window.cloned().or_else(|| app.active_window()) {
            explain_permission(&window, &game.title);
        }
        return;
    }
    let (app, window) = (app.clone(), window.cloned());
    let (library, sessions) = (library.clone(), sessions.clone());
    // The app stays up until the launch is through, even with no window.
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let mut config = library.launch_config(&game.id).await;
        if let Some(application) = app.downcast_ref::<PinduApplication>() {
            let gamescope = application.gamescope_service();
            config = config.map(|config| gamescope.launch_config(config));
            gamescope.set_started_safely(&game.id, start == Start::Safely);
        }
        let config = config.map(|config| start.config(config));
        match launch::route(&game, config.as_ref()) {
            Some(Route::StoreUri(uri)) => {
                let hand_off = store_hand_off(&app, &library, &game, uri).await;
                let recording =
                    outcome_service::begin(&app, &game.id, LaunchRoute::HandOff, None, None).await;
                hand_to_launcher(
                    &app,
                    window.as_ref(),
                    &game,
                    hand_off,
                    recording,
                    library,
                    sessions,
                    toast,
                );
            }
            Some(Route::HostSpawn(mut command)) => {
                let tracks = launch::tracks_sessions(&game);
                let mut log = None;
                // A short log reads the game's output through a pipe, which breaks once
                // the app quits: only a game whose session keeps the app up has one.
                if start.keeps_log() || tracks {
                    let (id, title) = (game.id.clone(), game.title.clone());
                    let created = gio::spawn_blocking(move || {
                        let log = LaunchLog::create(&id, &title, &mut command, false, start);
                        (log, command)
                    })
                    .await;
                    let Ok((created, logged)) = created else {
                        return;
                    };
                    command = logged;
                    match created {
                        Ok(created) => log = Some(created),
                        Err(error) => {
                            glib::g_warning!(
                                "pindu",
                                "Could not start a log of “{}”: {error}",
                                game.title
                            );
                            if start.keeps_log() {
                                toast(adw::Toast::new(&log_service::could_not_keep(&game.title)));
                            }
                        }
                    }
                }
                let log_name = log.as_ref().and_then(LaunchLog::name);
                let recording =
                    outcome_service::begin(&app, &game.id, LaunchRoute::Host, None, log_name).await;
                let spawned = if log.as_ref().is_some_and(LaunchLog::reads_pipe) {
                    session_service::spawn_piped(&command)
                } else {
                    let output = log
                        .as_ref()
                        .map(LaunchLog::output)
                        .transpose()
                        .unwrap_or_else(|error| {
                            glib::g_warning!(
                                "pindu",
                                "Could not open the log of “{}”: {error}",
                                game.title
                            );
                            None
                        })
                        .flatten();
                    session_service::spawn(&command, output)
                };
                let toast = Rc::new(toast);
                let message = match spawned {
                    Ok(child) => {
                        if tracks {
                            let signalled = child.clone();
                            sessions.track(app.upcast_ref(), &game.id, &child, move || {
                                signalled.send_signal(SIGTERM);
                            });
                            snapshot_after_exit(&game, &child, toast.clone());
                        }
                        // A game whose session keeps the app up keeps it up until its
                        // outcome and log are written too.
                        recording.follow(child, log, tracks.then(|| app.hold()));
                        library.mark_played(&game.id, session_service::now()).await;
                        starting(&game.title, start)
                    }
                    Err(error) => {
                        glib::g_warning!("pindu", "Could not start {:?}: {error}", command.argv);
                        recording.end(Ending::NotStarted).await;
                        gettext("Could not start “{title}”").replace("{title}", &game.title)
                    }
                };
                toast(adw::Toast::new(&message));
            }
            Some(Route::Proton(config)) => {
                proton_launch::play(&app, window.as_ref(), game, config, sessions, start, toast);
            }
            Some(Route::Runtime(command)) => {
                proton_launch::play_native(
                    &app,
                    window.as_ref(),
                    game,
                    command,
                    sessions,
                    start,
                    toast,
                );
            }
            None => toast(adw::Toast::new(&gettext("This game cannot be started yet"))),
        }
        drop(hold);
    });
}

/// Once `child`, the game `game` we started on the host, exits cleanly, takes a snapshot
/// of the save folders the user added for it, if any (see `save_service`), said with
/// `toast` when it fails.
fn snapshot_after_exit(
    game: &Game,
    child: &gio::Subprocess,
    toast: Rc<impl Fn(adw::Toast) + 'static>,
) {
    let (game, child) = (game.clone(), child.clone());
    glib::spawn_future_local(async move {
        if child.wait_future().await.is_ok() && child.is_successful() {
            save_service::snapshot_after_exit(&game, None, move |message| toast(message));
        }
    });
}

/// What the toast says when a game we run starts: that it starts, and safely for a safe
/// start.
pub(crate) fn starting(title: &str, start: Start) -> String {
    match start {
        Start::Safely => gettext("Starting “{title}” safely"),
        Start::Play | Start::WithLogs => gettext("Starting “{title}”"),
    }
    .replace("{title}", title)
}

/// How a store game goes to its launcher.
enum HandOff {
    /// Through its URI, which `GtkUriLauncher` opens (the OpenURI portal in the sandbox).
    Uri(String),
    /// Through a host command we start and leave alone: Steam with `-silent`.
    Command(Command),
}

/// How `game`'s store `uri` goes to its launcher: a Steam game in a `gamescope --steam`
/// session goes to Steam with `-silent` when we may run it on the host, so a Steam client
/// that is not running yet starts without its library window (see
/// [`launch::silent_steam`]); anything else goes through the URI.
async fn store_hand_off(
    app: &gtk::Application,
    library: &LibraryService,
    game: &Game,
    uri: String,
) -> HandOff {
    let steam_gamescope = app
        .downcast_ref::<PinduApplication>()
        .is_some_and(|application| application.gamescope_service().is_active());
    let host_spawn = permission_service::host_spawn_allowed();
    // Where Steam is matters only when its command could run, so the store is read
    // only then.
    let steam_root = if steam_gamescope && host_spawn && game.source() == Source::Steam {
        library.source_location(Source::Steam).await
    } else {
        None
    };
    let session = SteamSession {
        steam_gamescope,
        host_spawn,
        steam_root: steam_root.as_deref(),
        steam_client: None,
    };
    match launch::silent_steam(game, &uri, &session) {
        Some(command) => {
            glib::g_debug!("pindu", "Handing {} to Steam: {:?}", game.id, command.argv);
            HandOff::Command(command)
        }
        None => {
            glib::g_debug!("pindu", "Handing {} to its launcher: {uri}", game.id);
            HandOff::Uri(uri)
        }
    }
}

/// Hands a store game to its launcher, records whether the launcher answered, then quits
/// when the user asked for that and no game we started is running. A command is started
/// and left alone: the `steam` it runs passes the URI to a running Steam client and exits,
/// or becomes the client itself.
#[expect(
    clippy::too_many_arguments,
    reason = "each argument is a separate input of one launch; a struct would only rename them"
)]
fn hand_to_launcher(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &Game,
    hand_off: HandOff,
    recording: Recording,
    library: LibraryService,
    sessions: SessionService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let launcher = launcher_name(game.source()).unwrap_or_default();
    let (id, title) = (game.id.clone(), game.title.clone());
    let (app, hold) = (app.clone(), app.hold());
    let main_window = window.and_then(|window| window.downcast_ref::<PinduWindow>().cloned());
    let handed_off = move |result: Result<(), String>| {
        let (message, ending, exit) = match result {
            Ok(()) => {
                if let Some(application) = app.downcast_ref::<PinduApplication>() {
                    application.gamescope_service().store_launch_started(&id);
                }
                if let Some(window) = &main_window {
                    window.store_launch_opened();
                }
                let exit = gio::Settings::new(APP_ID).boolean("exit-after-launch")
                    && !sessions.any_playing();
                let message = gettext("Starting “{title}” in {launcher}");
                (message, Ending::HandedOff, exit)
            }
            Err(error) => {
                glib::g_warning!("pindu", "{error}");
                (
                    gettext("Could not open {launcher}"),
                    Ending::HandOffFailed,
                    false,
                )
            }
        };
        // Hold on until the outcome and the last-played time are stored, then quit if
        // asked to.
        glib::spawn_future_local(async move {
            recording.end(ending).await;
            if ending == Ending::HandedOff {
                library.mark_played(&id, session_service::now()).await;
            }
            drop(hold);
            if exit {
                app.quit();
            }
        });
        toast(adw::Toast::new(
            &message
                .replace("{title}", &title)
                .replace("{launcher}", launcher),
        ));
    };
    match hand_off {
        HandOff::Uri(uri) => {
            gtk::UriLauncher::new(&uri).launch(window, gio::Cancellable::NONE, move |result| {
                handed_off(result.map_err(|error| format!("Could not open {uri}: {error}")));
            });
        }
        HandOff::Command(command) => handed_off(
            session_service::spawn(&command, None)
                .map(drop)
                .map_err(|error| format!("Could not start {:?}: {error}", command.argv)),
        ),
    }
}

/// Explains, instead of failing, that a game needs the host-spawn permission.
fn explain_permission(window: &impl IsA<gtk::Window>, title: &str) {
    let dialog = adw::AlertDialog::new(
        Some(&permission_needed()),
        Some(&permission_explanation(title)),
    );
    dialog.add_responses(&[
        ("close", &gettext("_Close")),
        ("enable", &gettext("_How to Enable")),
    ]);
    dialog.set_response_appearance("enable", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("enable"));
    dialog.set_close_response("close");
    let parent = window.upcast_ref::<gtk::Window>().clone();
    dialog.present(Some(&parent));
    dialog.connect_response(Some("enable"), move |_, _| {
        let _ = parent.activate_action("app.permissions", None);
    });
}

/// The heading of the explanation that a game needs a permission.
fn permission_needed() -> String {
    gettext("Permission Needed")
}

/// Why the game titled `title` cannot start yet, and what to do about it.
fn permission_explanation(title: &str) -> String {
    gettext(
        "“{title}” starts on this computer, outside the sandbox. Allow that in \
         Preferences, then restart Pindu launcher.",
    )
    .replace("{title}", title)
}

/// Brings up the game's launcher: Steam on the game's page, or Heroic.
pub fn open_in_launcher(
    window: &impl IsA<gtk::Window>,
    game: &GameObject,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let (uri, launcher) = {
        let game = game.game();
        (
            launch::launcher_uri(&game),
            launcher_name(game.source()).unwrap_or_default(),
        )
    };
    let Some(uri) = uri else { return };
    gtk::UriLauncher::new(&uri).launch(Some(window), gio::Cancellable::NONE, move |result| {
        if let Err(error) = result {
            glib::g_warning!("pindu", "Could not open {uri}: {error}");
            toast(adw::Toast::new(
                &gettext("Could not open {launcher}").replace("{launcher}", launcher),
            ));
        }
    });
}

/// Opens `page` about the game in the browser, through the OpenURI portal in the sandbox;
/// nothing for a game that has no such page. Pindu launcher fetches nothing itself.
pub fn open_web_page(
    window: &impl IsA<gtk::Window>,
    game: &GameObject,
    page: WebPage,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let Some(uri) = launch::web_page_uri(&game.game(), page) else {
        return;
    };
    gtk::UriLauncher::new(&uri).launch(Some(window), gio::Cancellable::NONE, move |result| {
        if let Err(error) = result {
            glib::g_warning!("pindu", "Could not open {uri}: {error}");
            toast(adw::Toast::new(&gettext("Could not open the web page")));
        }
    });
}

/// How Steam Big Picture opens from Home, as [`launch::big_picture`] decides it for this
/// session, and whether it is Steam's first start (`SteamSession::first_start`); `None`
/// where Steam is neither imported nor installed, so Home does not offer it.
pub async fn big_picture_route(
    app: &gtk::Application,
    library: &LibraryService,
) -> Option<(Route, bool)> {
    let steam_gamescope = app
        .downcast_ref::<PinduApplication>()
        .is_some_and(|application| application.gamescope_service().is_active());
    let steam_root = library.source_location(Source::Steam).await;
    let steam_client =
        gio::spawn_blocking(|| add_store::store_client(StoreApp::Steam, &HostDirs::from_env()))
            .await
            .ok()
            .flatten();
    let session = SteamSession {
        steam_gamescope,
        host_spawn: permission_service::host_spawn_allowed(),
        steam_root: steam_root.as_deref(),
        steam_client,
    };
    launch::big_picture(&session).map(|route| (route, session.first_start()))
}

/// Hands Steam Big Picture to Steam, the way Play hands a Steam game over. In a
/// `gamescope --steam` session it then takes the launcher's place (see
/// `GamescopeService::big_picture_started`); elsewhere Steam's window takes focus, and
/// with it the controller.
pub fn open_big_picture(
    app: &gtk::Application,
    window: &gtk::Window,
    library: &LibraryService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let (app, window, library) = (app.clone(), window.clone(), library.clone());
    glib::spawn_future_local(async move {
        let route = big_picture_route(&app, &library).await;
        let first_start = route.as_ref().is_some_and(|&(_, first_start)| first_start);
        let opened = move |result: Result<(), String>| match result {
            Ok(()) => {
                if let Some(application) = app.downcast_ref::<PinduApplication>() {
                    application
                        .gamescope_service()
                        .big_picture_started(first_start);
                }
            }
            Err(error) => {
                glib::g_warning!("pindu", "{error}");
                toast(adw::Toast::new(
                    &gettext("Could not open {launcher}").replace("{launcher}", "Steam"),
                ));
            }
        };
        match route.map(|(route, _)| route) {
            Some(Route::HostSpawn(command)) => {
                glib::g_debug!("pindu", "Opening Big Picture: {:?}", command.argv);
                opened(
                    session_service::spawn(&command, None)
                        .map(drop)
                        .map_err(|error| format!("Could not start {:?}: {error}", command.argv)),
                );
            }
            Some(Route::StoreUri(uri)) => {
                glib::g_debug!("pindu", "Opening Big Picture: {uri}");
                gtk::UriLauncher::new(&uri).launch(
                    Some(&window),
                    gio::Cancellable::NONE,
                    move |result| {
                        opened(result.map_err(|error| format!("Could not open {uri}: {error}")));
                    },
                );
            }
            _ => {}
        }
    });
}

/// Opens `store`'s client on the host for its first sign-in (`add_store::open_command`):
/// the Flatpak or the host's package, whichever the desktop shows. In a `gamescope
/// --steam` session its windows then take the launcher's place until they close, as a game
/// handed to its store does (`GamescopeService::store_launch_started`); elsewhere its
/// window takes focus, and with it the controller.
pub fn open_store(app: &gtk::Application, store: StoreApp, toast: impl Fn(adw::Toast) + 'static) {
    let app = app.clone();
    glib::spawn_future_local(async move {
        let client =
            gio::spawn_blocking(move || add_store::store_client(store, &HostDirs::from_env()))
                .await
                .ok()
                .flatten();
        let opened = client.map(|client| {
            let command = Command {
                argv: add_store::open_command(store, client),
                environment: BTreeMap::new(),
                working_dir: None,
            };
            glib::g_debug!("pindu", "Opening a store: {:?}", command.argv);
            session_service::spawn(&command, None)
        });
        match opened {
            Some(Ok(_)) => {
                if let Some(application) = app.downcast_ref::<PinduApplication>() {
                    application
                        .gamescope_service()
                        .store_launch_started(&add_store::opened_id(store));
                }
            }
            failed => {
                if let Some(Err(error)) = failed {
                    glib::g_warning!("pindu", "Could not open a store: {error}");
                }
                toast(adw::Toast::new(
                    &gettext("Could not open {launcher}").replace("{launcher}", store_name(store)),
                ));
            }
        }
    });
}
