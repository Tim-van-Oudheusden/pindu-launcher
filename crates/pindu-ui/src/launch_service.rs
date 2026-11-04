//! Starts games and opens their launchers. Store games go to their launcher through a URI,
//! which `GtkUriLauncher` hands to the OpenURI portal inside the sandbox; games we start
//! ourselves run on the host as our child, from an argument list.

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::{self, HostCommand, Route};
use pindu_core::model::{Game, Source};

use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::permission_service;

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
/// game as our child. Either saves the last-played time, and the toast says what
/// happened, or why the game could not start.
pub fn play(
    window: &impl IsA<gtk::Window>,
    game: &GameObject,
    library: &LibraryService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let game = game.game().clone();
    if permission_service::needs_permission(&game) {
        explain_permission(window, &game.title);
        return;
    }
    let window = window.upcast_ref::<gtk::Window>().clone();
    let library = library.clone();
    glib::spawn_future_local(async move {
        let config = library.launch_config(&game.id).await;
        match launch::route(&game, config.as_ref()) {
            Some(Route::StoreUri(uri)) => open_store_uri(&window, &game, &uri, library, toast),
            Some(Route::HostSpawn(command)) => {
                let message = match spawn_on_host(&command, &game.title) {
                    Ok(()) => {
                        library.mark_played(&game.id, now());
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
    });
}

/// Hands a store game to its launcher.
fn open_store_uri(
    window: &gtk::Window,
    game: &Game,
    uri: &str,
    library: LibraryService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let launcher = launcher_name(game.source()).unwrap_or_default();
    let (id, title, uri) = (game.id.clone(), game.title.clone(), uri.to_owned());
    gtk::UriLauncher::new(&uri).launch(Some(window), gio::Cancellable::NONE, move |result| {
        let message = match result {
            Ok(()) => {
                library.mark_played(&id, now());
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

/// Runs a game on the host as our direct child: through `flatpak-spawn --host` inside
/// Flatpak, directly outside it. The child is watched until it exits, so its status is
/// known; tracking the session is a separate step.
fn spawn_on_host(command: &HostCommand, title: &str) -> Result<(), glib::Error> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
    let argv = if permission_service::in_flatpak() {
        command.flatpak_spawn_argv()
    } else {
        for (key, value) in &command.environment {
            launcher.setenv(key, value, true);
        }
        if let Some(dir) = &command.working_dir {
            launcher.set_cwd(dir);
        }
        command.argv.clone()
    };
    let argv: Vec<&std::ffi::OsStr> = argv.iter().map(std::ffi::OsStr::new).collect();
    let child = launcher.spawn(&argv)?;
    let title = title.to_owned();
    glib::spawn_future_local(async move {
        match child.wait_future().await {
            Ok(()) if child.is_successful() => {
                glib::g_info!("pindu", "“{title}” exited");
            }
            Ok(()) => glib::g_warning!(
                "pindu",
                "“{title}” exited with status {}",
                child.exit_status()
            ),
            Err(error) => glib::g_warning!("pindu", "Lost track of “{title}”: {error}"),
        }
    });
    Ok(())
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
