use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::model::*;
use crate::sources::sync::apply_snapshot;
use crate::sources::{ImportedGame, ServiceRef, Snapshot};

fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    (dir, store)
}

fn stored(store: &Store, source: Source, native_id: &str) -> GameId {
    let game = Game::new(
        GameId::new(source, native_id),
        native_id,
        LaunchKind::StoreUri,
        1_700_000_000,
    );
    store.insert_game(&game).unwrap();
    game.id
}

/// Three stored games: a Steam, a GOG and a Lutris copy.
fn three(store: &Store) -> [GameId; 3] {
    [
        stored(store, Source::Steam, "570"),
        stored(store, Source::HeroicGog, "1207"),
        stored(store, Source::Lutris, "dota"),
    ]
}

#[test]
fn grouping_copies_lists_them_as_one_group() {
    let (_dir, store) = temp_store();
    let steam = stored(&store, Source::Steam, "570");
    let gog = stored(&store, Source::HeroicGog, "1207");

    let id = store.create_group(&[steam.clone(), gog.clone()]).unwrap();

    let group = GameGroup {
        id,
        members: vec![gog.clone(), steam.clone()],
        preferred: None,
        automatic: false,
    };
    assert_eq!(store.game_groups().unwrap(), vec![group.clone()]);
    assert_eq!(store.game_group(id).unwrap(), Some(group.clone()));
    assert_eq!(store.game_group_of(&steam).unwrap(), Some(group));
}

#[test]
fn a_game_in_no_group_has_none() {
    let (_dir, store) = temp_store();
    let [steam, ..] = three(&store);

    assert_eq!(store.game_group_of(&steam).unwrap(), None);
    assert_eq!(store.game_group(1).unwrap(), None);
}

#[test]
fn a_group_needs_two_different_stored_games() {
    let (_dir, store) = temp_store();
    let [steam, ..] = three(&store);

    assert!(matches!(
        store.create_group(std::slice::from_ref(&steam)),
        Err(StoreError::TooFewCopies)
    ));
    assert!(matches!(
        store.create_group(&[steam.clone(), steam.clone()]),
        Err(StoreError::TooFewCopies)
    ));
    assert!(matches!(
        store.create_group(&[steam, GameId::new(Source::Steam, "404")]),
        Err(StoreError::NotFound)
    ));
    assert!(store.game_groups().unwrap().is_empty());
}

#[test]
fn a_game_is_in_at_most_one_group() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store.create_group(&[steam.clone(), gog.clone()]).unwrap();

    let result = store.create_group(&[lutris.clone(), gog.clone()]);

    assert!(matches!(result, Err(StoreError::AlreadyGrouped(id)) if id == gog));
    assert!(matches!(
        store.add_to_group(group, &steam),
        Err(StoreError::AlreadyGrouped(_))
    ));
    assert_eq!(store.game_groups().unwrap().len(), 1);
    assert_eq!(store.game_group_of(&lutris).unwrap(), None);
}

#[test]
fn a_copy_can_join_and_leave_a_group() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store.create_group(&[steam.clone(), gog.clone()]).unwrap();

    store.add_to_group(group, &lutris).unwrap();
    assert_eq!(
        store.game_group(group).unwrap().unwrap().members,
        vec![gog.clone(), lutris.clone(), steam.clone()]
    );

    store.remove_from_group(&gog).unwrap();
    assert_eq!(
        store.game_group(group).unwrap().unwrap().members,
        vec![lutris.clone(), steam.clone()]
    );
    assert_eq!(store.game_group_of(&gog).unwrap(), None);
    // Taking out a game in no group changes nothing.
    store.remove_from_group(&gog).unwrap();

    assert!(matches!(
        store.add_to_group(group + 1, &gog),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn a_group_left_with_one_copy_is_removed() {
    let (_dir, store) = temp_store();
    let [steam, gog, _] = three(&store);
    store.create_group(&[steam.clone(), gog.clone()]).unwrap();

    store.remove_from_group(&gog).unwrap();

    assert!(store.game_groups().unwrap().is_empty());
    assert_eq!(store.game_group_of(&steam).unwrap(), None);
}

#[test]
fn the_preferred_copy_is_one_of_the_groups_games() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store.create_group(&[steam.clone(), gog.clone()]).unwrap();

    store.set_preferred_copy(group, Some(&gog)).unwrap();
    assert_eq!(
        store.game_group(group).unwrap().unwrap().preferred,
        Some(gog.clone())
    );

    assert!(matches!(
        store.set_preferred_copy(group, Some(&lutris)),
        Err(StoreError::NotInGroup(id)) if id == lutris
    ));
    assert!(matches!(
        store.set_preferred_copy(group + 1, None),
        Err(StoreError::NotFound)
    ));

    store.set_preferred_copy(group, None).unwrap();
    assert_eq!(store.game_group(group).unwrap().unwrap().preferred, None);
}

#[test]
fn a_preferred_copy_that_leaves_the_group_is_forgotten() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store
        .create_group(&[steam.clone(), gog.clone(), lutris.clone()])
        .unwrap();
    store.set_preferred_copy(group, Some(&gog)).unwrap();

    store.remove_from_group(&gog).unwrap();

    assert_eq!(store.game_group(group).unwrap().unwrap().preferred, None);
}

#[test]
fn deleting_a_game_takes_it_out_of_its_group() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store
        .create_group(&[steam.clone(), gog.clone(), lutris.clone()])
        .unwrap();
    store.set_preferred_copy(group, Some(&gog)).unwrap();
    store.remember_split(&gog, &lutris).unwrap();

    store.delete_game(&gog).unwrap();

    let left = store.game_group(group).unwrap().unwrap();
    assert_eq!(left.members, vec![lutris.clone(), steam.clone()]);
    assert_eq!(left.preferred, None);
    assert!(store.splits().unwrap().is_empty());

    store.delete_game(&lutris).unwrap();
    assert!(store.game_groups().unwrap().is_empty());
}

#[test]
fn a_split_is_remembered_in_either_order() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);

    store.remember_split(&steam, &gog).unwrap();
    store.remember_split(&gog, &steam).unwrap();

    assert!(store.is_split(&steam, &gog).unwrap());
    assert!(store.is_split(&gog, &steam).unwrap());
    assert!(!store.is_split(&steam, &lutris).unwrap());
    assert_eq!(store.splits().unwrap(), vec![(gog, steam)]);
}

#[test]
fn splitting_a_group_remembers_every_pair() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store
        .create_group(&[steam.clone(), gog.clone(), lutris.clone()])
        .unwrap();

    store.split_group(group).unwrap();

    assert!(store.game_groups().unwrap().is_empty());
    assert_eq!(
        store.splits().unwrap(),
        vec![
            (gog.clone(), lutris.clone()),
            (gog, steam.clone()),
            (lutris, steam)
        ]
    );
    assert!(matches!(
        store.split_group(group),
        Err(StoreError::NotFound)
    ));
}

#[test]
fn an_automatic_group_is_made_once_and_its_undo_sticks() {
    let (_dir, store) = temp_store();
    let [steam, _, lutris] = three(&store);

    let group = store
        .create_automatic_group(&lutris, &steam)
        .unwrap()
        .unwrap();
    assert!(store.game_group(group).unwrap().unwrap().automatic);
    // Either copy is in a group already.
    assert_eq!(store.create_automatic_group(&lutris, &steam).unwrap(), None);

    store.split_group(group).unwrap();

    assert_eq!(store.create_automatic_group(&steam, &lutris).unwrap(), None);
    assert!(store.game_groups().unwrap().is_empty());
}

#[test]
fn changing_an_automatic_groups_copies_makes_it_the_users() {
    let (_dir, store) = temp_store();
    let [steam, gog, lutris] = three(&store);
    let group = store
        .create_automatic_group(&lutris, &steam)
        .unwrap()
        .unwrap();
    store.set_preferred_copy(group, Some(&lutris)).unwrap();
    assert!(store.game_group(group).unwrap().unwrap().automatic);

    store.add_to_group(group, &gog).unwrap();

    assert!(!store.game_group(group).unwrap().unwrap().automatic);
}

#[test]
fn source_details_round_trip_and_go_with_the_game() {
    let (_dir, store) = temp_store();
    let [_, _, lutris] = three(&store);
    let details = SourceDetails {
        runner: Some("steam".into()),
        service: Some(ServiceRef {
            service: "steam".into(),
            service_id: "570".into(),
        }),
        year: Some(2013),
    };

    store.set_source_details(&lutris, &details).unwrap();
    assert_eq!(store.source_details(&lutris).unwrap(), details);
    assert_eq!(
        store.all_source_details().unwrap(),
        std::collections::HashMap::from([(lutris.clone(), details)])
    );

    store.delete_game(&lutris).unwrap();
    assert_eq!(
        store.source_details(&lutris).unwrap(),
        SourceDetails::default()
    );
}

#[test]
fn a_database_at_schema_5_gains_groups_and_keeps_its_games() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("library.db");
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        for migration in &MIGRATIONS[..5] {
            conn.execute_batch(migration).unwrap();
        }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute(
            "INSERT INTO games (id, source, native_id, title, sort_title, added_at, launch_kind)
             VALUES ('steam:570', 'steam', '570', 'Dota 2', 'Dota 2', 1, 'store-uri')",
            [],
        )
        .unwrap();
    }

    let store = Store::open(&path).unwrap();

    assert_eq!(store.schema_version().unwrap(), 6);
    let steam = GameId::new(Source::Steam, "570");
    assert_eq!(store.game(&steam).unwrap().unwrap().title, "Dota 2");
    assert!(store.game_groups().unwrap().is_empty());
    assert!(store.splits().unwrap().is_empty());
    assert_eq!(
        store.source_details(&steam).unwrap(),
        SourceDetails::default()
    );
}

// Property-style tests: random sequences of operations, from fixed seeds, checked against
// a plain model of the groups after every step.

/// A small xorshift generator, so a failing seed replays exactly.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }

    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> Option<&'a T> {
        (!items.is_empty()).then(|| &items[self.below(items.len())])
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        for index in (1..items.len()).rev() {
            items.swap(index, self.below(index + 1));
        }
    }
}

const GAMES: usize = 8;

/// A library of GOG games with user state on every one: favourites, a hidden game, pins,
/// collections, sessions and the source's playtime.
fn library(seed: u64) -> (tempfile::TempDir, Store, Vec<GameId>) {
    let (dir, store) = temp_store();
    let games: Vec<ImportedGame> = (0..GAMES).map(imported).collect();
    apply_snapshot(&store, &gog(games), 1).unwrap();
    let ids: Vec<GameId> = (0..GAMES)
        .map(|n| GameId::new(Source::HeroicGog, &n.to_string()))
        .collect();
    let mut rng = Rng::new(seed);
    let collection = store.create_collection("Favourites").unwrap();
    for (n, id) in ids.iter().enumerate() {
        let mut game = store.game(id).unwrap().unwrap();
        game.favourite = rng.below(2) == 0;
        game.hidden = n == 3;
        game.pin_order = (n < 2).then_some(n as u8 + 1);
        store.update_game(&game).unwrap();
        if rng.below(2) == 0 {
            store.add_to_collection(collection.id, id).unwrap();
        }
        store.add_play_session(id, 10, 10 + n as i64).unwrap();
    }
    (dir, store, ids)
}

fn imported(n: usize) -> ImportedGame {
    let mut game = ImportedGame::new(n.to_string(), format!("Game {n}"), LaunchKind::StoreUri);
    game.playtime_secs = Some(n as i64 * 60);
    game
}

fn gog(games: Vec<ImportedGame>) -> Snapshot {
    Snapshot {
        source: Source::HeroicGog,
        games,
        errors: Vec::new(),
    }
}

/// Everything the library holds apart from groups and splits.
#[derive(Debug, PartialEq)]
struct UserState {
    games: Vec<Game>,
    memberships: std::collections::HashMap<GameId, Vec<i64>>,
    sessions: std::collections::HashMap<GameId, i64>,
    imported: std::collections::HashMap<GameId, i64>,
}

fn user_state(store: &Store) -> UserState {
    UserState {
        games: store.games().unwrap(),
        memberships: store.collection_memberships().unwrap(),
        sessions: store.session_totals().unwrap(),
        imported: store.imported_totals().unwrap(),
    }
}

/// The groups as the model expects them: members and preferred copy per group ID.
type Model = BTreeMap<i64, (BTreeSet<GameId>, Option<GameId>)>;

fn grouped(model: &Model, game: &GameId) -> Option<i64> {
    model
        .iter()
        .find(|(_, (members, _))| members.contains(game))
        .map(|(id, _)| *id)
}

fn check(store: &Store, model: &Model, splits: &BTreeSet<(GameId, GameId)>, seed: u64) {
    let groups = store.game_groups().unwrap();
    let actual: Model = groups
        .iter()
        .map(|group| {
            (
                group.id,
                (
                    group.members.iter().cloned().collect(),
                    group.preferred.clone(),
                ),
            )
        })
        .collect();
    assert_eq!(&actual, model, "seed {seed}");
    for group in &groups {
        assert!(group.members.len() >= 2, "seed {seed}: {group:?}");
        assert!(
            group.members.is_sorted(),
            "seed {seed}: members out of order {group:?}"
        );
        if let Some(preferred) = &group.preferred {
            assert!(group.members.contains(preferred), "seed {seed}: {group:?}");
        }
    }
    let stored: BTreeSet<(GameId, GameId)> = store.splits().unwrap().into_iter().collect();
    assert_eq!(&stored, splits, "seed {seed}");
}

#[test]
fn any_sequence_of_group_changes_and_resyncs_keeps_groups_as_modelled_and_user_state_alone() {
    for seed in 0..60 {
        let (_dir, store, ids) = library(seed);
        let mut rng = Rng::new(seed);
        let mut model = Model::new();
        let mut splits = BTreeSet::new();
        let user = |store: &Store| {
            let mut state = user_state(store);
            // A resync owns `missing_since`; everything else is the user's or unchanged.
            for game in &mut state.games {
                game.missing_since = None;
            }
            state
        };
        let before = user(&store);

        for step in 0..40 {
            let groups: Vec<i64> = model.keys().copied().collect();
            match rng.below(8) {
                0 | 1 => {
                    let mut chosen = ids.clone();
                    rng.shuffle(&mut chosen);
                    chosen.truncate(1 + rng.below(4));
                    let result = store.create_group(&chosen);
                    let distinct: BTreeSet<GameId> = chosen.iter().cloned().collect();
                    if distinct.len() < 2 {
                        assert!(matches!(result, Err(StoreError::TooFewCopies)));
                    } else if distinct.iter().any(|id| grouped(&model, id).is_some()) {
                        assert!(matches!(result, Err(StoreError::AlreadyGrouped(_))));
                    } else {
                        model.insert(result.unwrap(), (distinct, None));
                    }
                }
                2 => {
                    let (Some(&group), Some(game)) = (rng.pick(&groups), rng.pick(&ids)) else {
                        continue;
                    };
                    let result = store.add_to_group(group, game);
                    if grouped(&model, game).is_some() {
                        assert!(matches!(result, Err(StoreError::AlreadyGrouped(_))));
                    } else {
                        result.unwrap();
                        model.get_mut(&group).unwrap().0.insert(game.clone());
                    }
                }
                3 => {
                    let game = rng.pick(&ids).unwrap();
                    store.remove_from_group(game).unwrap();
                    if let Some(group) = grouped(&model, game) {
                        let (members, preferred) = model.get_mut(&group).unwrap();
                        members.remove(game);
                        if preferred.as_ref() == Some(game) {
                            *preferred = None;
                        }
                        if members.len() < 2 {
                            model.remove(&group);
                        }
                    }
                }
                4 => {
                    let Some(&group) = rng.pick(&groups) else {
                        continue;
                    };
                    store.ungroup(group).unwrap();
                    model.remove(&group);
                }
                5 => {
                    let Some(&group) = rng.pick(&groups) else {
                        continue;
                    };
                    let members: Vec<GameId> = model[&group].0.iter().cloned().collect();
                    let choice = rng.pick(&members).filter(|_| rng.below(4) > 0).cloned();
                    store.set_preferred_copy(group, choice.as_ref()).unwrap();
                    model.get_mut(&group).unwrap().1 = choice;
                }
                6 => {
                    let Some(&group) = rng.pick(&groups) else {
                        continue;
                    };
                    store.split_group(group).unwrap();
                    let members: Vec<GameId> =
                        model.remove(&group).unwrap().0.into_iter().collect();
                    for (index, first) in members.iter().enumerate() {
                        for second in &members[index + 1..] {
                            splits.insert((first.clone(), second.clone()));
                        }
                    }
                }
                _ => {
                    // A resync in which some games are gone from their source.
                    let present: Vec<ImportedGame> = (0..GAMES)
                        .filter(|_| rng.below(3) > 0)
                        .map(imported)
                        .collect();
                    apply_snapshot(&store, &gog(present), 100 + step).unwrap();
                }
            }
            check(&store, &model, &splits, seed);
            assert_eq!(user(&store), before, "seed {seed}, step {step}");
        }
    }
}

#[test]
fn ungrouping_in_any_order_restores_the_library_exactly() {
    for seed in 0..60 {
        let (_dir, store, ids) = library(seed);
        let mut rng = Rng::new(seed);
        let before = user_state(&store);

        // Disjoint groups of the shuffled games, made one after another.
        let mut shuffled = ids.clone();
        rng.shuffle(&mut shuffled);
        let mut groups = Vec::new();
        let mut rest = shuffled.as_slice();
        while rest.len() >= 2 {
            let size = (2 + rng.below(3)).min(rest.len());
            let (members, after) = rest.split_at(size);
            let group = store.create_group(members).unwrap();
            if rng.below(2) == 0 {
                store.set_preferred_copy(group, rng.pick(members)).unwrap();
            }
            groups.push(group);
            rest = after;
        }
        assert_eq!(user_state(&store), before, "seed {seed}");

        rng.shuffle(&mut groups);
        for group in groups {
            store.ungroup(group).unwrap();
        }

        assert_eq!(user_state(&store), before, "seed {seed}");
        assert!(store.game_groups().unwrap().is_empty(), "seed {seed}");
        assert!(store.splits().unwrap().is_empty(), "seed {seed}");
    }
}

#[test]
fn grouping_in_any_order_gives_the_same_groups() {
    for seed in 0..60 {
        let mut rng = Rng::new(seed);
        let partition: Vec<Vec<usize>> = vec![vec![0, 5], vec![1, 2, 7], vec![3, 6]];
        let mut results = Vec::new();
        for _ in 0..2 {
            let (_dir, store, ids) = library(seed);
            let mut order = partition.clone();
            rng.shuffle(&mut order);
            for members in &mut order {
                rng.shuffle(members);
                let members: Vec<GameId> = members.iter().map(|&n| ids[n].clone()).collect();
                store.create_group(&members).unwrap();
            }
            let groups: BTreeSet<Vec<GameId>> = store
                .game_groups()
                .unwrap()
                .into_iter()
                .map(|group| group.members)
                .collect();
            results.push(groups);
        }
        assert_eq!(results[0], results[1], "seed {seed}");
    }
}
