//! Loads the library from the store into the shared [`LibraryModel`], off the main thread.

use gtk::glib;
use gtk::subclass::prelude::*;
use pindu_core::model::Game;
use pindu_core::paths::default_library_db_path;
use pindu_core::store::Store;

use crate::models::LibraryModel;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LibraryService {
        pub model: LibraryModel,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryService {
        const NAME: &'static str = "PinduLibraryService";
        type Type = super::LibraryService;
    }

    impl ObjectImpl for LibraryService {}
}

glib::wrapper! {
    pub struct LibraryService(ObjectSubclass<imp::LibraryService>);
}

impl LibraryService {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The model both shells show.
    pub fn model(&self) -> LibraryModel {
        self.imp().model.clone()
    }

    /// Reads every game from the store on a worker thread and updates the model. A store
    /// that cannot be read leaves the model as it was.
    pub fn reload(&self) {
        let service = self.clone();
        glib::spawn_future_local(async move {
            match gtk::gio::spawn_blocking(read_games).await {
                Ok(Ok(games)) => service.imp().model.set_games(games),
                Ok(Err(error)) => glib::g_warning!("pindu", "Could not read the library: {error}"),
                Err(_) => glib::g_warning!("pindu", "Reading the library stopped unexpectedly"),
            }
        });
    }
}

impl Default for LibraryService {
    fn default() -> Self {
        Self::new()
    }
}

fn read_games() -> Result<Vec<Game>, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    store.games().map_err(|error| error.to_string())
}
