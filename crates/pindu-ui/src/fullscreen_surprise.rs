//! Quick Access's Surprise Me entry: closes the panel and opens the Game page of a game
//! `pindu_core::library::surprise_me` picks among the installed ones, never the one it
//! picked last while there is another. It is unavailable while no game is installed.

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::library::{View, surprise_me};

use crate::window::PinduWindow;

impl PinduWindow {
    /// The `win.surprise-me` action, available while the library has an installed game.
    pub(crate) fn setup_surprise_me(&self) {
        let surprise = gio::SimpleAction::new("surprise-me", None);
        surprise.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _| window.surprise_me()
        ));
        self.add_action(&surprise);
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[weak]
            surprise,
            move |_| window.show_surprise_me(&surprise)
        ));
        self.show_surprise_me(&surprise);
    }

    /// Opens the page of a game picked at random, seeded afresh by the standard library's
    /// randomly keyed hasher.
    fn surprise_me(&self) {
        let random = RandomState::new().hash_one(glib::monotonic_time());
        let objects = self.library().all_games();
        let games: Vec<_> = objects.iter().map(|object| object.game()).collect();
        let previous = self.imp().surprise_previous.borrow().clone();
        let Some(id) = surprise_me(games.iter().map(|game| &**game), previous.as_ref(), random)
            .map(|game| game.id.clone())
        else {
            return;
        };
        drop(games);
        self.imp().surprise_previous.replace(Some(id.clone()));
        self.close_panel();
        self.return_to_game(&id);
    }

    /// Surprise Me is available while a game is installed; if it had focus when the last
    /// one went, Design, the entry after it, takes it.
    fn show_surprise_me(&self, surprise: &gio::SimpleAction) {
        let imp = self.imp();
        let installed = self.library().counts().get(View::Installed) > 0;
        let had_focus = imp.quick_access_surprise_row.has_focus();
        surprise.set_enabled(installed);
        if had_focus && !installed {
            imp.quick_access_design_row.grab_focus();
        }
    }
}
