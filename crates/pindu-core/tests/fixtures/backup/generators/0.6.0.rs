//! Writes `0.6.0.json`: a small library built through pindu-core 0.6.0's own API and
//! exported with its own `backup::export`. Not built here; see `../README.md`.
//!
//! Copy it to `crates/pindu-core/examples/backup_fixture.rs` in a checkout of `v0.6.0`
//! and run `cargo run -p pindu-core --example backup_fixture -- <new folder>`; the
//! backup goes to standard output.

use std::path::PathBuf;

use pindu_core::backup;
use pindu_core::library::{Filters, View};
use pindu_core::manual::{self, ManualGame, WindowsOptions};
use pindu_core::metadata::{self, Metadata};
use pindu_core::model::{
    ControllerSupport, DeckCompatibility, DeckRating, Game, GameId, LaunchKind,
    MetadataOrigin, PerformanceTool, Source,
};
use pindu_core::proton::Defaults;
use pindu_core::saved_filters::SavedFilter;
use pindu_core::search;
use pindu_core::store::Store;

/// 2026-01-01 00:00 UTC, so the fixture's times do not depend on the day it is made.
const NOW: i64 = 1_767_225_600;
const DAY: i64 = 86_400;

fn main() {
    let folder = PathBuf::from(std::env::args().nth(1).expect("a new folder for the library"));
    assert!(!folder.exists(), "the library folder must be new");
    let store = Store::open(&folder.join("library.db")).unwrap();

    // What a sync of the launchers adds.
    let synced = |source, native_id: &str, title: &str| {
        let mut game = Game::new(
            GameId::new(source, native_id),
            title,
            LaunchKind::StoreUri,
            NOW - 30 * DAY,
        );
        game.installed = true;
        store.insert_game(&game).unwrap();
        game.id
    };
    let quest = synced(Source::Steam, "4000001", "Fixture Quest");
    let racer = synced(Source::HeroicGog, "1400000001", "Fixture Racer");
    let caves = synced(Source::Lutris, "fixture-caves", "Fixture Caves");
    let demo = synced(Source::Faugus, "fixture-demo", "Fixture Demo");
    synced(Source::Steam, "4000002", "Fixture Untouched");
    // Other copies: of Fixture Quest, grouped by hand; of Fixture Arena, grouped from a
    // store cross-reference; of Fixture Caves, split by the user as not the same game.
    let quest_gog = synced(Source::HeroicGog, "1400000002", "Fixture Quest");
    let arena_epic = synced(Source::HeroicEpic, "fixturearena", "Fixture Arena");
    let arena_legendary = synced(Source::Legendary, "fixturearena", "Fixture Arena");
    let caves_desktop = synced(Source::Desktop, "fixture-caves.desktop", "Fixture Caves");

    // Favourites, pins in "Jump back in" and a hidden game.
    let edit = |id: &GameId, change: &dyn Fn(&mut Game)| {
        let mut game = store.game(id).unwrap().unwrap();
        change(&mut game);
        store.update_game(&game).unwrap();
    };
    edit(&quest, &|game| {
        game.favourite = true;
        game.pin_order = Some(1);
    });
    edit(&racer, &|game| {
        game.favourite = true;
        game.pin_order = Some(2);
    });
    edit(&demo, &|game| game.hidden = true);

    // Games added by hand, a native one run on the host and a Windows one.
    let defaults = Defaults {
        proton_version: None,
        prefixes: PathBuf::from("/var/fixture/prefixes"),
        mangohud: false,
        gamemode: true,
    };
    let tool = manual::add_game(
        &store,
        &ManualGame {
            title: "Fixture Native Tool".to_owned(),
            folder: PathBuf::from("/var/fixture/games/native-tool"),
            executable: "bin/tool".to_owned(),
            arguments: "--fullscreen --name \"two words\"".to_owned(),
            working_dir: "bin".to_owned(),
            host: true,
            windows: None,
        },
        &defaults,
        NOW - 20 * DAY,
    )
    .unwrap();
    let port = manual::add_game(
        &store,
        &ManualGame {
            title: "Fixture Windows Port".to_owned(),
            folder: PathBuf::from("/var/fixture/games/windows-port"),
            executable: "Port.exe".to_owned(),
            arguments: String::new(),
            working_dir: String::new(),
            host: false,
            windows: Some(WindowsOptions {
                proton_version: Some("GE-Proton9-20".to_owned()),
                prefix: None,
            }),
        },
        &defaults,
        NOW - 10 * DAY,
    )
    .unwrap();
    edit(&tool, &|game| {
        game.favourite = true;
        game.last_played_at = Some(NOW - DAY);
    });

    // Launch settings set after adding: environment, a wrapper and the overlay.
    let mut config = store.launch_config(&port).unwrap().unwrap();
    config
        .environment
        .insert("DXVK_HUD".to_owned(), "fps".to_owned());
    config.wrappers.push("obs-gamecapture".to_owned());
    store.set_launch_config(&port, &config).unwrap();
    store
        .set_performance_tool(&port, PerformanceTool::Overlay, true)
        .unwrap();

    // Collections, one of them empty.
    let couch = store.create_collection("Couch Co-op").unwrap();
    for id in [&quest, &racer, &tool] {
        store.add_to_collection(couch.id, id).unwrap();
    }
    let weekend = store.create_collection("Weekend").unwrap();
    store.add_to_collection(weekend.id, &caves).unwrap();
    store.create_collection("Empty Shelf").unwrap();

    // Play sessions tracked here.
    for (id, start, minutes) in [
        (&quest, NOW - 5 * DAY, 90),
        (&quest, NOW - 2 * DAY, 45),
        (&caves, NOW - 3 * DAY, 30),
        (&tool, NOW - DAY, 15),
        (&port, NOW - 4 * DAY, 120),
    ] {
        store
            .add_play_session(id, start, start + minutes * 60)
            .unwrap();
    }

    // Groups: one by hand with a preferred copy, one automatic; and a split pair.
    let group = store.create_group(&[quest.clone(), quest_gog.clone()]).unwrap();
    store.set_preferred_copy(group, Some(&quest_gog)).unwrap();
    store
        .create_automatic_group(&arena_epic, &arena_legendary)
        .unwrap()
        .unwrap();
    store.remember_split(&caves, &caves_desktop).unwrap();

    // Saved filters: one over every game, one over a store, one over a collection.
    store
        .create_saved_filter(
            "Unplayed Puzzles",
            &SavedFilter {
                view: View::All,
                filters: Filters {
                    native: true,
                    never_played: true,
                    genre: Some("Puzzle".to_owned()),
                    ..Filters::default()
                },
                search: search::normalize("Fixture"),
            },
        )
        .unwrap();
    store
        .create_saved_filter(
            "Installed on Steam",
            &SavedFilter {
                view: View::Source(Source::Steam),
                filters: Filters {
                    installed: true,
                    ..Filters::default()
                },
                search: String::new(),
            },
        )
        .unwrap();
    store
        .create_saved_filter(
            "Couch Favourites",
            &SavedFilter {
                view: View::Collection(couch.id),
                filters: Filters {
                    favourites: true,
                    ..Filters::default()
                },
                search: String::new(),
            },
        )
        .unwrap();

    // Metadata: what a sync gave (not backed up), what a provider fetched, and what the
    // user wrote over Fixture Quest's details, which goes to both its copies.
    let from_source = Metadata {
        developer: Some("Fixture Source Studio".to_owned()),
        genres: vec!["Action".to_owned()],
        ..Metadata::default()
    };
    store
        .set_metadata(&quest, MetadataOrigin::Source, &from_source, NOW - 30 * DAY)
        .unwrap();
    let fetched = Metadata {
        description: Some("A fixture game about caves.".to_owned()),
        genres: vec!["Adventure".to_owned()],
        release_year: Some(2008),
        ..Metadata::default()
    };
    store
        .set_metadata(&caves, MetadataOrigin::Provider, &fetched, NOW - 9 * DAY)
        .unwrap();
    let rated = Metadata {
        deck_compatibility: Some(DeckCompatibility {
            rating: DeckRating::Verified,
            tested_at: Some(NOW - 100 * DAY),
        }),
        ..Metadata::default()
    };
    store
        .set_metadata(&quest, MetadataOrigin::Provider, &rated, NOW - 8 * DAY)
        .unwrap();
    let shown = Metadata::of(&store.game(&quest).unwrap().unwrap());
    let edited = Metadata {
        title: Some("Fixture Quest: Director's Cut".to_owned()),
        developer: Some("Fixture Studio".to_owned()),
        genres: vec!["Puzzle".to_owned(), "Adventure".to_owned()],
        release_year: Some(2019),
        controller_support: Some(ControllerSupport::Full),
        ..shown.clone()
    };
    metadata::edit_by_user(&store, &quest, &shown, &edited, NOW - 7 * DAY).unwrap();

    println!("{}", backup::export(&store, NOW).unwrap());
}
