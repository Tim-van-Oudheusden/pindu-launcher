//! Steam: libraries from `libraryfolders.vdf`, install state from `appmanifest_*.acf`,
//! type, platform and developer from the binary `appinfo.vdf`, art from `librarycache`,
//! and playtime from the most recently used account's
//! `userdata/<account>/config/localconfig.vdf`. Everything is read offline; nothing is
//! written.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::vdf::{self, Value};
use super::{ImportError, ImportedGame, Snapshot};
use crate::model::{ArtLayout, LaunchKind, Platform, Source};

/// `StateFlags` bit for a fully installed app.
const STATE_FULLY_INSTALLED: i64 = 4;

/// Imports the Steam installation at `root` (a resolved Steam location).
pub fn import(root: &Path) -> Result<Snapshot, ImportError> {
    let folders_path = root.join("steamapps/libraryfolders.vdf");
    let folders = fs::read_to_string(&folders_path)
        .map_err(|error| error.to_string())
        .and_then(|text| vdf::parse_text(&text).map_err(|error| error.to_string()))
        .map_err(|message| ImportError {
            path: folders_path.clone(),
            message,
        })?;

    let mut snapshot = Snapshot::new(Source::Steam);
    let mut apps: Vec<LocalApp> = Vec::new();
    let mut seen = HashSet::new();
    for library in libraries(root, &folders) {
        for app in library_apps(&library, &mut snapshot.errors) {
            if seen.insert(app.app_id) {
                apps.push(app);
            }
        }
    }

    let appinfo = read_appinfo(root, &seen, &mut snapshot.errors);
    let playtime = read_playtime(root);
    let cache = root.join("appcache/librarycache");
    snapshot.games = apps
        .into_iter()
        .filter_map(|app| {
            let info = appinfo.get(&app.app_id);
            let played = playtime.get(&app.app_id).copied().unwrap_or_default();
            let mut game = imported_game(app, info, &cache)?;
            game.playtime_secs = played.minutes.filter(|m| *m > 0).map(|m| m * 60);
            game.last_played_at = game.last_played_at.max(played.last_played_at);
            Some(game)
        })
        .collect();
    Ok(snapshot)
}

/// What an account's `localconfig.vdf` records about an app.
#[derive(Debug, Clone, Copy, Default)]
struct Played {
    minutes: Option<i64>,
    last_played_at: Option<i64>,
}

/// Playtime per app from the account used last: the `userdata` folder whose
/// `localconfig.vdf` changed last (Steam rewrites it while that account is signed in).
/// Account `0` is Steam's anonymous one. A missing or broken file means no playtime.
fn read_playtime(root: &Path) -> HashMap<u32, Played> {
    let newest = fs::read_dir(root.join("userdata"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name() != "0")
        .map(|entry| entry.path().join("config/localconfig.vdf"))
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .max_by_key(|(modified, _)| *modified);
    let Some((_, path)) = newest else {
        return HashMap::new();
    };
    let Some(config) = fs::read_to_string(path)
        .ok()
        .and_then(|text| vdf::parse_text(&text).ok())
    else {
        return HashMap::new();
    };
    let apps = config
        .path(&["UserLocalConfigStore", "Software", "Valve", "Steam", "apps"])
        .map(Value::entries)
        .unwrap_or_default();
    apps.iter()
        .filter_map(|(id, app)| {
            let played = Played {
                minutes: app.get("Playtime").and_then(Value::as_i64),
                last_played_at: app
                    .get("LastPlayed")
                    .and_then(Value::as_i64)
                    .filter(|time| *time > 0),
            };
            Some((id.parse().ok()?, played))
        })
        .collect()
}

/// One library folder and the apps `libraryfolders.vdf` says it holds.
struct Library {
    path: PathBuf,
    /// App ID and size on disk, from the file's `apps` list.
    listed_apps: Vec<(u32, Option<i64>)>,
}

/// An installed or installing app found in a library.
struct LocalApp {
    app_id: u32,
    name: Option<String>,
    installed: bool,
    install_path: Option<PathBuf>,
    size_bytes: Option<i64>,
    last_played_at: Option<i64>,
}

/// The libraries to read, the main one first. Entry `"0"` is always the Steam folder
/// itself, whatever path it records: Flatpak Steam records its sandbox's view of it.
fn libraries(root: &Path, folders: &Value) -> Vec<Library> {
    let entries = folders
        .get("libraryfolders")
        .map(Value::entries)
        .unwrap_or_default();
    let canonical_root = fs::canonicalize(root).unwrap_or_else(|_| root.to_owned());
    let mut libraries = vec![Library {
        path: root.to_owned(),
        listed_apps: Vec::new(),
    }];
    for (key, entry) in entries {
        let listed_apps = entry
            .get("apps")
            .map(Value::entries)
            .unwrap_or_default()
            .iter()
            .filter_map(|(id, size)| Some((id.parse().ok()?, size.as_i64())))
            .collect();
        if key == "0" {
            libraries[0].listed_apps = listed_apps;
            continue;
        }
        // Old files list bare paths instead of maps.
        let Some(path) = entry.str("path").or_else(|| entry.as_str()) else {
            continue;
        };
        let path = PathBuf::from(path);
        if fs::canonicalize(&path).is_ok_and(|path| path == canonical_root) {
            continue;
        }
        libraries.push(Library { path, listed_apps });
    }
    libraries
}

/// Apps from a library's manifests, or from its `apps` list when the sandbox cannot see
/// the library.
fn library_apps(library: &Library, errors: &mut Vec<String>) -> Vec<LocalApp> {
    let steamapps = library.path.join("steamapps");
    let Ok(entries) = fs::read_dir(&steamapps) else {
        return library
            .listed_apps
            .iter()
            .map(|&(app_id, size_bytes)| LocalApp {
                app_id,
                name: None,
                installed: true,
                install_path: None,
                size_bytes,
                last_played_at: None,
            })
            .collect();
    };
    let mut manifests: Vec<PathBuf> = entries
        .filter_map(|entry| entry.ok().map(|entry| entry.path()))
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("appmanifest_") && name.ends_with(".acf"))
        })
        .collect();
    manifests.sort();
    manifests
        .iter()
        .filter_map(|path| match read_manifest(path, &steamapps) {
            Ok(app) => Some(app),
            Err(message) => {
                errors.push(format!("{}: {message}", path.display()));
                None
            }
        })
        .collect()
}

fn read_manifest(path: &Path, steamapps: &Path) -> Result<LocalApp, String> {
    let text = fs::read_to_string(path).map_err(|error| error.to_string())?;
    let root = vdf::parse_text(&text).map_err(|error| error.to_string())?;
    let state = root.get("AppState").ok_or("no AppState")?;
    let app_id = state
        .get("appid")
        .and_then(Value::as_i64)
        .and_then(|id| u32::try_from(id).ok())
        .ok_or("no appid")?;
    let installed = state
        .get("StateFlags")
        .and_then(Value::as_i64)
        .is_some_and(|flags| flags & STATE_FULLY_INSTALLED != 0);
    Ok(LocalApp {
        app_id,
        name: state.str("name").map(str::to_owned),
        installed,
        install_path: state
            .str("installdir")
            .filter(|dir| !dir.is_empty())
            .map(|dir| steamapps.join("common").join(dir)),
        size_bytes: state
            .get("SizeOnDisk")
            .and_then(Value::as_i64)
            .filter(|size| *size > 0),
        last_played_at: state
            .get("LastPlayed")
            .and_then(Value::as_i64)
            .filter(|time| *time > 0),
    })
}

fn read_appinfo(
    root: &Path,
    wanted: &HashSet<u32>,
    errors: &mut Vec<String>,
) -> HashMap<u32, Value> {
    let path = root.join("appcache/appinfo.vdf");
    let Ok(bytes) = fs::read(&path) else {
        return HashMap::new();
    };
    vdf::read_appinfo(&bytes, |app_id| wanted.contains(&app_id)).unwrap_or_else(|error| {
        errors.push(format!("{}: {error}", path.display()));
        HashMap::new()
    })
}

/// Builds the game for an app, or `None` for tools, runtimes, DLC and unnamed apps.
fn imported_game(app: LocalApp, info: Option<&Value>, cache: &Path) -> Option<ImportedGame> {
    let common = info.and_then(|info| info.get("common"));
    let kind = common.and_then(|common| common.str("type"));
    if kind.is_some_and(|kind| {
        !kind.eq_ignore_ascii_case("game") && !kind.eq_ignore_ascii_case("demo")
    }) {
        return None;
    }
    let title = app.name.or_else(|| {
        common
            .and_then(|common| common.str("name"))
            .map(str::to_owned)
    })?;
    let mut game = ImportedGame::new(app.app_id.to_string(), title, LaunchKind::StoreUri);
    game.installed = app.installed;
    game.install_path = app.install_path;
    game.size_bytes = app.size_bytes;
    game.last_played_at = app.last_played_at;
    if let Some(info) = info {
        game.developer = developer(info);
        game.platform = common
            .and_then(|common| common.str("oslist"))
            .and_then(platform);
    }
    let icon = common.and_then(|common| common.str("icon"));
    game.artwork = artwork(cache, app.app_id, icon);
    Some(game)
}

fn developer(info: &Value) -> Option<String> {
    let from_associations = info
        .path(&["common", "associations"])
        .map(Value::entries)
        .unwrap_or_default()
        .iter()
        .find(|(_, association)| association.str("type") == Some("developer"))
        .and_then(|(_, association)| association.str("name"));
    from_associations
        .or_else(|| {
            info.path(&["extended", "developer"])
                .and_then(Value::as_str)
        })
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
}

/// Linux when Steam ships a Linux build, otherwise Windows.
fn platform(oslist: &str) -> Option<Platform> {
    let systems: Vec<&str> = oslist.split(',').map(str::trim).collect();
    if systems.iter().any(|os| os.eq_ignore_ascii_case("linux")) {
        Some(Platform::Linux)
    } else if systems.iter().any(|os| os.eq_ignore_ascii_case("windows")) {
        Some(Platform::Windows)
    } else {
        None
    }
}

/// The app's `librarycache` images. Current Steam keeps them in `<app id>/`, sometimes
/// one folder deeper, and newer apps only have a 300×450 `library_capsule.jpg` cover;
/// older Steam kept flat `<app id>_<name>` files.
fn artwork(cache: &Path, app_id: u32, icon: Option<&str>) -> Vec<(ArtLayout, PathBuf)> {
    let app_dir = cache.join(app_id.to_string());
    let mut dirs: Vec<PathBuf> = fs::read_dir(&app_dir)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.path())
        .collect();
    dirs.sort();
    dirs.insert(0, app_dir);
    let icon_name = icon.map(|hash| format!("{hash}.jpg"));
    let layouts: [(ArtLayout, Vec<&str>, &str); 4] = [
        (
            ArtLayout::Cover,
            vec!["library_600x900.jpg", "library_capsule.jpg"],
            "library_600x900.jpg",
        ),
        (
            ArtLayout::Hero,
            vec!["library_hero.jpg"],
            "library_hero.jpg",
        ),
        (ArtLayout::Logo, vec!["logo.png"], "logo.png"),
        (
            ArtLayout::Icon,
            icon_name.as_deref().into_iter().collect(),
            "icon.jpg",
        ),
    ];
    layouts
        .into_iter()
        .filter_map(|(layout, names, legacy)| {
            names
                .iter()
                .flat_map(|name| dirs.iter().map(move |dir| dir.join(name)))
                .chain([cache.join(format!("{app_id}_{legacy}"))])
                .find(|path| path.is_file())
                .map(|path| (layout, path))
        })
        .collect()
}
