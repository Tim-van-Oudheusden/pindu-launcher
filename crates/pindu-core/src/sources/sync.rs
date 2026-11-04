//! Reconciles an importer's [`Snapshot`] with the library.

use super::{ImportedGame, Snapshot};
use crate::model::{ArtOrigin, Artwork, Game, GameId, Timestamp};
use crate::store::{self, Store};

/// Writes a snapshot into the library in one transaction.
///
/// New games take the source's hidden, favourite and category flags; for games already in
/// the library those flags are ours and only the source's own fields are refreshed.
pub fn apply_snapshot(store: &Store, snapshot: &Snapshot, now: Timestamp) -> store::Result<()> {
    store.in_transaction(|store| {
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
        }
        Ok(())
    })
}

/// Copies the fields the source owns onto a game.
fn refresh(game: &mut Game, imported: &ImportedGame) {
    game.title.clone_from(&imported.title);
    game.sort_title.clone_from(&imported.title);
    game.developer.clone_from(&imported.developer);
    game.platform = imported.platform;
    game.installed = imported.installed;
    game.install_path.clone_from(&imported.install_path);
    game.size_bytes = imported.size_bytes;
    game.last_played_at = game.last_played_at.max(imported.last_played_at);
    game.launch_kind = imported.launch_kind;
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

        apply_snapshot(&store, &snapshot(vec![renamed]), 200).unwrap();

        let game = store.game(&id()).unwrap().unwrap();
        assert_eq!(game.title, "UT2004");
        assert!(game.installed);
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
}
