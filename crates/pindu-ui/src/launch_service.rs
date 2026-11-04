//! Starts games and opens their launchers. Store games go to their launcher through a URI,
//! which `GtkUriLauncher` hands to the OpenURI portal inside the sandbox; games we start
//! ourselves run on the host as our child, from an argument list.

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::{self, Route};
use pindu_core::model::{Game, Source};

use crate::config::APP_ID;
use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::permission_service;
use crate::session_service::{self, SessionService};

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
/// game as our child, tracking its session unless its launcher tracks playtime itself.
/// Either saves the last-played time, and the toast says what happened, or why the game
/// could not start. Without a window (`--launch`, Shell search) nothing is shown, unless
/// the game needs a permission: then the window opens to explain it.
pub fn play(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &GameObject,
    library: &LibraryService,
    sessions: &SessionService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let game = game.game().clone();
    if permission_service::needs_permission(&game) {
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
        let config = library.launch_config(&game.id).await;
        match launch::route(&game, config.as_ref()) {
            Some(Route::StoreUri(uri)) => {
                open_store_uri(&app, window.as_ref(), &game, &uri, library, sessions, toast);
            }
            Some(Route::HostSpawn(command)) => {
                let started = if launch::tracks_sessions(&game) {
                    sessions.start(app.upcast_ref(), &game.id, &command)
                } else {
                    session_service::spawn(&command).map(drop)
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
            None => toast(adw::Toast::new(&gettext("This game cannot be started yet"))),
        }
        drop(hold);
    });
}

/// Hands a store game to its launcher, then quits when the user asked for that and no
/// game we started is running.
fn open_store_uri(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: &Game,
    uri: &str,
    library: LibraryService,
    sessions: SessionService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let launcher = launcher_name(game.source()).unwrap_or_default();
    let (id, title, uri) = (game.id.clone(), game.title.clone(), uri.to_owned());
    let (app, hold) = (app.clone(), app.hold());
    gtk::UriLauncher::new(&uri).launch(window, gio::Cancellable::NONE, move |result| {
        let message = match result {
            Ok(()) => {
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
                glib::g_warning!("pindu", "Could not open {uri}: {error}");
                gettext("Could not open {launcher}")
            }
        };
        toast(adw::Toast::new(
            &message
                .replace("{title}", &title)
                .replace("{launcher}", launcher),
        ));
    });
}

/// Explains, instead of failing, that a game needs the host-spawn permission.
fn explain_permission(window: &impl IsA<gtk::Window>, title: &str) {
    let dialog = adw::AlertDialog::new(
        Some(&gettext("Permission Needed")),
        Some(
            &gettext(
                "“{title}” starts on this computer, outside the sandbox. Allow that in \
                 Preferences, then restart Pindu launcher.",
            )
            .replace("{title}", title),
        ),
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
        let _ = parent.activate_action("app.preferences", None);
    });
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

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
