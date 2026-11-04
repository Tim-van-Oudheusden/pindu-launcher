//! The library pipeline both shells observe: game objects → incremental filter → incremental
//! sort. Filtering and sorting work in chunks on the main loop, so a big library never
//! blocks a frame.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::library::{
    Counts, FilterChange, LibraryEntry, LibraryFilter, SortMode, View, compare,
};

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
        pub library_filter: RefCell<LibraryFilter>,
        pub sort_mode: Cell<SortMode>,
        pub counts: RefCell<Counts>,
    }

    impl Default for LibraryModel {
        fn default() -> Self {
            let games = gio::ListStore::new::<GameObject>();
            let filter = gtk::CustomFilter::default();
            let filtered = gtk::FilterListModel::builder()
                .model(&games)
                .filter(&filter)
                .incremental(true)
                .build();
            let sorter = gtk::CustomSorter::default();
            let sorted = gtk::SortListModel::builder()
                .model(&filtered)
                .sorter(&sorter)
                .incremental(true)
                .build();
            Self {
                games,
                filter,
                filtered,
                sorter,
                sorted,
                library_filter: RefCell::default(),
                sort_mode: Cell::default(),
                counts: RefCell::default(),
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
                    vec![glib::subclass::Signal::builder("counts-changed").build()]
                });
            SIGNALS.as_ref()
        }

        fn constructed(&self) {
            self.parent_constructed();
            let model = self.obj().downgrade();
            self.filter.set_filter_func(move |item| {
                let (Some(model), Some(game)) =
                    (model.upgrade(), item.downcast_ref::<GameObject>())
                else {
                    return false;
                };
                let entry = game.entry();
                model
                    .imp()
                    .library_filter
                    .borrow()
                    .matches(&entry.game, &entry.search_key)
            });
            let model = self.obj().downgrade();
            self.sorter.set_sort_func(move |a, b| {
                let (Some(model), Some(a), Some(b)) = (
                    model.upgrade(),
                    a.downcast_ref::<GameObject>(),
                    b.downcast_ref::<GameObject>(),
                ) else {
                    return gtk::Ordering::Equal;
                };
                compare(
                    model.imp().sort_mode.get(),
                    &a.entry().sort_key,
                    &b.entry().sort_key,
                )
                .into()
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

    /// The filtered, sorted games; a `GListModel` of [`GameObject`].
    pub fn model(&self) -> gio::ListModel {
        self.imp().sorted.clone().upcast()
    }

    /// The filter stage, whose `pending` count shows an incremental filter at work.
    pub fn filter_model(&self) -> gtk::FilterListModel {
        self.imp().filtered.clone()
    }

    /// The sort stage, whose `pending` count shows an incremental sort at work.
    pub fn sort_model(&self) -> gtk::SortListModel {
        self.imp().sorted.clone()
    }

    /// How many games each view shows, before any search.
    pub fn counts(&self) -> Counts {
        self.imp().counts.borrow().clone()
    }

    /// Calls `callback` whenever the games change and so may the [`Self::counts`].
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
        imp.counts
            .replace(Counts::of(entries.iter().map(|entry| &entry.game)));
        self.emit_by_name::<()>("counts-changed", &[]);
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
    }

    /// The title of a game in the library, shown or not.
    pub fn game_title(&self, id: &pindu_core::model::GameId) -> Option<String> {
        let games = &self.imp().games;
        (0..games.n_items())
            .filter_map(|position| games.item(position).and_downcast::<GameObject>())
            .find(|game| game.game().id == *id)
            .map(|game| game.game().title.clone())
    }

    /// Shows only the games of a sidebar view.
    pub fn set_view(&self, view: View) {
        let change = self.imp().library_filter.borrow_mut().set_view(view);
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
