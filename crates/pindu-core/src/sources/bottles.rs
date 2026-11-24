//! Bottles: programs the user added to the Bottles library (`library.yml`).
//!
//! Each entry names its bottle; grid art lives in `<bottles folder>/<bottle path>/grids/`,
//! where the bottles folder is `custom_bottles_path` from `data.yml` or `bottles/` under
//! the Bottles folder. Programs start through `bottles:run/<bottle>/<program>`. Entries
//! for Steam's Proton prefixes and umu games have no bottle to run in and are left to
//! their own sources. Nothing is written.

use std::path::{Path, PathBuf};

use yaml_rust2::{Yaml, YamlLoader};

use super::external::protocol::Probe;
use super::{ImportError, ImportedGame, Snapshot, watch};
use crate::model::{ArtLayout, LaunchKind, Platform, Source};

/// The Bottles folder among `candidates`, for the protocol's probe: the first that holds
/// `library.yml`, as [`super::location::Location`] resolves it, with the files whose
/// change means a resync ([`watch::key_paths`]).
pub fn probe(candidates: &[PathBuf]) -> Option<Probe> {
    let root = candidates
        .iter()
        .find(|candidate| candidate.join("library.yml").exists())?;
    Some(Probe {
        root: root.clone(),
        watch: watch::key_paths(Source::Bottles, root),
    })
}

/// Imports the Bottles folder at `root` (a resolved Bottles location).
pub fn import(root: &Path) -> Result<Snapshot, ImportError> {
    let library_path = root.join("library.yml");
    let library = read_yaml(&library_path).map_err(|message| ImportError {
        path: library_path.clone(),
        message,
    })?;
    let mut snapshot = Snapshot::new(Source::Bottles);
    // A missing or broken data.yml only loses the custom path.
    let bottles_folder = read_yaml(&root.join("data.yml"))
        .ok()
        .and_then(|data| data["custom_bottles_path"].as_str().map(PathBuf::from))
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| root.join("bottles"));

    let Some(entries) = library.as_hash() else {
        return Ok(snapshot);
    };
    for (key, entry) in entries {
        let key = text(key).unwrap_or_default();
        let bottle = &entry["bottle"];
        let is_steam = entry["steam"].as_bool() == Some(true);
        let (Some(bottle_name), false) = (bottle["name"].as_str(), is_steam) else {
            continue;
        };
        let Some(name) = entry["name"].as_str().filter(|name| !name.is_empty()) else {
            snapshot
                .errors
                .push(format!("Bottles library entry {key} has no name"));
            continue;
        };
        let id = text(&entry["id"]).unwrap_or(key);
        let mut game = ImportedGame::new(id, name, LaunchKind::StoreUri);
        game.platform = Some(Platform::Windows);
        game.installed = true;
        // Bottles splits the URI on `/` and does not decode it.
        if bottle_name.contains('/') || name.contains('/') {
            snapshot.errors.push(format!(
                "“{name}” in bottle “{bottle_name}” cannot be started through Bottles: \
                 its name has a “/”"
            ));
        } else {
            game.launch_uri = Some(format!("bottles:run/{bottle_name}/{name}"));
        }
        let grid = entry["thumbnail"]
            .as_str()
            .and_then(|thumbnail| thumbnail.strip_prefix("grid:"))
            .zip(bottle["path"].as_str())
            .map(|(file, bottle_path)| bottles_folder.join(bottle_path).join("grids").join(file));
        let icon = entry["icon"].as_str().map(PathBuf::from);
        game.artwork = [(ArtLayout::Cover, grid), (ArtLayout::Icon, icon)]
            .into_iter()
            .filter_map(|(layout, path)| Some((layout, path?)))
            .filter(|(_, path)| path.is_file())
            .collect();
        snapshot.games.push(game);
    }
    Ok(snapshot)
}

/// The first document of a YAML file.
fn read_yaml(path: &Path) -> Result<Yaml, String> {
    let source = std::fs::read_to_string(path).map_err(|error| error.to_string())?;
    let mut documents = YamlLoader::load_from_str(&source).map_err(|error| error.to_string())?;
    Ok(if documents.is_empty() {
        Yaml::Null
    } else {
        documents.swap_remove(0)
    })
}

/// A string or integer scalar as text.
fn text(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(text) => Some(text.clone()),
        Yaml::Integer(number) => Some(number.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_probe_finds_the_first_candidate_with_a_library_and_watches_it() {
        let dir = tempfile::tempdir().unwrap();
        let candidates: Vec<PathBuf> = ["native", "flatpak", "other"]
            .into_iter()
            .map(|name| dir.path().join(name))
            .collect();
        std::fs::create_dir(&candidates[0]).unwrap();
        for candidate in &candidates[1..] {
            std::fs::create_dir(candidate).unwrap();
            std::fs::write(candidate.join("library.yml"), "{}\n").unwrap();
        }

        assert_eq!(
            probe(&candidates),
            Some(Probe {
                root: candidates[1].clone(),
                watch: vec![candidates[1].join("library.yml")],
            })
        );
        assert_eq!(probe(&candidates[..1]), None);
    }
}
