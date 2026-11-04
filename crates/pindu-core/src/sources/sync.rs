//! Reconciles an importer's [`Snapshot`] with the library, and runs the importers.

use std::collections::HashSet;
use std::path::Path;

use super::location::{self, HostDirs, ResolveError};
use super::{ImportError, ImportedGame, Snapshot, bottles, heroic, lutris, steam};
use crate::model::{
    ArtOrigin, Artwork, Game, GameId, ImportedPlaytime, Source, SourceState, Timestamp,
};
use crate::store::{self, Store};

/// Sources that have an importer, in sync order.
pub const IMPORTED_SOURCES: &[Source] = &[
    Source::Steam,
    Source::HeroicEpic,
    Source::HeroicGog,
    Source::HeroicAmazon,
    Source::HeroicSideload,
    Source::Lutris,
    Source::Bottles,
];

/// What a sync of every source could not read, by source.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SyncSummary {
    pub problems: Vec<(Source, String)>,
}

/// Syncs every enabled source with an importer.
pub fn sync_all(store: &Store, dirs: &HostDirs, now: Timestamp) -> store::Result<SyncSummary> {
    let mut summary = SyncSummary::default();
    for &source in IMPORTED_SOURCES {
        let enabled = store
            .source_state(source)?
            .is_none_or(|state| state.enabled);
        if enabled {
            let problems = sync_source(store, source, dirs, now)?;
            summary
                .problems
                .extend(problems.into_iter().map(|problem| (source, problem)));
        }
    }
    Ok(summary)
}

/// Resolves, imports and applies one source, and records the outcome in its
/// [`SourceState`]. Returns the source's problems: unreadable entries, an incomplete
/// override, or a source that could not be read at all (whose games are then left as
/// they were). A source no longer on the machine is an empty source.
pub fn sync_source(
    store: &Store,
    source: Source,
    dirs: &HostDirs,
    now: Timestamp,
) -> store::Result<Vec<String>> {
    let snapshot = match location::resolve_and_record(store, source, dirs)? {
        Ok(root) => import(source, &root).map_err(|error| error.to_string()),
        Err(ResolveError::NotFound) => Ok(Snapshot::new(source)),
        Err(error) => Err(error.to_string()),
    };
    let problems = match snapshot {
        Ok(snapshot) => {
            apply_snapshot(store, &snapshot, now)?;
            snapshot.errors
        }
        Err(problem) => vec![problem],
    };
    let mut state = store
        .source_state(source)?
        .unwrap_or_else(|| SourceState::new(source));
    state.last_sync = Some(now);
    state.errors.clone_from(&problems);
    store.set_source_state(&state)?;
    Ok(problems)
}

fn import(source: Source, root: &Path) -> Result<Snapshot, ImportError> {
    match source {
        Source::Steam => steam::import(root),
        Source::Lutris => lutris::import(root),
        Source::Bottles => bottles::import(root),
        _ => heroic::import(root, source),
    }
}

/// Writes a snapshot into the library in one transaction.
///
/// New games take the source's hidden, favourite and category flags; for games already in
/// the library those flags are ours and only the source's own fields are refreshed. Games
/// of the source that the snapshot lacks get `missing_since`, keeping all their data for
/// when they return; a user-removed game stays removed only while its source lists it.
pub fn apply_snapshot(store: &Store, snapshot: &Snapshot, now: Timestamp) -> store::Result<()> {
    store.in_transaction(|store| {
        let mut present = HashSet::new();
        for imported in &snapshot.games {
            let id = GameId::new(snapshot.source, &imported.native_id);
            match store.game(&id)? {
                Some(mut game) => {
                    refresh(&mut game, imported);
                    store.update_game(&game)?;
                }
                None => {
                    let mut game =
                        Game::new(id.clone(), &imported.title, imported.launch_kind, now);
                    refresh(&mut game, imported);
                    game.hidden = imported.source_flags.hidden;
                    game.favourite = imported.source_flags.favourite;
                    store.insert_game(&game)?;
                    for name in &imported.source_flags.categories {
                        let collection =
                            match store.collections()?.into_iter().find(|c| &c.name == name) {
                                Some(collection) => collection,
                                None => store.create_collection(name)?,
                            };
                        store.add_to_collection(collection.id, &id)?;
                    }
                }
            }
            replace_source_art(store, &id, imported)?;
            if let Some(total_secs) = imported.playtime_secs {
                store.set_imported_playtime(&ImportedPlaytime {
                    game_id: id.clone(),
                    source: snapshot.source,
                    total_secs,
                    last_played_at: imported.last_played_at,
                })?;
            }
            present.insert(id);
        }
        for mut game in store.games_from(snapshot.source)? {
            if present.contains(&game.id) || game.missing_since.is_some() && !game.user_removed {
                continue;
            }
            game.missing_since.get_or_insert(now);
            game.user_removed = false;
            store.update_game(&game)?;
        }
        Ok(())
    })
}

/// Copies the fields the source owns onto a game, which the source currently lists.
fn refresh(game: &mut Game, imported: &ImportedGame) {
    game.missing_since = None;
    game.title.clone_from(&imported.title);
    game.sort_title.clone_from(&imported.title);
    game.developer.clone_from(&imported.developer);
    game.platform = imported.platform;
    game.installed = imported.installed;
    game.install_path.clone_from(&imported.install_path);
    game.size_bytes = imported.size_bytes;
    game.last_played_at = game.last_played_at.max(imported.last_played_at);
    game.launch_kind = imported.launch_kind;
    game.launch_uri.clone_from(&imported.launch_uri);
    game.genres.clone_from(&imported.genres);
    game.description.clone_from(&imported.description);
}

fn replace_source_art(store: &Store, id: &GameId, imported: &ImportedGame) -> store::Result<()> {
    for stale in store.artwork(id)? {
        let still_there = imported
            .artwork
            .iter()
            .any(|(layout, _)| *layout == stale.layout);
        if stale.origin == ArtOrigin::Source && !still_there {
            store.delete_artwork(id, stale.layout, stale.origin)?;
        }
    }
    for (layout, path) in &imported.artwork {
        store.set_artwork(&Artwork {
            game_id: id.clone(),
            layout: *layout,
            origin: ArtOrigin::Source,
            path: path.clone(),
        })?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtLayout, LaunchKind, Source};
    use crate::sources::SourceFlags;

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        (dir, store)
    }

    fn snapshot(games: Vec<ImportedGame>) -> Snapshot {
        Snapshot {
            source: Source::HeroicGog,
            games,
            errors: Vec::new(),
        }
    }

    fn flagged(hidden: bool, favourite: bool, categories: &[&str]) -> ImportedGame {
        let mut game =
            ImportedGame::new("1207658924", "Unreal Tournament 2004", LaunchKind::StoreUri);
        game.source_flags = SourceFlags {
            hidden,
            favourite,
            categories: categories.iter().map(|name| name.to_string()).collect(),
        };
        game
    }

    fn id() -> GameId {
        GameId::new(Source::HeroicGog, "1207658924")
    }

    #[test]
    fn a_new_game_takes_the_sources_flags_and_categories() {
        let (_dir, store) = temp_store();

        apply_snapshot(
            &store,
            &snapshot(vec![flagged(true, true, &["Shooters"])]),
            100,
        )
        .unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert!(game.hidden);
        assert!(game.favourite);
        assert_eq!(game.added_at, 100);
        let collections = store.collections().unwrap();
        assert_eq!(collections.len(), 1);
        assert_eq!(collections[0].name, "Shooters");
        assert_eq!(
            store.collection_members(collections[0].id).unwrap(),
            vec![id()]
        );
    }

    #[test]
    fn after_the_first_import_our_own_flags_win() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        let mut game = store.game(&id()).unwrap().unwrap();
        game.favourite = true;
        store.update_game(&game).unwrap();

        apply_snapshot(
            &store,
            &snapshot(vec![flagged(true, false, &["Shooters"])]),
            200,
        )
        .unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert!(game.favourite);
        assert!(!game.hidden);
        assert_eq!(game.added_at, 100);
        assert!(store.collections().unwrap().is_empty());
    }

    #[test]
    fn source_fields_are_refreshed_on_every_import() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        let mut renamed = flagged(false, false, &[]);
        renamed.title = "UT2004".into();
        renamed.installed = true;
        renamed.launch_uri = Some("heroic://launch?appName=UT2004".into());

        apply_snapshot(&store, &snapshot(vec![renamed]), 200).unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert_eq!(game.title, "UT2004");
        assert!(game.installed);
        assert_eq!(
            game.launch_uri.as_deref(),
            Some("heroic://launch?appName=UT2004")
        );
    }

    #[test]
    fn the_sources_playtime_is_stored_and_refreshed() {
        let (_dir, store) = temp_store();
        let mut game = flagged(false, false, &[]);
        game.playtime_secs = Some(3_600);
        game.last_played_at = Some(150);
        apply_snapshot(&store, &snapshot(vec![game.clone()]), 100).unwrap();
        game.playtime_secs = Some(7_200);

        apply_snapshot(&store, &snapshot(vec![game]), 200).unwrap();

        assert_eq!(
            store.imported_playtime(&id()).unwrap(),
            Some(ImportedPlaytime {
                game_id: id(),
                source: Source::HeroicGog,
                total_secs: 7_200,
                last_played_at: Some(150),
            })
        );
    }

    #[test]
    fn a_game_without_source_playtime_stores_none() {
        let (_dir, store) = temp_store();

        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();

        assert_eq!(store.imported_playtime(&id()).unwrap(), None);
    }

    #[test]
    fn an_existing_collection_of_the_same_name_is_reused() {
        let (_dir, store) = temp_store();
        let shooters = store.create_collection("Shooters").unwrap();

        apply_snapshot(
            &store,
            &snapshot(vec![flagged(false, false, &["Shooters"])]),
            100,
        )
        .unwrap();

        assert_eq!(store.collections().unwrap(), vec![shooters.clone()]);
        assert_eq!(store.collection_members(shooters.id).unwrap(), vec![id()]);
    }

    #[test]
    fn source_art_is_replaced_and_other_art_kept() {
        let (_dir, store) = temp_store();
        let mut game = flagged(false, false, &[]);
        game.artwork = vec![
            (ArtLayout::Cover, "/old-cover.jpg".into()),
            (ArtLayout::Hero, "/hero.jpg".into()),
        ];
        apply_snapshot(&store, &snapshot(vec![game.clone()]), 100).unwrap();
        let user_cover = Artwork {
            game_id: id(),
            layout: ArtLayout::Cover,
            origin: ArtOrigin::User,
            path: "/mine.png".into(),
        };
        store.set_artwork(&user_cover).unwrap();
        game.artwork = vec![(ArtLayout::Cover, "/new-cover.jpg".into())];

        apply_snapshot(&store, &snapshot(vec![game]), 200).unwrap();

        let source_cover = Artwork {
            game_id: id(),
            layout: ArtLayout::Cover,
            origin: ArtOrigin::Source,
            path: "/new-cover.jpg".into(),
        };
        assert_eq!(
            store.artwork(&id()).unwrap(),
            vec![source_cover, user_cover]
        );
    }

    #[test]
    fn last_played_keeps_the_later_of_ours_and_the_sources() {
        let (_dir, store) = temp_store();
        let mut game = flagged(false, false, &[]);
        game.last_played_at = Some(500);
        apply_snapshot(&store, &snapshot(vec![game.clone()]), 100).unwrap();
        game.last_played_at = Some(300);

        apply_snapshot(&store, &snapshot(vec![game]), 200).unwrap();

        assert_eq!(
            store.game(&id()).unwrap().unwrap().last_played_at,
            Some(500)
        );
    }

    fn other_game(native_id: &str) -> ImportedGame {
        ImportedGame::new(native_id, native_id, LaunchKind::StoreUri)
    }

    #[test]
    fn a_game_gone_from_its_source_gets_missing_since_and_leaves_the_library() {
        let (_dir, store) = temp_store();
        apply_snapshot(
            &store,
            &snapshot(vec![flagged(false, false, &[]), other_game("2")]),
            100,
        )
        .unwrap();

        apply_snapshot(&store, &snapshot(vec![other_game("2")]), 200).unwrap();

        let gone = store.game(&id()).unwrap().unwrap();
        assert_eq!(gone.missing_since, Some(200));
        assert!(!gone.in_library());
        assert!(
            store
                .game(&GameId::new(Source::HeroicGog, "2"))
                .unwrap()
                .unwrap()
                .in_library()
        );
    }

    #[test]
    fn missing_since_keeps_the_first_time_the_game_was_missed() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        apply_snapshot(&store, &snapshot(vec![]), 200).unwrap();

        apply_snapshot(&store, &snapshot(vec![]), 300).unwrap();

        assert_eq!(store.game(&id()).unwrap().unwrap().missing_since, Some(200));
    }

    #[test]
    fn a_returning_game_comes_back_with_its_user_data() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        let mut game = store.game(&id()).unwrap().unwrap();
        game.favourite = true;
        store.update_game(&game).unwrap();
        let collection = store.create_collection("Mine").unwrap();
        store.add_to_collection(collection.id, &id()).unwrap();
        apply_snapshot(&store, &snapshot(vec![]), 200).unwrap();

        apply_snapshot(&store, &snapshot(vec![flagged(true, false, &[])]), 300).unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert!(game.in_library());
        assert!(game.favourite);
        assert!(!game.hidden);
        assert_eq!(store.collection_members(collection.id).unwrap(), vec![id()]);
    }

    #[test]
    fn a_user_removed_game_stays_removed_while_its_source_lists_it() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        let mut game = store.game(&id()).unwrap().unwrap();
        game.user_removed = true;
        store.update_game(&game).unwrap();

        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 200).unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert!(game.user_removed);
        assert!(!game.in_library());
    }

    #[test]
    fn a_user_removed_game_that_leaves_its_source_returns_when_it_comes_back() {
        let (_dir, store) = temp_store();
        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 100).unwrap();
        let mut game = store.game(&id()).unwrap().unwrap();
        game.user_removed = true;
        store.update_game(&game).unwrap();
        apply_snapshot(&store, &snapshot(vec![]), 200).unwrap();

        apply_snapshot(&store, &snapshot(vec![flagged(false, false, &[])]), 300).unwrap();

        assert!(store.game(&id()).unwrap().unwrap().in_library());
    }

    #[test]
    fn games_of_other_sources_are_left_alone() {
        let (_dir, store) = temp_store();
        let steam = Snapshot {
            source: Source::Steam,
            games: vec![other_game("570")],
            errors: Vec::new(),
        };
        apply_snapshot(&store, &steam, 100).unwrap();

        apply_snapshot(&store, &snapshot(vec![]), 200).unwrap();

        let dota = store
            .game(&GameId::new(Source::Steam, "570"))
            .unwrap()
            .unwrap();
        assert_eq!(dota.missing_since, None);
    }

    fn sideload_home(home: &std::path::Path, games: &str) {
        let dir = home.join(".config/heroic/sideload_apps");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("library.json"),
            format!(r#"{{"games": [{games}]}}"#),
        )
        .unwrap();
    }

    fn host_dirs(home: &std::path::Path) -> HostDirs {
        HostDirs {
            home: home.to_owned(),
            data_home: home.join(".local/share"),
            config_home: home.join(".config"),
        }
    }

    const SIDELOADED: &str = r#"{"app_name": "side", "title": "Side", "is_installed": true}"#;

    #[test]
    fn syncing_a_source_imports_it_and_records_its_state() {
        let (dir, store) = temp_store();
        sideload_home(dir.path(), SIDELOADED);

        let problems =
            sync_source(&store, Source::HeroicSideload, &host_dirs(dir.path()), 100).unwrap();

        assert!(problems.is_empty(), "{problems:?}");
        assert!(
            store
                .game(&GameId::new(Source::HeroicSideload, "side"))
                .unwrap()
                .is_some()
        );
        let state = store.source_state(Source::HeroicSideload).unwrap().unwrap();
        assert_eq!(state.last_sync, Some(100));
        assert_eq!(
            state.resolved_location,
            Some(dir.path().join(".config/heroic"))
        );
        assert!(state.errors.is_empty());
    }

    #[test]
    fn a_source_that_cannot_be_read_keeps_its_games_and_reports_why() {
        let (dir, store) = temp_store();
        sideload_home(dir.path(), SIDELOADED);
        sync_source(&store, Source::HeroicSideload, &host_dirs(dir.path()), 100).unwrap();
        std::fs::write(
            dir.path().join(".config/heroic/sideload_apps/library.json"),
            "{ broken",
        )
        .unwrap();

        let problems =
            sync_source(&store, Source::HeroicSideload, &host_dirs(dir.path()), 200).unwrap();

        assert_eq!(problems.len(), 1);
        let game = store
            .game(&GameId::new(Source::HeroicSideload, "side"))
            .unwrap()
            .unwrap();
        assert!(game.in_library());
        let state = store.source_state(Source::HeroicSideload).unwrap().unwrap();
        assert_eq!(state.errors, problems);
    }

    #[test]
    fn a_source_no_longer_on_the_machine_has_its_games_marked_missing() {
        let (dir, store) = temp_store();
        sideload_home(dir.path(), SIDELOADED);
        sync_source(&store, Source::HeroicSideload, &host_dirs(dir.path()), 100).unwrap();
        std::fs::remove_dir_all(dir.path().join(".config/heroic")).unwrap();

        let problems =
            sync_source(&store, Source::HeroicSideload, &host_dirs(dir.path()), 200).unwrap();

        assert!(problems.is_empty());
        let game = store
            .game(&GameId::new(Source::HeroicSideload, "side"))
            .unwrap()
            .unwrap();
        assert_eq!(game.missing_since, Some(200));
    }

    #[test]
    fn sync_all_skips_disabled_sources_and_collects_problems_per_source() {
        let (dir, store) = temp_store();
        sideload_home(dir.path(), SIDELOADED);
        let gog_cache = dir.path().join(".config/heroic/store_cache");
        std::fs::create_dir_all(&gog_cache).unwrap();
        std::fs::write(gog_cache.join("gog_library.json"), "{ broken").unwrap();
        let mut disabled = SourceState::new(Source::HeroicSideload);
        disabled.enabled = false;
        store.set_source_state(&disabled).unwrap();

        let summary = sync_all(&store, &host_dirs(dir.path()), 100).unwrap();

        assert!(
            store
                .game(&GameId::new(Source::HeroicSideload, "side"))
                .unwrap()
                .is_none()
        );
        assert_eq!(summary.problems.len(), 1);
        assert_eq!(summary.problems[0].0, Source::HeroicGog);
    }
}
