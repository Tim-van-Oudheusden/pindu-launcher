//! The desktop library's selection mode, as a GObject the tiles, list cells and action
//! bar observe: whether it is on and which games it selected.

use std::cell::{Cell, RefCell};
use std::sync::LazyLock;

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::bulk::Selection;
use pindu_core::model::GameId;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LibrarySelection {
        /// Whether selection mode is on.
        pub active: Cell<bool>,
        /// The selected games.
        pub games: RefCell<Selection>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibrarySelection {
        const NAME: &'static str = "PinduLibrarySelection";
        type Type = super::LibrarySelection;
    }

    impl ObjectImpl for LibrarySelection {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: LazyLock<Vec<glib::subclass::Signal>> =
                LazyLock::new(|| vec![glib::subclass::Signal::builder("changed").build()]);
            SIGNALS.as_ref()
        }
    }
}

glib::wrapper! {
    /// Selection mode and the games it selected; `changed` follows both.
    pub struct LibrarySelection(ObjectSubclass<imp::LibrarySelection>);
}

impl LibrarySelection {
    /// Selection mode off, with nothing selected.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether selection mode is on.
    pub fn is_active(&self) -> bool {
        self.imp().active.get()
    }

    /// Turns selection mode on or off; turning it off deselects every game.
    pub fn set_active(&self, active: bool) {
        let imp = self.imp();
        if imp.active.replace(active) == active {
            return;
        }
        if !active {
            imp.games.borrow_mut().clear();
        }
        self.emit_changed();
    }

    /// Selects `game` when it is not selected, or deselects it.
    pub fn toggle(&self, game: &GameId) {
        self.imp().games.borrow_mut().toggle(game);
        self.emit_changed();
    }

    /// Selects every game of `games` too.
    pub fn select_all(&self, games: impl IntoIterator<Item = GameId>) {
        self.imp().games.borrow_mut().select_all(games);
        self.emit_changed();
    }

    /// Whether `game` is selected.
    pub fn contains(&self, game: &GameId) -> bool {
        self.imp().games.borrow().contains(game)
    }

    /// The selected games, by ID.
    pub fn games(&self) -> Vec<GameId> {
        self.imp().games.borrow().games()
    }

    /// Calls `callback` when selection mode turns on or off or the selected games change.
    pub fn connect_changed<F: Fn(&Self) + 'static>(&self, callback: F) -> glib::SignalHandlerId {
        self.connect_local("changed", false, move |values| {
            let selection = values[0].get::<Self>().expect("emitted by the selection");
            callback(&selection);
            None
        })
    }

    fn emit_changed(&self) {
        self.emit_by_name::<()>("changed", &[]);
    }
}

impl Default for LibrarySelection {
    fn default() -> Self {
        Self::new()
    }
}
