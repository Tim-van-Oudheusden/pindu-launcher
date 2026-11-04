//! A library game as a GObject, so list models and widgets can observe it.

use std::cell::{Ref, RefCell};
use std::sync::LazyLock;

use gtk::glib;
use gtk::glib::ParamSpec;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::library::{SearchKey, SortKey};
use pindu_core::model::{Game, GameId};

/// The game plus the keys filtering and sorting need, recomputed when the game changes.
#[derive(Debug)]
pub struct Entry {
    pub game: Game,
    pub search_key: SearchKey,
    pub sort_key: SortKey,
}

impl Entry {
    fn new(game: Game) -> Self {
        Self {
            search_key: SearchKey::new(&game.title),
            sort_key: SortKey::new(&game),
            game,
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct GameObject {
        pub entry: RefCell<Option<Entry>>,
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
                name => unreachable!("unknown property {name}"),
            }
        }
    }
}

glib::wrapper! {
    pub struct GameObject(ObjectSubclass<imp::GameObject>);
}

impl GameObject {
    pub fn new(game: Game) -> Self {
        let object: Self = glib::Object::new();
        object.imp().entry.replace(Some(Entry::new(game)));
        object
    }

    /// The game with its search and sort keys.
    pub fn entry(&self) -> Ref<'_, Entry> {
        Ref::map(self.imp().entry.borrow(), |entry| {
            entry.as_ref().expect("constructed with a game")
        })
    }

    pub fn game(&self) -> Ref<'_, Game> {
        Ref::map(self.entry(), |entry| &entry.game)
    }

    pub fn id(&self) -> GameId {
        self.game().id.clone()
    }

    /// Replaces the game's data and notifies the properties that changed. Returns whether
    /// anything changed.
    pub fn update(&self, game: Game) -> bool {
        let changed: Vec<&str> = {
            let old = self.game();
            if *old == game {
                return false;
            }
            [
                ("title", old.title != game.title),
                ("installed", old.installed != game.installed),
                ("favourite", old.favourite != game.favourite),
                ("hidden", old.hidden != game.hidden),
            ]
            .into_iter()
            .filter_map(|(name, changed)| changed.then_some(name))
            .collect()
        };
        self.imp().entry.replace(Some(Entry::new(game)));
        for name in changed {
            self.notify(name);
        }
        true
    }
}
