//! A JSON backup of what the user made here and exists nowhere else: collections,
//! favourites, hidden games, games added by hand, the playtime tracked here; from
//! format 2, game groups with their preferred copies, the pairs remembered as not the
//! same game, the filter of each saved filter and the metadata values the user set or a
//! provider fetched, each with its time; and from format 3, how each game added by hand
//! runs (its launch kind and platform), the games pinned to "Jump back in" and the
//! launch settings set on launchers' games.
//!
//! Games are named by ID, so a backup restores into a library where the launchers have
//! been synced again; a game the library does not have is skipped and counted, and so is
//! a group left with fewer than two of its games. A saved filter's view of a collection
//! names that collection, as its ID differs between libraries. A metadata value is
//! restored only where its origin has no value for that field here. Format 1 and 2
//! backups still import.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use crate::category;
use crate::library::{self, Filters, View};
use crate::metadata::{self, FieldValue, MetadataValue};
use crate::model::{
    Game, GameId, LaunchConfig, LaunchKind, MetadataField, MetadataOrigin, Platform, Source,
    Timestamp,
};
use crate::saved_filters::{self, SavedFilter};
use crate::search;
use crate::store::{Store, StoreError};

/// What the `format` field of a backup says.
const FORMAT: &str = "pindu-launcher-backup";
/// The backup version this build writes and the newest it reads. Version 3 added a game
/// added by hand's `launch_kind` and `platform`, `pins` and `launch_settings`; version 2
/// added `groups`, `not_the_same`, a collection's `filter` and `metadata`. Version 1 and
/// 2 backups lack what came later and still import.
const VERSION: u64 = 3;

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
    /// Groups of copies made again.
    pub groups: usize,
    /// Groups not made again: fewer than two of their games are in this library, or one
    /// of them is in another group here already.
    pub skipped_groups: usize,
    /// Pairs newly remembered as not the same game.
    pub splits: usize,
    /// Metadata values the user set or a provider fetched, for a field that origin had
    /// no value for here.
    pub metadata: usize,
    /// Launchers' games whose launch settings here gained some from the backup.
    pub launch_settings: usize,
    /// Games newly pinned to "Jump back in", at the places free here.
    pub pins: usize,
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
    let saved = store.saved_filters()?;
    let all_collections = store.collections()?;
    let names: HashMap<i64, &str> = all_collections
        .iter()
        .map(|collection| (collection.id, collection.name.as_str()))
        .collect();
    let mut collections = Vec::new();
    for collection in &all_collections {
        if let Some(filter) = saved.get(&collection.id) {
            let mut entry = json!({ "name": collection.name, "games": [] });
            if let Some(filter) = saved_filter_json(filter, &names) {
                entry["filter"] = filter;
            }
            collections.push(entry);
            continue;
        }
        let members: Vec<String> = store
            .collection_members(collection.id)?
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        collections.push(json!({ "name": collection.name, "games": members }));
    }
    let mut manual_games = Vec::new();
    let mut launch_settings = Vec::new();
    let mut sessions = Vec::new();
    let mut metadata = Vec::new();
    for game in &games {
        let launch = store.launch_config(&game.id)?;
        if game.id.source() == Source::Manual {
            manual_games.push(manual_game_json(game, launch.as_ref()));
        } else if let Some(launch) = launch.filter(has_launch_settings) {
            let mut entry = launch_settings_json(&launch);
            entry["game"] = game.id.as_str().into();
            launch_settings.push(entry);
        }
        for session in store.play_sessions(&game.id)? {
            sessions.push(json!({
                "game": game.id.as_str(),
                "started_at": session.started_at,
                "ended_at": session.ended_at,
            }));
        }
        for value in store.metadata_values(&game.id)? {
            if value.origin != MetadataOrigin::Source {
                metadata.push(metadata_value_json(&game.id, &value));
            }
        }
    }
    let groups: Vec<Value> = store
        .game_groups()?
        .iter()
        .map(|group| {
            let games: Vec<&str> = group.members.iter().map(GameId::as_str).collect();
            json!({
                "games": games,
                "preferred": group.preferred.as_ref().map(GameId::as_str),
                "automatic": group.automatic,
            })
        })
        .collect();
    let not_the_same: Vec<Value> = store
        .splits()?
        .iter()
        .map(|(first, second)| json!([first.as_str(), second.as_str()]))
        .collect();
    let mut pinned: Vec<&Game> = games
        .iter()
        .filter(|game| game.pin_order.is_some())
        .collect();
    pinned.sort_by_key(|game| game.pin_order);
    let pins: Vec<&str> = pinned.iter().map(|game| game.id.as_str()).collect();
    let backup = json!({
        "format": FORMAT,
        "version": VERSION,
        "exported_at": now,
        "collections": collections,
        "favourites": ids(|game| game.favourite),
        "hidden": ids(|game| game.hidden),
        "pins": pins,
        "manual_games": manual_games,
        "launch_settings": launch_settings,
        "play_sessions": sessions,
        "groups": groups,
        "not_the_same": not_the_same,
        "metadata": metadata,
    });
    Ok(serde_json::to_string_pretty(&backup).expect("JSON values always serialize"))
}

/// When the library was last backed up, from the time the app stored after its last
/// export: zero (the setting's default) or less means it never was.
pub fn last_backup(stored: Timestamp) -> Option<Timestamp> {
    (stored > 0).then_some(stored)
}

/// Restores a backup into the library, in one transaction. It adds and never removes:
/// favourites and hidden games stay as they are unless the backup marks them, existing
/// collections gain members, sessions or games already here are not added twice, a
/// group is made again only from games that are in no group here, a metadata value is
/// added only where its origin has none for that field, keeping the time it was set, a
/// pin goes to the first place free here, in the backup's order, and launch settings
/// add to a launcher's game's only what it does not have here.
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
        for id in list(&backup["pins"]).filter_map(|v| known(v, &mut missing)) {
            let Some(place) = library::next_pin(games.values()) else {
                continue;
            };
            if let Some(game) = games.get_mut(&id).filter(|game| game.pin_order.is_none()) {
                game.pin_order = Some(place);
                store.update_game(game)?;
                summary.pins += 1;
            }
        }
        let mut collections: HashMap<String, i64> = store
            .collections()?
            .into_iter()
            .map(|collection| (collection.name, collection.id))
            .collect();
        let mut saved_here: HashSet<i64> = store.saved_filters()?.into_keys().collect();
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
                    let filter = match saved_filter_from(&entry["filter"]) {
                        Some((BackupView::View(view), filters, search)) => Some(SavedFilter {
                            view,
                            filters,
                            search,
                        }),
                        Some((BackupView::Collection(target), filters, search))
                            if target != name =>
                        {
                            let target = collection_named(
                                store,
                                target,
                                &mut collections,
                                &mut summary.collections,
                            )?;
                            (!saved_here.contains(&target)).then_some(SavedFilter {
                                view: View::Collection(target),
                                filters,
                                search,
                            })
                        }
                        _ => None,
                    };
                    let collection = match &filter {
                        Some(filter) => {
                            let collection = store.create_saved_filter(name, filter)?;
                            saved_here.insert(collection.id);
                            collection
                        }
                        None => store.create_collection(name)?,
                    };
                    collections.insert(collection.name, collection.id);
                    summary.collections += 1;
                    collection.id
                }
            };
            // A saved filter's games are computed, never stored.
            if !entry["filter"].is_null() || saved_here.contains(&id) {
                continue;
            }
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
        for entry in list(&backup["groups"]) {
            let mut members: Vec<GameId> = list(&entry["games"])
                .filter_map(|v| known(v, &mut missing))
                .collect();
            members.sort_unstable();
            members.dedup();
            let mut groups_here = BTreeSet::new();
            let mut ungrouped = 0;
            for game in &members {
                match store.game_group_of(game)? {
                    Some(group) => {
                        groups_here.insert(group.id);
                    }
                    None => ungrouped += 1,
                }
            }
            if members.len() >= 2 && ungrouped == 0 && groups_here.len() == 1 {
                continue;
            }
            if members.len() < 2 || ungrouped < members.len() {
                summary.skipped_groups += 1;
                continue;
            }
            let group_id = if entry["automatic"] == true && members.len() == 2 {
                store.create_automatic_group(&members[0], &members[1])?
            } else {
                Some(store.create_group(&members)?)
            };
            let Some(group_id) = group_id else {
                summary.skipped_groups += 1;
                continue;
            };
            let preferred = entry["preferred"]
                .as_str()
                .and_then(|text| text.parse::<GameId>().ok())
                .filter(|game| members.contains(game));
            if let Some(preferred) = preferred {
                store.set_preferred_copy(group_id, Some(&preferred))?;
            }
            summary.groups += 1;
        }
        for pair in list(&backup["not_the_same"]) {
            let (Some(first), Some(second)) =
                (known(&pair[0], &mut missing), known(&pair[1], &mut missing))
            else {
                continue;
            };
            if first != second && !store.is_split(&first, &second)? {
                store.remember_split(&first, &second)?;
                summary.splits += 1;
            }
        }
        for entry in list(&backup["metadata"]) {
            let (Some(game), Some(restored)) = (
                known(&entry["game"], &mut missing),
                metadata_value_from(entry),
            ) else {
                continue;
            };
            let mut own: Vec<MetadataValue> = store
                .metadata_values(&game)?
                .into_iter()
                .filter(|value| value.origin == restored.origin)
                .collect();
            let field = restored.value.field();
            if own.iter().any(|value| value.value.field() == field) {
                continue;
            }
            let (origin, updated_at) = (restored.origin, restored.updated_at);
            own.push(restored);
            // Values already here are unchanged, so they keep their time.
            store.set_metadata(&game, origin, &metadata::resolve(&own), updated_at)?;
            summary.metadata += 1;
        }
        for entry in list(&backup["launch_settings"]) {
            let Some(game) = known(&entry["game"], &mut missing) else {
                continue;
            };
            // A game without a command here has no launch settings to add to.
            let Some(mut config) = store.launch_config(&game)? else {
                continue;
            };
            if add_launch_settings(&mut config, launch_settings_from(entry)) {
                store.set_launch_config(&game, &config)?;
                summary.launch_settings += 1;
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

/// The ID of the collection named `name`, made (and counted in `created`) if this library
/// has none.
fn collection_named(
    store: &Store,
    name: &str,
    collections: &mut HashMap<String, i64>,
    created: &mut usize,
) -> Result<i64, StoreError> {
    if let Some(&id) = collections.get(name) {
        return Ok(id);
    }
    let collection = store.create_collection(name)?;
    collections.insert(collection.name, collection.id);
    *created += 1;
    Ok(collection.id)
}

/// A saved filter's definition, beside its collection in a backup. Collection IDs differ
/// between libraries, so a view of a collection names it instead; `None` when that
/// collection is gone.
fn saved_filter_json(filter: &SavedFilter, names: &HashMap<i64, &str>) -> Option<Value> {
    let mut value = match filter.view {
        View::Collection(id) => json!({ "view": "collection", "collection": names.get(&id)? }),
        view => json!({ "view": saved_filters::view_text(view) }),
    };
    let toggles = &filter.filters;
    value["installed"] = toggles.installed.into();
    value["not_installed"] = toggles.not_installed.into();
    value["favourites"] = toggles.favourites.into();
    value["windows"] = toggles.windows.into();
    value["native"] = toggles.native.into();
    value["never_played"] = toggles.never_played.into();
    value["genre"] = toggles.genre.as_deref().into();
    value["search"] = filter.search.as_str().into();
    Some(value)
}

/// The view a saved filter in a backup starts from.
enum BackupView<'a> {
    /// A view every library has.
    View(View),
    /// The collection with this name.
    Collection(&'a str),
}

/// A saved filter's view, toggles, genre and normalised search as a backup holds them (see
/// [`saved_filter_json`]); `None` for none, or a view this version does not know.
fn saved_filter_from(value: &Value) -> Option<(BackupView<'_>, Filters, String)> {
    let view = match value["view"].as_str()? {
        "collection" => BackupView::Collection(
            value["collection"]
                .as_str()
                .filter(|name| !name.trim().is_empty())?,
        ),
        text => match saved_filters::view_from_text(text)? {
            View::Collection(_) | View::Hidden => return None,
            view => BackupView::View(view),
        },
    };
    let toggle = |key: &str| value[key] == true;
    let filters = Filters {
        installed: toggle("installed"),
        not_installed: toggle("not_installed"),
        favourites: toggle("favourites"),
        windows: toggle("windows"),
        native: toggle("native"),
        never_played: toggle("never_played"),
        genre: value["genre"]
            .as_str()
            .filter(|genre| !genre.trim().is_empty())
            .map(str::to_owned),
        hidden: false,
    };
    let search = search::normalize(value["search"].as_str().unwrap_or_default());
    Some((view, filters, search))
}

/// A game added by hand: what it is, how it runs and its launch config.
fn manual_game_json(game: &Game, launch: Option<&LaunchConfig>) -> Value {
    let launch = launch.map(|launch| {
        let mut value = launch_settings_json(launch);
        value["executable"] = launch.executable.to_string_lossy().into();
        value["arguments"] = json!(launch.arguments);
        value["working_dir"] = path_json(launch.working_dir.as_deref());
        value
    });
    json!({
        "id": game.id.as_str(),
        "title": game.title,
        "launch_kind": game.launch_kind.as_str(),
        "platform": game.platform.map(|platform| platform.as_str()),
        "install_path": path_json(game.install_path.as_deref()),
        "added_at": game.added_at,
        "last_played_at": game.last_played_at,
        "launch": launch,
    })
}

/// Whether a launch config has settings on top of its command. Its environment includes
/// what the source set, which a backup carries too: restoring it adds nothing a sync
/// has not added already.
fn has_launch_settings(launch: &LaunchConfig) -> bool {
    let settings = LaunchConfig {
        executable: PathBuf::new(),
        arguments: Vec::new(),
        working_dir: None,
        ..launch.clone()
    };
    settings != LaunchConfig::default()
}

/// Adds a backup's launch settings to a game's here, which win: its environment
/// variables and wrappers this one lacks, its Proton build and prefix where this has
/// none, and MangoHud and GameMode where it has them on. Whether anything was added.
fn add_launch_settings(config: &mut LaunchConfig, theirs: LaunchConfig) -> bool {
    let before = config.clone();
    for (key, value) in theirs.environment {
        config.environment.entry(key).or_insert(value);
    }
    if config.proton_version.is_none() {
        config.proton_version = theirs.proton_version;
    }
    if config.prefix_path.is_none() {
        config.prefix_path = theirs.prefix_path;
    }
    config.mangohud |= theirs.mangohud;
    config.gamemode |= theirs.gamemode;
    for wrapper in theirs.wrappers {
        if !config.wrappers.contains(&wrapper) {
            config.wrappers.push(wrapper);
        }
    }
    *config != before
}

/// The launch settings of a launch config, on top of its command (see
/// [`launch_settings_from`]).
fn launch_settings_json(launch: &LaunchConfig) -> Value {
    json!({
        "environment": launch.environment,
        "proton_version": launch.proton_version,
        "prefix_path": path_json(launch.prefix_path.as_deref()),
        "mangohud": launch.mangohud,
        "gamemode": launch.gamemode,
        "wrappers": launch.wrappers,
    })
}

/// A path as a backup holds it: text, or null for none.
fn path_json(path: Option<&Path>) -> Value {
    path.map(|path| path.to_string_lossy()).into()
}

/// A game added by hand and how it starts, if the backup says. A backup from before
/// format 3 does not say how the game runs: a launch config with a prefix is a Windows
/// game's, run through Proton, and any other game runs on the host, as these formats
/// cannot tell it from one run in the Steam Linux Runtime.
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
    let config = if launch.is_null() {
        None
    } else {
        Some(LaunchConfig {
            executable: path(&launch["executable"])?,
            arguments: strings(&launch["arguments"]),
            working_dir: path(&launch["working_dir"]),
            ..launch_settings_from(launch)
        })
    };
    let recorded = entry["launch_kind"]
        .as_str()
        .and_then(|text| text.parse().ok());
    (game.launch_kind, game.platform) = match (recorded, config.as_ref()) {
        (Some(kind), _) => (
            kind,
            entry["platform"]
                .as_str()
                .and_then(|text| text.parse().ok()),
        ),
        (None, Some(config)) if config.prefix_path.is_some() => {
            (LaunchKind::Proton, Some(Platform::Windows))
        }
        (None, _) => (LaunchKind::HostCommand, Some(Platform::Linux)),
    };
    Some((game, config))
}

/// The launch settings a backup holds on top of a game's command: its environment,
/// Proton build, prefix, MangoHud, GameMode and wrappers; the command stays empty.
fn launch_settings_from(launch: &Value) -> LaunchConfig {
    let environment: BTreeMap<String, String> = launch["environment"]
        .as_object()
        .map(Map::iter)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
        .collect();
    LaunchConfig {
        environment,
        proton_version: launch["proton_version"].as_str().map(str::to_owned),
        prefix_path: path(&launch["prefix_path"]),
        mangohud: launch["mangohud"].as_bool().unwrap_or(false),
        gamemode: launch["gamemode"].as_bool().unwrap_or(false),
        wrappers: strings(&launch["wrappers"]),
        ..LaunchConfig::default()
    }
}

/// The strings in a JSON array; anything else in it is left out.
fn strings(value: &Value) -> Vec<String> {
    list(value)
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect()
}

fn path(value: &Value) -> Option<PathBuf> {
    value
        .as_str()
        .filter(|text| !text.is_empty())
        .map(PathBuf::from)
}

/// A metadata value the user set or a provider fetched, with its game, field, origin and
/// time. Genres are a list, a release year a number, a Steam Deck rating an object, other
/// fields text.
fn metadata_value_json(id: &GameId, value: &MetadataValue) -> Value {
    let text = match &value.value {
        FieldValue::Title(text) | FieldValue::Developer(text) | FieldValue::Description(text) => {
            json!(text)
        }
        FieldValue::Genres(genres) => json!(genres),
        FieldValue::ReleaseYear(year) => json!(year),
        FieldValue::ControllerSupport(support) => json!(support.as_str()),
        FieldValue::DeckCompatibility(deck) => metadata::deck_json(deck),
    };
    json!({
        "game": id.as_str(),
        "field": value.value.field().as_str(),
        "origin": value.origin.as_str(),
        "value": text,
        "updated_at": value.updated_at,
    })
}

/// A metadata value from a backup (see [`metadata_value_json`]), without its game; `None`
/// for an empty value, a source's (a sync writes those) or a field or origin this version
/// does not know. A provider's genres keep only Pindu's categories ([`category`]): a
/// backup from before #759 holds Wikidata's labels, which a refresh maps again.
fn metadata_value_from(entry: &Value) -> Option<MetadataValue> {
    let origin: MetadataOrigin = entry["origin"].as_str()?.parse().ok()?;
    if origin == MetadataOrigin::Source {
        return None;
    }
    let text = || {
        entry["value"]
            .as_str()
            .filter(|text| !text.is_empty())
            .map(str::to_owned)
    };
    let value = match entry["field"].as_str()?.parse().ok()? {
        MetadataField::Title => FieldValue::Title(text()?),
        MetadataField::Developer => FieldValue::Developer(text()?),
        MetadataField::Description => FieldValue::Description(text()?),
        MetadataField::Genres => {
            let genres: Vec<String> = list(&entry["value"])
                .filter_map(Value::as_str)
                .filter(|genre| origin != MetadataOrigin::Provider || category::is_category(genre))
                .map(str::to_owned)
                .collect();
            if genres.is_empty() {
                return None;
            }
            FieldValue::Genres(genres)
        }
        MetadataField::ReleaseYear => {
            FieldValue::ReleaseYear(i32::try_from(entry["value"].as_i64()?).ok()?)
        }
        MetadataField::ControllerSupport => FieldValue::ControllerSupport(text()?.parse().ok()?),
        MetadataField::DeckCompatibility => {
            FieldValue::DeckCompatibility(metadata::deck_from_json(&entry["value"])?)
        }
    };
    Some(MetadataValue {
        value,
        origin,
        updated_at: entry["updated_at"].as_i64()?,
    })
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::metadata::Metadata;
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
                r#"{"format": "pindu-launcher-backup", "version": 4}"#
            ),
            Err(BackupError::TooNew { version: 4 })
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

    #[test]
    fn a_stored_backup_time_of_zero_means_never_backed_up() {
        assert_eq!(last_backup(0), None);
        assert_eq!(last_backup(-5), None);
        assert_eq!(last_backup(1_700_000_000), Some(1_700_000_000));
    }

    /// A library with copies of Portal from three sources and Portal 2 from two.
    fn store_with_copies() -> (tempfile::TempDir, Store) {
        let (dir, store) = synced_store();
        for id in [
            GameId::new(Source::HeroicGog, "1207658691"),
            GameId::new(Source::Lutris, "portal"),
            GameId::new(Source::Lutris, "portal-2"),
            GameId::new(Source::Desktop, "portal-2.desktop"),
        ] {
            store
                .insert_game(&Game::new(id, "Copy", LaunchKind::HostCommand, 1))
                .unwrap();
        }
        (dir, store)
    }

    #[test]
    fn groups_preferred_copies_and_remembered_splits_are_restored() {
        let (_dir, store) = store_with_copies();
        let portal = GameId::new(Source::Steam, "400");
        let gog = GameId::new(Source::HeroicGog, "1207658691");
        let lutris = GameId::new(Source::Lutris, "portal");
        let portal2 = GameId::new(Source::Steam, "620");
        let lutris2 = GameId::new(Source::Lutris, "portal-2");
        let desktop2 = GameId::new(Source::Desktop, "portal-2.desktop");
        let by_hand = store
            .create_group(&[portal.clone(), gog.clone(), lutris.clone()])
            .unwrap();
        store.set_preferred_copy(by_hand, Some(&gog)).unwrap();
        store
            .create_automatic_group(&portal2, &lutris2)
            .unwrap()
            .unwrap();
        store.remember_split(&desktop2, &portal2).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = store_with_copies();

        let summary = import(&fresh, &backup).unwrap();

        assert_eq!(
            summary,
            ImportSummary {
                groups: 2,
                splits: 1,
                ..ImportSummary::default()
            }
        );
        let groups: Vec<_> = fresh
            .game_groups()
            .unwrap()
            .into_iter()
            .map(|group| (group.members, group.preferred, group.automatic))
            .collect();
        assert_eq!(
            groups,
            [
                (vec![gog.clone(), lutris, portal], Some(gog), false),
                (vec![lutris2, portal2.clone()], None, true),
            ]
        );
        assert_eq!(fresh.splits().unwrap(), [(desktop2, portal2)]);
        assert_eq!(import(&fresh, &backup).unwrap(), ImportSummary::default());
    }

    #[test]
    fn a_group_with_fewer_than_two_of_its_games_here_is_skipped_and_counted() {
        let (_dir, store) = store_with_copies();
        let portal = GameId::new(Source::Steam, "400");
        let gog = GameId::new(Source::HeroicGog, "1207658691");
        let lutris = GameId::new(Source::Lutris, "portal");
        let portal2 = GameId::new(Source::Steam, "620");
        let lutris2 = GameId::new(Source::Lutris, "portal-2");
        let desktop2 = GameId::new(Source::Desktop, "portal-2.desktop");
        let three = store
            .create_group(&[portal.clone(), gog.clone(), lutris.clone()])
            .unwrap();
        store.set_preferred_copy(three, Some(&gog)).unwrap();
        store
            .create_group(&[portal2.clone(), lutris2.clone()])
            .unwrap();
        store.remember_split(&desktop2, &portal2).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();
        fresh
            .insert_game(&Game::new(
                lutris.clone(),
                "Copy",
                LaunchKind::HostCommand,
                1,
            ))
            .unwrap();

        let summary = import(&fresh, &backup).unwrap();

        assert_eq!((summary.groups, summary.skipped_groups), (1, 1));
        assert_eq!(summary.splits, 0);
        assert_eq!(summary.missing_games, 3);
        let groups = fresh.game_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, [lutris, portal]);
        assert_eq!(groups[0].preferred, None);
        assert!(fresh.splits().unwrap().is_empty());
    }

    #[test]
    fn a_group_touching_a_game_grouped_here_otherwise_is_skipped_and_counted() {
        let (_dir, store) = store_with_copies();
        let portal = GameId::new(Source::Steam, "400");
        let gog = GameId::new(Source::HeroicGog, "1207658691");
        let lutris = GameId::new(Source::Lutris, "portal");
        store.create_group(&[portal.clone(), gog.clone()]).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = store_with_copies();
        fresh
            .create_group(&[portal.clone(), lutris.clone()])
            .unwrap();

        let summary = import(&fresh, &backup).unwrap();

        assert_eq!((summary.groups, summary.skipped_groups), (0, 1));
        let groups = fresh.game_groups().unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].members, [lutris, portal]);
    }

    #[test]
    fn a_format_1_backup_still_imports() {
        let (_dir, store) = synced_store();
        let backup = r#"{
            "format": "pindu-launcher-backup",
            "version": 1,
            "exported_at": 300,
            "collections": [{"name": "Puzzles", "games": ["steam:400", "steam:620"]}],
            "favourites": ["steam:570"],
            "hidden": [],
            "manual_games": [],
            "play_sessions": [{"game": "steam:570", "started_at": 100, "ended_at": 200}]
        }"#;

        let summary = import(&store, backup).unwrap();

        assert_eq!(
            summary,
            ImportSummary {
                favourites: 1,
                collections: 1,
                sessions: 1,
                ..ImportSummary::default()
            }
        );
        assert!(store.game_groups().unwrap().is_empty());
    }

    #[test]
    fn a_backup_is_written_in_format_3() {
        let (_dir, store) = synced_store();

        let backup: Value = serde_json::from_str(&export(&store, 300).unwrap()).unwrap();

        assert_eq!(backup["version"], 3);
        assert_eq!(backup["groups"], json!([]));
        assert_eq!(backup["not_the_same"], json!([]));
        assert_eq!(backup["pins"], json!([]));
        assert_eq!(backup["launch_settings"], json!([]));
    }

    #[test]
    fn how_each_game_added_by_hand_runs_survives_a_backup() {
        let (_dir, store) = synced_store();
        let (mut runtime, config) = manual_game();
        runtime.launch_kind = LaunchKind::RuntimeNative;
        runtime.platform = Some(Platform::Linux);
        store.insert_game(&runtime).unwrap();
        store.set_launch_config(&runtime.id, &config).unwrap();
        let mut host = Game::new(
            GameId::new(Source::Manual, "host-1"),
            "Host Game",
            LaunchKind::HostCommand,
            1_700_000_200,
        );
        host.platform = Some(Platform::Linux);
        store.insert_game(&host).unwrap();
        let mut windows = Game::new(
            GameId::new(Source::Manual, "windows-1"),
            "Windows Game",
            LaunchKind::Proton,
            1_700_000_300,
        );
        windows.platform = Some(Platform::Windows);
        store.insert_game(&windows).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        import(&fresh, &backup).unwrap();

        let runs = |id: &GameId| {
            let game = fresh.game(id).unwrap().unwrap();
            (game.launch_kind, game.platform)
        };
        assert_eq!(
            runs(&runtime.id),
            (LaunchKind::RuntimeNative, Some(Platform::Linux))
        );
        assert_eq!(
            runs(&host.id),
            (LaunchKind::HostCommand, Some(Platform::Linux))
        );
        assert_eq!(
            runs(&windows.id),
            (LaunchKind::Proton, Some(Platform::Windows))
        );
    }

    #[test]
    fn pins_are_restored_in_their_order_to_the_places_free_here() {
        let (_dir, store) = synced_store();
        let pin = |store: &Store, native_id: &str, place: u8| {
            let mut game = store
                .game(&GameId::new(Source::Steam, native_id))
                .unwrap()
                .unwrap();
            game.pin_order = Some(place);
            store.update_game(&game).unwrap();
        };
        pin(&store, "620", 3);
        pin(&store, "400", 1);
        pin(&store, "570", 2);
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();
        pin(&fresh, "620", 1);

        let first = import(&fresh, &backup).unwrap();
        let second = import(&fresh, &backup).unwrap();

        assert_eq!((first.pins, second.pins), (2, 0));
        let places: Vec<_> = ["400", "570", "620"]
            .iter()
            .map(|native_id| {
                fresh
                    .game(&GameId::new(Source::Steam, native_id))
                    .unwrap()
                    .unwrap()
                    .pin_order
            })
            .collect();
        assert_eq!(places, [Some(2), Some(3), Some(1)]);
    }

    #[test]
    fn launch_settings_set_on_launchers_games_are_added_to_theirs_here() {
        let lutris = GameId::new(Source::Lutris, "portal");
        let desktop = GameId::new(Source::Desktop, "portal-2.desktop");
        let only_a_command = LaunchConfig {
            executable: PathBuf::from("/usr/bin/portal2"),
            arguments: vec!["-novid".into()],
            working_dir: Some(PathBuf::from("/usr/share/portal2")),
            ..LaunchConfig::default()
        };
        let synced = |store: &Store, id: &GameId| {
            let command = LaunchConfig {
                executable: PathBuf::from("/usr/bin/lutris"),
                arguments: vec!["lutris:rungame/portal".into()],
                environment: BTreeMap::from([("FROM_SOURCE".into(), "1".into())]),
                ..LaunchConfig::default()
            };
            store.set_launch_config(id, &command).unwrap();
            command
        };
        let (_dir, store) = store_with_copies();
        let mut mine = synced(&store, &lutris);
        mine.environment.insert("MINE".into(), "yes".into());
        mine.proton_version = Some("GE-Proton10-34".into());
        mine.prefix_path = Some(PathBuf::from("/prefixes/portal"));
        mine.mangohud = true;
        mine.gamemode = true;
        mine.wrappers = vec!["obs-gamecapture".into(), "strace".into()];
        store.set_launch_config(&lutris, &mine).unwrap();
        store.set_launch_config(&desktop, &only_a_command).unwrap();
        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = store_with_copies();
        let mut here = synced(&fresh, &lutris);
        here.environment.insert("MINE".into(), "kept".into());
        here.proton_version = Some("GE-Proton9-1".into());
        here.wrappers = vec!["strace".into()];
        fresh.set_launch_config(&lutris, &here).unwrap();

        let first = import(&fresh, &backup).unwrap();
        let second = import(&fresh, &backup).unwrap();

        assert_eq!((first.launch_settings, second.launch_settings), (1, 0));
        assert_eq!(
            fresh.launch_config(&lutris).unwrap(),
            Some(LaunchConfig {
                environment: BTreeMap::from([
                    ("FROM_SOURCE".into(), "1".into()),
                    ("MINE".into(), "kept".into()),
                ]),
                proton_version: Some("GE-Proton9-1".into()),
                prefix_path: Some(PathBuf::from("/prefixes/portal")),
                mangohud: true,
                gamemode: true,
                wrappers: vec!["strace".into(), "obs-gamecapture".into()],
                ..here
            })
        );
        let written: Value = serde_json::from_str(&backup).unwrap();
        let named: Vec<_> = list(&written["launch_settings"])
            .map(|entry| entry["game"].clone())
            .collect();
        assert_eq!(named, [json!("lutris:portal")]);
    }

    fn saved(view: View, filters: Filters, search: &str) -> SavedFilter {
        SavedFilter {
            view,
            filters,
            search: search.to_owned(),
        }
    }

    /// The saved filters of `store` by collection name.
    fn saved_by_name(store: &Store) -> BTreeMap<String, SavedFilter> {
        let saved = store.saved_filters().unwrap();
        store
            .collections()
            .unwrap()
            .into_iter()
            .filter_map(|c| Some((c.name, saved.get(&c.id)?.clone())))
            .collect()
    }

    #[test]
    fn saved_filters_are_restored_as_saved_filters() {
        let (_dir, store) = synced_store();
        let portal = GameId::new(Source::Steam, "400");
        let puzzles = store.create_collection("Puzzles").unwrap();
        store.add_to_collection(puzzles.id, &portal).unwrap();
        let windows = Filters {
            windows: true,
            favourites: true,
            ..Filters::default()
        };
        store
            .create_saved_filter(
                "Steam Windows",
                &saved(View::Source(Source::Steam), windows.clone(), "portal"),
            )
            .unwrap();
        let installed = Filters {
            installed: true,
            ..Filters::default()
        };
        store
            .create_saved_filter(
                "Installed Puzzles",
                &saved(View::Collection(puzzles.id), installed.clone(), ""),
            )
            .unwrap();

        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();
        fresh.create_collection("Already here").unwrap();
        let summary = import(&fresh, &backup).unwrap();

        assert_eq!(summary.collections, 3);
        let collections = fresh.collections().unwrap();
        let names: Vec<_> = collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "Already here",
                "Puzzles",
                "Steam Windows",
                "Installed Puzzles"
            ]
        );
        let restored_puzzles = collections[1].id;
        assert_eq!(
            saved_by_name(&fresh),
            BTreeMap::from([
                (
                    "Installed Puzzles".to_owned(),
                    saved(View::Collection(restored_puzzles), installed, "")
                ),
                (
                    "Steam Windows".to_owned(),
                    saved(View::Source(Source::Steam), windows, "portal")
                ),
            ])
        );
        assert!(
            fresh
                .collection_members(collections[2].id)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_saved_filters_genre_and_never_played_survive_a_backup() {
        let (_dir, store) = synced_store();
        let backlog = Filters {
            never_played: true,
            genre: Some("Puzzle".to_owned()),
            ..Filters::default()
        };
        store
            .create_saved_filter("Puzzle Backlog", &saved(View::All, backlog.clone(), ""))
            .unwrap();

        let backup = export(&store, 300).unwrap();
        let (_fresh_dir, fresh) = synced_store();
        import(&fresh, &backup).unwrap();

        assert_eq!(
            saved_by_name(&fresh),
            BTreeMap::from([("Puzzle Backlog".to_owned(), saved(View::All, backlog, ""))])
        );
    }

    #[test]
    fn a_format_2_backup_with_saved_filters_imports() {
        let (_dir, store) = synced_store();
        let backup = r#"{
            "format": "pindu-launcher-backup",
            "version": 2,
            "exported_at": 300,
            "collections": [
                {"name": "Favourite Puzzles", "games": ["steam:400"],
                 "filter": {"view": "collection", "collection": "Puzzles",
                            "favourites": true, "search": "  PORTAL  "}},
                {"name": "From the future", "games": [],
                 "filter": {"view": "never-played"}},
                {"name": "Puzzles", "games": ["steam:400"]},
                {"name": "Plain", "games": ["steam:620"]}
            ],
            "favourites": [],
            "hidden": [],
            "manual_games": [],
            "play_sessions": [],
            "groups": [],
            "not_the_same": []
        }"#;

        let summary = import(&store, backup).unwrap();

        assert_eq!(summary.collections, 4);
        let collections = store.collections().unwrap();
        let names: Vec<_> = collections.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(
            names,
            ["Puzzles", "Favourite Puzzles", "From the future", "Plain"]
        );
        let favourites = Filters {
            favourites: true,
            ..Filters::default()
        };
        assert_eq!(
            saved_by_name(&store),
            BTreeMap::from([(
                "Favourite Puzzles".to_owned(),
                saved(View::Collection(collections[0].id), favourites, "portal")
            )])
        );
        assert!(
            store
                .collection_members(collections[1].id)
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .collection_members(collections[2].id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.collection_members(collections[0].id).unwrap(),
            [GameId::new(Source::Steam, "400")]
        );
    }

    #[test]
    fn a_name_taken_here_keeps_what_it_is_here() {
        let (_dir, store) = synced_store();
        let native = Filters {
            native: true,
            ..Filters::default()
        };
        let mine = store
            .create_saved_filter("Mine", &saved(View::All, native.clone(), ""))
            .unwrap();
        let theirs = store.create_collection("Theirs").unwrap();
        let backup = r#"{
            "format": "pindu-launcher-backup",
            "version": 2,
            "collections": [
                {"name": "Mine", "games": ["steam:400"]},
                {"name": "Theirs", "games": [],
                 "filter": {"view": "all", "windows": true}}
            ]
        }"#;

        let summary = import(&store, backup).unwrap();

        assert_eq!(summary.collections, 0);
        assert!(store.collection_members(mine.id).unwrap().is_empty());
        assert_eq!(
            store.saved_filters().unwrap(),
            HashMap::from([(mine.id, saved(View::All, native, ""))])
        );
        assert!(store.collection_members(theirs.id).unwrap().is_empty());
    }

    fn developer_and_genres(developer: &str, genres: &[&str]) -> Metadata {
        Metadata {
            developer: Some(developer.to_owned()),
            genres: genres.iter().map(|genre| (*genre).to_owned()).collect(),
            description: None,
            ..Metadata::default()
        }
    }

    #[test]
    fn user_and_provider_metadata_is_restored_with_its_time() {
        let (_dir, store) = synced_store();
        let portal = GameId::new(Source::Steam, "400");
        store
            .set_metadata(
                &portal,
                MetadataOrigin::Source,
                &developer_and_genres("Valve", &["Puzzle"]),
                1_700_000_100,
            )
            .unwrap();
        store
            .set_metadata(
                &portal,
                MetadataOrigin::User,
                &developer_and_genres("Valve Software", &["Puzzle", "Classic"]),
                1_700_000_500,
            )
            .unwrap();
        let described = Metadata {
            description: Some("Think with portals.".to_owned()),
            ..Metadata::default()
        };
        store
            .set_metadata(&portal, MetadataOrigin::Provider, &described, 1_700_000_400)
            .unwrap();
        let backup = export(&store, 1_700_009_000).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        import(&fresh, &backup).unwrap();

        let carried: Vec<MetadataValue> = store
            .metadata_values(&portal)
            .unwrap()
            .into_iter()
            .filter(|value| value.origin != MetadataOrigin::Source)
            .collect();
        assert_eq!(carried.len(), 3);
        assert_eq!(fresh.metadata_values(&portal).unwrap(), carried);
    }

    #[test]
    fn a_users_release_year_and_controller_support_are_restored() {
        use crate::model::ControllerSupport;
        let (_dir, store) = synced_store();
        let portal = GameId::new(Source::Steam, "400");
        let mine = Metadata {
            release_year: Some(2007),
            controller_support: Some(ControllerSupport::Full),
            ..Metadata::default()
        };
        store
            .set_metadata(&portal, MetadataOrigin::User, &mine, 1_700_000_500)
            .unwrap();
        let backup = export(&store, 1_700_009_000).unwrap();
        let (_fresh_dir, fresh) = synced_store();

        import(&fresh, &backup).unwrap();

        let mine_only = |store: &Store| -> Vec<MetadataValue> {
            store
                .metadata_values(&portal)
                .unwrap()
                .into_iter()
                .filter(|value| value.origin == MetadataOrigin::User)
                .collect()
        };
        assert_eq!(mine_only(&fresh).len(), 2);
        assert_eq!(mine_only(&fresh), mine_only(&store));
        let game = fresh.game(&portal).unwrap().unwrap();
        assert_eq!(game.release_year, Some(2007));
        assert_eq!(game.controller_support, Some(ControllerSupport::Full));
    }

    #[test]
    fn restoring_metadata_never_overwrites_a_value_already_here() {
        let (_dir, store) = synced_store();
        let portal = GameId::new(Source::Steam, "400");
        let backup = r#"{
            "format": "pindu-launcher-backup",
            "version": 2,
            "metadata": [
                {"game": "steam:400", "field": "developer", "origin": "user",
                 "value": "Theirs", "updated_at": 900},
                {"game": "steam:400", "field": "genres", "origin": "user",
                 "value": ["Puzzle"], "updated_at": 900},
                {"game": "steam:400", "field": "description", "origin": "source",
                 "value": "Not a sync", "updated_at": 900},
                {"game": "steam:999", "field": "developer", "origin": "user",
                 "value": "Gone", "updated_at": 900}
            ]
        }"#;
        let mine = Metadata {
            developer: Some("Mine".to_owned()),
            ..Metadata::default()
        };
        store
            .set_metadata(&portal, MetadataOrigin::User, &mine, 500)
            .unwrap();

        let first = import(&store, backup).unwrap();
        let second = import(&store, backup).unwrap();

        assert_eq!(
            first,
            ImportSummary {
                metadata: 1,
                missing_games: 1,
                ..ImportSummary::default()
            }
        );
        assert_eq!(second.metadata, 0);
        assert_eq!(
            store.metadata_values(&portal).unwrap(),
            [
                MetadataValue {
                    value: FieldValue::Developer("Mine".to_owned()),
                    origin: MetadataOrigin::User,
                    updated_at: 500,
                },
                MetadataValue {
                    value: FieldValue::Genres(vec!["Puzzle".to_owned()]),
                    origin: MetadataOrigin::User,
                    updated_at: 900,
                },
            ]
        );
    }

    #[test]
    fn a_providers_genres_restore_only_as_categories_and_the_users_as_written() {
        let (_dir, store) = synced_store();
        let portal = GameId::new(Source::Steam, "400");
        let portal2 = GameId::new(Source::Steam, "620");
        // A backup from before #759: Wikidata's genres were its labels.
        let backup = r#"{
            "format": "pindu-launcher-backup",
            "version": 2,
            "metadata": [
                {"game": "steam:400", "field": "genres", "origin": "provider",
                 "value": ["Puzzle-Computerspiel", "Puzzle", "Ego-Shooter"], "updated_at": 900},
                {"game": "steam:400", "field": "genres", "origin": "user",
                 "value": ["Portal-like"], "updated_at": 900},
                {"game": "steam:620", "field": "genres", "origin": "provider",
                 "value": ["Ego-Shooter"], "updated_at": 900}
            ]
        }"#;

        let summary = import(&store, backup).unwrap();

        assert_eq!((summary.metadata, summary.missing_games), (2, 0));
        let genres = |id: &GameId, origin| {
            store
                .metadata_values(id)
                .unwrap()
                .into_iter()
                .filter(|value| value.origin == origin)
                .map(|value| value.value)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            genres(&portal, MetadataOrigin::Provider),
            [FieldValue::Genres(vec!["Puzzle".to_owned()])]
        );
        assert_eq!(
            genres(&portal, MetadataOrigin::User),
            [FieldValue::Genres(vec!["Portal-like".to_owned()])]
        );
        assert!(genres(&portal2, MetadataOrigin::Provider).is_empty());
    }
}
