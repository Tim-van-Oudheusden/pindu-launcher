//! Selection mode's bulk actions: the games the desktop library has selected, what its
//! action bar offers for them, and applying one action to all of them at once.

use std::collections::{BTreeSet, HashMap, HashSet};

use crate::model::{Collection, Game, GameId};
use crate::store::{self, Store};

/// One action on every selected game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BulkAction {
    /// Adds the games to Favourites.
    Favourite,
    /// Takes the games out of Favourites.
    Unfavourite,
    /// Hides the games.
    Hide,
    /// Shows the hidden games again.
    Unhide,
    /// Adds the games to the collection with this ID.
    AddToCollection(i64),
    /// Takes the games out of the collection with this ID.
    RemoveFromCollection(i64),
}

impl BulkAction {
    /// The action that undoes this one, applied to the games it changed.
    pub fn undone(self) -> Self {
        match self {
            Self::Favourite => Self::Unfavourite,
            Self::Unfavourite => Self::Favourite,
            Self::Hide => Self::Unhide,
            Self::Unhide => Self::Hide,
            Self::AddToCollection(id) => Self::RemoveFromCollection(id),
            Self::RemoveFromCollection(id) => Self::AddToCollection(id),
        }
    }
}

/// What selection mode's action bar offers for the selected games.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offers {
    /// [`BulkAction::Unfavourite`] when every selected game is a favourite, otherwise
    /// [`BulkAction::Favourite`].
    pub favourite: BulkAction,
    /// [`BulkAction::Unhide`] when every selected game is hidden, otherwise
    /// [`BulkAction::Hide`].
    pub hide: BulkAction,
    /// The collections, in the user's order, that do not hold every selected game.
    pub add_to: Vec<i64>,
    /// The collections, in the user's order, that hold at least one selected game.
    pub remove_from: Vec<i64>,
}

impl Offers {
    /// The offers for `selected`, each game with the IDs of the collections holding it,
    /// among `collections` in the user's order.
    pub fn of<'a>(
        selected: impl IntoIterator<Item = (&'a Game, &'a [i64])>,
        collections: &[Collection],
    ) -> Self {
        let (mut count, mut favourites, mut hidden) = (0, 0, 0);
        let mut held: HashMap<i64, usize> = HashMap::new();
        for (game, member_of) in selected {
            count += 1;
            favourites += usize::from(game.favourite);
            hidden += usize::from(game.hidden);
            for collection in member_of {
                *held.entry(*collection).or_default() += 1;
            }
        }
        let all = |shared: usize| count > 0 && shared == count;
        let holding = |collection: &Collection| held.get(&collection.id).copied().unwrap_or(0);
        Self {
            favourite: if all(favourites) {
                BulkAction::Unfavourite
            } else {
                BulkAction::Favourite
            },
            hide: if all(hidden) {
                BulkAction::Unhide
            } else {
                BulkAction::Hide
            },
            add_to: collections
                .iter()
                .filter(|collection| !all(holding(collection)))
                .map(|collection| collection.id)
                .collect(),
            remove_from: collections
                .iter()
                .filter(|collection| holding(collection) > 0)
                .map(|collection| collection.id)
                .collect(),
        }
    }
}

/// The games selection mode has selected, by ID, so they stay selected while a search,
/// filter or view hides some of them, or a reload remakes the shown games.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    games: BTreeSet<GameId>,
}

impl Selection {
    /// Selects `game` when it is not selected, or deselects it; whether it is selected
    /// now.
    pub fn toggle(&mut self, game: &GameId) -> bool {
        let selected = !self.games.remove(game);
        if selected {
            self.games.insert(game.clone());
        }
        selected
    }

    /// Selects every game of `games` too.
    pub fn select_all(&mut self, games: impl IntoIterator<Item = GameId>) {
        self.games.extend(games);
    }

    /// Deselects every game.
    pub fn clear(&mut self) {
        self.games.clear();
    }

    /// Whether `game` is selected.
    pub fn contains(&self, game: &GameId) -> bool {
        self.games.contains(game)
    }

    /// How many games are selected.
    pub fn len(&self) -> usize {
        self.games.len()
    }

    /// Whether no game is selected.
    pub fn is_empty(&self) -> bool {
        self.games.is_empty()
    }

    /// The selected games, by ID.
    pub fn games(&self) -> Vec<GameId> {
        self.games.iter().cloned().collect()
    }
}

/// Applies `action` to `games` in one transaction and returns the games it changed, in
/// the order given: games already as the action leaves them, or no longer stored, are
/// left out, so undoing touches only what the action did.
pub fn apply(store: &Store, games: &[GameId], action: BulkAction) -> store::Result<Vec<GameId>> {
    store.in_transaction(|store| {
        let members: HashSet<GameId> = match action {
            BulkAction::AddToCollection(collection)
            | BulkAction::RemoveFromCollection(collection) => {
                store.collection_members(collection)?.into_iter().collect()
            }
            _ => HashSet::new(),
        };
        let mut changed = Vec::new();
        for id in games {
            let Some(mut game) = store.game(id)? else {
                continue;
            };
            let flag = match action {
                BulkAction::Favourite | BulkAction::Unfavourite => Some(&mut game.favourite),
                BulkAction::Hide | BulkAction::Unhide => Some(&mut game.hidden),
                BulkAction::AddToCollection(_) | BulkAction::RemoveFromCollection(_) => None,
            };
            let changes = match (action, flag) {
                (_, Some(flag)) => {
                    let wanted = matches!(action, BulkAction::Favourite | BulkAction::Hide);
                    let changes = *flag != wanted;
                    *flag = wanted;
                    changes
                }
                (BulkAction::AddToCollection(_), None) => !members.contains(id),
                (_, None) => members.contains(id),
            };
            if !changes {
                continue;
            }
            match action {
                BulkAction::AddToCollection(collection) => {
                    store.add_to_collection(collection, id)?
                }
                BulkAction::RemoveFromCollection(collection) => {
                    store.remove_from_collection(collection, id)?;
                }
                _ => store.update_game(&game)?,
            }
            changed.push(id.clone());
        }
        Ok(changed)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, LaunchKind, Source};

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        for (native_id, title) in [("570", "Dota 2"), ("620", "Portal 2"), ("400", "Portal")] {
            let game = Game::new(
                GameId::new(Source::Steam, native_id),
                title,
                LaunchKind::StoreUri,
                1_700_000_000,
            );
            store.insert_game(&game).unwrap();
        }
        (dir, store)
    }

    fn id(native_id: &str) -> GameId {
        GameId::new(Source::Steam, native_id)
    }

    #[test]
    fn favouriting_marks_every_selected_game_and_returns_those_it_changed() {
        let (_dir, store) = temp_store();
        let mut portal = store.game(&id("400")).unwrap().unwrap();
        portal.favourite = true;
        store.update_game(&portal).unwrap();

        let changed = apply(
            &store,
            &[id("570"), id("400"), id("620")],
            BulkAction::Favourite,
        )
        .unwrap();

        assert_eq!(changed, [id("570"), id("620")]);
        for native_id in ["570", "620", "400"] {
            assert!(store.game(&id(native_id)).unwrap().unwrap().favourite);
        }
    }

    #[test]
    fn adding_to_a_collection_returns_only_the_games_not_in_it_yet() {
        let (_dir, store) = temp_store();
        let puzzles = store.create_collection("Puzzles").unwrap();
        store.add_to_collection(puzzles.id, &id("400")).unwrap();

        let changed = apply(
            &store,
            &[id("620"), id("400"), id("999")],
            BulkAction::AddToCollection(puzzles.id),
        )
        .unwrap();

        assert_eq!(changed, [id("620")]);
        assert_eq!(
            store.collection_members(puzzles.id).unwrap(),
            [id("400"), id("620")]
        );

        let changed = apply(
            &store,
            &[id("570"), id("400")],
            BulkAction::RemoveFromCollection(puzzles.id),
        )
        .unwrap();

        assert_eq!(changed, [id("400")]);
        assert_eq!(store.collection_members(puzzles.id).unwrap(), [id("620")]);
    }

    #[test]
    fn undoing_a_hide_shows_only_the_games_it_hid() {
        let (_dir, store) = temp_store();
        let mut portal = store.game(&id("400")).unwrap().unwrap();
        portal.hidden = true;
        store.update_game(&portal).unwrap();

        let changed = apply(&store, &[id("570"), id("400")], BulkAction::Hide).unwrap();
        apply(&store, &changed, BulkAction::Hide.undone()).unwrap();

        assert!(!store.game(&id("570")).unwrap().unwrap().hidden);
        assert!(store.game(&id("400")).unwrap().unwrap().hidden);
    }

    fn collection(id: i64, name: &str) -> Collection {
        Collection {
            id,
            name: name.to_owned(),
            position: id,
        }
    }

    #[test]
    fn offers_undo_what_every_selected_game_shares_and_list_fitting_collections() {
        let mut dota = Game::new(id("570"), "Dota 2", LaunchKind::StoreUri, 0);
        let mut portal = Game::new(id("400"), "Portal", LaunchKind::StoreUri, 0);
        dota.favourite = true;
        portal.favourite = true;
        portal.hidden = true;
        let collections = [
            collection(3, "Puzzles"),
            collection(1, "Later"),
            collection(2, "Done"),
        ];

        let offers = Offers::of([(&dota, &[3][..]), (&portal, &[3, 1][..])], &collections);

        assert_eq!(
            offers,
            Offers {
                favourite: BulkAction::Unfavourite,
                hide: BulkAction::Hide,
                add_to: vec![1, 2],
                remove_from: vec![3, 1],
            }
        );
    }

    #[test]
    fn a_selection_toggles_games_selects_many_at_once_and_clears() {
        let mut selection = Selection::default();

        assert!(selection.toggle(&id("570")));
        assert!(selection.toggle(&id("400")));
        assert!(!selection.toggle(&id("570")));
        assert_eq!(selection.games(), [id("400")]);

        selection.select_all([id("620"), id("400")]);
        assert_eq!(selection.len(), 2);
        assert!(selection.contains(&id("620")));

        selection.clear();
        assert!(selection.is_empty());
    }
}
