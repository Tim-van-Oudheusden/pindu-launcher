//! Saved filters: a library view with its filter toggles and search, kept as a dynamic
//! collection. Its members are computed from the library each time it loads, never stored,
//! by the same matching as the library views ([`LibraryFilter::matches`]).
//!
//! A saved filter is a row of `collections` (so it is listed, ordered, renamed and deleted
//! like any collection) with its filter in `saved_filters`. Games cannot be added to it by
//! hand.

use std::collections::HashMap;

use crate::library::{Filters, LibraryEntry, LibraryFilter, SearchKey, View};
use crate::model::{Game, Source};

/// What a saved filter shows: a library view, narrowed by its filter toggles and search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFilter {
    /// The library view it starts from; never [`View::Hidden`].
    pub view: View,
    /// The filter toggles; never [`Filters::hidden`].
    pub filters: Filters,
    /// The search, normalised (see [`crate::search::normalize`]); empty for none.
    pub search: String,
}

impl SavedFilter {
    /// The filter `filter` shows, to save: `None` when there is nothing to save beyond a
    /// plain view (no toggle and no search), in the Hidden view, and in the view of a saved
    /// filter, one of `saved` by collection ID. Fullscreen mode's Show hidden games is left
    /// out.
    pub fn of(filter: &LibraryFilter, saved: &HashMap<i64, SavedFilter>) -> Option<Self> {
        let view = filter.view();
        let filters = Filters {
            hidden: false,
            ..filter.filters().clone()
        };
        let search = filter.search().to_owned();
        let narrowed = filters.any() || !search.is_empty();
        let saveable = match view {
            View::Hidden => false,
            View::Collection(id) => !saved.contains_key(&id),
            _ => true,
        };
        (narrowed && saveable).then_some(Self {
            view,
            filters,
            search,
        })
    }

    /// Whether the saved filter holds `game`, held by the collections `collections`.
    pub fn matches(&self, game: &Game, collections: &[i64]) -> bool {
        self.library_filter()
            .matches(game, collections, &SearchKey::new(&game.title))
    }

    fn library_filter(&self) -> LibraryFilter {
        let mut filter = LibraryFilter::default();
        filter.set_view(self.view);
        filter.set_filters(Filters {
            hidden: false,
            ..self.filters.clone()
        });
        filter.set_search(&self.search);
        filter
    }
}

/// Adds the saved filters holding each game to its collections, kept ascending. The
/// filters see only the games' own collections; games stored in a saved filter's
/// collection are dropped from it, as its members are computed.
pub(crate) fn add_members(entries: &mut [LibraryEntry], saved: &HashMap<i64, SavedFilter>) {
    if saved.is_empty() {
        return;
    }
    let mut filters: Vec<(i64, LibraryFilter)> = saved
        .iter()
        .map(|(&id, filter)| (id, filter.library_filter()))
        .collect();
    filters.sort_unstable_by_key(|(id, _)| *id);
    for entry in entries {
        entry.collections.retain(|id| !saved.contains_key(id));
        let key = SearchKey::new(&entry.game.title);
        let held: Vec<i64> = filters
            .iter()
            .filter(|(_, filter)| filter.matches(&entry.game, &entry.collections, &key))
            .map(|(id, _)| *id)
            .collect();
        if !held.is_empty() {
            entry.collections.extend(held);
            entry.collections.sort_unstable();
        }
    }
}

/// Shelves offered for saving that need nothing the library does not hold already.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suggestion {
    /// Native Linux games.
    Native,
    /// Windows games, run through Proton.
    Windows,
    /// Installed games never played: the backlog ready to start.
    InstalledNeverPlayed,
}

impl Suggestion {
    /// Every suggestion, in the order they are offered.
    pub const ALL: [Self; 3] = [Self::Native, Self::Windows, Self::InstalledNeverPlayed];

    /// The suggestion's stable name, as action targets carry it.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Native => "native",
            Self::Windows => "windows",
            Self::InstalledNeverPlayed => "installed-never-played",
        }
    }

    /// The suggestion named `name` (see [`Self::as_str`]).
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|suggestion| suggestion.as_str() == name)
    }

    /// The filter the suggestion saves.
    pub fn filter(self) -> SavedFilter {
        let filters = match self {
            Self::Native => Filters {
                native: true,
                ..Filters::default()
            },
            Self::Windows => Filters {
                windows: true,
                ..Filters::default()
            },
            Self::InstalledNeverPlayed => Filters {
                installed: true,
                never_played: true,
                ..Filters::default()
            },
        };
        SavedFilter {
            view: View::All,
            filters,
            search: String::new(),
        }
    }
}

const SOURCE_PREFIX: &str = "source:";
const COLLECTION_PREFIX: &str = "collection:";

/// How the store keeps a saved filter's view.
pub(crate) fn view_text(view: View) -> String {
    match view {
        View::All => "all".to_owned(),
        View::Installed => "installed".to_owned(),
        View::Favourites => "favourites".to_owned(),
        View::PlayHistory => "play-history".to_owned(),
        View::Hidden => "hidden".to_owned(),
        View::Source(source) => format!("{SOURCE_PREFIX}{}", source.as_str()),
        View::Collection(id) => format!("{COLLECTION_PREFIX}{id}"),
    }
}

/// The view the store kept as `text` (see [`view_text`]); `None` for one this version does
/// not know. The Hidden view is never a saved filter's.
pub(crate) fn view_from_text(text: &str) -> Option<View> {
    match text {
        "all" => Some(View::All),
        "installed" => Some(View::Installed),
        "favourites" => Some(View::Favourites),
        "play-history" => Some(View::PlayHistory),
        _ => {
            if let Some(source) = text.strip_prefix(SOURCE_PREFIX) {
                source.parse::<Source>().ok().map(View::Source)
            } else {
                let id = text.strip_prefix(COLLECTION_PREFIX)?;
                id.parse().ok().map(View::Collection)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::library::{Filters, LibraryFilter, View};
    use crate::model::{Game, GameId, LaunchKind, Platform, Source};

    fn game(id: &str, title: &str, platform: Platform, installed: bool) -> Game {
        let mut game = Game::new(
            GameId::new(Source::Steam, id),
            title,
            LaunchKind::StoreUri,
            0,
        );
        game.platform = Some(platform);
        game.installed = installed;
        game
    }

    fn windows_only() -> Filters {
        Filters {
            windows: true,
            ..Filters::default()
        }
    }

    #[test]
    fn a_saved_filter_holds_the_games_its_view_toggles_and_search_show() {
        let saved = SavedFilter {
            view: View::Installed,
            filters: windows_only(),
            search: "witcher".to_owned(),
        };

        assert!(saved.matches(&game("1", "The Witcher 3", Platform::Windows, true), &[]));
        assert!(!saved.matches(&game("2", "The Witcher 3", Platform::Linux, true), &[]));
        assert!(!saved.matches(&game("3", "The Witcher 3", Platform::Windows, false), &[]));
        assert!(!saved.matches(&game("4", "Portal", Platform::Windows, true), &[]));
    }

    #[test]
    fn a_saved_filter_never_holds_hidden_or_removed_games() {
        let saved = SavedFilter {
            view: View::All,
            filters: windows_only(),
            search: String::new(),
        };
        let mut hidden = game("1", "Hidden", Platform::Windows, true);
        hidden.hidden = true;
        let mut removed = game("2", "Removed", Platform::Windows, true);
        removed.user_removed = true;

        assert!(!saved.matches(&hidden, &[]));
        assert!(!saved.matches(&removed, &[]));
    }

    #[test]
    fn a_saved_filter_on_a_collection_holds_only_its_members() {
        let saved = SavedFilter {
            view: View::Collection(4),
            filters: windows_only(),
            search: String::new(),
        };
        let member = game("1", "Member", Platform::Windows, true);

        assert!(saved.matches(&member, &[4]));
        assert!(!saved.matches(&member, &[5]));
    }

    #[test]
    fn the_shown_filter_saves_its_view_toggles_and_search() {
        let mut filter = LibraryFilter::default();
        filter.set_view(View::Source(Source::Lutris));
        filter.set_filters(Filters {
            hidden: true,
            ..windows_only()
        });
        filter.set_search("Witcher");

        assert_eq!(
            SavedFilter::of(&filter, &HashMap::new()),
            Some(SavedFilter {
                view: View::Source(Source::Lutris),
                // Showing hidden games is fullscreen mode's, not part of a filter.
                filters: windows_only(),
                search: "witcher".to_owned(),
            })
        );
    }

    #[test]
    fn a_search_alone_is_a_filter_to_save() {
        let mut filter = LibraryFilter::default();
        filter.set_search("portal");

        assert_eq!(
            SavedFilter::of(&filter, &HashMap::new()).map(|saved| saved.search),
            Some("portal".to_owned())
        );
    }

    #[test]
    fn a_plain_view_hidden_games_or_another_saved_filter_is_nothing_to_save() {
        let mut filter = LibraryFilter::default();
        filter.set_view(View::Installed);
        assert_eq!(SavedFilter::of(&filter, &HashMap::new()), None);

        filter.set_filters(windows_only());
        filter.set_view(View::Hidden);
        assert_eq!(SavedFilter::of(&filter, &HashMap::new()), None);

        let saved = SavedFilter {
            view: View::All,
            filters: windows_only(),
            search: String::new(),
        };
        filter.set_view(View::Collection(9));
        assert_eq!(
            SavedFilter::of(&filter, &HashMap::from([(9, saved.clone())])),
            None
        );
        filter.set_view(View::Collection(8));
        assert!(SavedFilter::of(&filter, &HashMap::from([(9, saved)])).is_some());
    }

    #[test]
    fn views_are_stored_as_text_and_read_back() {
        let views = [
            View::All,
            View::Installed,
            View::Favourites,
            View::PlayHistory,
            View::Source(Source::HeroicGog),
            View::Collection(12),
        ];
        for view in views {
            assert_eq!(view_from_text(&view_text(view)), Some(view), "{view:?}");
        }
        assert_eq!(view_text(View::Source(Source::Steam)), "source:steam");
        assert_eq!(view_text(View::Collection(12)), "collection:12");
        assert_eq!(view_from_text("collection:x"), None);
        assert_eq!(view_from_text("nonsense"), None);
    }

    #[test]
    fn suggestions_are_native_windows_and_installed_never_played_games() {
        assert_eq!(
            Suggestion::ALL.map(|suggestion| (suggestion.as_str(), suggestion.filter())),
            [
                (
                    "native",
                    SavedFilter {
                        view: View::All,
                        filters: Filters {
                            native: true,
                            ..Filters::default()
                        },
                        search: String::new(),
                    }
                ),
                (
                    "windows",
                    SavedFilter {
                        view: View::All,
                        filters: windows_only(),
                        search: String::new(),
                    }
                ),
                (
                    "installed-never-played",
                    SavedFilter {
                        view: View::All,
                        filters: Filters {
                            installed: true,
                            never_played: true,
                            ..Filters::default()
                        },
                        search: String::new(),
                    }
                ),
            ]
        );
        assert_eq!(Suggestion::named("windows"), Some(Suggestion::Windows));
        assert_eq!(Suggestion::named("other"), None);
    }

    #[test]
    fn loading_the_library_computes_saved_filter_members() {
        use crate::store::Store;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let windows = game("1", "Windows", Platform::Windows, true);
        let native = game("2", "Native", Platform::Linux, true);
        for game in [&windows, &native] {
            store.insert_game(game).unwrap();
        }
        let manual = store.create_collection("Manual").unwrap();
        store.add_to_collection(manual.id, &native.id).unwrap();
        let saved = store
            .create_saved_filter("Windows Games", &Suggestion::Windows.filter())
            .unwrap();

        let entries =
            crate::library::load(&store, false, crate::game_groups::DEFAULT_SOURCE_ORDER).unwrap();
        let collections = |id: &GameId| {
            entries
                .iter()
                .find(|entry| entry.game.id == *id)
                .unwrap()
                .collections
                .clone()
        };
        assert_eq!(collections(&windows.id), [saved.id]);
        assert_eq!(collections(&native.id), [manual.id]);
    }

    #[test]
    fn games_stored_in_a_saved_filter_do_not_count_as_members() {
        use crate::store::Store;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let native = game("2", "Native", Platform::Linux, true);
        store.insert_game(&native).unwrap();
        let saved = store
            .create_saved_filter("Windows Games", &Suggestion::Windows.filter())
            .unwrap();
        store.add_to_collection(saved.id, &native.id).unwrap();

        let entries =
            crate::library::load(&store, false, crate::game_groups::DEFAULT_SOURCE_ORDER).unwrap();
        assert!(entries[0].collections.is_empty());
    }
}
