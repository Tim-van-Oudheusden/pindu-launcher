use std::path::{Path, PathBuf};

use pindu_core::model::{ArtLayout, LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::{ImportedGame, Snapshot, lutris};

use crate::support::fixture_into;

const NATIVE: &str = ".local/share";
const FLATPAK: &str = ".var/app/net.lutris.Lutris/data";

/// Copies the Lutris fixture into `<home>/<data>` (Lutris's data home), builds its
/// `pga.db` from `pga.sql`, and returns the temp dir and the resolved Lutris folder.
fn lutris_home(data: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fixture_into("lutris", &home.join(data));
    let sql_path = home.join(data).join("lutris/pga.sql");
    let sql = std::fs::read_to_string(&sql_path).unwrap();
    rusqlite::Connection::open(home.join(data).join("lutris/pga.db"))
        .unwrap()
        .execute_batch(&sql)
        .unwrap();
    std::fs::remove_file(sql_path).unwrap();
    let dirs = HostDirs {
        home: home.clone(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
    };
    let root = Location::of(Source::Lutris, &dirs)
        .unwrap()
        .resolve(None)
        .unwrap();
    (dir, root)
}

fn game<'a>(snapshot: &'a Snapshot, native_id: &str) -> &'a ImportedGame {
    snapshot
        .games
        .iter()
        .find(|game| game.native_id == native_id)
        .unwrap_or_else(|| panic!("no game {native_id} in {:?}", snapshot.games))
}

#[test]
fn native_and_flatpak_lutris_import_the_same_games() {
    for data in [NATIVE, FLATPAK] {
        let (_dir, root) = lutris_home(data);
        assert!(root.ends_with(Path::new(data).join("lutris")), "{data}");

        let snapshot = lutris::import(&root).unwrap();

        let mut ids: Vec<_> = snapshot
            .games
            .iter()
            .map(|g| g.native_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, ["1", "2", "3"], "{data}");
        assert_eq!(snapshot.source, Source::Lutris);
    }
}

#[test]
fn a_game_takes_its_details_playtime_and_art() {
    let (_dir, root) = lutris_home(FLATPAK);

    let snapshot = lutris::import(&root).unwrap();

    let celeste = game(&snapshot, "1");
    assert_eq!(celeste.title, "Celeste");
    assert_eq!(celeste.platform, Some(Platform::Linux));
    assert!(celeste.installed);
    assert_eq!(
        celeste.install_path.as_deref(),
        Some(Path::new("/games/celeste"))
    );
    assert_eq!(celeste.last_played_at, Some(1_700_000_000));
    // Lutris keeps hours: 12.5 h.
    assert_eq!(celeste.playtime_secs, Some(45_000));
    assert_eq!(celeste.launch_kind, LaunchKind::StoreUri);
    assert_eq!(
        celeste.artwork,
        vec![
            (ArtLayout::Cover, root.join("coverart/celeste.jpg")),
            (
                ArtLayout::Icon,
                root.join("../icons/hicolor/128x128/apps/lutris_celeste.png")
            ),
        ]
    );
}

#[test]
fn lutris_categories_become_flags_and_collections() {
    let (_dir, root) = lutris_home(NATIVE);

    let snapshot = lutris::import(&root).unwrap();

    let celeste = &game(&snapshot, "1").source_flags;
    assert!(celeste.favourite);
    assert!(!celeste.hidden);
    assert_eq!(celeste.categories, ["Platformers"]);
    let witcher = &game(&snapshot, "2").source_flags;
    assert!(witcher.hidden);
    assert!(!witcher.favourite);
    assert!(witcher.categories.is_empty());
}

#[test]
fn games_without_art_playtime_or_install_import_without_them() {
    let (_dir, root) = lutris_home(NATIVE);

    let snapshot = lutris::import(&root).unwrap();

    let witcher = game(&snapshot, "2");
    assert_eq!(witcher.platform, Some(Platform::Windows));
    assert_eq!(witcher.playtime_secs, None);
    assert_eq!(witcher.last_played_at, None);
    assert!(witcher.artwork.is_empty());
    let hollow_knight = game(&snapshot, "3");
    assert!(!hollow_knight.installed);
    assert_eq!(hollow_knight.platform, None);
    assert_eq!(hollow_knight.install_path, None);
}

#[test]
fn a_row_without_a_name_is_reported() {
    let (_dir, root) = lutris_home(NATIVE);

    let snapshot = lutris::import(&root).unwrap();

    assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
    assert!(snapshot.errors[0].contains('4'), "{:?}", snapshot.errors);
}

#[test]
fn the_live_database_is_copied_not_opened() {
    let (_dir, root) = lutris_home(NATIVE);
    let live = root.join("pga.db");
    let before = std::fs::read(&live).unwrap();
    // Lutris holding a write lock would block any reader of the live file.
    let lutris = rusqlite::Connection::open(&live).unwrap();
    lutris.execute_batch("BEGIN EXCLUSIVE").unwrap();

    let snapshot = lutris::import(&root).unwrap();

    assert_eq!(snapshot.games.len(), 3);
    lutris.execute_batch("ROLLBACK").unwrap();
    drop(lutris);
    assert_eq!(std::fs::read(&live).unwrap(), before);
    let mut names: Vec<_> = std::fs::read_dir(&root)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort_unstable();
    assert_eq!(names, ["coverart", "pga.db"]);
}

#[test]
fn an_unreadable_database_fails_the_source() {
    let (_dir, root) = lutris_home(NATIVE);
    std::fs::write(root.join("pga.db"), "not a database").unwrap();

    let error = lutris::import(&root).unwrap_err();

    assert_eq!(error.path, root.join("pga.db"));
}
