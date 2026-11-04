//! Starts games and opens their launchers. Store games go to their launcher through a URI,
//! which `GtkUriLauncher` hands to the OpenURI portal inside the sandbox.

use std::time::{SystemTime, UNIX_EPOCH};

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::{self, Route};
use pindu_core::model::Source;

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

/// Starts a game. A store launch hands over and saves the last-played time; the toast
/// names the launcher, or says why the game could not start.
pub fn play(
    window: &impl IsA<gtk::Window>,
    game: &GameObject,
    library: &LibraryService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let (id, title, source, route) = {
        let game = game.game();
        (
            game.id.clone(),
            game.title.clone(),
            game.source(),
            launch::route(&game),
        )
    };
    if permission_service::needs_permission(&game.game()) {
        explain_permission(window, &title);
        return;
    }
    let launcher = launcher_name(source).unwrap_or_default();
    let Some(Route::StoreUri(uri)) = route else {
        toast(adw::Toast::new(&gettext("This game cannot be started yet")));
        return;
    };
    let library = library.clone();
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
