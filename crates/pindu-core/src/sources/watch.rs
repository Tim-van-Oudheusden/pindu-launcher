//! What to watch so a launcher's changes resync the library: each source's key files.

use std::path::{Path, PathBuf};

use super::desktop;
use super::location::{HostDirs, Location};
use crate::model::Source;

/// The files (and for Steam, the manifest folder) whose changes mean a source changed,
/// below the source's resolved folder `root`.
pub fn key_paths(source: Source, root: &Path) -> Vec<PathBuf> {
    let heroic = |library: &str, installed: Option<&str>| {
        [Some(library), installed]
            .into_iter()
            .flatten()
            .chain([
                "store/config.json",
                "store/game-overrides.json",
                "store/timestamp.json",
            ])
            .map(|path| root.join(path))
            .collect()
    };
    match source {
        // Manifests come and go as games are installed, so the folder is watched.
        Source::Steam => vec![
            root.join("steamapps/libraryfolders.vdf"),
            root.join("steamapps"),
        ],
        Source::HeroicEpic => heroic(
            "store_cache/legendary_library.json",
            Some("legendaryConfig/legendary/installed.json"),
        ),
        Source::HeroicGog => heroic(
            "store_cache/gog_library.json",
            Some("gog_store/installed.json"),
        ),
        Source::HeroicAmazon => heroic(
            "store_cache/nile_library.json",
            Some("nile_config/nile/installed.json"),
        ),
        Source::HeroicSideload => heroic("sideload_apps/library.json", None),
        Source::Lutris => vec![root.join("pga.db")],
        Source::Bottles => vec![root.join("library.yml")],
        Source::Faugus => vec![root.join("games.json")],
        // butler keeps the database in WAL mode, so new rows land in the log first.
        Source::Itch => vec![root.join("db/butler.db"), root.join("db/butler.db-wal")],
        Source::Legendary => vec![root.join("installed.json")],
        // An external importer's helper names its own paths in its probe.
        Source::Desktop | Source::Manual | Source::External => Vec::new(),
    }
}

/// Where a source the last sync did not find would appear: the files that complete each
/// of its candidate folders ([`Location::required_paths`]), so a launcher's first start,
/// which writes them, resyncs the library. Nothing for sources without one data folder.
pub fn appear_paths(source: Source, dirs: &HostDirs) -> Vec<PathBuf> {
    Location::of(source, dirs).map_or_else(Vec::new, |location| location.required_paths())
}

/// The folders desktop entries come from.
pub fn desktop_paths(dirs: &HostDirs) -> Vec<PathBuf> {
    desktop::application_dirs(dirs)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn steam_watches_its_library_list_and_every_manifest_folder() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("Steam");
        touch(&root.join("steamapps/libraryfolders.vdf"));

        let paths = key_paths(Source::Steam, &root);

        assert_eq!(
            paths,
            [
                root.join("steamapps/libraryfolders.vdf"),
                root.join("steamapps"),
            ]
        );
    }

    #[test]
    fn heroic_stores_watch_their_library_installed_and_shared_files() {
        let root = Path::new("/heroic");

        assert_eq!(
            key_paths(Source::HeroicGog, root),
            [
                root.join("store_cache/gog_library.json"),
                root.join("gog_store/installed.json"),
                root.join("store/config.json"),
                root.join("store/game-overrides.json"),
                root.join("store/timestamp.json"),
            ]
        );
        assert_eq!(
            key_paths(Source::HeroicSideload, root)[0],
            root.join("sideload_apps/library.json")
        );
    }

    #[test]
    fn other_sources_watch_their_library_file() {
        let root = Path::new("/data");

        assert_eq!(key_paths(Source::Lutris, root), [root.join("pga.db")]);
        assert_eq!(key_paths(Source::Bottles, root), [root.join("library.yml")]);
        assert_eq!(key_paths(Source::Faugus, root), [root.join("games.json")]);
        assert_eq!(
            key_paths(Source::Legendary, root),
            [root.join("installed.json")]
        );
    }

    #[test]
    fn itch_watches_its_database_and_its_write_ahead_log() {
        let root = Path::new("/itch");

        assert_eq!(
            key_paths(Source::Itch, root),
            [root.join("db/butler.db"), root.join("db/butler.db-wal")]
        );
    }

    #[test]
    fn a_source_not_found_yet_watches_where_its_files_would_appear() {
        let dirs = HostDirs {
            home: "/home/me".into(),
            data_home: "/home/me/.local/share".into(),
            config_home: "/home/me/.config".into(),
            host_usr: "/usr".into(),
            system_flatpak: "/var/lib/flatpak".into(),
        };

        assert_eq!(
            appear_paths(Source::Steam, &dirs),
            [
                PathBuf::from("/home/me/.steam/steam/steamapps/libraryfolders.vdf"),
                PathBuf::from("/home/me/.local/share/Steam/steamapps/libraryfolders.vdf"),
                PathBuf::from(
                    "/home/me/.var/app/com.valvesoftware.Steam/data/Steam/steamapps/libraryfolders.vdf"
                ),
            ]
        );
        assert_eq!(appear_paths(Source::Desktop, &dirs), [] as [PathBuf; 0]);
    }

    #[test]
    fn desktop_entries_watch_every_application_folder() {
        let dirs = HostDirs {
            home: "/home/me".into(),
            data_home: "/home/me/.local/share".into(),
            config_home: "/home/me/.config".into(),
            host_usr: "/usr".into(),
            system_flatpak: "/var/lib/flatpak".into(),
        };

        assert_eq!(
            desktop_paths(&dirs),
            super::super::desktop::application_dirs(&dirs)
        );
    }
}
