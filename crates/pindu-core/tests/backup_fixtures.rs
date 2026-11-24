//! Backups as released versions wrote them, imported by this one. Each fixture in
//! `fixtures/backup/` was exported by the release it is named for, from the library its
//! generator in `fixtures/backup/generators/` built (see `fixtures/backup/README.md`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use pindu_core::backup::{self, ImportSummary};
use pindu_core::library::{Filters, View};
use pindu_core::metadata::{FieldValue, MetadataValue};
use pindu_core::model::{
    ControllerSupport, DeckCompatibility, DeckRating, Game, GameId, LaunchConfig, LaunchKind,
    MetadataOrigin, Source, Timestamp,
};
use pindu_core::saved_filters::SavedFilter;
use pindu_core::store::Store;

/// The time the generators build their library at.
const NOW: Timestamp = 1_767_225_600;
const DAY: Timestamp = 86_400;

/// The launchers' games the fixtures' libraries had, as a sync adds them again.
const SYNCED: [(Source, &str, &str); 9] = [
    (Source::Steam, "4000001", "Fixture Quest"),
    (Source::HeroicGog, "1400000001", "Fixture Racer"),
    (Source::Lutris, "fixture-caves", "Fixture Caves"),
    (Source::Faugus, "fixture-demo", "Fixture Demo"),
    (Source::Steam, "4000002", "Fixture Untouched"),
    (Source::HeroicGog, "1400000002", "Fixture Quest"),
    (Source::HeroicEpic, "fixturearena", "Fixture Arena"),
    (Source::Legendary, "fixturearena", "Fixture Arena"),
    (Source::Desktop, "fixture-caves.desktop", "Fixture Caves"),
];

/// A library the launchers have synced, with nothing the user made.
fn synced_library() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    for (source, native_id, title) in SYNCED {
        let mut game = Game::new(
            GameId::new(source, native_id),
            title,
            LaunchKind::StoreUri,
            NOW,
        );
        game.installed = true;
        store.insert_game(&game).unwrap();
    }
    (dir, store)
}

/// The fixture `release` wrote, imported into a freshly synced library.
fn import_fixture(release: &str) -> (tempfile::TempDir, Store, ImportSummary) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/backup")
        .join(format!("{release}.json"));
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
    let (dir, store) = synced_library();
    let summary = backup::import(&store, &text).unwrap();
    (dir, store, summary)
}

fn id(text: &str) -> GameId {
    text.parse().unwrap()
}

/// The game added by hand called `title`.
fn added_by_hand(store: &Store, title: &str) -> GameId {
    let games = store.games().unwrap();
    let mut found = games
        .iter()
        .filter(|game| game.id.source() == Source::Manual && game.title == title);
    let game = found.next().unwrap_or_else(|| panic!("no {title}"));
    assert!(found.next().is_none(), "two of {title}");
    game.id.clone()
}

fn sorted(mut ids: Vec<GameId>) -> Vec<GameId> {
    ids.sort_unstable();
    ids
}

/// The user state both formats carry, as both generators made it.
fn assert_format_1_state(store: &Store) {
    let quest = id("steam:4000001");
    let racer = id("heroic-gog:1400000001");
    let caves = id("lutris:fixture-caves");
    let tool = added_by_hand(store, "Fixture Native Tool");
    let port = added_by_hand(store, "Fixture Windows Port");
    let games = store.games().unwrap();
    let marked = |keep: fn(&Game) -> bool| {
        sorted(
            games
                .iter()
                .filter(|game| keep(game))
                .map(|game| game.id.clone())
                .collect(),
        )
    };
    assert_eq!(
        marked(|game| game.favourite),
        sorted(vec![quest.clone(), racer.clone(), tool.clone()])
    );
    assert_eq!(marked(|game| game.hidden), [id("faugus:fixture-demo")]);

    let members: BTreeMap<String, Vec<GameId>> = store
        .collections()
        .unwrap()
        .into_iter()
        .map(|c| (c.name, sorted(store.collection_members(c.id).unwrap())))
        .collect();
    assert_eq!(
        members["Couch Co-op"],
        sorted(vec![quest.clone(), racer.clone(), tool.clone()])
    );
    assert_eq!(members["Weekend"], [id("lutris:fixture-caves")]);
    assert_eq!(members["Empty Shelf"], []);

    let restored = store.game(&tool).unwrap().unwrap();
    assert_eq!(
        (
            restored.installed,
            restored.install_path,
            restored.added_at,
            restored.last_played_at
        ),
        (
            true,
            Some(PathBuf::from("/var/fixture/games/native-tool")),
            NOW - 20 * DAY,
            Some(NOW - DAY)
        )
    );
    assert_eq!(
        store.launch_config(&tool).unwrap(),
        Some(LaunchConfig {
            executable: PathBuf::from("/var/fixture/games/native-tool/bin/tool"),
            arguments: vec!["--fullscreen".into(), "--name".into(), "two words".into()],
            working_dir: Some(PathBuf::from("/var/fixture/games/native-tool/bin")),
            ..LaunchConfig::default()
        })
    );
    let restored = store.game(&port).unwrap().unwrap();
    assert_eq!(
        (
            restored.install_path,
            restored.added_at,
            restored.last_played_at
        ),
        (
            Some(PathBuf::from("/var/fixture/games/windows-port")),
            NOW - 10 * DAY,
            None
        )
    );
    assert_eq!(
        store.launch_config(&port).unwrap(),
        Some(LaunchConfig {
            executable: PathBuf::from("/var/fixture/games/windows-port/Port.exe"),
            arguments: Vec::new(),
            working_dir: Some(PathBuf::from("/var/fixture/games/windows-port")),
            environment: BTreeMap::from([("DXVK_HUD".to_owned(), "fps".to_owned())]),
            proton_version: Some("GE-Proton9-20".to_owned()),
            prefix_path: Some(PathBuf::from(format!(
                "/var/fixture/prefixes/{}",
                port.native_id()
            ))),
            mangohud: true,
            gamemode: true,
            wrappers: vec!["obs-gamecapture".to_owned()],
        })
    );

    let sessions = |game: &GameId| -> Vec<(Timestamp, Timestamp)> {
        store
            .play_sessions(game)
            .unwrap()
            .iter()
            .map(|session| (session.started_at, session.ended_at))
            .collect()
    };
    let played = |start: Timestamp, minutes: Timestamp| (start, start + minutes * 60);
    assert_eq!(
        sessions(&quest),
        [played(NOW - 5 * DAY, 90), played(NOW - 2 * DAY, 45)]
    );
    assert_eq!(sessions(&caves), [played(NOW - 3 * DAY, 30)]);
    assert_eq!(sessions(&tool), [played(NOW - DAY, 15)]);
    assert_eq!(sessions(&port), [played(NOW - 4 * DAY, 120)]);
    assert_eq!(sessions(&racer), []);
}

#[test]
fn a_backup_0_5_1_wrote_in_format_1_restores_everything_it_holds() {
    let (_dir, store, summary) = import_fixture("0.5.1");

    assert_eq!(
        summary,
        ImportSummary {
            manual_games: 2,
            favourites: 3,
            hidden: 1,
            collections: 3,
            sessions: 5,
            ..ImportSummary::default()
        }
    );
    assert_format_1_state(&store);
}

/// The user's and providers' metadata values of `game`, by origin and field.
fn carried_metadata(store: &Store, game: &GameId) -> Vec<MetadataValue> {
    let mut values: Vec<MetadataValue> = store
        .metadata_values(game)
        .unwrap()
        .into_iter()
        .filter(|value| value.origin != MetadataOrigin::Source)
        .collect();
    values.sort_by_key(|value| (value.origin.as_str(), value.value.field().as_str()));
    values
}

fn metadata(
    origin: MetadataOrigin,
    updated_at: Timestamp,
    values: Vec<FieldValue>,
) -> Vec<MetadataValue> {
    let mut values: Vec<MetadataValue> = values
        .into_iter()
        .map(|value| MetadataValue {
            value,
            origin,
            updated_at,
        })
        .collect();
    values.sort_by_key(|value| (value.origin.as_str(), value.value.field().as_str()));
    values
}

#[test]
fn a_backup_0_6_0_wrote_in_format_2_restores_everything_it_holds() {
    let (_dir, store, summary) = import_fixture("0.6.0");

    assert_eq!(
        summary,
        ImportSummary {
            manual_games: 2,
            favourites: 3,
            hidden: 1,
            collections: 6,
            sessions: 5,
            groups: 2,
            splits: 1,
            metadata: 14,
            ..ImportSummary::default()
        }
    );
    assert_format_1_state(&store);

    let quest = id("steam:4000001");
    let quest_gog = id("heroic-gog:1400000002");
    let mut groups: Vec<_> = store
        .game_groups()
        .unwrap()
        .into_iter()
        .map(|group| (sorted(group.members), group.preferred, group.automatic))
        .collect();
    groups.sort_unstable();
    assert_eq!(
        groups,
        [
            (
                vec![id("heroic-epic:fixturearena"), id("legendary:fixturearena")],
                None,
                true
            ),
            (
                sorted(vec![quest.clone(), quest_gog.clone()]),
                Some(quest_gog.clone()),
                false
            ),
        ]
    );
    assert!(
        store
            .is_split(
                &id("lutris:fixture-caves"),
                &id("desktop:fixture-caves.desktop")
            )
            .unwrap()
    );

    let collections = store.collections().unwrap();
    let couch = collections
        .iter()
        .find(|c| c.name == "Couch Co-op")
        .unwrap()
        .id;
    let saved = store.saved_filters().unwrap();
    let saved: BTreeMap<String, SavedFilter> = collections
        .into_iter()
        .filter_map(|c| Some((c.name, saved.get(&c.id)?.clone())))
        .collect();
    assert_eq!(
        saved,
        BTreeMap::from([
            (
                "Unplayed Puzzles".to_owned(),
                SavedFilter {
                    view: View::All,
                    filters: Filters {
                        native: true,
                        never_played: true,
                        genre: Some("Puzzle".to_owned()),
                        ..Filters::default()
                    },
                    search: "fixture".to_owned(),
                }
            ),
            (
                "Installed on Steam".to_owned(),
                SavedFilter {
                    view: View::Source(Source::Steam),
                    filters: Filters {
                        installed: true,
                        ..Filters::default()
                    },
                    search: String::new(),
                }
            ),
            (
                "Couch Favourites".to_owned(),
                SavedFilter {
                    view: View::Collection(couch),
                    filters: Filters {
                        favourites: true,
                        ..Filters::default()
                    },
                    search: String::new(),
                }
            ),
        ])
    );

    let edited = || {
        metadata(
            MetadataOrigin::User,
            NOW - 7 * DAY,
            vec![
                FieldValue::Title("Fixture Quest: Director's Cut".to_owned()),
                FieldValue::Developer("Fixture Studio".to_owned()),
                FieldValue::Genres(vec!["Puzzle".to_owned(), "Adventure".to_owned()]),
                FieldValue::ReleaseYear(2019),
                FieldValue::ControllerSupport(ControllerSupport::Full),
            ],
        )
    };
    let mut expected = edited();
    expected.extend(metadata(
        MetadataOrigin::Provider,
        NOW - 8 * DAY,
        vec![FieldValue::DeckCompatibility(DeckCompatibility {
            rating: DeckRating::Verified,
            tested_at: Some(NOW - 100 * DAY),
        })],
    ));
    expected.sort_by_key(|value| (value.origin.as_str(), value.value.field().as_str()));
    assert_eq!(carried_metadata(&store, &quest), expected);
    assert_eq!(carried_metadata(&store, &quest_gog), edited());
    assert_eq!(
        carried_metadata(&store, &id("lutris:fixture-caves")),
        metadata(
            MetadataOrigin::Provider,
            NOW - 9 * DAY,
            vec![
                FieldValue::Description("A fixture game about caves.".to_owned()),
                FieldValue::Genres(vec!["Adventure".to_owned()]),
                FieldValue::ReleaseYear(2008),
            ],
        )
    );
    assert_eq!(
        store.game(&quest).unwrap().unwrap().custom_title.as_deref(),
        Some("Fixture Quest: Director's Cut")
    );
}
