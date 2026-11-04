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
