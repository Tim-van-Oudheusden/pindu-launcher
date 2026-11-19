use super::*;
use crate::model::*;

fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("pindu-launcher/library.db")).unwrap();
    (dir, store)
}

#[test]
fn open_creates_the_database_in_wal_mode_with_the_latest_schema() {
    let (dir, store) = temp_store();

    assert!(dir.path().join("pindu-launcher/library.db").is_file());
    assert_eq!(store.journal_mode().unwrap(), "wal");
    assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
}

#[test]
fn a_new_database_opened_from_several_threads_at_once_opens_everywhere() {
    // The app loads the library and syncs on two threads right at start-up, so on a
    // first run both create the database at the same time.
    // A race shows up only sometimes, so try a few fresh databases.
    for _ in 0..20 {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("pindu-launcher/library.db");
        let barrier = std::sync::Barrier::new(8);

        let results: Vec<_> = std::thread::scope(|scope| {
            let handles: Vec<_> = (0..8)
                .map(|_| {
                    scope.spawn(|| {
                        barrier.wait();
                        Store::open(&path).map(|store| store.schema_version().unwrap())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });

        for result in results {
            assert_eq!(result.unwrap(), LATEST_SCHEMA_VERSION);
        }
    }
}

fn steam_game(native_id: &str, title: &str) -> Game {
    Game::new(
        GameId::new(Source::Steam, native_id),
        title,
        LaunchKind::StoreUri,
        1_700_000_000,
    )
}

#[test]
fn opening_a_database_from_a_newer_version_fails() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 1)
        .unwrap();

    let result = Store::open(&path);

    assert!(
        matches!(result, Err(StoreError::TooNew { version }) if version == LATEST_SCHEMA_VERSION + 1)
    );
}

#[test]
fn a_game_round_trips_with_every_field() {
    let (_dir, store) = temp_store();
    let mut game = steam_game("570", "Dota 2");
    game.sort_title = "dota 2".into();
    game.developer = Some("Valve".into());
    game.platform = Some(Platform::Linux);
    game.installed = true;
    game.install_path = Some("/games/dota 2".into());
    game.size_bytes = Some(40_000_000_000);
    game.last_played_at = Some(1_700_000_500);
    game.favourite = true;
    game.hidden = true;
    game.pin_order = Some(2);
    game.user_removed = true;
    game.missing_since = Some(1_700_000_900);
    game.genres = vec!["Strategy".into(), "MOBA".into()];
    game.description = Some("Every day, millions of players".into());
    game.steam_launch_dialog = true;
    game.launch_uri = Some("steam://rungameid/570".into());

    store.insert_game(&game).unwrap();

    assert_eq!(store.game(&game.id).unwrap(), Some(game));
}

#[test]
fn a_database_from_the_first_release_is_migrated_with_its_games() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(include_str!("migrations/0001_initial.sql"))
        .unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute(
        "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
         VALUES ('steam:570', 'steam', '570', 'Dota 2', 'Dota 2', 1, 'store-uri')",
        [],
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), LATEST_SCHEMA_VERSION);
    let game = store
        .game(&GameId::new(Source::Steam, "570"))
        .unwrap()
        .unwrap();
    assert_eq!(game.title, "Dota 2");
    assert!(!game.steam_launch_dialog);
    assert_eq!(game.launch_uri, None);
}

#[test]
fn games_survive_reopening_the_database() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let game = steam_game("570", "Dota 2");
    Store::open(&path).unwrap().insert_game(&game).unwrap();

    let reopened = Store::open(&path).unwrap();

    assert_eq!(reopened.games().unwrap(), vec![game]);
}

/// Writes a database at schema version 1 holding one game.
fn first_release_database(path: &Path) {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch(include_str!("migrations/0001_initial.sql"))
        .unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
    conn.execute(
        "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
         VALUES ('steam:570', 'steam', '570', 'Dota 2', 'Dota 2', 1, 'store-uri')",
        [],
    )
    .unwrap();
}

/// The names of the files in `dir`, sorted.
fn file_names(dir: &Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    names
}

#[test]
fn migrating_a_database_first_keeps_a_copy_at_its_old_version() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    first_release_database(&path);

    Store::open(&path).unwrap();

    // Only the copy is left beside the database: no partial copy or journal of it.
    let beside: Vec<_> = file_names(dir.path())
        .into_iter()
        .filter(|name| name != "library.db" && !name.starts_with("library.db-"))
        .collect();
    assert_eq!(beside, ["library.db.v1.bak"]);

    let copy = rusqlite::Connection::open(dir.path().join("library.db.v1.bak")).unwrap();
    let version: u32 = copy
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
    let title: String = copy
        .query_row(
            "SELECT title FROM games WHERE id = 'steam:570'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(title, "Dota 2");
    // Version 2 added the column; the copy still has the first release's schema.
    assert!(
        copy.prepare("SELECT steam_launch_dialog FROM games")
            .is_err()
    );
}

#[test]
fn a_copy_includes_writes_still_in_the_write_ahead_log() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    first_release_database(&path);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "journal_mode", "wal").unwrap();
    conn.pragma_update(None, "wal_autocheckpoint", 0).unwrap();
    conn.execute(
        "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
         VALUES ('steam:730', 'steam', '730', 'Counter-Strike 2', 'Counter-Strike 2', 1, 'store-uri')",
        [],
    )
    .unwrap();

    // `conn` stays open, so its write is still only in the log.
    Store::open(&path).unwrap();

    let copy = rusqlite::Connection::open(dir.path().join("library.db.v1.bak")).unwrap();
    let count: u32 = copy
        .query_row("SELECT count(*) FROM games", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
    drop(conn);
}

#[test]
fn a_new_or_current_database_gets_no_copy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");

    Store::open(&path).unwrap();
    Store::open(&path).unwrap();

    assert!(
        file_names(dir.path())
            .iter()
            .all(|name| !name.contains(".bak")),
        "{:?}",
        file_names(dir.path())
    );
}

#[test]
fn only_the_newest_two_copies_are_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    for old in [
        "library.db.v1.bak",
        "library.db.v2.bak",
        "library.db.v3.bak",
    ] {
        std::fs::write(dir.path().join(old), "old copy").unwrap();
    }
    // Another database's copies in the same folder are not ours to remove.
    std::fs::write(dir.path().join("other.db.v1.bak"), "other").unwrap();
    first_release_database(&path);
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.pragma_update(None, "user_version", 4).unwrap();
    drop(conn);

    Store::open(&path).unwrap();

    let copies: Vec<_> = file_names(dir.path())
        .into_iter()
        .filter(|name| name.ends_with(".bak"))
        .collect();
    assert_eq!(
        copies,
        ["library.db.v3.bak", "library.db.v4.bak", "other.db.v1.bak"]
    );
}

#[test]
fn a_failed_copy_stops_the_migration() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    first_release_database(&path);
    // A folder in the copy's place makes the copy fail.
    std::fs::create_dir(dir.path().join("library.db.v1.bak")).unwrap();

    let result = Store::open(&path);

    assert!(
        matches!(&result, Err(StoreError::Backup { path: copy, .. }) if copy.ends_with("library.db.v1.bak")),
        "{:?}",
        result.err()
    );
    let conn = rusqlite::Connection::open(&path).unwrap();
    let version: u32 = conn
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .unwrap();
    assert_eq!(version, 1);
}

#[test]
fn inserting_a_game_twice_fails() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();

    assert!(store.insert_game(&game).is_err());
}

#[test]
fn a_missing_game_reads_as_none() {
    let (_dir, store) = temp_store();

    assert_eq!(store.game(&GameId::new(Source::Steam, "1")).unwrap(), None);
}

#[test]
fn updating_a_game_replaces_its_fields() {
    let (_dir, store) = temp_store();
    let mut game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    game.title = "Dota 2 Reborn".into();
    game.favourite = true;

    store.update_game(&game).unwrap();

    assert_eq!(store.game(&game.id).unwrap(), Some(game));
}

#[test]
fn updating_an_unknown_game_fails() {
    let (_dir, store) = temp_store();

    let result = store.update_game(&steam_game("570", "Dota 2"));

    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn deleting_a_game_removes_it() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();

    store.delete_game(&game.id).unwrap();

    assert_eq!(store.game(&game.id).unwrap(), None);
}

#[test]
fn two_games_cannot_share_a_pin_position() {
    let (_dir, store) = temp_store();
    let mut first = steam_game("1", "First");
    first.pin_order = Some(1);
    let mut second = steam_game("2", "Second");
    second.pin_order = Some(1);
    store.insert_game(&first).unwrap();

    assert!(store.insert_game(&second).is_err());
}

fn stored_game(store: &Store, native_id: &str) -> GameId {
    let game = Game::new(
        GameId::new(Source::Manual, native_id),
        native_id,
        LaunchKind::Proton,
        1_700_000_000,
    );
    store.insert_game(&game).unwrap();
    game.id
}

#[test]
fn a_launch_config_round_trips_and_is_replaced_on_write() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let mut config = LaunchConfig {
        executable: "/games/witcher/witcher.exe".into(),
        arguments: vec!["-skipintro".into()],
        working_dir: Some("/games/witcher".into()),
        environment: [("DXVK_HUD".to_owned(), "fps".to_owned())].into(),
        proton_version: Some("GE-Proton10-1".into()),
        prefix_path: Some("/prefixes/witcher".into()),
        mangohud: true,
        gamemode: true,
        wrappers: vec!["strace".into()],
    };
    store.set_launch_config(&id, &config).unwrap();
    config.arguments.clear();

    store.set_launch_config(&id, &config).unwrap();

    assert_eq!(store.launch_config(&id).unwrap(), Some(config));
}

#[test]
fn a_performance_switch_writes_only_that_tool_of_only_that_game() {
    let (_dir, store) = temp_store();
    let witcher = stored_game(&store, "witcher");
    let celeste = stored_game(&store, "celeste");
    let config = LaunchConfig {
        executable: "/games/witcher/witcher.exe".into(),
        arguments: vec!["-skipintro".into()],
        working_dir: Some("/games/witcher".into()),
        environment: [("DXVK_HUD".to_owned(), "fps".to_owned())].into(),
        proton_version: Some("GE-Proton10-1".into()),
        prefix_path: Some("/prefixes/witcher".into()),
        mangohud: false,
        gamemode: true,
        wrappers: vec!["strace".into()],
    };
    store.set_launch_config(&witcher, &config).unwrap();
    store.set_launch_config(&celeste, &config).unwrap();

    store
        .set_performance_tool(&witcher, PerformanceTool::Overlay, true)
        .unwrap();
    store
        .set_performance_tool(&witcher, PerformanceTool::GameMode, false)
        .unwrap();

    assert_eq!(
        store.launch_config(&witcher).unwrap(),
        Some(LaunchConfig {
            mangohud: true,
            gamemode: false,
            ..config.clone()
        }),
        "every other launch setting stays"
    );
    assert_eq!(
        store.launch_config(&celeste).unwrap(),
        Some(config),
        "another game's settings stay as they were"
    );
    let written = store.launch_config(&witcher).unwrap().unwrap();
    assert!(PerformanceTool::Overlay.is_on(&written));
    assert!(!PerformanceTool::GameMode.is_on(&written));
}

#[test]
fn a_performance_switch_leaves_a_game_without_launch_settings_without_them() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");

    store
        .set_performance_tool(&id, PerformanceTool::Overlay, true)
        .unwrap();

    assert_eq!(store.launch_config(&id).unwrap(), None);
}

#[test]
fn deleting_a_launch_config_removes_it() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    store
        .set_launch_config(&id, &LaunchConfig::default())
        .unwrap();

    store.delete_launch_config(&id).unwrap();

    assert_eq!(store.launch_config(&id).unwrap(), None);
}

#[test]
fn artwork_is_kept_per_layout_and_origin() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let cover = |origin, path: &str| Artwork {
        game_id: id.clone(),
        layout: ArtLayout::Cover,
        origin,
        path: path.into(),
    };
    store
        .set_artwork(&cover(ArtOrigin::Source, "/old.png"))
        .unwrap();
    store
        .set_artwork(&cover(ArtOrigin::Source, "/source.png"))
        .unwrap();
    store
        .set_artwork(&cover(ArtOrigin::User, "/user.png"))
        .unwrap();

    assert_eq!(
        store.artwork(&id).unwrap(),
        vec![
            cover(ArtOrigin::Source, "/source.png"),
            cover(ArtOrigin::User, "/user.png")
        ]
    );
}

#[test]
fn deleting_artwork_removes_only_that_layout_and_origin() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let art = |layout| Artwork {
        game_id: id.clone(),
        layout,
        origin: ArtOrigin::SteamGridDb,
        path: "/art.png".into(),
    };
    store.set_artwork(&art(ArtLayout::Hero)).unwrap();
    store.set_artwork(&art(ArtLayout::Logo)).unwrap();

    store
        .delete_artwork(&id, ArtLayout::Hero, ArtOrigin::SteamGridDb)
        .unwrap();

    assert_eq!(store.artwork(&id).unwrap(), vec![art(ArtLayout::Logo)]);
}

#[test]
fn every_layout_can_be_stored() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let art = |layout: ArtLayout| Artwork {
        game_id: id.clone(),
        layout,
        origin: ArtOrigin::SteamGridDb,
        path: format!("/{layout}.png").into(),
    };

    for layout in ArtLayout::ALL {
        store.set_artwork(&art(*layout)).unwrap();
    }

    let mut stored = store.artwork(&id).unwrap();
    stored.sort_by_key(|art| art.layout);
    assert_eq!(
        stored,
        ArtLayout::ALL
            .iter()
            .map(|layout| art(*layout))
            .collect::<Vec<_>>()
    );
}

#[test]
fn artwork_is_kept_when_the_store_learns_square_art() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    for migration in [
        include_str!("migrations/0001_initial.sql"),
        include_str!("migrations/0002_steam_launch_dialog.sql"),
        include_str!("migrations/0003_launch_uri.sql"),
    ] {
        conn.execute_batch(migration).unwrap();
    }
    conn.pragma_update(None, "user_version", 3).unwrap();
    conn.execute_batch(
        "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
         VALUES ('steam:570', 'steam', '570', 'Dota 2', 'Dota 2', 1, 'store-uri');
         INSERT INTO artwork (game_id, layout, origin, path)
         VALUES ('steam:570', 'cover', 'source', '/cover.jpg');",
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();
    let id = GameId::new(Source::Steam, "570");
    let art = |layout, origin, path: &str| Artwork {
        game_id: id.clone(),
        layout,
        origin,
        path: path.into(),
    };
    store
        .set_artwork(&art(
            ArtLayout::Square,
            ArtOrigin::SteamGridDb,
            "/square.png",
        ))
        .unwrap();

    assert_eq!(
        store.artwork(&id).unwrap(),
        vec![
            art(ArtLayout::Cover, ArtOrigin::Source, "/cover.jpg"),
            art(ArtLayout::Square, ArtOrigin::SteamGridDb, "/square.png"),
        ]
    );
    store.delete_game(&id).unwrap();
    assert!(store.artwork(&id).unwrap().is_empty());
}

#[test]
fn a_games_steamgriddb_match_is_remembered_replaced_and_dropped_with_the_game() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");

    assert_eq!(store.steamgriddb_match(&id).unwrap(), None);
    store.set_steamgriddb_match(&id, 5248).unwrap();
    store.set_steamgriddb_match(&id, 38015).unwrap();
    assert_eq!(store.steamgriddb_match(&id).unwrap(), Some(38015));

    store.delete_game(&id).unwrap();
    assert_eq!(store.steamgriddb_match(&id).unwrap(), None);
}

#[test]
fn collections_are_listed_by_position() {
    let (_dir, store) = temp_store();
    let rpgs = store.create_collection("RPGs").unwrap();
    let shooters = store.create_collection("Shooters").unwrap();

    store
        .update_collection(&Collection {
            position: -1,
            ..shooters.clone()
        })
        .unwrap();

    let names: Vec<_> = store
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| c.name)
        .collect();
    assert_eq!(names, ["Shooters", "RPGs"]);
    assert_ne!(rpgs.id, shooters.id);
}

#[test]
fn a_new_collection_goes_last() {
    let (_dir, store) = temp_store();
    let first = store.create_collection("First").unwrap();

    let second = store.create_collection("Second").unwrap();

    assert!(second.position > first.position);
}

#[test]
fn renaming_a_collection_keeps_its_members() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let collection = store.create_collection("RPG").unwrap();
    store.add_to_collection(collection.id, &id).unwrap();

    store
        .update_collection(&Collection {
            name: "RPGs".into(),
            ..collection.clone()
        })
        .unwrap();

    assert_eq!(store.collections().unwrap()[0].name, "RPGs");
    assert_eq!(store.collection_members(collection.id).unwrap(), vec![id]);
}

#[test]
fn updating_an_unknown_collection_fails() {
    let (_dir, store) = temp_store();

    let result = store.update_collection(&Collection {
        id: 7,
        name: "Gone".into(),
        position: 0,
    });

    assert!(matches!(result, Err(StoreError::NotFound)));
}

#[test]
fn collection_membership_can_be_added_twice_and_removed() {
    let (_dir, store) = temp_store();
    let witcher = stored_game(&store, "witcher");
    let doom = stored_game(&store, "doom");
    let collection = store.create_collection("Favourites").unwrap();
    store.add_to_collection(collection.id, &witcher).unwrap();
    store.add_to_collection(collection.id, &witcher).unwrap();
    store.add_to_collection(collection.id, &doom).unwrap();

    store
        .remove_from_collection(collection.id, &witcher)
        .unwrap();

    assert_eq!(store.collection_members(collection.id).unwrap(), vec![doom]);
}

#[test]
fn deleting_a_collection_keeps_its_games() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let collection = store.create_collection("RPGs").unwrap();
    store.add_to_collection(collection.id, &id).unwrap();

    store.delete_collection(collection.id).unwrap();

    assert!(store.collections().unwrap().is_empty());
    assert!(store.game(&id).unwrap().is_some());
}

#[test]
fn play_sessions_are_listed_oldest_first_and_can_be_extended() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let later = store.add_play_session(&id, 2_000, 2_060).unwrap();
    let earlier = store.add_play_session(&id, 1_000, 1_030).unwrap();

    store.update_play_session_end(later.id, 2_090).unwrap();

    let sessions = store.play_sessions(&id).unwrap();
    assert_eq!(
        sessions,
        vec![
            earlier,
            PlaySession {
                ended_at: 2_090,
                duration_secs: 90,
                ..later
            },
        ]
    );
    assert_eq!(sessions[0].duration_secs, 30);
}

#[test]
fn extending_an_unknown_play_session_fails() {
    let (_dir, store) = temp_store();

    assert!(matches!(
        store.update_play_session_end(42, 10),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn collections_can_be_put_in_a_new_order() {
    let (_dir, store) = temp_store();
    let a = store.create_collection("A").unwrap();
    let b = store.create_collection("B").unwrap();
    let c = store.create_collection("C").unwrap();

    store.set_collection_order(&[c.id, a.id, b.id]).unwrap();

    let names: Vec<_> = store
        .collections()
        .unwrap()
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    assert_eq!(names, ["C", "A", "B"]);
}

#[test]
fn transactions_nest_into_the_outer_one() {
    let (_dir, store) = temp_store();
    let a = store.create_collection("A").unwrap();

    store
        .in_transaction(|store| {
            let b = store.create_collection("B")?;
            store.set_collection_order(&[b.id, a.id])
        })
        .unwrap();

    let names: Vec<_> = store
        .collections()
        .unwrap()
        .into_iter()
        .map(|collection| collection.name)
        .collect();
    assert_eq!(names, ["B", "A"]);
}

#[test]
fn memberships_list_every_games_collections() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    let a = store.create_collection("A").unwrap();
    let b = store.create_collection("B").unwrap();
    store.add_to_collection(b.id, &game.id).unwrap();
    store.add_to_collection(a.id, &game.id).unwrap();

    let memberships = store.collection_memberships().unwrap();

    assert_eq!(memberships.get(&game.id), Some(&vec![a.id, b.id]));
}

#[test]
fn imported_playtime_round_trips_and_is_replaced_on_write() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    let mut playtime = ImportedPlaytime {
        game_id: game.id.clone(),
        source: Source::Steam,
        total_secs: 3_600,
        last_played_at: Some(1_700_000_000),
    };
    store.set_imported_playtime(&playtime).unwrap();
    playtime.total_secs = 7_200;

    store.set_imported_playtime(&playtime).unwrap();

    assert_eq!(store.imported_playtime(&game.id).unwrap(), Some(playtime));
}

#[test]
fn source_state_round_trips_and_is_replaced_on_write() {
    let (_dir, store) = temp_store();
    let mut state = SourceState {
        source: Source::HeroicGog,
        enabled: false,
        resolved_location: Some("/home/me/.config/heroic".into()),
        user_override: Some("/mnt/heroic".into()),
        last_sync: Some(1_700_000_000),
        errors: vec!["installed.json: unexpected end of file".into()],
    };
    store.set_source_state(&state).unwrap();
    state.errors.clear();

    store.set_source_state(&state).unwrap();

    assert_eq!(
        store.source_state(Source::HeroicGog).unwrap(),
        Some(state.clone())
    );
    assert_eq!(store.source_states().unwrap(), vec![state]);
}

#[test]
fn deleting_a_game_deletes_everything_attached_to_it() {
    let (_dir, store) = temp_store();
    let id = stored_game(&store, "witcher");
    let collection = store.create_collection("RPGs").unwrap();
    store.add_to_collection(collection.id, &id).unwrap();
    store
        .set_launch_config(&id, &LaunchConfig::default())
        .unwrap();
    store
        .set_artwork(&Artwork {
            game_id: id.clone(),
            layout: ArtLayout::Icon,
            origin: ArtOrigin::Generated,
            path: "/icon.png".into(),
        })
        .unwrap();
    store.add_play_session(&id, 1, 2).unwrap();
    store
        .set_imported_playtime(&ImportedPlaytime {
            game_id: id.clone(),
            source: Source::Manual,
            total_secs: 1,
            last_played_at: None,
        })
        .unwrap();

    store.delete_game(&id).unwrap();

    assert!(store.collection_members(collection.id).unwrap().is_empty());
    assert_eq!(store.launch_config(&id).unwrap(), None);
    assert!(store.artwork(&id).unwrap().is_empty());
    assert!(store.play_sessions(&id).unwrap().is_empty());
    assert_eq!(store.imported_playtime(&id).unwrap(), None);
}

#[test]
fn data_for_an_unknown_game_is_rejected() {
    let (_dir, store) = temp_store();
    let unknown = GameId::new(Source::Steam, "404");

    assert!(
        store
            .set_launch_config(&unknown, &LaunchConfig::default())
            .is_err()
    );
    assert!(store.add_play_session(&unknown, 1, 2).is_err());
}

#[test]
fn a_saved_filter_is_a_collection_with_its_filter() {
    use crate::library::{Filters, View};
    use crate::saved_filters::SavedFilter;

    let (_dir, store) = temp_store();
    let manual = store.create_collection("Manual").unwrap();
    let filter = SavedFilter {
        view: View::Source(Source::Lutris),
        filters: Filters {
            installed: true,
            not_installed: true,
            favourites: true,
            windows: true,
            native: true,
            never_played: true,
            genre: Some("Role-playing".to_owned()),
            hidden: false,
        },
        search: "witcher".to_owned(),
    };

    let saved = store.create_saved_filter("Witchers", &filter).unwrap();

    assert_eq!(saved.name, "Witchers");
    assert_eq!(saved.position, manual.position + 1);
    assert_eq!(
        store.collections().unwrap(),
        [manual.clone(), saved.clone()]
    );
    assert_eq!(
        store.saved_filters().unwrap(),
        std::collections::HashMap::from([(saved.id, filter)])
    );

    store.delete_collection(saved.id).unwrap();
    assert!(store.saved_filters().unwrap().is_empty());
    assert_eq!(store.collections().unwrap(), [manual]);
}

#[test]
fn a_saved_filter_with_a_view_this_version_does_not_know_is_left_out() {
    let (_dir, store) = temp_store();
    let saved = store
        .create_saved_filter("Later", &crate::saved_filters::Suggestion::Native.filter())
        .unwrap();
    store
        .conn
        .execute("UPDATE saved_filters SET view = 'future:1'", [])
        .unwrap();

    assert!(store.saved_filters().unwrap().is_empty());
    assert!(store.collections().unwrap().contains(&saved));
}

fn metadata(developer: &str) -> crate::metadata::Metadata {
    crate::metadata::Metadata {
        developer: Some(developer.into()),
        ..Default::default()
    }
}

#[test]
fn a_game_shows_the_users_metadata_over_a_providers_over_the_sources() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    let id = &game.id;

    store
        .set_metadata(id, MetadataOrigin::Source, &metadata("Valve"), 10)
        .unwrap();
    assert_eq!(
        store.game(id).unwrap().unwrap().developer.as_deref(),
        Some("Valve")
    );
    store
        .set_metadata(id, MetadataOrigin::User, &metadata("Me"), 20)
        .unwrap();
    store
        .set_metadata(
            id,
            MetadataOrigin::Provider,
            &metadata("Valve Corporation"),
            30,
        )
        .unwrap();

    assert_eq!(
        store.game(id).unwrap().unwrap().developer.as_deref(),
        Some("Me")
    );
    assert_eq!(store.games().unwrap()[0].developer.as_deref(), Some("Me"));
    assert_eq!(
        store.games_from(Source::Steam).unwrap()[0]
            .developer
            .as_deref(),
        Some("Me")
    );
    store
        .set_metadata(id, MetadataOrigin::User, &Default::default(), 40)
        .unwrap();
    assert_eq!(
        store.game(id).unwrap().unwrap().developer.as_deref(),
        Some("Valve Corporation")
    );
}

#[test]
fn metadata_values_keep_their_origin_and_the_time_they_last_changed() {
    use crate::metadata::{FieldValue, MetadataValue};
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    let first = crate::metadata::Metadata {
        developer: Some("Valve".into()),
        genres: vec!["Strategy".into(), "MOBA".into()],
        description: Some("Every day".into()),
        deck_compatibility: None,
    };
    store
        .set_metadata(&game.id, MetadataOrigin::Source, &first, 10)
        .unwrap();

    let second = crate::metadata::Metadata {
        description: Some("Every night".into()),
        genres: Vec::new(),
        ..first
    };
    store
        .set_metadata(&game.id, MetadataOrigin::Source, &second, 20)
        .unwrap();

    assert_eq!(
        store.metadata_values(&game.id).unwrap(),
        [
            MetadataValue {
                value: FieldValue::Description("Every night".into()),
                origin: MetadataOrigin::Source,
                updated_at: 20,
            },
            MetadataValue {
                value: FieldValue::Developer("Valve".into()),
                origin: MetadataOrigin::Source,
                updated_at: 10,
            },
        ]
    );
}

#[test]
fn updating_a_game_leaves_its_metadata_alone() {
    let (_dir, store) = temp_store();
    let mut game = steam_game("570", "Dota 2");
    game.developer = Some("Valve".into());
    store.insert_game(&game).unwrap();
    store
        .set_metadata(&game.id, MetadataOrigin::User, &metadata("Me"), 20)
        .unwrap();

    let mut shown = store.game(&game.id).unwrap().unwrap();
    shown.favourite = true;
    store.update_game(&shown).unwrap();
    store
        .set_metadata(&game.id, MetadataOrigin::User, &Default::default(), 30)
        .unwrap();

    let game = store.game(&game.id).unwrap().unwrap();
    assert!(game.favourite);
    assert_eq!(game.developer.as_deref(), Some("Valve"));
}

#[test]
fn deleting_a_game_deletes_its_metadata() {
    let (_dir, store) = temp_store();
    let game = steam_game("570", "Dota 2");
    store.insert_game(&game).unwrap();
    store
        .set_metadata(&game.id, MetadataOrigin::User, &metadata("Me"), 20)
        .unwrap();

    store.delete_game(&game.id).unwrap();

    assert!(store.metadata_values(&game.id).unwrap().is_empty());
}

#[test]
fn a_games_metadata_columns_move_into_source_values_when_migrated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    for migration in &MIGRATIONS[..7] {
        conn.execute_batch(migration).unwrap();
    }
    conn.pragma_update(None, "user_version", 7).unwrap();
    conn.execute_batch(
        "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind,
             developer, genres, description)
         VALUES ('steam:570', 'steam', '570', 'Dota 2', 'Dota 2', 1, 'store-uri',
             'Valve', '[\"Strategy\",\"MOBA\"]', 'Every day');
         INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
         VALUES ('steam:440', 'steam', '440', 'TF2', 'TF2', 1, 'store-uri');",
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&path).unwrap();

    let dota = store
        .game(&GameId::new(Source::Steam, "570"))
        .unwrap()
        .unwrap();
    assert_eq!(dota.developer.as_deref(), Some("Valve"));
    assert_eq!(dota.genres, ["Strategy", "MOBA"]);
    assert_eq!(dota.description.as_deref(), Some("Every day"));
    let values = store.metadata_values(&dota.id).unwrap();
    assert_eq!(values.len(), 3);
    assert!(values.iter().all(|v| v.origin == MetadataOrigin::Source));
    let tf2 = GameId::new(Source::Steam, "440");
    assert!(store.metadata_values(&tf2).unwrap().is_empty());
}
