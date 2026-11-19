//! Loads the library from the store into the shared [`LibraryModel`], off the main thread.

use std::path::PathBuf;

use crate::config::APP_ID;
use gtk::glib;
use gtk::subclass::prelude::*;
use pindu_core::backup::{self, BackupError, ImportSummary};
use pindu_core::bulk::{self, BulkAction};
use pindu_core::game_groups;
use pindu_core::library::{self, LibraryEntry};
use pindu_core::manual;
use pindu_core::model::{
    ArtLayout, ArtOrigin, Collection, Game, GameId, LaunchConfig, PerformanceTool, Source,
    Timestamp,
};
use pindu_core::paths::{default_library_db_path, default_user_art_dir};
use pindu_core::store::{self, Store};

use crate::file_service::PickedImage;
use crate::models::LibraryModel;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct LibraryService {
        pub model: LibraryModel,
        /// Whether the library was read once.
        pub loaded: std::cell::Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for LibraryService {
        const NAME: &'static str = "PinduLibraryService";
        type Type = super::LibraryService;
    }

    impl ObjectImpl for LibraryService {}
}

glib::wrapper! {
    /// Loads the library into the shared model and writes the user's changes to the store.
    pub struct LibraryService(ObjectSubclass<imp::LibraryService>);
}

impl LibraryService {
    /// A service with an empty model; [`reload`](Self::reload) fills it.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// The model both shells show.
    pub fn model(&self) -> LibraryModel {
        self.imp().model.clone()
    }

    /// Waits until the library was read once (at most a few seconds).
    pub async fn wait_loaded(&self) {
        for _ in 0..100 {
            if self.imp().loaded.get() {
                return;
            }
            glib::timeout_future(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Reads every game from the store on a worker thread and updates the model. A store
    /// that cannot be read leaves the model as it was.
    pub fn reload(&self) {
        let service = self.clone();
        let prefer_steamgriddb = gtk::gio::prelude::SettingsExt::boolean(
            &gtk::gio::Settings::new(APP_ID),
            "prefer-steamgriddb",
        );
        glib::spawn_future_local(async move {
            match gtk::gio::spawn_blocking(move || read_games(prefer_steamgriddb)).await {
                Ok(Ok((games, collections))) => {
                    let model = &service.imp().model;
                    model.set_collections(collections);
                    model.set_games(games);
                    service.imp().loaded.set(true);
                }
                Ok(Err(error)) => glib::g_warning!("pindu", "Could not read the library: {error}"),
                Err(_) => glib::g_warning!("pindu", "Reading the library stopped unexpectedly"),
            }
        });
    }

    /// Hides or shows a game, and every copy of its group, in the library views.
    pub fn set_hidden(&self, id: &GameId, hidden: bool) {
        self.spawn_edit_copies(id, move |game| game.hidden = hidden);
    }

    /// Removes a game, and every copy of its group, from the library (it stays out while
    /// its source lists it), or brings it back.
    pub fn set_removed(&self, id: &GameId, removed: bool) {
        self.spawn_edit_copies(id, move |game| game.user_removed = removed);
    }

    /// Adds a game, and every copy of its group, to Favourites or takes it out.
    pub fn set_favourite(&self, id: &GameId, favourite: bool) {
        self.spawn_edit_copies(id, move |game| game.favourite = favourite);
    }

    /// Pins a game to "Jump back in" at the first free place; `false` when every place
    /// is taken. A place holds one game, so a group's pin sits on the copy given.
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

    /// Takes a game, and every copy of its group, out of "Jump back in".
    pub fn unpin(&self, id: &GameId) {
        self.spawn_edit_copies(id, |game| game.pin_order = None);
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

    /// Creates a collection after the others, optionally with a first game (every copy of
    /// its group), and returns it once stored.
    pub async fn create_collection(&self, name: &str, game: Option<&GameId>) -> Option<Collection> {
        let (name, game) = (name.to_owned(), game.cloned());
        self.write(move |store| {
            store.in_transaction(|store| {
                let collection = store.create_collection(&name)?;
                if let Some(game) = &game {
                    for copy in game_groups::copies_of(store, game)? {
                        store.add_to_collection(collection.id, &copy)?;
                    }
                }
                Ok(collection)
            })
        })
        .await
    }

    /// Renames a collection.
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

    /// Adds a game, and every copy of its group, to a collection, or takes it out.
    pub fn set_in_collection(&self, collection: i64, game: &GameId, member: bool) {
        let game = game.clone();
        self.spawn_write(move |store| {
            store.in_transaction(|store| {
                for copy in game_groups::copies_of(store, &game)? {
                    if member {
                        store.add_to_collection(collection, &copy)?;
                    } else {
                        store.remove_from_collection(collection, &copy)?;
                    }
                }
                Ok(())
            })
        });
    }

    /// Applies `action` to `games` in one go and returns the games it changed, which
    /// undoing applies the opposite action to; `None` if the store could not be changed.
    pub async fn apply_bulk(&self, games: Vec<GameId>, action: BulkAction) -> Option<Vec<GameId>> {
        self.write(move |store| bulk::apply(store, &games, action))
            .await
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

    /// The folder the last sync read `source` from, read on a worker thread; `None` when
    /// it found none or the store cannot be read.
    pub async fn source_location(&self, source: Source) -> Option<PathBuf> {
        let read = gtk::gio::spawn_blocking(move || {
            Store::open(&default_library_db_path())?.source_state(source)
        })
        .await;
        match read {
            Ok(Ok(state)) => state.and_then(|state| state.resolved_location),
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not read where {source} is: {error}");
                None
            }
            Err(_) => None,
        }
    }

    /// Changes a game's stored launch config in the background. The library model holds
    /// no launch configs, so nothing reloads.
    pub fn edit_launch_config(
        &self,
        id: &GameId,
        edit: impl FnOnce(&mut LaunchConfig) + Send + 'static,
    ) {
        let id = id.clone();
        glib::spawn_future_local(async move {
            let written = gtk::gio::spawn_blocking(move || {
                let store = Store::open(&default_library_db_path())?;
                store.in_transaction(|store| {
                    let Some(mut config) = store.launch_config(&id)? else {
                        return Ok(());
                    };
                    edit(&mut config);
                    store.set_launch_config(&id, &config)
                })
            })
            .await;
            match written {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    glib::g_warning!("pindu", "Could not save a launch config: {error}");
                }
                Err(_) => glib::g_warning!("pindu", "Saving a launch config stopped unexpectedly"),
            }
        });
    }

    /// Switches one performance tool of a game's stored launch config on or off in the
    /// background, from its next launch; the rest of its config stays.
    pub fn set_performance_tool(&self, id: &GameId, tool: PerformanceTool, on: bool) {
        let id = id.clone();
        glib::spawn_future_local(async move {
            let written = gtk::gio::spawn_blocking(move || {
                Store::open(&default_library_db_path())?.set_performance_tool(&id, tool, on)
            })
            .await;
            match written {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    glib::g_warning!("pindu", "Could not save a performance switch: {error}");
                }
                Err(_) => {
                    glib::g_warning!("pindu", "Saving a performance switch stopped unexpectedly")
                }
            }
        });
    }

    /// Makes `image` the game's own art in `layout` at once, then reloads the model.
    pub async fn set_user_art(
        &self,
        id: &GameId,
        layout: ArtLayout,
        image: PickedImage,
    ) -> Result<(), String> {
        let id = id.clone();
        self.change_art(move |store| {
            manual::set_user_art(
                store,
                &default_user_art_dir(),
                &id,
                layout,
                &image.extension,
                &image.bytes,
            )
            .map(drop)
        })
        .await
    }

    /// Removes the game's own art in `layout`, so the launcher's or SteamGridDB's shows,
    /// then reloads the model.
    pub async fn remove_user_art(&self, id: &GameId, layout: ArtLayout) -> Result<(), String> {
        let id = id.clone();
        self.change_art(move |store| manual::remove_user_art(store, &id, layout))
            .await
    }

    /// The layouts the user gave a game its own art in.
    pub async fn user_art_layouts(&self, id: &GameId) -> Vec<ArtLayout> {
        let id = id.clone();
        self.read(move |store| {
            Ok(store
                .artwork(&id)?
                .into_iter()
                .filter(|art| art.origin == ArtOrigin::User)
                .map(|art| art.layout)
                .collect())
        })
        .await
        .unwrap_or_default()
    }

    /// The SteamGridDB game the user chose for a game, if they chose one.
    pub async fn steamgriddb_match(&self, id: &GameId) -> Option<u64> {
        let id = id.clone();
        self.read(move |store| store.steamgriddb_match(&id))
            .await
            .flatten()
    }

    /// Remembers the SteamGridDB game the user chose for a game.
    pub async fn set_steamgriddb_match(&self, id: &GameId, grid_id: u64) {
        let id = id.clone();
        self.change(move |store| store.set_steamgriddb_match(&id, grid_id))
            .await;
    }

    /// Changes a game's art on a worker thread, then reloads the model.
    async fn change_art(
        &self,
        work: impl FnOnce(&Store) -> Result<(), manual::ManualError> + Send + 'static,
    ) -> Result<(), String> {
        let written = gtk::gio::spawn_blocking(move || {
            let store = Store::open(&default_library_db_path()).map_err(|e| e.to_string())?;
            work(&store).map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|_| Err("changing the art stopped unexpectedly".into()));
        if written.is_ok() {
            self.reload();
        }
        written
    }

    /// Reads from the store on a worker thread; `None` when it cannot be read.
    async fn read<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Store) -> store::Result<T> + Send + 'static,
    ) -> Option<T> {
        let read = gtk::gio::spawn_blocking(move || {
            let store = Store::open(&default_library_db_path())?;
            work(&store)
        })
        .await;
        match read {
            Ok(Ok(value)) => Some(value),
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not read the library: {error}");
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

    /// Changes a game and every copy of its group in the background: a group is a view on
    /// its copies, so a change made on it is written to each.
    fn spawn_edit_copies(&self, id: &GameId, edit: impl Fn(&mut Game) + Send + 'static) {
        let id = id.clone();
        self.spawn_write(move |store| {
            store.in_transaction(|store| {
                for copy in game_groups::copies_of(store, &id)? {
                    if let Some(mut game) = store.game(&copy)? {
                        edit(&mut game);
                        store.update_game(&game)?;
                    }
                }
                Ok(())
            })
        });
    }

    /// Writes a backup of what the user made here to `path`, and records `now` as the time
    /// of the last backup once it is written.
    pub async fn export_backup(&self, path: PathBuf, now: Timestamp) -> Result<(), String> {
        gtk::gio::spawn_blocking(move || {
            let store =
                Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
            let text = backup::export(&store, now).map_err(|error| error.to_string())?;
            std::fs::write(&path, text).map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|_| Err("the export stopped unexpectedly".into()))?;
        let settings = gtk::gio::Settings::new(APP_ID);
        if let Err(error) = gtk::gio::prelude::SettingsExt::set_int64(&settings, "last-backup", now)
        {
            glib::g_warning!("pindu", "Could not record the backup time: {error}");
        }
        Ok(())
    }

    /// When the library was last backed up (exported), if it ever was; the last-backup
    /// input of the Library Health report's Library data section.
    pub fn last_backup(&self) -> Option<Timestamp> {
        backup::last_backup(gtk::gio::prelude::SettingsExt::int64(
            &gtk::gio::Settings::new(APP_ID),
            "last-backup",
        ))
    }

    /// Restores the backup at `path` into the library, then reloads the model.
    pub async fn import_backup(&self, path: PathBuf) -> Result<ImportSummary, ImportFailure> {
        let imported = gtk::gio::spawn_blocking(move || {
            let text = std::fs::read_to_string(&path).map_err(|error| match error.kind() {
                std::io::ErrorKind::InvalidData => ImportFailure::NotABackup,
                _ => ImportFailure::Failed(error.to_string()),
            })?;
            let store = Store::open(&default_library_db_path())
                .map_err(|error| ImportFailure::Failed(error.to_string()))?;
            backup::import(&store, &text).map_err(|error| match error {
                BackupError::NotABackup => ImportFailure::NotABackup,
                BackupError::TooNew { .. } => ImportFailure::TooNew,
                BackupError::Store(error) => ImportFailure::Failed(error.to_string()),
            })
        })
        .await
        .unwrap_or_else(|_| {
            Err(ImportFailure::Failed(
                "the import stopped unexpectedly".into(),
            ))
        });
        if imported.is_ok() {
            self.reload();
        }
        imported
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

fn read_games(prefer_steamgriddb: bool) -> Result<(Vec<LibraryEntry>, Vec<Collection>), String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let games = library::load(&store, prefer_steamgriddb).map_err(|error| error.to_string())?;
    let collections = store.collections().map_err(|error| error.to_string())?;
    Ok((games, collections))
}

/// Why a backup could not be imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportFailure {
    /// The file is not a backup of ours (or not text at all).
    NotABackup,
    /// Written by a newer version of the app.
    TooNew,
    /// The import failed; the message says why.
    Failed(String),
}
