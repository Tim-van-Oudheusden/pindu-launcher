use super::*;
use crate::model::LaunchKind;
use crate::sources::ServiceRef;

/// A library to look for duplicates in, built game by game.
#[derive(Default)]
struct Library {
    games: Vec<Game>,
    details: HashMap<GameId, SourceDetails>,
    groups: Vec<GameGroup>,
    splits: Vec<(GameId, GameId)>,
}

impl Library {
    fn add(&mut self, source: Source, native_id: &str, title: &str) -> GameId {
        let game = Game::new(
            GameId::new(source, native_id),
            title,
            LaunchKind::StoreUri,
            1_700_000_000,
        );
        let id = game.id.clone();
        self.games.push(game);
        id
    }

    fn game_mut(&mut self, id: &GameId) -> &mut Game {
        self.games.iter_mut().find(|game| game.id == *id).unwrap()
    }

    fn lutris(&mut self, native_id: &str, title: &str, service: &str, service_id: &str) -> GameId {
        let id = self.add(Source::Lutris, native_id, title);
        self.details.insert(
            id.clone(),
            SourceDetails {
                service: Some(ServiceRef {
                    service: service.into(),
                    service_id: service_id.into(),
                }),
                ..SourceDetails::default()
            },
        );
        id
    }

    fn developer(&mut self, id: &GameId, developer: &str) {
        self.game_mut(id).developer = Some(developer.into());
    }

    fn year(&mut self, id: &GameId, year: i32) {
        self.details.entry(id.clone()).or_default().year = Some(year);
    }

    fn suggestions(&self) -> Vec<Suggestion> {
        suggestions(&self.games, &self.details, &self.groups, &self.splits)
    }
}

fn pair(first: &GameId, second: &GameId) -> (GameId, GameId) {
    if first <= second {
        (first.clone(), second.clone())
    } else {
        (second.clone(), first.clone())
    }
}

fn pairs(suggestions: &[Suggestion]) -> Vec<(GameId, GameId)> {
    suggestions
        .iter()
        .map(|suggestion| (suggestion.first.clone(), suggestion.second.clone()))
        .collect()
}

#[test]
fn a_lutris_gog_link_is_an_exact_match_with_the_heroic_gog_record() {
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207664643", "The Witcher 3");
    let lutris = library.lutris("2", "Witcher 3: GOTY", "gog", "1207664643");

    let (first, second) = pair(&gog, &lutris);
    assert_eq!(
        library.suggestions(),
        vec![Suggestion {
            first,
            second,
            reason: Reason::CrossReference {
                service: "gog".into()
            },
            confidence: Confidence::Exact,
        }]
    );
}

#[test]
fn lutris_steam_and_windows_steam_links_match_steam_and_egs_matches_heroic_epic() {
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "367520", "Hollow Knight");
    let epic = library.add(Source::HeroicEpic, "Sugar", "Celeste");
    let native = library.lutris("1", "HK", "steam", "367520");
    let windows = library.lutris("2", "HK (Windows)", "steamwindows", "367520");
    let wine = library.lutris("3", "Celeste (Epic)", "egs", "Sugar");

    let found = library.suggestions();

    assert_eq!(
        pairs(&found),
        vec![
            pair(&epic, &wine),
            pair(&native, &steam),
            pair(&steam, &windows)
        ]
    );
    assert!(
        found
            .iter()
            .all(|suggestion| suggestion.confidence == Confidence::Exact)
    );
    assert_eq!(
        found[2].reason,
        Reason::CrossReference {
            service: "steamwindows".into()
        }
    );
}

#[test]
fn amazon_and_unknown_services_are_no_cross_reference() {
    let mut library = Library::default();
    library.add(Source::HeroicAmazon, "amzn1.adg.product.1", "Amazon Game");
    library.add(Source::Steam, "42", "Steam Game");
    library.lutris("1", "Other Amazon Game", "amazon", "amzn1.adg.product.1");
    library.lutris("2", "Other Steam Game", "humblebundle", "42");

    assert_eq!(library.suggestions(), vec![]);
}

#[test]
fn a_cross_reference_ignores_surrounding_whitespace() {
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Gog");
    let lutris = library.lutris("1", "Lutris", " gog ", " 1207\n");

    assert_eq!(pairs(&library.suggestions()), vec![pair(&gog, &lutris)]);
}

#[test]
fn records_gone_from_their_source_or_removed_are_never_suggested() {
    let mut library = Library::default();
    let gone = library.add(Source::HeroicGog, "1207", "Celeste");
    library.game_mut(&gone).missing_since = Some(1);
    let removed = library.add(Source::Steam, "504230", "Celeste");
    library.game_mut(&removed).user_removed = true;
    library.lutris("1", "Celeste", "gog", "1207");

    assert_eq!(library.suggestions(), vec![]);
}

#[test]
fn equal_titles_from_different_sources_are_a_low_confidence_suggestion() {
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "504230", "Celeste™");
    let desktop = library.add(Source::Desktop, "celeste", "celeste");

    let (first, second) = pair(&steam, &desktop);
    assert_eq!(
        library.suggestions(),
        vec![Suggestion {
            first,
            second,
            reason: Reason::Title {
                same_developer: false,
                close_year: false,
            },
            confidence: Confidence::Low,
        }]
    );
}

#[test]
fn two_records_from_one_source_are_never_suggested() {
    let mut library = Library::default();
    library.add(Source::Desktop, "disc1", "Final Fantasy VII");
    library.add(Source::Desktop, "disc2", "Final Fantasy VII");

    assert_eq!(library.suggestions(), vec![]);
}

#[test]
fn titles_match_only_when_equal_never_on_a_prefix() {
    let mut library = Library::default();
    library.add(Source::Steam, "1", "Metal Gear Solid");
    library.add(Source::Desktop, "mgs2", "Metal Gear Solid 2");

    assert_eq!(library.suggestions(), vec![]);
}

#[test]
fn an_agreeing_developer_raises_the_confidence_ignoring_company_words() {
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "504230", "Celeste");
    library.developer(&steam, "Maddy Makes Games Inc.");
    let lutris = library.add(Source::Lutris, "1", "Celeste");
    library.developer(&lutris, "maddy makes games");

    let found = library.suggestions();

    assert_eq!(found[0].confidence, Confidence::High);
    assert_eq!(
        found[0].reason,
        Reason::Title {
            same_developer: true,
            close_year: false,
        }
    );
}

#[test]
fn a_year_one_apart_raises_the_confidence() {
    let mut library = Library::default();
    let gog = library.lutris("1", "Celeste", "gog", "999");
    library.year(&gog, 2018);
    let faugus = library.add(Source::Faugus, "celeste", "Celeste");
    library.year(&faugus, 2019);

    let found = library.suggestions();

    assert_eq!(found[0].confidence, Confidence::High);
    assert_eq!(
        found[0].reason,
        Reason::Title {
            same_developer: false,
            close_year: true,
        }
    );
}

#[test]
fn a_disagreeing_developer_or_year_only_lowers_the_confidence() {
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "1", "Doom");
    library.developer(&steam, "id Software");
    library.year(&steam, 1993);
    let lutris = library.add(Source::Lutris, "1", "DOOM");
    library.developer(&lutris, "Nightdive Studios");
    library.year(&lutris, 2019);

    let found = library.suggestions();

    assert_eq!(pairs(&found), vec![pair(&steam, &lutris)]);
    assert_eq!(found[0].confidence, Confidence::Low);
}

#[test]
fn cross_references_come_before_title_matches_and_are_not_repeated_as_one() {
    let mut library = Library::default();
    let desktop = library.add(Source::Desktop, "celeste", "Celeste");
    let manual = library.add(Source::Manual, "celeste", "Celeste");
    library.developer(&desktop, "Maddy Makes Games");
    library.developer(&manual, "Maddy Makes Games");
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let lutris = library.lutris("1", "Celeste", "gog", "1207");

    let found = library.suggestions();

    assert_eq!(found[0].first, pair(&gog, &lutris).0);
    assert_eq!(found[0].second, pair(&gog, &lutris).1);
    assert_eq!(found[0].confidence, Confidence::Exact);
    assert_eq!(found[1].confidence, Confidence::High);
    assert_eq!(pairs(&found[1..2]), vec![pair(&desktop, &manual)]);
    assert!(
        found[2..]
            .iter()
            .all(|suggestion| suggestion.confidence == Confidence::Low)
    );
    assert_eq!(found.len(), 6);
}

#[test]
fn a_pair_in_one_group_is_not_suggested_but_a_copy_beside_the_group_is() {
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "504230", "Celeste");
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let desktop = library.add(Source::Desktop, "celeste", "Celeste");
    library.groups.push(GameGroup {
        id: 1,
        members: vec![gog.clone(), steam.clone()],
        preferred: None,
        automatic: false,
    });

    assert_eq!(
        pairs(&library.suggestions()),
        vec![pair(&desktop, &gog), pair(&desktop, &steam)]
    );
}

/// A small, seeded xorshift generator, so the property tests are repeatable.
struct Random(u64);

impl Random {
    fn below(&mut self, n: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 % n as u64) as usize
    }

    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const SOURCES: &[Source] = &[
    Source::Steam,
    Source::HeroicGog,
    Source::HeroicEpic,
    Source::HeroicAmazon,
    Source::Lutris,
    Source::Desktop,
    Source::Faugus,
];

/// A library of a few dozen records whose titles, developers, years and store links
/// collide often.
fn random_library(random: &mut Random) -> Library {
    const TITLES: &[&str] = &[
        "Celeste",
        "celeste™",
        "The Celeste",
        "Hades",
        "HADES",
        "Doom",
    ];
    const DEVELOPERS: &[&str] = &["Supergiant Games", "Supergiant Games, LLC", "id Software"];
    const SERVICES: &[&str] = &["steam", "steamwindows", "gog", "egs", "amazon", "itchio"];
    let mut library = Library::default();
    for index in 0..30 {
        let source = *random.pick(SOURCES);
        let native_id = random.below(4).to_string();
        let title = *random.pick(TITLES);
        if library
            .games
            .iter()
            .any(|game| game.id == GameId::new(source, &native_id))
        {
            continue;
        }
        let id = if source == Source::Lutris && random.below(2) == 0 {
            let service = *random.pick(SERVICES);
            library.lutris(&format!("{native_id}{index}"), title, service, &native_id)
        } else {
            library.add(source, &native_id, title)
        };
        if random.below(2) == 0 {
            library.developer(&id, random.pick(DEVELOPERS));
        }
        if random.below(2) == 0 {
            library.year(&id, 2018 + random.below(3) as i32);
        }
        if random.below(8) == 0 {
            library.game_mut(&id).missing_since = Some(1);
        }
    }
    library
}

#[test]
fn matching_is_symmetric() {
    let mut random = Random(0x0505_5eed);
    for _ in 0..200 {
        let mut library = random_library(&mut random);
        for a in &library.games {
            for b in &library.games {
                assert_eq!(
                    pair_match(a, b, &library.details),
                    pair_match(b, a, &library.details),
                    "{} and {}",
                    a.id,
                    b.id,
                );
            }
        }
        let found = library.suggestions();
        assert!(
            found
                .iter()
                .all(|suggestion| suggestion.first < suggestion.second)
        );
        library.games.reverse();
        assert_eq!(library.suggestions(), found);
        let shift = random.below(library.games.len().max(1));
        library.games.rotate_left(shift);
        assert_eq!(library.suggestions(), found);
    }
}

#[test]
fn remembered_splits_never_return() {
    let mut random = Random(0xdead_beef);
    for _ in 0..200 {
        let mut library = random_library(&mut random);
        let found = library.suggestions();
        let mut kept = Vec::new();
        for suggestion in &found {
            match random.below(3) {
                0 => library
                    .splits
                    .push((suggestion.first.clone(), suggestion.second.clone())),
                1 => library
                    .splits
                    .push((suggestion.second.clone(), suggestion.first.clone())),
                _ => kept.push(suggestion.clone()),
            }
        }

        assert_eq!(library.suggestions(), kept);
    }
}

fn temp_store() -> (tempfile::TempDir, Store) {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    (dir, store)
}

/// Stores a library's games and source details.
fn store_library(store: &Store, library: &Library) {
    for game in &library.games {
        store.insert_game(game).unwrap();
    }
    for (id, details) in &library.details {
        store.set_source_details(id, details).unwrap();
    }
}

#[test]
fn loading_suggestions_reads_the_store_and_writes_nothing() {
    let (dir, store) = temp_store();
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let lutris = library.lutris("1", "Celeste", "gog", "1207");
    let desktop = library.add(Source::Desktop, "celeste", "Celeste");
    let steam = library.add(Source::Steam, "504230", "Celeste");
    store_library(&store, &library);
    store.remember_split(&desktop, &steam).unwrap();
    let database = dir.path().join("library.db");
    let before = std::fs::read(&database).unwrap();

    let found = load(&store).unwrap();

    assert_eq!(std::fs::read(&database).unwrap(), before);
    assert_eq!(store.game_groups().unwrap(), vec![]);
    library.splits = store.splits().unwrap();
    assert_eq!(found, library.suggestions());
    assert_eq!(found[0].confidence, Confidence::Exact);
    assert_eq!(pairs(&found[..1]), vec![pair(&gog, &lutris)]);
    assert!(!pairs(&found).contains(&pair(&desktop, &steam)));
}

#[test]
fn exact_matches_are_grouped_automatically_and_title_matches_are_not() {
    let (_dir, store) = temp_store();
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let lutris = library.lutris("1", "Celeste", "gog", "1207");
    let steam = library.add(Source::Steam, "504230", "Celeste");
    library.developer(&steam, "Maddy Makes Games");
    let desktop = library.add(Source::Desktop, "celeste", "Celeste");
    library.developer(&desktop, "Maddy Makes Games");
    store_library(&store, &library);

    let grouped = group_exact_matches(&store).unwrap();

    let groups = store.game_groups().unwrap();
    assert_eq!(grouped, vec![groups[0].id]);
    assert_eq!(groups.len(), 1);
    assert!(groups[0].automatic);
    let (first, second) = pair(&gog, &lutris);
    assert_eq!(groups[0].members, vec![first, second]);
    assert_eq!(store.game_group_of(&steam).unwrap(), None);
    assert_eq!(store.game_group_of(&desktop).unwrap(), None);
    assert!(group_exact_matches(&store).unwrap().is_empty());
}

#[test]
fn undoing_an_automatic_group_keeps_the_pair_apart_and_unsuggested() {
    let (_dir, store) = temp_store();
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let lutris = library.lutris("1", "Celeste", "gog", "1207");
    store_library(&store, &library);
    let [group] = group_exact_matches(&store).unwrap()[..] else {
        panic!("one group expected");
    };

    store.split_group(group).unwrap();

    assert!(group_exact_matches(&store).unwrap().is_empty());
    assert_eq!(store.game_groups().unwrap(), vec![]);
    assert!(store.is_split(&gog, &lutris).unwrap());
    assert_eq!(load(&store).unwrap(), vec![]);
}

#[test]
fn an_exact_match_is_not_grouped_when_a_copy_is_in_a_group_already() {
    let (_dir, store) = temp_store();
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Gog");
    library.lutris("1", "Lutris", "gog", "1207");
    let desktop = library.add(Source::Desktop, "gog", "Desktop");
    store_library(&store, &library);
    store.create_group(&[gog, desktop]).unwrap();

    assert!(group_exact_matches(&store).unwrap().is_empty());
    assert_eq!(store.game_groups().unwrap().len(), 1);
}

#[test]
fn an_accepted_suggestion_is_a_group_by_hand_and_is_not_suggested_again() {
    let (_dir, store) = temp_store();
    let mut library = Library::default();
    let steam = library.add(Source::Steam, "504230", "Celeste");
    let desktop = library.add(Source::Desktop, "celeste", "Celeste");
    store_library(&store, &library);

    let group = store.merge_games(&[steam.clone(), desktop.clone()]).unwrap();

    let (first, second) = pair(&steam, &desktop);
    assert_eq!(
        store.game_groups().unwrap(),
        vec![GameGroup {
            id: group,
            members: vec![first, second],
            preferred: None,
            automatic: false,
        }]
    );
    assert_eq!(load(&store).unwrap(), vec![]);
}

#[test]
fn the_review_lists_the_suggestions_and_the_automatic_groups_with_their_titles() {
    let (_dir, store) = temp_store();
    let mut library = Library::default();
    let gog = library.add(Source::HeroicGog, "1207", "Celeste");
    let lutris = library.lutris("1", "Celeste (Lutris)", "gog", "1207");
    let steam = library.add(Source::Steam, "504230", "Hades");
    let desktop = library.add(Source::Desktop, "hades", "Hades");
    let bottles = library.add(Source::Bottles, "a", "Tunic");
    let manual = library.add(Source::Manual, "b", "Tunic");
    store_library(&store, &library);
    group_exact_matches(&store).unwrap();
    store
        .create_group(&[bottles.clone(), manual.clone()])
        .unwrap();

    let review = review(&store).unwrap();

    assert_eq!(review.suggestions, load(&store).unwrap());
    assert_eq!(pairs(&review.suggestions), vec![pair(&steam, &desktop)]);
    assert_eq!(review.automatic.len(), 1);
    assert_eq!(review.automatic[0].members, {
        let (first, second) = pair(&gog, &lutris);
        vec![first, second]
    });
    assert_eq!(review.title(&lutris), "Celeste (Lutris)");
    assert_eq!(review.title(&desktop), "Hades");
}
