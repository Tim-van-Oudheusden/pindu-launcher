//! Quick Access's Surprise Me entry: closes the panel and opens the Game page of a game
//! `pindu_core::library::surprise_me` picks among the installed ones, never the one it
//! picked last while there is another. It is unavailable while no game is installed.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::library::{View, surprise_me};

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;

impl PinduQuickAccess {
    /// The `win.surprise-me` action, available while the library has an installed game.
    pub(crate) fn setup_surprise_me(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        let surprise = self.add_host_action(actions, "surprise-me", None, |quick_access, _| {
            quick_access.surprise_me();
        });
        application
            .library_service()
            .model()
            .connect_counts_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                #[weak]
                surprise,
                move |_| quick_access.show_surprise_me(&surprise)
            ));
        self.show_surprise_me(&surprise);
    }

    /// Asks for the page of a game picked at random, seeded afresh by the standard
    /// library's randomly keyed hasher.
    fn surprise_me(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let random = RandomState::new().hash_one(glib::monotonic_time());
        let objects = application.library_service().model().all_games();
        let games: Vec<_> = objects.iter().map(|object| object.game()).collect();
        let previous = self.imp().surprise_previous.borrow().clone();
        let Some(id) = surprise_me(games.iter().map(|game| &**game), previous.as_ref(), random)
            .map(|game| game.id.clone())
        else {
            return;
        };
        drop(games);
        self.imp().surprise_previous.replace(Some(id.clone()));
        self.request_game(&id);
    }

    /// Surprise Me is available while a game is installed; if it had focus when the last
    /// one went, the entry beside it takes it.
    fn show_surprise_me(&self, surprise: &gio::SimpleAction) {
        let Some(application) = self.application() else {
            return;
        };
        let imp = self.imp();
        let installed = application
            .library_service()
            .model()
            .counts()
            .get(View::Installed)
            > 0;
        let had_focus = imp.surprise_row.has_focus();
        surprise.set_enabled(installed);
        if had_focus && !installed {
            self.focus_beside(&imp.surprise_row);
        }
    }
}
