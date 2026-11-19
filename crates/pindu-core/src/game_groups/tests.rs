use super::*;
use crate::model::{ImportedPlaytime, LaunchKind, SourceDetails};

const HOUR: i64 = 3600;

fn game(source: Source, native_id: &str) -> Game {
    Game::new(
        GameId::new(source, native_id),
        "Celeste",
        LaunchKind::StoreUri,
        1_700_000_000,
    )
}

fn installed(source: Source, native_id: &str) -> Game {
    let mut game = game(source, native_id);
    game.installed = true;
    game
}

fn group_of(members: &[&Game], preferred: Option<&Game>) -> GameGroup {
    GameGroup {
        id: 1,
        members: members.iter().map(|game| game.id.clone()).collect(),
        preferred: preferred.map(|game| game.id.clone()),
        automatic: false,
    }
}

fn default_of<'a>(members: &[&'a Game]) -> &'a Game {
    default_copy(members.iter().copied(), DEFAULT_SOURCE_ORDER).unwrap()
}

#[test]
fn the_default_copy_is_an_installed_one() {
    let steam = game(Source::Steam, "1");
    let desktop = installed(Source::Desktop, "celeste");

    assert_eq!(default_of(&[&steam, &desktop]).id, desktop.id);
}

#[test]
fn a_copy_gone_from_its_source_does_not_count_as_installed() {
    let mut steam = installed(Source::Steam, "1");
    steam.missing_since = Some(1);
    let desktop = installed(Source::Desktop, "celeste");

    assert_eq!(default_of(&[&steam, &desktop]).id, desktop.id);
}

#[test]
fn the_default_copy_is_linux_then_unknown_then_windows() {
    let mut windows = installed(Source::Steam, "1");
    windows.platform = Some(Platform::Windows);
    let unknown = installed(Source::Lutris, "celeste");
    let mut linux = installed(Source::Desktop, "celeste");
    linux.platform = Some(Platform::Linux);

    assert_eq!(default_of(&[&windows, &unknown, &linux]).id, linux.id);
    assert_eq!(default_of(&[&windows, &unknown]).id, unknown.id);
}

#[test]
fn the_default_copy_follows_the_source_order() {
    let lutris = installed(Source::Lutris, "celeste");
    let steam = installed(Source::Steam, "1");
    let gog = installed(Source::HeroicGog, "2");

    assert_eq!(default_of(&[&lutris, &gog, &steam]).id, steam.id);
    assert_eq!(default_of(&[&lutris, &gog]).id, gog.id);
    let users_order = [Source::Lutris, Source::Steam];
    assert_eq!(
        default_copy([&steam, &gog, &lutris], &users_order)
            .unwrap()
            .id,
        lutris.id
    );
    // A source the user's order leaves out comes after the ones it names.
    assert_eq!(
        default_copy([&gog, &steam], &users_order).unwrap().id,
        steam.id
    );
}

#[test]
fn the_source_order_setting_drops_unknown_sources_and_appends_missing_ones() {
    assert_eq!(source_order::<&str>([]), DEFAULT_SOURCE_ORDER);
    let order = source_order(["lutris", "itch.io", "steam", "lutris"]);
    assert_eq!(order[..2], [Source::Lutris, Source::Steam]);
    let rest: Vec<Source> = DEFAULT_SOURCE_ORDER
        .iter()
        .copied()
        .filter(|source| ![Source::Lutris, Source::Steam].contains(source))
        .collect();
    assert_eq!(order[2..], rest);
}

#[test]
fn the_default_source_order_names_every_source_once() {
    let mut sorted = DEFAULT_SOURCE_ORDER.to_vec();
    sorted.sort_by_key(|source| source.as_str());
    let mut all = Source::ALL.to_vec();
    all.sort_by_key(|source| source.as_str());
    assert_eq!(sorted, all);
}

#[test]
fn moving_a_source_swaps_it_with_its_neighbour_and_stops_at_either_end() {
    let order = [Source::Steam, Source::Lutris, Source::Desktop];

    assert_eq!(
        moved_source(&order, Source::Lutris, true),
        [Source::Lutris, Source::Steam, Source::Desktop]
    );
    assert_eq!(
        moved_source(&order, Source::Lutris, false),
        [Source::Steam, Source::Desktop, Source::Lutris]
    );
    assert_eq!(moved_source(&order, Source::Steam, true), order);
    assert_eq!(moved_source(&order, Source::Desktop, false), order);
    assert_eq!(moved_source(&order, Source::Itch, true), order);
}

#[test]
fn the_default_copy_is_the_most_recently_played_then_the_lowest_id() {
    let mut older = installed(Source::Desktop, "a");
    older.last_played_at = Some(10);
    let mut newer = installed(Source::Desktop, "b");
    newer.last_played_at = Some(20);
    let never = installed(Source::Desktop, "c");
    let also_never = installed(Source::Desktop, "d");

    assert_eq!(default_of(&[&older, &never, &newer]).id, newer.id);
    assert_eq!(default_of(&[&also_never, &never]).id, never.id);
}

#[test]
fn the_users_choice_is_the_preferred_copy() {
    let steam = installed(Source::Steam, "1");
    let gog = installed(Source::HeroicGog, "2");

    let chosen = group_of(&[&steam, &gog], Some(&gog));
    let unchosen = group_of(&[&steam, &gog], None);

    assert_eq!(
        preferred_copy(&chosen, [&steam, &gog], DEFAULT_SOURCE_ORDER)
            .unwrap()
            .id,
        gog.id
    );
    assert_eq!(
        preferred_copy(&unchosen, [&steam, &gog], DEFAULT_SOURCE_ORDER)
            .unwrap()
            .id,
        steam.id
    );
}

#[test]
fn play_starts_an_installed_copy_when_the_chosen_one_is_not_installed() {
    let steam = game(Source::Steam, "1");
    let lutris = installed(Source::Lutris, "celeste");
    let desktop = installed(Source::Desktop, "celeste");
    let group = group_of(&[&steam, &lutris, &desktop], Some(&steam));

    assert_eq!(
        copy_to_play(&group, [&steam, &desktop, &lutris], DEFAULT_SOURCE_ORDER)
            .unwrap()
            .id,
        lutris.id
    );
}

#[test]
fn with_no_installed_copy_play_names_the_preferred_one() {
    let steam = game(Source::Steam, "1");
    let gog = game(Source::HeroicGog, "2");
    let group = group_of(&[&steam, &gog], Some(&gog));

    assert_eq!(
        copy_to_play(&group, [&steam, &gog], DEFAULT_SOURCE_ORDER)
            .unwrap()
            .id,
        gog.id
    );
}

fn copy<'a>(game: &'a Game, hours: i64, runner: Option<&'a str>) -> CopyPlaytime<'a> {
    CopyPlaytime {
        game,
        secs: hours * HOUR,
        runner,
    }
}

#[test]
fn steam_and_a_lutris_copy_that_starts_steam_count_once() {
    let steam = game(Source::Steam, "1");
    let lutris = game(Source::Lutris, "celeste");

    let playtime = group_playtime(&[copy(&steam, 10, None), copy(&lutris, 2, Some("steam"))]);

    assert_eq!(
        playtime,
        GroupPlaytime {
            secs: 10 * HOUR,
            played_copies: 1,
            from: Some(steam.id.clone()),
        }
    );
    // Lutris's figure wins when Steam's own is smaller or missing.
    let playtime = group_playtime(&[copy(&steam, 0, None), copy(&lutris, 2, Some(" steam "))]);
    assert_eq!(playtime.secs, 2 * HOUR);
    assert_eq!(playtime.from, Some(lutris.id));
}

#[test]
fn copies_played_apart_add_up() {
    let steam = game(Source::Steam, "1");
    let gog = game(Source::HeroicGog, "2");

    let playtime = group_playtime(&[copy(&steam, 10, None), copy(&gog, 5, None)]);

    assert_eq!(
        playtime,
        GroupPlaytime {
            secs: 15 * HOUR,
            played_copies: 2,
            from: None,
        }
    );
}

#[test]
fn only_the_copies_timed_together_share_a_figure() {
    let steam = game(Source::Steam, "1");
    let lutris_steam = game(Source::Lutris, "celeste");
    let lutris_gog = game(Source::Lutris, "celeste-gog");

    let playtime = group_playtime(&[
        copy(&steam, 10, None),
        copy(&lutris_steam, 2, Some("steam")),
        copy(&lutris_gog, 3, Some("wine")),
    ]);

    assert_eq!(playtime.secs, 13 * HOUR);
    assert_eq!(playtime.played_copies, 2);
}

#[test]
fn a_group_never_played_has_no_playtime() {
    let steam = game(Source::Steam, "1");
    let gog = game(Source::HeroicGog, "2");

    let playtime = group_playtime(&[copy(&steam, 0, None), copy(&gog, 0, None)]);

    assert_eq!(
        playtime,
        GroupPlaytime {
            secs: 0,
            played_copies: 0,
            from: None,
        }
    );
}

#[test]
fn a_group_was_last_played_when_any_copy_was() {
    let mut steam = game(Source::Steam, "1");
    steam.last_played_at = Some(30);
    let mut gog = game(Source::HeroicGog, "2");
    gog.last_played_at = Some(50);
    let never = game(Source::Desktop, "celeste");

    assert_eq!(group_last_played([&steam, &gog, &never]), Some(50));
    assert_eq!(group_last_played([&never]), None);
}

#[test]
fn loading_works_out_each_groups_playtime_from_the_stored_figures() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    let mut steam = game(Source::Steam, "1");
    steam.last_played_at = Some(40);
    let mut lutris = game(Source::Lutris, "celeste");
    // Gone from its source: still counts.
    lutris.missing_since = Some(1);
    let desktop = game(Source::Desktop, "celeste");
    for game in [&steam, &lutris, &desktop] {
        store.insert_game(game).unwrap();
    }
    store
        .set_imported_playtime(&ImportedPlaytime {
            game_id: steam.id.clone(),
            source: Source::Steam,
            total_secs: 10 * HOUR,
            last_played_at: Some(40),
        })
        .unwrap();
    store
        .set_imported_playtime(&ImportedPlaytime {
            game_id: lutris.id.clone(),
            source: Source::Lutris,
            total_secs: 2 * HOUR,
            last_played_at: None,
        })
        .unwrap();
    store
        .set_source_details(
            &lutris.id,
            &SourceDetails {
                runner: Some("steam".into()),
                ..SourceDetails::default()
            },
        )
        .unwrap();
    store.add_play_session(&desktop.id, 0, HOUR).unwrap();
    let id = store
        .create_group(&[steam.id.clone(), lutris.id.clone(), desktop.id.clone()])
        .unwrap();

    let summaries = load(&store).unwrap();

    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].group.id, id);
    assert_eq!(
        summaries[0].playtime,
        GroupPlaytime {
            secs: 11 * HOUR,
            played_copies: 2,
            from: None,
        }
    );
    assert_eq!(summaries[0].last_played, Some(40));
}

#[test]
fn a_change_to_a_grouped_game_reaches_every_copy() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    let steam = installed(Source::Steam, "1");
    let gog = game(Source::HeroicGog, "celeste");
    let loner = game(Source::Desktop, "loner");
    for game in [&steam, &gog, &loner] {
        store.insert_game(game).unwrap();
    }
    store
        .create_group(&[steam.id.clone(), gog.id.clone()])
        .unwrap();

    assert_eq!(
        copies_of(&store, &gog.id).unwrap(),
        [gog.id.clone(), steam.id.clone()]
    );
    assert_eq!(
        copies_of(&store, &loner.id).unwrap(),
        std::slice::from_ref(&loner.id)
    );
}

fn titled(source: Source, native_id: &str, title: &str) -> Game {
    let mut game = game(source, native_id);
    game.title = title.to_owned();
    game
}

#[test]
fn merge_candidates_list_likely_copies_first_then_the_rest_by_title() {
    let target = titled(Source::Steam, "1", "Celeste");
    let hollow = titled(Source::Steam, "2", "Hollow Knight");
    let classic = titled(Source::Lutris, "classic", "Celeste Classic");
    let gog = titled(Source::HeroicGog, "3", "CELESTE™");
    let mut removed = titled(Source::Desktop, "celeste", "Celeste");
    removed.user_removed = true;
    let games = [&target, &hollow, &classic, &gog, &removed];

    let ids = |query: &str| -> Vec<GameId> {
        merge_candidates(&target, games.iter().copied(), query)
            .into_iter()
            .map(|game| game.id.clone())
            .collect()
    };

    assert_eq!(
        ids(""),
        [gog.id.clone(), classic.id.clone(), hollow.id.clone()]
    );
    assert_eq!(ids("hol"), std::slice::from_ref(&hollow.id));
}
