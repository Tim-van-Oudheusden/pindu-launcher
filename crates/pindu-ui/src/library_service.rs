//! Loads the library from the store into the shared [`LibraryModel`], off the main thread.

use gtk::glib;
use gtk::subclass::prelude::*;
use pindu_core::library::{self, LibraryEntry};
use pindu_core::model::{Collection, Game, GameId, LaunchConfig, Timestamp};
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
                Ok(Ok((games, collections))) => {
                    let model = &service.imp().model;
                    model.set_collections(collections);
                    model.set_games(games);
                }
                Ok(Err(error)) => glib::g_warning!("pindu", "Could not read the library: {error}"),
                Err(_) => glib::g_warning!("pindu", "Reading the library stopped unexpectedly"),
            }
        });
    }

    /// Hides or shows a game in the library views.
    pub fn set_hidden(&self, id: &GameId, hidden: bool) {
        self.spawn_edit(id, move |game| game.hidden = hidden);
    }

    /// Removes a game from the library (it stays out while its source lists it), or brings
    /// it back.
    pub fn set_removed(&self, id: &GameId, removed: bool) {
        self.spawn_edit(id, move |game| game.user_removed = removed);
    }

    /// Adds a game to Favourites or takes it out.
    pub fn set_favourite(&self, id: &GameId, favourite: bool) {
        self.spawn_edit(id, move |game| game.favourite = favourite);
    }

    /// Pins a game to "Jump back in" at the first free place; `false` when every place
    /// is taken.
    pub async fn pin(&self, id: &GameId) -> bool {
        let id = id.clone();
        self.write(move |store| {
            store.in_transaction(|store| {
                let Some(place) = library::next_pin(&store.games()?) else {
                    return Ok(false);
                };
                let Some(mut game) = store.game(&id)? else {
                    return Ok(false);
                };
                game.pin_order = Some(place);
                store.update_game(&game)?;
                Ok(true)
            })
        })
        .await
        .unwrap_or(false)
    }

    pub fn unpin(&self, id: &GameId) {
        self.spawn_edit(id, |game| game.pin_order = None);
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

    /// Creates a collection after the others, optionally with a first game, and returns
    /// it once stored.
    pub async fn create_collection(&self, name: &str, game: Option<&GameId>) -> Option<Collection> {
        let (name, game) = (name.to_owned(), game.cloned());
        self.write(move |store| {
            store.in_transaction(|store| {
                let collection = store.create_collection(&name)?;
                if let Some(game) = &game {
                    store.add_to_collection(collection.id, game)?;
                }
                Ok(collection)
            })
        })
        .await
    }

    pub fn rename_collection(&self, id: i64, name: &str) {
        let name = name.to_owned();
        self.spawn_write(move |store| {
            let Some(mut collection) = store.collections()?.into_iter().find(|c| c.id == id) else {
                return Ok(());
            };
            collection.name = name;
            store.update_collection(&collection)
        });
    }

    /// Moves a collection one place up (`-1`) or down (`1`).
    pub fn move_collection(&self, id: i64, step: isize) {
        self.spawn_write(move |store| {
            let mut ids: Vec<i64> = store.collections()?.iter().map(|c| c.id).collect();
            let Some(from) = ids.iter().position(|&candidate| candidate == id) else {
                return Ok(());
            };
            let to = from.saturating_add_signed(step).min(ids.len() - 1);
            ids.swap(from, to);
            store.set_collection_order(&ids)
        });
    }

    /// Deletes a collection and returns what undoing needs: the collection and its games.
    pub async fn delete_collection(&self, id: i64) -> Option<(Collection, Vec<GameId>)> {
        self.write(move |store| {
            store.in_transaction(|store| {
                let collection = store
                    .collections()?
                    .into_iter()
                    .find(|c| c.id == id)
                    .ok_or(store::StoreError::NotFound)?;
                let members = store.collection_members(id)?;
                store.delete_collection(id)?;
                Ok((collection, members))
            })
        })
        .await
    }

    /// Brings back a deleted collection at its old place, with its games.
    pub fn restore_collection(&self, collection: Collection, members: Vec<GameId>) {
        self.spawn_write(move |store| {
            store.in_transaction(|store| {
                let restored = store.create_collection(&collection.name)?;
                for game in &members {
                    store.add_to_collection(restored.id, game)?;
                }
                let mut ids: Vec<i64> = store
                    .collections()?
                    .iter()
                    .map(|c| c.id)
                    .filter(|&id| id != restored.id)
                    .collect();
                let at = (collection.position.max(0) as usize).min(ids.len());
                ids.insert(at, restored.id);
                store.set_collection_order(&ids)
            })
        });
    }

    /// Adds a game to a collection, or takes it out.
    pub fn set_in_collection(&self, collection: i64, game: &GameId, member: bool) {
        let game = game.clone();
        self.spawn_write(move |store| {
            if member {
                store.add_to_collection(collection, &game)
            } else {
                store.remove_from_collection(collection, &game)
            }
        });
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

    /// Runs a store write in the background, then reloads the model.
    fn spawn_write(&self, work: impl FnOnce(&Store) -> store::Result<()> + Send + 'static) {
        let service = self.clone();
        glib::spawn_future_local(async move { service.change(work).await });
    }

    /// Runs a store write on a worker thread and reloads the model; `None` if it failed.
    async fn write<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Store) -> store::Result<T> + Send + 'static,
    ) -> Option<T> {
        let written = gtk::gio::spawn_blocking(move || {
            let store = Store::open(&default_library_db_path())?;
            work(&store)
        })
        .await;
        match written {
            Ok(Ok(value)) => {
                self.reload();
                Some(value)
            }
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not change the library: {error}");
                None
            }
            Err(_) => None,
        }
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

fn read_games() -> Result<(Vec<LibraryEntry>, Vec<Collection>), String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let games = library::load(&store).map_err(|error| error.to_string())?;
    let collections = store.collections().map_err(|error| error.to_string())?;
    Ok((games, collections))
}
