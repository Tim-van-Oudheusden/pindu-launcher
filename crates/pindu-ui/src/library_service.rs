//! Loads the library from the store into the shared [`LibraryModel`], off the main thread.

use gtk::glib;
use gtk::subclass::prelude::*;
use pindu_core::library::{self, LibraryEntry};
use pindu_core::model::{Game, GameId, LaunchConfig, Timestamp};
use pindu_core::paths::default_library_db_path;
use pindu_core::store::{self, Store};

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

    /// Hides or shows a game in the library views.
    pub fn set_hidden(&self, id: &GameId, hidden: bool) {
        self.spawn_edit(id, move |game| game.hidden = hidden);
    }

    /// Records that a game was just started; done once the store has it.
    pub async fn mark_played(&self, id: &GameId, now: Timestamp) {
        self.edit_game(id, move |game| game.last_played_at = Some(now))
            .await;
    }

    /// Whether a Steam game starts through Steam's launch-options dialog.
    pub fn set_steam_launch_dialog(&self, id: &GameId, dialog: bool) {
        self.spawn_edit(id, move |game| game.steam_launch_dialog = dialog);
    }

    /// The game's launch config, read on a worker thread; `None` when there is none or
    /// the store cannot be read.
    pub async fn launch_config(&self, id: &GameId) -> Option<LaunchConfig> {
        let id = id.clone();
        let read = gtk::gio::spawn_blocking(move || {
            Store::open(&default_library_db_path())?.launch_config(&id)
        })
        .await;
        match read {
            Ok(Ok(config)) => config,
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not read a launch config: {error}");
                None
            }
            Err(_) => None,
        }
    }

    /// Changes one stored game in the background.
    fn spawn_edit(&self, id: &GameId, edit: impl FnOnce(&mut Game) + Send + 'static) {
        let (service, id) = (self.clone(), id.clone());
        glib::spawn_future_local(async move { service.edit_game(&id, edit).await });
    }

    /// Changes one stored game on a worker thread, then reloads the model.
    async fn edit_game(&self, id: &GameId, edit: impl FnOnce(&mut Game) + Send + 'static) {
        let id = id.clone();
        self.change(move |store| {
            let Some(mut game) = store.game(&id)? else {
                return Ok(());
            };
            edit(&mut game);
            store.update_game(&game)
        })
        .await;
    }

    /// Writes to the store on a worker thread, then reloads the model.
    async fn change(&self, work: impl FnOnce(&Store) -> store::Result<()> + Send + 'static) {
        let written = gtk::gio::spawn_blocking(move || {
            let store =
                Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
            work(&store).map_err(|error| error.to_string())
        })
        .await;
        match written {
            Ok(Ok(())) => self.reload(),
            Ok(Err(error)) => glib::g_warning!("pindu", "Could not change the library: {error}"),
            Err(_) => glib::g_warning!("pindu", "Changing the library stopped unexpectedly"),
        }
    }
}

impl Default for LibraryService {
    fn default() -> Self {
        Self::new()
    }
}

fn read_games() -> Result<Vec<LibraryEntry>, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    library::load(&store).map_err(|error| error.to_string())
}
