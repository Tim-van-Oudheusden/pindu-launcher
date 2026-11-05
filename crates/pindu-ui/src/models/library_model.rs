//! The library pipeline both shells observe: game objects → incremental sort → filter.
//! Sorting works in chunks on the main loop, so loading a big library never blocks a frame.
//! It comes first, so a search or a filter toggle, the frequent changes, only re-checks the
//! filter and never re-sorts. The filter runs in one go: an incremental filter empties the
//! grid on every keystroke and refills it chunk by chunk, which costs far more than checking
//! a few thousand precomputed search keys.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::library::{
    Counts, FilterChange, Filters, LibraryEntry, LibraryFilter, SortMode, View, compare,
};

use pindu_core::model::{Collection, GameId};
use pindu_core::sections::{self, Section};

use super::GameObject;

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct LibraryModel {
        pub games: gio::ListStore,
        pub filter: gtk::CustomFilter,
        pub filtered: gtk::FilterListModel,
        pub sorter: gtk::CustomSorter,
        pub sorted: gtk::SortListModel,
        /// Shared with the filter and sort functions, which run for every game on every
        /// change, so they need no model lookup.
        pub library_filter: Rc<RefCell<LibraryFilter>>,
        pub sort_mode: Rc<Cell<SortMode>>,
        pub counts: RefCell<Counts>,
        /// The user's collections, in their order.
        pub collections: RefCell<Vec<Collection>>,
    }

    impl Default for LibraryModel {
        fn default() -> Self {
            let games = gio::ListStore::new::<GameObject>();
            let sorter = gtk::CustomSorter::default();
            let sorted = gtk::SortListModel::builder()
                .model(&games)
                .sorter(&sorter)
                .incremental(true)
                .build();
            let filter = gtk::CustomFilter::default();
            let filtered = gtk::FilterListModel::builder()
                .model(&sorted)
                .filter(&filter)
                .build();
            Self {
                games,
                filter,
                filtered,
                sorter,
                sorted,
                library_filter: Rc::default(),
                sort_mode: Rc::default(),
                counts: RefCell::default(),
                collections: RefCell::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryModel {
        const NAME: &'static str = "PinduLibraryModel";
        type Type = super::LibraryModel;
    }

    impl ObjectImpl for LibraryModel {
        fn signals() -> &'static [glib::subclass::Signal] {
            static SIGNALS: std::sync::LazyLock<Vec<glib::subclass::Signal>> =
                std::sync::LazyLock::new(|| {
                    vec![
                        glib::subclass::Signal::builder("counts-changed").build(),
                        glib::subclass::Signal::builder("collections-changed").build(),
                    ]
                });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let library_filter = self.library_filter.clone();
            self.filter.set_filter_func(move |item| {
                let Some(game) = item.downcast_ref::<GameObject>() else {
                    return false;
                };
                let entry = game.entry();
                library_filter
                    .borrow()
                    .matches(&entry.game, &entry.collections, &entry.search_key)
            });
            let sort_mode = self.sort_mode.clone();
            self.sorter.set_sort_func(move |a, b| {
                let (Some(a), Some(b)) = (
                    a.downcast_ref::<GameObject>(),
                    b.downcast_ref::<GameObject>(),
                ) else {
                    return gtk::Ordering::Equal;
                };
                compare(sort_mode.get(), &a.entry().sort_key, &b.entry().sort_key).into()
            });
        }
    }
}

glib::wrapper! {
    pub struct LibraryModel(ObjectSubclass<imp::LibraryModel>);
}

impl LibraryModel {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The sorted, filtered games; a `GListModel` of [`GameObject`].
    pub fn model(&self) -> gio::ListModel {
        self.imp().filtered.clone().upcast()
    }

    /// The sort stage, whose `pending` count shows an incremental sort at work.
    pub fn sort_model(&self) -> gtk::SortListModel {
        self.imp().sorted.clone()
    }

    /// How many games each view shows, before any search.
    pub fn counts(&self) -> Counts {
        self.imp().counts.borrow().clone()
    }

    /// Calls `callback` whenever the games change and so may the [`Self::counts`]; the
    /// games are updated by then.
    pub fn connect_counts_changed<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local("counts-changed", false, move |values| {
            let model = values[0].get::<Self>().expect("emitted by the model");
            callback(&model);
            None
        })
    }

    /// Replaces the games. Objects of games that stay are kept and updated in place, so
    /// widgets bound to them stay put.
    pub fn set_games(&self, entries: Vec<LibraryEntry>) {
        let imp = self.imp();
        imp.counts.replace(Counts::of(
            entries
                .iter()
                .map(|entry| (&entry.game, entry.collections.as_slice())),
        ));
        let mut existing: HashMap<String, GameObject> = (0..imp.games.n_items())
            .filter_map(|position| imp.games.item(position).and_downcast::<GameObject>())
            .map(|object| (object.id().as_str().to_owned(), object))
            .collect();
        let same_games = existing.len() == entries.len()
            && entries
                .iter()
                .all(|entry| existing.contains_key(entry.game.id.as_str()));
        if same_games {
            let mut changed = false;
            for entry in entries {
                let object = &existing[entry.game.id.as_str()];
                changed |= object.update(entry);
            }
            if changed {
                imp.filter.changed(gtk::FilterChange::Different);
                imp.sorter.changed(gtk::SorterChange::Different);
            }
            self.emit_by_name::<()>("counts-changed", &[]);
            return;
        }
        let objects: Vec<GameObject> = entries
            .into_iter()
            .map(|entry| match existing.remove(entry.game.id.as_str()) {
                Some(object) => {
                    object.update(entry);
                    object
                }
                None => GameObject::new(entry),
            })
            .collect();
        imp.games.splice(0, imp.games.n_items(), &objects);
        // Observers read the games too, so they hear of new counts once the games are in.
        self.emit_by_name::<()>("counts-changed", &[]);
    }

    /// The "Jump back in" games: pins first, then the most recently played.
    pub fn jump_back_in(&self) -> Vec<GameObject> {
        let games = &self.imp().games;
        let objects: Vec<GameObject> = (0..games.n_items())
            .filter_map(|position| games.item(position).and_downcast::<GameObject>())
            .collect();
        let entries: Vec<_> = objects.iter().map(|object| object.game()).collect();
        let chosen: Vec<GameId> = pindu_core::library::jump_back_in(entries.iter().map(|g| &**g))
            .into_iter()
            .map(|game| game.id.clone())
            .collect();
        drop(entries);
        chosen
            .iter()
            .filter_map(|id| objects.iter().find(|object| object.id() == *id).cloned())
            .collect()
    }

    /// Fullscreen Home's sections (`pindu_core::sections`), each with its games.
    pub fn home_sections(&self) -> Vec<(Section, Vec<GameObject>)> {
        let games = &self.imp().games;
        let objects: Vec<GameObject> = (0..games.n_items())
            .filter_map(|position| games.item(position).and_downcast::<GameObject>())
            .collect();
        let entries: Vec<_> = objects.iter().map(GameObject::entry).collect();
        let pairs: Vec<_> = entries
            .iter()
            .map(|entry| (&entry.game, entry.collections.as_slice()))
            .collect();
        let sections = sections::sections(&pairs, &self.imp().collections.borrow());
        drop(pairs);
        drop(entries);
        let objects: HashMap<GameId, GameObject> = objects
            .into_iter()
            .map(|object| (object.id(), object))
            .collect();
        sections
            .into_iter()
            .map(|section| {
                let games = section
                    .games
                    .iter()
                    .filter_map(|id| objects.get(id).cloned())
                    .collect();
                (section, games)
            })
            .collect()
    }

    /// The user's collections, in their order.
    pub fn collections(&self) -> Vec<Collection> {
        self.imp().collections.borrow().clone()
    }

    /// Replaces the collections, telling observers when they changed.
    pub fn set_collections(&self, collections: Vec<Collection>) {
        if *self.imp().collections.borrow() == collections {
            return;
        }
        self.imp().collections.replace(collections);
        self.emit_by_name::<()>("collections-changed", &[]);
    }

    /// Calls `callback` when collections are added, renamed, reordered or deleted.
    pub fn connect_collections_changed<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local("collections-changed", false, move |values| {
            let model = values[0].get::<Self>().expect("emitted by the model");
            callback(&model);
            None
        })
    }

    /// Every game object, shown or not.
    pub fn all_games(&self) -> Vec<GameObject> {
        let games = &self.imp().games;
        (0..games.n_items())
            .filter_map(|position| games.item(position).and_downcast::<GameObject>())
            .collect()
    }

    /// A game in the library by ID, shown or not.
    pub fn game_object(&self, id: &pindu_core::model::GameId) -> Option<GameObject> {
        let games = &self.imp().games;
        (0..games.n_items())
            .filter_map(|position| games.item(position).and_downcast::<GameObject>())
            .find(|game| game.game().id == *id)
    }

    /// The title of a game in the library, shown or not.
    pub fn game_title(&self, id: &pindu_core::model::GameId) -> Option<String> {
        self.game_object(id).map(|game| game.game().title.clone())
    }

    /// The sidebar view shown.
    pub fn view(&self) -> View {
        self.imp().library_filter.borrow().view()
    }

    /// Shows only the games of a sidebar view.
    pub fn set_view(&self, view: View) {
        let change = self.imp().library_filter.borrow_mut().set_view(view);
        self.apply_filter_change(change);
    }

    /// Shows only the games the filter toggles allow.
    pub fn set_filters(&self, filters: Filters) {
        let change = self.imp().library_filter.borrow_mut().set_filters(filters);
        self.apply_filter_change(change);
    }

    /// Shows only games whose title contains `text`, ignoring case.
    pub fn set_search(&self, text: &str) {
        let change = self.imp().library_filter.borrow_mut().set_search(text);
        self.apply_filter_change(change);
    }

    fn apply_filter_change(&self, change: FilterChange) {
        let change = match change {
            FilterChange::Unchanged => return,
            FilterChange::MoreStrict => gtk::FilterChange::MoreStrict,
            FilterChange::LessStrict => gtk::FilterChange::LessStrict,
            FilterChange::Different => gtk::FilterChange::Different,
        };
        self.imp().filter.changed(change);
    }

    pub fn set_sort_mode(&self, mode: SortMode) {
        let imp = self.imp();
        if imp.sort_mode.replace(mode) != mode {
            imp.sorter.changed(gtk::SorterChange::Different);
        }
    }
}

impl Default for LibraryModel {
    fn default() -> Self {
        Self::new()
    }
}
