//! Starts games and opens their launchers. Store games go to their launcher through a URI,
//! which `GtkUriLauncher` hands to the OpenURI portal inside the sandbox, or, in a
//! `gamescope --steam` session, a Steam game goes to Steam as `steam -silent <uri>` on the
//! host; games we start ourselves run on the host as our child, from an argument list, and
//! Windows games and native games in the Steam Linux Runtime run through umu in the
//! sandbox (see `proton_launch`).

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::{self, Command, Route, SteamSession};
use pindu_core::model::{Game, Source};
use pindu_core::notifications::{Kind, Target};

use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::library_service::LibraryService;
use crate::log_service::{self, GameLog};
use crate::models::GameObject;
use crate::permission_service;
use crate::proton_launch;
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
        Source::Faugus | Source::Desktop | Source::Manual => None,
    }
}

/// Starts a game. A store launch hands the game to its launcher; a host launch runs the
/// game as our child, tracking its session unless its launcher tracks playtime itself; a
/// Windows game, or a native game in the Steam Linux Runtime, runs through umu, its
/// session tracked from the moment umu starts it. Either saves the last-played time, and
/// the toast says what happened, or why the game could not start. Without a window
/// (`--launch`, Shell search) nothing is shown, unless the game needs a permission: then
/// the window opens to explain it, and a notification keeps the explanation for Quick
/// Access. With `logs` ("Play with Logs"), a game we start keeps its output, and a Windows
/// game Proton's log too, in a new log of the game (see `log_service`).
pub fn play(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &GameObject,
    library: &LibraryService,
    sessions: &SessionService,
    logs: bool,
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
        }
        match launch::route(&game, config.as_ref()) {
            Some(Route::StoreUri(uri)) => {
                let hand_off = store_hand_off(&app, &library, &game, uri).await;
                hand_to_launcher(
                    &app,
                    window.as_ref(),
                    &game,
                    hand_off,
                    library,
                    sessions,
                    toast,
                );
            }
            Some(Route::HostSpawn(mut command)) => {
                let mut output = None;
                if logs {
                    let (id, title) = (game.id.clone(), game.title.clone());
                    let created = gio::spawn_blocking(move || {
                        let log = GameLog::create(&id, &title, &mut command, false)
                            .and_then(|log| log.output());
                        (log, command)
                    })
                    .await;
                    let Ok((created, logged)) = created else {
                        return;
                    };
                    command = logged;
                    match created {
                        Ok(created) => output = Some(created),
                        Err(error) => {
                            glib::g_warning!(
                                "pindu",
                                "Could not start a log of “{}”: {error}",
                                game.title
                            );
                            toast(adw::Toast::new(&log_service::could_not_keep(&game.title)));
                        }
                    }
                }
                let started = if launch::tracks_sessions(&game) {
                    sessions.start(app.upcast_ref(), &game.id, &command, output)
                } else {
                    session_service::spawn(&command, output).map(drop)
                };
                let message = match started {
                    Ok(()) => {
                        library.mark_played(&game.id, now()).await;
                        gettext("Starting “{title}”")
                    }
                    Err(error) => {
                        glib::g_warning!("pindu", "Could not start {:?}: {error}", command.argv);
                        gettext("Could not start “{title}”")
                    }
                };
                toast(adw::Toast::new(&message.replace("{title}", &game.title)));
            }
            Some(Route::Proton(config)) => {
                proton_launch::play(&app, window.as_ref(), game, config, sessions, logs, toast);
            }
            Some(Route::Runtime(command)) => {
                proton_launch::play_native(
                    &app,
                    window.as_ref(),
                    game,
                    command,
                    sessions,
                    logs,
                    toast,
                );
            }
            None => toast(adw::Toast::new(&gettext("This game cannot be started yet"))),
        }
        drop(hold);
    });
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

/// Hands a store game to its launcher, then quits when the user asked for that and no
/// game we started is running. A command is started and left alone: the `steam` it runs
/// passes the URI to a running Steam client and exits, or becomes the client itself.
fn hand_to_launcher(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &Game,
    hand_off: HandOff,
    library: LibraryService,
    sessions: SessionService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let launcher = launcher_name(game.source()).unwrap_or_default();
    let (id, title) = (game.id.clone(), game.title.clone());
    let (app, hold) = (app.clone(), app.hold());
    let main_window = window.and_then(|window| window.downcast_ref::<PinduWindow>().cloned());
    let handed_off = move |result: Result<(), String>| {
        let message = match result {
            Ok(()) => {
                if let Some(application) = app.downcast_ref::<PinduApplication>() {
                    application.gamescope_service().store_launch_started(&id);
                }
                if let Some(window) = &main_window {
                    window.store_launch_opened();
                }
                let exit = gio::Settings::new(APP_ID).boolean("exit-after-launch")
                    && !sessions.any_playing();
                // Hold on until the last-played time is stored, then quit if asked to.
                glib::spawn_future_local(async move {
                    library.mark_played(&id, now()).await;
                    drop(hold);
                    if exit {
                        app.quit();
                    }
                });
                gettext("Starting “{title}” in {launcher}")
            }
            Err(error) => {
                glib::g_warning!("pindu", "{error}");
                gettext("Could not open {launcher}")
            }
        };
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

pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
