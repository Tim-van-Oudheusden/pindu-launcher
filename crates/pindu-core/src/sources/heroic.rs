//! Heroic Games Launcher: Epic (legendary), GOG, Amazon (nile) and sideloaded games.
//!
//! Each store's `store_cache/*_library.json` is joined with that store's installed file,
//! then `store/game-overrides.json` and the hidden, favourite and category lists in
//! `store/config.json` are applied; playtime comes from `store/timestamp.json`. Art comes
//! from Heroic's `images-cache`. Credential files (`gog_store/auth.json`,
//! `legendaryConfig/legendary/user.json`, `nile_config/nile/user.json`) are never opened,
//! and nothing is written.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;

use super::iso_time::parse_iso_timestamp;
use super::sha256::sha256_hex;
use super::{ImportError, ImportedGame, Snapshot, SourceFlags};
use crate::model::{ArtLayout, LaunchKind, Platform, Source};

/// URL parameters Heroic's frontend adds to Epic art before caching it.
const EPIC_ART_RESIZE: &str = "?h=400&resize=1&w=300";

/// Install details from a store's installed file.
#[derive(Default)]
struct Install {
    path: Option<PathBuf>,
    size_bytes: Option<i64>,
    platform: Option<Platform>,
}

/// Where one Heroic store keeps its files.
struct Store {
    library: &'static str,
    /// Runner name, used in category entries (`<appName>_<runner>`).
    runner: &'static str,
}

impl Store {
    fn of(source: Source) -> Option<Self> {
        let (library, runner) = match source {
            Source::HeroicEpic => ("store_cache/legendary_library.json", "legendary"),
            Source::HeroicGog => ("store_cache/gog_library.json", "gog"),
            Source::HeroicAmazon => ("store_cache/nile_library.json", "nile"),
            Source::HeroicSideload => ("sideload_apps/library.json", "sideload"),
            _ => return None,
        };
        Some(Self { library, runner })
    }
}

/// Imports one Heroic store from the Heroic folder at `root`.
pub fn import(root: &Path, source: Source) -> Result<Snapshot, ImportError> {
    let store = Store::of(source).ok_or_else(|| ImportError {
        path: root.to_owned(),
        message: format!("{source} is not a Heroic store"),
    })?;
    let library_path = root.join(store.library);
    let library = read_json(&library_path).map_err(|message| ImportError {
        path: library_path,
        message,
    })?;

    let mut snapshot = Snapshot::new(source);
    let installed = read_installed(root, source, &mut snapshot.errors);
    let overrides = read_optional(
        &root.join("store/game-overrides.json"),
        &mut snapshot.errors,
    );
    let config = read_optional(&root.join("store/config.json"), &mut snapshot.errors);
    let flags = Flags::from_config(&config);
    let timestamps = read_optional(&root.join("store/timestamp.json"), &mut snapshot.errors);

    let entries = library
        .get("library")
        .or_else(|| library.get("games"))
        .and_then(Json::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default();
    for info in entries {
        if info.pointer("/install/is_dlc").and_then(Json::as_bool) == Some(true) {
            continue;
        }
        let Some(app_name) = info
            .get("app_name")
            .and_then(Json::as_str)
            .filter(|name| !name.is_empty())
        else {
            snapshot
                .errors
                .push(format!("{}: an entry has no app_name", store.library));
            continue;
        };
        let game_override = overrides.pointer(&format!("/overrides/{}", escape_pointer(app_name)));
        let title = game_override
            .and_then(|o| text(o, "title"))
            .or_else(|| text(info, "title"))
            .unwrap_or(app_name);
        let mut game = ImportedGame::new(app_name, title, LaunchKind::StoreUri);
        game.developer = text(info, "developer").map(str::to_owned);
        game.genres = info
            .pointer("/extra/genres")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter_map(Json::as_str)
            .map(str::to_owned)
            .collect();
        game.description = [
            "/extra/about/description",
            "/extra/about/shortDescription",
            "/description",
        ]
        .iter()
        .find_map(|pointer| {
            info.pointer(pointer)
                .and_then(Json::as_str)
                .filter(|text| !text.is_empty())
        })
        .map(str::to_owned);

        let install = if source == Source::HeroicSideload {
            info.get("is_installed")
                .and_then(Json::as_bool)
                .unwrap_or(false)
                .then(|| Install {
                    path: info
                        .pointer("/install/install_path")
                        .or_else(|| info.get("folder_name"))
                        .and_then(Json::as_str)
                        .filter(|path| !path.is_empty())
                        .map(PathBuf::from),
                    size_bytes: None,
                    platform: info
                        .pointer("/install/platform")
                        .and_then(Json::as_str)
                        .and_then(platform),
                })
        } else {
            installed.get(app_name).map(|install| Install {
                path: install.path.clone(),
                size_bytes: install.size_bytes,
                platform: install.platform,
            })
        };
        if let Some(install) = install {
            game.installed = true;
            game.install_path = install.path;
            game.size_bytes = install.size_bytes;
            game.platform = install.platform;
        }

        game.artwork = artwork(root, info, game_override);
        game.source_flags = flags.for_game(app_name, store.runner);
        if let Some(played) = timestamps.get(app_name) {
            // Heroic keeps minutes.
            game.playtime_secs = played
                .get("totalPlayed")
                .and_then(Json::as_f64)
                .map(|minutes| (minutes * 60.0).round() as i64)
                .filter(|secs| *secs > 0);
            game.last_played_at =
                text(played, "lastPlayed").and_then(|time| parse_iso_timestamp(time, 0));
        }
        snapshot.games.push(game);
    }
    Ok(snapshot)
}

fn read_json(path: &Path) -> Result<Json, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&text).map_err(|error| error.to_string())
}

/// Reads a file that may be absent; a broken one is reported and treated as empty.
fn read_optional(path: &Path, errors: &mut Vec<String>) -> Json {
    if !path.exists() {
        return Json::Null;
    }
    read_json(path).unwrap_or_else(|message| {
        errors.push(format!("{}: {message}", path.display()));
        Json::Null
    })
}

fn text<'a>(value: &'a Json, key: &str) -> Option<&'a str> {
    value
        .get(key)
        .and_then(Json::as_str)
        .filter(|text| !text.is_empty())
}

fn platform(name: &str) -> Option<Platform> {
    if name.eq_ignore_ascii_case("windows") {
        Some(Platform::Windows)
    } else if name.eq_ignore_ascii_case("linux") {
        Some(Platform::Linux)
    } else {
        None
    }
}

/// Escapes a key for use in a JSON pointer.
fn escape_pointer(key: &str) -> String {
    key.replace('~', "~0").replace('/', "~1")
}

/// Installed games of a store, by app name.
fn read_installed(
    root: &Path,
    source: Source,
    errors: &mut Vec<String>,
) -> HashMap<String, Install> {
    let relative = match source {
        Source::HeroicEpic => "legendaryConfig/legendary/installed.json",
        Source::HeroicGog => "gog_store/installed.json",
        Source::HeroicAmazon => "nile_config/nile/installed.json",
        _ => return HashMap::new(),
    };
    let json = read_optional(&root.join(relative), errors);
    let path = |value: &Json, key| text(value, key).map(PathBuf::from);
    match source {
        // {"<appName>": {install_path, install_size, platform, is_dlc}}
        Source::HeroicEpic => json
            .as_object()
            .into_iter()
            .flatten()
            .filter(|(_, info)| info.get("is_dlc").and_then(Json::as_bool) != Some(true))
            .map(|(app_name, info)| {
                let install = Install {
                    path: path(info, "install_path"),
                    size_bytes: info.get("install_size").and_then(Json::as_i64),
                    platform: text(info, "platform").and_then(platform),
                };
                (app_name.clone(), install)
            })
            .collect(),
        // {"installed": [{appName, install_path, platform}]}
        Source::HeroicGog => json
            .get("installed")
            .and_then(Json::as_array)
            .into_iter()
            .flatten()
            .filter(|info| info.get("is_dlc").and_then(Json::as_bool) != Some(true))
            .filter_map(|info| {
                let install = Install {
                    path: path(info, "install_path"),
                    size_bytes: None,
                    platform: text(info, "platform").and_then(platform),
                };
                Some((text(info, "appName")?.to_owned(), install))
            })
            .collect(),
        // [{id, path, size}]; Amazon Games only ships Windows games.
        _ => json
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|info| {
                let install = Install {
                    path: path(info, "path"),
                    size_bytes: info.get("size").and_then(Json::as_i64),
                    platform: Some(Platform::Windows),
                };
                Some((text(info, "id")?.to_owned(), install))
            })
            .collect(),
    }
}

/// The game's art in each layout, from local files or Heroic's cache of the art URLs.
fn artwork(root: &Path, info: &Json, game_override: Option<&Json>) -> Vec<(ArtLayout, PathBuf)> {
    let cache = root.join("images-cache");
    let from_override = |key| game_override.and_then(|o| text(o, key));
    let layouts = [
        (
            ArtLayout::Cover,
            vec![from_override("art_square"), text(info, "art_square")],
        ),
        (
            ArtLayout::Hero,
            vec![
                from_override("art_cover"),
                text(info, "art_cover"),
                text(info, "art_background"),
            ],
        ),
        (ArtLayout::Logo, vec![text(info, "art_logo")]),
        (ArtLayout::Icon, vec![text(info, "art_icon")]),
    ];
    layouts
        .into_iter()
        .filter_map(|(layout, sources)| {
            let path = sources
                .into_iter()
                .flatten()
                .find_map(|source| local_art(&cache, source))?;
            Some((layout, path))
        })
        .collect()
}

/// A local file for an art reference: the path itself, or the cached download of a URL.
fn local_art(cache: &Path, reference: &str) -> Option<PathBuf> {
    if reference.starts_with("http://") || reference.starts_with("https://") {
        return [
            reference.to_owned(),
            format!("{reference}{EPIC_ART_RESIZE}"),
        ]
        .iter()
        .map(|url| cache.join(sha256_hex(url.as_bytes())))
        .find(|path| path.is_file());
    }
    let path = Path::new(reference.strip_prefix("file://").unwrap_or(reference));
    (path.is_absolute() && path.is_file()).then(|| path.to_owned())
}

/// Heroic's own hidden, favourite and category lists from `store/config.json`.
struct Flags {
    hidden: Vec<String>,
    favourites: Vec<String>,
    /// Category name and its `<appName>_<runner>` entries.
    categories: Vec<(String, Vec<String>)>,
}

impl Flags {
    fn from_config(config: &Json) -> Self {
        let app_names = |pointer: &str| -> Vec<String> {
            config
                .pointer(pointer)
                .and_then(Json::as_array)
                .into_iter()
                .flatten()
                .filter_map(|entry| text(entry, "appName").map(str::to_owned))
                .collect()
        };
        let mut categories: Vec<(String, Vec<String>)> = config
            .pointer("/games/customCategories")
            .and_then(Json::as_object)
            .into_iter()
            .flatten()
            .map(|(name, members)| {
                let members = members
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Json::as_str)
                    .map(str::to_owned)
                    .collect();
                (name.clone(), members)
            })
            .collect();
        categories.sort();
        Self {
            hidden: app_names("/games/hidden"),
            favourites: app_names("/games/favourites"),
            categories,
        }
    }

    fn for_game(&self, app_name: &str, runner: &str) -> SourceFlags {
        let key = format!("{app_name}_{runner}");
        SourceFlags {
            hidden: self.hidden.iter().any(|name| name == app_name),
            favourite: self.favourites.iter().any(|name| name == app_name),
            categories: self
                .categories
                .iter()
                .filter(|(_, members)| members.contains(&key))
                .map(|(name, _)| name.clone())
                .collect(),
        }
    }
}
