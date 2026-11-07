//! A JSON backup of what the user made here and exists nowhere else: collections,
//! favourites, hidden games, games added by hand and the playtime tracked here.
//!
//! Games are named by ID, so a backup restores into a library where the launchers have
//! been synced again; a game the library does not have is skipped and counted.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::PathBuf;

use serde_json::{Map, Value, json};

use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Source, Timestamp};
use crate::store::{Store, StoreError};

/// What the `format` field of a backup says.
const FORMAT: &str = "pindu-launcher-backup";
/// The backup version this build writes and the newest it reads.
const VERSION: u64 = 1;

/// Why a backup could not be written or restored.
#[derive(Debug)]
pub enum BackupError {
    /// The file is not JSON, or not a backup of ours.
    NotABackup,
    /// Written by a newer version of the app.
    TooNew {
        /// The backup's format version.
        version: u64,
    },
    /// The library could not be read or written.
    Store(StoreError),
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotABackup => write!(f, "the file is not a library backup"),
            Self::TooNew { version } => {
                write!(f, "the backup is from a newer version (format {version})")
            }
            Self::Store(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for BackupError {}

impl From<StoreError> for BackupError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

/// What an import restored.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportSummary {
    /// Games added by hand that the library did not have yet.
    pub manual_games: usize,
    /// Games newly marked as favourites.
    pub favourites: usize,
    /// Games newly hidden.
    pub hidden: usize,
    /// Collections created; existing ones with the same name are reused, not counted.
    pub collections: usize,
    /// Play sessions not already recorded.
    pub sessions: usize,
    /// Games the backup names that this library does not have.
    pub missing_games: usize,
}

/// The library's backup as JSON text.
pub fn export(store: &Store, now: Timestamp) -> Result<String, BackupError> {
    let games = store.games()?;
    let ids = |keep: fn(&Game) -> bool| -> Vec<&str> {
        games
            .iter()
            .filter(|game| keep(game))
            .map(|game| game.id.as_str())
            .collect()
    };
    let mut collections = Vec::new();
    for collection in store.collections()? {
        let members: Vec<String> = store
            .collection_members(collection.id)?
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        collections.push(json!({ "name": collection.name, "games": members }));
    }
    let mut manual_games = Vec::new();
    let mut sessions = Vec::new();
    for game in &games {
        if game.id.source() == Source::Manual {
            let launch = store.launch_config(&game.id)?;
            manual_games.push(manual_game_json(game, launch.as_ref()));
        }
        for session in store.play_sessions(&game.id)? {
            sessions.push(json!({
                "game": game.id.as_str(),
                "started_at": session.started_at,
                "ended_at": session.ended_at,
            }));
        }
    }
    let backup = json!({
        "format": FORMAT,
        "version": VERSION,
        "exported_at": now,
        "collections": collections,
        "favourites": ids(|game| game.favourite),
        "hidden": ids(|game| game.hidden),
        "manual_games": manual_games,
        "play_sessions": sessions,
    });
    Ok(serde_json::to_string_pretty(&backup).expect("JSON values always serialize"))
}

/// Restores a backup into the library, in one transaction. It adds and never removes:
/// favourites and hidden games stay as they are unless the backup marks them, existing
/// collections gain members, and sessions or games already here are not added twice.
pub fn import(store: &Store, text: &str) -> Result<ImportSummary, BackupError> {
    let backup: Value = serde_json::from_str(text).map_err(|_| BackupError::NotABackup)?;
    if backup["format"] != FORMAT {
        return Err(BackupError::NotABackup);
    }
    let version = backup["version"].as_u64().ok_or(BackupError::NotABackup)?;
    if version > VERSION {
        return Err(BackupError::TooNew { version });
    }
    let mut summary = ImportSummary::default();
    store.in_transaction(|store| {
        let mut missing = BTreeSet::new();
        for entry in list(&backup["manual_games"]) {
            let Some((game, launch)) = manual_game_from(entry) else {
                continue;
            };
            if store.game(&game.id)?.is_none() {
                store.insert_game(&game)?;
                if let Some(launch) = launch {
                    store.set_launch_config(&game.id, &launch)?;
                }
                summary.manual_games += 1;
            }
        }
        let mut games: HashMap<GameId, Game> = store
            .games()?
            .into_iter()
            .map(|game| (game.id.clone(), game))
            .collect();
        let ids: HashSet<GameId> = games.keys().cloned().collect();
        let known = |value: &Value, missing: &mut BTreeSet<String>| known(&ids, value, missing);
        for id in list(&backup["favourites"]).filter_map(|v| known(v, &mut missing)) {
            if let Some(game) = games.get_mut(&id).filter(|game| !game.favourite) {
                game.favourite = true;
                store.update_game(game)?;
                summary.favourites += 1;
            }
        }
        for id in list(&backup["hidden"]).filter_map(|v| known(v, &mut missing)) {
            if let Some(game) = games.get_mut(&id).filter(|game| !game.hidden) {
                game.hidden = true;
                store.update_game(game)?;
                summary.hidden += 1;
            }
        }
        let mut collections: HashMap<String, i64> = store
            .collections()?
            .into_iter()
            .map(|collection| (collection.name, collection.id))
            .collect();
        for entry in list(&backup["collections"]) {
            let Some(name) = entry["name"]
                .as_str()
                .filter(|name| !name.trim().is_empty())
            else {
                continue;
            };
            let id = match collections.get(name) {
                Some(&id) => id,
                None => {
                    let collection = store.create_collection(name)?;
                    collections.insert(collection.name, collection.id);
                    summary.collections += 1;
                    collection.id
                }
            };
            for game in list(&entry["games"]).filter_map(|v| known(v, &mut missing)) {
                store.add_to_collection(id, &game)?;
            }
        }
        for entry in list(&backup["play_sessions"]) {
            let (Some(game), Some(started_at), Some(ended_at)) = (
                known(&entry["game"], &mut missing),
                entry["started_at"].as_i64(),
                entry["ended_at"].as_i64(),
            ) else {
                continue;
            };
            let recorded = store
                .play_sessions(&game)?
                .iter()
                .any(|s| s.started_at == started_at && s.ended_at == ended_at);
            if !recorded && ended_at >= started_at {
                store.add_play_session(&game, started_at, ended_at)?;
                summary.sessions += 1;
            }
        }
        summary.missing_games = missing.len();
        Ok(())
    })?;
    Ok(summary)
}

/// The game a backup names, if this library has it; otherwise it is noted as missing.
fn known(ids: &HashSet<GameId>, value: &Value, missing: &mut BTreeSet<String>) -> Option<GameId> {
    let text = value.as_str()?;
    match text.parse::<GameId>() {
        Ok(id) if ids.contains(&id) => Some(id),
        _ => {
            missing.insert(text.to_owned());
            None
        }
    }
}

/// The items of a JSON array; nothing for anything else.
fn list(value: &Value) -> impl Iterator<Item = &Value> {
    value.as_array().into_iter().flatten()
}

fn manual_game_json(game: &Game, launch: Option<&LaunchConfig>) -> Value {
    let path = |path: &Option<PathBuf>| path.as_ref().map(|p| p.to_string_lossy().into_owned());
    let launch = launch.map(|launch| {
        json!({
            "executable": launch.executable.to_string_lossy(),
            "arguments": launch.arguments,
            "working_dir": path(&launch.working_dir),
            "environment": launch.environment,
            "proton_version": launch.proton_version,
            "prefix_path": path(&launch.prefix_path),
            "mangohud": launch.mangohud,
            "gamemode": launch.gamemode,
            "wrappers": launch.wrappers,
        })
    });
    json!({
        "id": game.id.as_str(),
        "title": game.title,
        "install_path": path(&game.install_path),
        "added_at": game.added_at,
        "last_played_at": game.last_played_at,
        "launch": launch,
    })
}

/// A game added by hand and how it starts, if the backup says.
fn manual_game_from(entry: &Value) -> Option<(Game, Option<LaunchConfig>)> {
    let id: GameId = entry["id"].as_str()?.parse().ok()?;
    if id.source() != Source::Manual {
        return None;
    }
    let title = entry["title"].as_str()?.trim();
    if title.is_empty() {
        return None;
    }
    let mut game = Game::new(
        id,
        title,
        LaunchKind::HostCommand,
        entry["added_at"].as_i64()?,
    );
    game.install_path = path(&entry["install_path"]);
    game.installed = true;
    game.last_played_at = entry["last_played_at"].as_i64();
    let launch = &entry["launch"];
    if launch.is_null() {
        return Some((game, None));
    }
    let strings = |value: &Value| -> Vec<String> {
        list(value)
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect()
    };
    let environment: BTreeMap<String, String> = launch["environment"]
        .as_object()
        .map(Map::iter)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
        .collect();
    let config = LaunchConfig {
        executable: path(&launch["executable"])?,
        arguments: strings(&launch["arguments"]),
        working_dir: path(&launch["working_dir"]),
        environment,
        proton_version: launch["proton_version"].as_str().map(str::to_owned),
        prefix_path: path(&launch["prefix_path"]),
        mangohud: launch["mangohud"].as_bool().unwrap_or(false),
        gamemode: launch["gamemode"].as_bool().unwrap_or(false),
        wrappers: strings(&launch["wrappers"]),
    };
    Some((game, Some(config)))
}

fn path(value: &Value) -> Option<PathBuf> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Source};
    use crate::store::Store;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        (dir, store)
    }

    fn steam(native_id: &str, title: &str) -> Game {
        Game::new(
            GameId::new(Source::Steam, native_id),
            title,
            LaunchKind::StoreUri,
            1_700_000_000,
        )
    }

    fn manual_game() -> (Game, LaunchConfig) {
        let mut game = Game::new(
            GameId::new(Source::Manual, "my-game-1"),
            "My Game",
            LaunchKind::HostCommand,
            1_700_000_100,
        );
        game.install_path = Some(PathBuf::from("/games/my-game"));
        let config = LaunchConfig {
            executable: PathBuf::from("/games/my-game/run.sh"),
            arguments: vec!["--fullscreen".into(), "two words".into()],
            working_dir: Some(PathBuf::from("/games/my-game")),
            gamemode: true,
            ..LaunchConfig::default()
        };
        (game, config)
    }

    /// A library as a launcher sync leaves it, before the user did anything.
    fn synced_store() -> (tempfile::TempDir, Store) {
        let (dir, store) = temp_store();
        for game in [
            steam("570", "Dota 2"),
            steam("620", "Portal 2"),
            steam("400", "Portal"),
        ] {
            store.insert_game(&game).unwrap();
        }
        (dir, store)
    }

    #[test]
    fn a_backup_restores_what_the_user_made_into_a_freshly_synced_library() {
        let (_dir, store) = synced_store();
        let dota = GameId::new(Source::Steam, "570");
        let portal2 = GameId::new(Source::Steam, "620");
        let portal = GameId::new(Source::Steam, "400");
        let mut game = store.game(&dota).unwrap().unwrap();
        game.favourite = true;
        store.update_game(&game).unwrap();
        let mut game = store.game(&portal).unwrap().unwrap();
        game.hidden = true;
        store.update_game(&game).unwrap();
        let puzzles = store.create_collection("Puzzles").unwrap();
        store.add_to_collection(puzzles.id, &portal2).unwrap();
        store.add_to_collection(puzzles.id, &portal).unwrap();
        store.create_collection("Later").unwrap();
        let (manual, config) = manual_game();
        store.insert_game(&manual).unwrap();
        store.set_launch_config(&manual.id, &config).unwrap();
        store
            .add_play_session(&manual.id, 1_700_001_000, 1_700_004_600)
            .unwrap();
        store
            .add_play_session(&dota, 1_700_002_000, 1_700_002_600)
            .unwrap();

        let backup = export(&store, 1_700_009_000).unwrap();
        let (_fresh_dir, fresh) = synced_store();
        import(&fresh, &backup).unwrap();

        assert!(fresh.game(&dota).unwrap().unwrap().favourite);
        assert!(fresh.game(&portal).unwrap().unwrap().hidden);
        assert!(!fresh.game(&portal2).unwrap().unwrap().favourite);
        let collections = fresh.collections().unwrap();
        let names: Vec<_> = collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Puzzles", "Later"]);
        assert_eq!(
            fresh.collection_members(collections[0].id).unwrap(),
            [portal.clone(), portal2.clone()]
        );
        let restored = fresh.game(&manual.id).unwrap().unwrap();
        assert_eq!(restored.title, "My Game");
        assert_eq!(restored.launch_kind, LaunchKind::HostCommand);
        assert_eq!(restored.install_path, manual.install_path);
        assert_eq!(fresh.launch_config(&manual.id).unwrap(), Some(config));
        let sessions: Vec<_> = [&manual.id, &dota]
            .iter()
            .flat_map(|id| fresh.play_sessions(id).unwrap())
            .map(|s| (s.game_id, s.started_at, s.ended_at))
            .collect();
        assert_eq!(
            sessions,
            [
                (manual.id.clone(), 1_700_001_000, 1_700_004_600),
                (dota.clone(), 1_700_002_000, 1_700_002_600),
            ]
        );
    }

    #[test]
    fn importing_a_backup_twice_adds_nothing_the_second_time() {
        let (_dir, store) = synced_store();
        let dota = GameId::new(Source::Steam, "570");
        let mut game = store.game(&dota).unwrap().unwrap();
        game.favourite = true;
        store.update_game(&game).unwrap();
        let collection = store.create_collection("Mine").unwrap();
        store.add_to_collection(collection.id, &dota).unwrap();
        store.add_play_session(&dota, 100, 200).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        let first = import(&fresh, &backup).unwrap();
        let second = import(&fresh, &backup).unwrap();

        assert_eq!(
            first,
            ImportSummary {
                favourites: 1,
                collections: 1,
                sessions: 1,
                ..ImportSummary::default()
            }
        );
        assert_eq!(second, ImportSummary::default());
        assert_eq!(fresh.collections().unwrap().len(), 1);
        assert_eq!(fresh.play_sessions(&dota).unwrap().len(), 1);
    }

    #[test]
    fn games_the_library_lacks_are_skipped_and_counted() {
        let (_dir, store) = synced_store();
        let gone = steam("999", "Uninstalled Long Ago");
        store.insert_game(&gone).unwrap();
        let mut game = gone.clone();
        game.favourite = true;
        store.update_game(&game).unwrap();
        let collection = store.create_collection("Old").unwrap();
        store.add_to_collection(collection.id, &gone.id).unwrap();
        store.add_play_session(&gone.id, 100, 200).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        let summary = import(&fresh, &backup).unwrap();

        assert_eq!(summary.missing_games, 1);
        assert_eq!((summary.favourites, summary.sessions), (0, 0));
        assert!(fresh.game(&gone.id).unwrap().is_none());
        let old = &fresh.collections().unwrap()[0];
        assert!(fresh.collection_members(old.id).unwrap().is_empty());
    }

    #[test]
    fn only_backups_this_version_can_read_are_imported() {
        let (_dir, store) = synced_store();

        assert!(matches!(
            import(&store, "not json"),
            Err(BackupError::NotABackup)
        ));
        assert!(matches!(
            import(&store, r#"{"format": "something-else", "version": 1}"#),
            Err(BackupError::NotABackup)
        ));
        assert!(matches!(
            import(
                &store,
                r#"{"format": "pindu-launcher-backup", "version": 2}"#
            ),
            Err(BackupError::TooNew { version: 2 })
        ));
        assert_eq!(
            import(
                &store,
                r#"{"format": "pindu-launcher-backup", "version": 1}"#
            )
            .unwrap(),
            ImportSummary::default()
        );
    }

    #[test]
    fn a_game_added_by_hand_without_a_program_is_restored_as_it_was() {
        let (_dir, store) = synced_store();
        let (manual, _) = manual_game();
        store.insert_game(&manual).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        let summary = import(&fresh, &backup).unwrap();

        assert_eq!(summary.manual_games, 1);
        assert_eq!(fresh.game(&manual.id).unwrap().unwrap().title, "My Game");
        assert_eq!(fresh.launch_config(&manual.id).unwrap(), None);
    }
}
