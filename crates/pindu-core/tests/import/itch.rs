use std::path::{Path, PathBuf};

use pindu_core::model::{LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::{ImportedGame, Snapshot, itch};

use crate::support::fixture_into;

const NATIVE: &str = ".config/itch";
const FLATPAK: &str = ".var/app/io.itch.itch/config/itch";

/// Copies the itch fixture into `<home>/<at>`, builds its `db/butler.db` from
/// `butler.sql`, and returns the temp dir and the resolved itch folder.
fn itch_home(at: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fixture_into("itch", &home.join(at));
    let db = home.join(at).join("db");
    let sql = std::fs::read_to_string(db.join("butler.sql")).unwrap();
    rusqlite::Connection::open(db.join("butler.db"))
        .unwrap()
        .execute_batch(&sql)
        .unwrap();
    std::fs::remove_file(db.join("butler.sql")).unwrap();
    let dirs = HostDirs {
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
        home,
    };
    let root = Location::of(Source::Itch, &dirs)
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
fn native_and_flatpak_itch_import_the_same_games() {
    for at in [NATIVE, FLATPAK] {
        let (_dir, root) = itch_home(at);
        assert!(root.ends_with(at), "{at}");

        let snapshot = itch::import(&root).unwrap();

        let mut ids: Vec<_> = snapshot
            .games
            .iter()
            .map(|g| g.native_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, ["1001", "1002", "1003"], "{at}");
        assert_eq!(snapshot.source, Source::Itch);
    }
}

#[test]
fn itch_games_start_through_the_itch_app() {
    let (_dir, root) = itch_home(NATIVE);

    let snapshot = itch::import(&root).unwrap();

    let lighthouse = game(&snapshot, "1001");
    assert_eq!(lighthouse.launch_kind, LaunchKind::StoreUri);
    assert_eq!(
        lighthouse.launch_uri.as_deref(),
        Some("itch://caves/cave-lighthouse/launch")
    );
    assert_eq!(lighthouse.launch_config, None);
}

#[test]
fn an_itch_game_carries_its_install_playtime_and_platform() {
    let (_dir, root) = itch_home(NATIVE);

    let snapshot = itch::import(&root).unwrap();

    let lighthouse = game(&snapshot, "1001");
    assert_eq!(lighthouse.title, "Lighthouse Keeper");
    assert!(lighthouse.installed);
    assert_eq!(lighthouse.platform, Some(Platform::Linux));
    assert_eq!(
        lighthouse.install_path.as_deref(),
        Some(Path::new("/home/me/.config/itch/apps/lighthouse-keeper"))
    );
    assert_eq!(lighthouse.size_bytes, Some(734_003_200));
    // The server's total, never summed with the local count.
    assert_eq!(lighthouse.playtime_secs, Some(5400));
    // 2025-03-02T18:30:00Z, the local run, is later than the last sync.
    assert_eq!(lighthouse.last_played_at, Some(1_740_940_200));
}

#[test]
fn a_never_played_windows_game_keeps_its_chosen_folder() {
    let (_dir, root) = itch_home(NATIVE);

    let snapshot = itch::import(&root).unwrap();

    let frog = game(&snapshot, "1002");
    assert_eq!(frog.platform, Some(Platform::Windows));
    assert_eq!(
        frog.install_path.as_deref(),
        Some(Path::new("/mnt/games/frog-pond"))
    );
    assert_eq!(frog.playtime_secs, None);
    assert_eq!(frog.last_played_at, None);
}

#[test]
fn a_game_installed_twice_starts_its_last_run_install() {
    let (_dir, root) = itch_home(NATIVE);

    let snapshot = itch::import(&root).unwrap();

    let tower: Vec<_> = snapshot
        .games
        .iter()
        .filter(|g| g.native_id == "1003")
        .collect();
    assert_eq!(tower.len(), 1);
    assert_eq!(
        tower[0].launch_uri.as_deref(),
        Some("itch://caves/cave-tower-new/launch")
    );
}

#[test]
fn soundtracks_are_left_out_and_orphaned_installs_reported() {
    let (_dir, root) = itch_home(NATIVE);

    let snapshot = itch::import(&root).unwrap();

    assert!(snapshot.games.iter().all(|g| g.native_id != "1004"));
    assert_eq!(
        snapshot.errors,
        ["itch install cave-orphan has no game record"]
    );
}

#[test]
fn installs_still_in_the_write_ahead_log_are_imported() {
    let (_dir, root) = itch_home(NATIVE);
    // butler keeps the database in WAL mode; while it runs, new rows sit in the log.
    let live = rusqlite::Connection::open(root.join("db/butler.db")).unwrap();
    live.execute_batch(
        "PRAGMA journal_mode = WAL; PRAGMA wal_autocheckpoint = 0;
         INSERT INTO games (id, title, classification, linux) VALUES (1006, 'New', 'game', 'all');
         INSERT INTO caves (id, game_id, install_folder_name) VALUES ('cave-new', 1006, 'new');",
    )
    .unwrap();

    let snapshot = itch::import(&root).unwrap();

    assert_eq!(game(&snapshot, "1006").title, "New");
    drop(live);
}

#[test]
fn the_live_database_is_left_untouched() {
    let (_dir, root) = itch_home(NATIVE);
    let db = root.join("db/butler.db");
    let before = std::fs::read(&db).unwrap();

    itch::import(&root).unwrap();

    assert_eq!(std::fs::read(&db).unwrap(), before);
    let mut files: Vec<_> = std::fs::read_dir(root.join("db"))
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(files, ["butler.db"]);
}

#[test]
fn a_missing_database_is_an_import_error() {
    let (_dir, root) = itch_home(NATIVE);
    std::fs::remove_file(root.join("db/butler.db")).unwrap();

    let error = itch::import(&root).unwrap_err();

    assert_eq!(error.path, root.join("db/butler.db"));
}
