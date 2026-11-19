//! Legendary, the standalone command-line client for the Epic Games Store.
//!
//! Its installed games come from `installed.json` in its folder (`~/.config/legendary`),
//! with the developer and description from each game's `metadata/<app_name>.json`. Games
//! start on the host through `legendary launch <app_name>`. Credentials (`user.json`) are
//! never opened, and nothing is written. Heroic keeps its own Legendary folder, which the
//! Heroic importer reads; games Heroic lists are left out at sync.

use std::path::Path;

use serde_json::Value as Json;

use super::{ImportError, ImportedGame, Snapshot};
use crate::model::{LaunchConfig, LaunchKind, Platform, Source};

/// Imports the Legendary folder at `root`.
pub fn import(root: &Path) -> Result<Snapshot, ImportError> {
    let installed_path = root.join("installed.json");
    let fail = |message: String| ImportError {
        path: installed_path.clone(),
        message,
    };
    let text = std::fs::read_to_string(&installed_path).map_err(|error| fail(error.to_string()))?;
    let installed: serde_json::Map<String, Json> =
        serde_json::from_str(&text).map_err(|error| fail(error.to_string()))?;

    let mut snapshot = Snapshot::new(Source::Legendary);
    for (app_name, info) in &installed {
        if info["is_dlc"].as_bool() == Some(true) {
            continue;
        }
        let metadata = read_metadata(root, app_name, &mut snapshot.errors);
        let title = text_of(&info["title"])
            .or_else(|| text_of(&metadata["app_title"]))
            .unwrap_or(app_name);
        let mut game = ImportedGame::new(app_name, title, LaunchKind::HostCommand);
        game.installed = true;
        game.install_path = text_of(&info["install_path"]).map(Into::into);
        game.size_bytes = info["install_size"].as_i64();
        game.platform = match text_of(&info["platform"]) {
            Some("Windows" | "Win32") => Some(Platform::Windows),
            _ => None,
        };
        game.developer = text_of(&metadata["metadata"]["developer"]).map(str::to_owned);
        game.description = text_of(&metadata["metadata"]["description"]).map(str::to_owned);
        game.launch_config = Some(LaunchConfig {
            executable: "legendary".into(),
            arguments: vec!["launch".to_owned(), app_name.clone()],
            ..LaunchConfig::default()
        });
        snapshot.games.push(game);
    }
    Ok(snapshot)
}

/// A game's `metadata/<app_name>.json`; a missing one is empty, a broken one is reported
/// and treated as empty.
fn read_metadata(root: &Path, app_name: &str, errors: &mut Vec<String>) -> Json {
    let path = root.join("metadata").join(format!("{app_name}.json"));
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Json::Null;
    };
    serde_json::from_str(&text).unwrap_or_else(|error| {
        errors.push(format!("{}: {error}", path.display()));
        Json::Null
    })
}

/// A non-empty string.
fn text_of(value: &Json) -> Option<&str> {
    value.as_str().filter(|text| !text.is_empty())
}
