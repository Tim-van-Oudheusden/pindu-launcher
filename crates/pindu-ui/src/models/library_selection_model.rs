//! The library's games as a selection model that follows selection mode: the grid and the
//! list mark the games [`LibrarySelection`] holds as selected, so a screen reader hears a
//! game's tile or row turn selected. Selection mode alone changes the selection; clicks
//! and keys on the grid or the list select nothing.

use std::cell::OnceCell;

use gtk::subclass::prelude::*;
use gtk::{gio, glib, prelude::*};

use super::{GameObject, LibrarySelection};

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LibrarySelectionModel {
        /// The shown games.
        pub model: OnceCell<gio::ListModel>,
        /// Selection mode and the games it selected.
        pub selection: OnceCell<LibrarySelection>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibrarySelectionModel {
        const NAME: &'static str = "PinduLibrarySelectionModel";
        type Type = super::LibrarySelectionModel;
        type Interfaces = (gio::ListModel, gtk::SelectionModel);
    }

    impl ObjectImpl for LibrarySelectionModel {}

    impl ListModelImpl for LibrarySelectionModel {
        fn item_type(&self) -> glib::Type {
            GameObject::static_type()
        }

        fn n_items(&self) -> u32 {
            self.model.get().map_or(0, |model| model.n_items())
        }

        fn item(&self, position: u32) -> Option<glib::Object> {
            self.model.get()?.item(position)
        }
    }

    impl SelectionModelImpl for LibrarySelectionModel {
        fn is_selected(&self, position: u32) -> bool {
            let (Some(selection), Some(game)) = (
                self.selection.get(),
                self.item(position).and_downcast::<GameObject>(),
            ) else {
                return false;
            };
            selection.is_active() && selection.contains(&game.id())
        }
    }
}

glib::wrapper! {
    /// The library's games, selected as selection mode selected them.
    pub struct LibrarySelectionModel(ObjectSubclass<imp::LibrarySelectionModel>)
        @implements gio::ListModel, gtk::SelectionModel;
}

impl LibrarySelectionModel {
    /// `model`'s games, selected while `selection` holds them.
    pub fn new(model: &gio::ListModel, selection: &LibrarySelection) -> Self {
        let this: Self = glib::Object::new();
        let imp = this.imp();
        imp.model.set(model.clone()).expect("set once, here");
        imp.selection
            .set(selection.clone())
            .expect("set once, here");
        model.connect_items_changed(glib::clone!(
            #[weak]
            this,
            move |_, position, removed, added| this.items_changed(position, removed, added)
        ));
        selection.connect_changed(glib::clone!(
            #[weak]
            this,
            move |_| {
                let n_items = this.n_items();
                if n_items > 0 {
                    this.selection_changed(0, n_items);
                }
            }
        ));
        this
    }
}
