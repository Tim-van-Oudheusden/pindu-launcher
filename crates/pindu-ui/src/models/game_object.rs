//! A library game as a GObject, so list models and widgets can observe it.

use std::cell::{Cell, Ref, RefCell};
use std::sync::LazyLock;

use gtk::glib;
use gtk::glib::ParamSpec;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::library::{ArtSet, LibraryEntry, PlaytimeOrigin, SearchKey, SortKey};
use pindu_core::model::{Game, GameId};

/// The game, its art and playtime, and the keys filtering and sorting need, recomputed
/// when it changes.
#[derive(Debug)]
pub struct Entry {
    /// The game itself.
    pub game: Game,
    /// The game's art in every layout.
    pub art: ArtSet,
    /// Total playtime, in seconds.
    pub playtime_secs: i64,
    /// Where the playtime comes from; `None` when the game was never played.
    pub playtime_origin: Option<PlaytimeOrigin>,
    /// IDs of the collections holding the game.
    pub collections: Vec<i64>,
    /// The key searches match the title against.
    pub search_key: SearchKey,
    /// The key the library sorts by.
    pub sort_key: SortKey,
}

impl Entry {
    fn new(entry: LibraryEntry) -> Self {
        Self {
            search_key: SearchKey::new(&entry.game.title),
            sort_key: SortKey::new(&entry.game, entry.playtime_secs),
            game: entry.game,
            art: entry.art,
            playtime_secs: entry.playtime_secs,
            playtime_origin: entry.playtime_origin,
            collections: entry.collections,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct GameObject {
        pub entry: RefCell<Option<Entry>>,
        /// A game we started is running.
        pub playing: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GameObject {
        const NAME: &'static str = "PinduGameObject";
        type Type = super::GameObject;
    }

    impl ObjectImpl for GameObject {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: LazyLock<Vec<ParamSpec>> = LazyLock::new(|| {
                vec![
                    glib::ParamSpecString::builder("id").read_only().build(),
                    glib::ParamSpecString::builder("title").read_only().build(),
                    glib::ParamSpecBoolean::builder("installed")
                        .read_only()
                        .build(),
                    glib::ParamSpecBoolean::builder("favourite")
                        .read_only()
                        .build(),
                    glib::ParamSpecBoolean::builder("hidden")
                        .read_only()
                        .build(),
                    glib::ParamSpecBoolean::builder("playing")
                        .read_only()
                        .build(),
                ]
            });
            PROPERTIES.as_ref()
        }

        fn property(&self, _id: usize, pspec: &ParamSpec) -> glib::Value {
            let object = self.obj();
            let game = object.game();
            match pspec.name() {
                "id" => game.id.as_str().to_value(),
                "title" => game.title.to_value(),
                "installed" => game.installed.to_value(),
                "favourite" => game.favourite.to_value(),
                "hidden" => game.hidden.to_value(),
                "playing" => self.playing.get().to_value(),
                name => unreachable!("unknown property {name}"),
            }
        }

        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: LazyLock<Vec<glib::subclass::Signal>> =
                LazyLock::new(|| vec![glib::subclass::Signal::builder("art-changed").build()]);
            SIGNALS.as_ref()
        }
    }
}

glib::wrapper! {
    /// One library game.
    pub struct GameObject(ObjectSubclass<imp::GameObject>);
}

impl GameObject {
    /// An object holding `entry`.
    pub fn new(entry: LibraryEntry) -> Self {
        let object: Self = glib::Object::new();
        object.imp().entry.replace(Some(Entry::new(entry)));
        object
    }

    /// The game with its art and its search and sort keys.
    pub fn entry(&self) -> Ref<'_, Entry> {
        Ref::map(self.imp().entry.borrow(), |entry| {
            entry.as_ref().expect("constructed with a game")
        })
    }

    /// The game itself.
    pub fn game(&self) -> Ref<'_, Game> {
        Ref::map(self.entry(), |entry| &entry.game)
    }

    /// The game's art.
    pub fn art(&self) -> Ref<'_, ArtSet> {
        Ref::map(self.entry(), |entry| &entry.art)
    }

    /// The game's ID.
    pub fn id(&self) -> GameId {
        self.game().id.clone()
    }

    /// Whether a game we started is running.
    pub fn is_playing(&self) -> bool {
        self.imp().playing.get()
    }

    /// Sets whether a game we started is running, notifying `playing` when it changes.
    pub fn set_playing(&self, playing: bool) {
        if self.imp().playing.replace(playing) != playing {
            self.notify("playing");
        }
    }

    /// Calls `callback` when the game's art changes.
    pub fn connect_art_changed<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local("art-changed", false, move |values| {
            let object = values[0].get::<Self>().expect("emitted by the game");
            callback(&object);
            None
        })
    }

    /// Replaces the game's data and notifies what changed. Returns whether anything did.
    pub fn update(&self, entry: LibraryEntry) -> bool {
        let (changed, art_changed): (Vec<&str>, bool) = {
            let old = self.entry();
            let (game, art) = (&entry.game, &entry.art);
            if old.game == *game
                && old.art == *art
                && old.playtime_secs == entry.playtime_secs
                && old.playtime_origin == entry.playtime_origin
                && old.collections == entry.collections
            {
                return false;
            }
            let changed = [
                ("title", old.game.title != game.title),
                ("installed", old.game.installed != game.installed),
                ("favourite", old.game.favourite != game.favourite),
                ("hidden", old.game.hidden != game.hidden),
            ]
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect();
            (changed, old.art != *art)
        };
        self.imp().entry.replace(Some(Entry::new(entry)));
        for name in changed {
            self.notify(name);
        }
        if art_changed {
            self.emit_by_name::<()>("art-changed", &[]);
        }
        true
    }
}
