//! The library as the shells see it: games with their art, and their ordering and
//! matching, shared by both shells' list models.

use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::art;
use crate::model::{ArtLayout, Artwork, Game, GameId, Platform, Source, Timestamp};
use crate::search::{self, Query};
use crate::store::{self, Store};

/// The image chosen for each layout of a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtSet {
    pub cover: Option<PathBuf>,
    pub hero: Option<PathBuf>,
    pub logo: Option<PathBuf>,
    pub icon: Option<PathBuf>,
}

impl ArtSet {
    fn from(artwork: &[Artwork]) -> Self {
        let path = |layout| art::best(artwork, layout).map(|art| art.path.clone());
        Self {
            cover: path(ArtLayout::Cover),
            hero: path(ArtLayout::Hero),
            logo: path(ArtLayout::Logo),
            icon: path(ArtLayout::Icon),
        }
    }
}

/// A game with its art and total playtime, as the library views show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryEntry {
    pub game: Game,
    pub art: ArtSet,
    pub playtime_secs: i64,
}

/// Every stored game with its best art per layout and its playtime: the source's own
/// total when it tracks one, else the sum of our sessions.
pub fn load(store: &Store) -> store::Result<Vec<LibraryEntry>> {
    let mut artwork: HashMap<GameId, Vec<Artwork>> = HashMap::new();
    for art in store.all_artwork()? {
        artwork.entry(art.game_id.clone()).or_default().push(art);
    }
    let session_totals = store.session_totals()?;
    let imported_totals = store.imported_totals()?;
    Ok(store
        .games()?
        .into_iter()
        .map(|game| {
            let art = artwork
                .get(&game.id)
                .map_or_else(ArtSet::default, |art| ArtSet::from(art));
            let playtime_secs = imported_totals
                .get(&game.id)
                .or_else(|| session_totals.get(&game.id))
                .copied()
                .unwrap_or_default();
            LibraryEntry {
                game,
                art,
                playtime_secs,
            }
        })
        .collect())
}

/// The library sort orders, as offered in the sort menu.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SortMode {
    /// Most recently played first; games never played follow by name.
    #[default]
    LastPlayed,
    NameAscending,
    NameDescending,
    /// Newest first.
    RecentlyAdded,
    /// Most played first.
    Playtime,
}

/// What sorting needs from a game, computed once so comparisons stay cheap in big
/// libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    name: String,
    last_played_at: Option<Timestamp>,
    added_at: Timestamp,
    playtime_secs: i64,
    id: String,
}

impl SortKey {
    pub fn new(game: &Game, playtime_secs: i64) -> Self {
        Self {
            name: search::sort_name(&game.sort_title),
            last_played_at: game.last_played_at,
            added_at: game.added_at,
            playtime_secs,
            id: game.id.as_str().to_owned(),
        }
    }
}

/// Compares two games for `mode`; ties fall back to the name, then the ID, so the order is
/// total and stable.
pub fn compare(mode: SortMode, a: &SortKey, b: &SortKey) -> Ordering {
    let by_name = || a.name.cmp(&b.name).then_with(|| a.id.cmp(&b.id));
    match mode {
        SortMode::LastPlayed => Reverse(a.last_played_at)
            .cmp(&Reverse(b.last_played_at))
            .then_with(by_name),
        SortMode::NameAscending => by_name(),
        SortMode::NameDescending => b.name.cmp(&a.name).then_with(|| a.id.cmp(&b.id)),
        SortMode::RecentlyAdded => b.added_at.cmp(&a.added_at).then_with(by_name),
        SortMode::Playtime => b.playtime_secs.cmp(&a.playtime_secs).then_with(by_name),
    }
}

/// A game title prepared for searching, computed once per title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchKey(String);

impl SearchKey {
    pub fn new(title: &str) -> Self {
        Self(search::normalize(title))
    }
}

/// How a filter changed, so list models can re-check only the games that can change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterChange {
    Unchanged,
    /// Only games shown before can be dropped.
    MoreStrict,
    /// Only games hidden before can appear.
    LessStrict,
    Different,
}

/// The library views of the sidebar.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum View {
    #[default]
    All,
    Installed,
    Favourites,
    /// Games that have been played.
    PlayHistory,
    Source(Source),
}

impl View {
    fn includes(self, game: &Game) -> bool {
        match self {
            Self::All => true,
            Self::Installed => game.installed,
            Self::Favourites => game.favourite,
            Self::PlayHistory => game.last_played_at.is_some(),
            Self::Source(source) => game.source() == source,
        }
    }
}

/// The stacking filter toggles of the library page. Toggles of one kind widen each other
/// (Installed and Not installed together show both); kinds narrow each other.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Filters {
    pub installed: bool,
    pub not_installed: bool,
    pub favourites: bool,
    /// Windows games, run through Proton.
    pub windows: bool,
    /// Native Linux games.
    pub native: bool,
}

impl Filters {
    /// Whether any toggle is on.
    pub fn any(self) -> bool {
        self != Self::default()
    }

    fn includes(self, game: &Game) -> bool {
        let install_state = (!self.installed && !self.not_installed)
            || (self.installed && game.installed)
            || (self.not_installed && !game.installed);
        let platform = (!self.windows && !self.native)
            || (self.windows && game.platform == Some(Platform::Windows))
            || (self.native && game.platform == Some(Platform::Linux));
        install_state && platform && (!self.favourites || game.favourite)
    }
}

/// Which games the library views show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    view: View,
    filters: Filters,
    search: Query,
}

impl LibraryFilter {
    /// Shows only the games of `view`.
    pub fn set_view(&mut self, view: View) -> FilterChange {
        let change = match (self.view, view) {
            (old, new) if old == new => FilterChange::Unchanged,
            (View::All, _) => FilterChange::MoreStrict,
            (_, View::All) => FilterChange::LessStrict,
            _ => FilterChange::Different,
        };
        self.view = view;
        change
    }

    /// Shows only the games the toggles allow.
    pub fn set_filters(&mut self, filters: Filters) -> FilterChange {
        let change = if filters == self.filters {
            FilterChange::Unchanged
        } else {
            FilterChange::Different
        };
        self.filters = filters;
        change
    }

    /// Shows only games matching `text` (see [`search`]); empty shows all.
    pub fn set_search(&mut self, text: &str) -> FilterChange {
        let query = Query::new(text);
        let (old, new) = (self.search.as_str(), query.as_str());
        let change = if new == old {
            FilterChange::Unchanged
        } else if new.contains(old) {
            FilterChange::MoreStrict
        } else if old.contains(new) {
            FilterChange::LessStrict
        } else {
            FilterChange::Different
        };
        self.search = query;
        change
    }

    /// Whether a game shows: in the library, not hidden, in the view, allowed by the
    /// toggles and matching the search.
    pub fn matches(&self, game: &Game, key: &SearchKey) -> bool {
        game.in_library()
            && !game.hidden
            && self.view.includes(game)
            && self.filters.includes(game)
            && self.search.matches(&key.0)
    }
}

/// How many games each view shows, before any search.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Counts(HashMap<View, u32>);

impl Counts {
    pub fn of<'a>(games: impl IntoIterator<Item = &'a Game>) -> Self {
        let mut counts = HashMap::new();
        for game in games
            .into_iter()
            .filter(|game| game.in_library() && !game.hidden)
        {
            let views = [
                View::All,
                View::Installed,
                View::Favourites,
                View::PlayHistory,
                View::Source(game.source()),
            ];
            for view in views.into_iter().filter(|view| view.includes(game)) {
                *counts.entry(view).or_default() += 1;
            }
        }
        Self(counts)
    }

    pub fn get(&self, view: View) -> u32 {
        self.0.get(&view).copied().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;
    use crate::model::{Game, GameId, LaunchKind, Source};

    fn game(id: &str, title: &str, last_played_at: Option<i64>, added_at: i64) -> Game {
        let mut game = Game::new(
            GameId::new(Source::Steam, id),
            title,
            LaunchKind::StoreUri,
            added_at,
        );
        game.last_played_at = last_played_at;
        game
    }

    fn sorted(mode: SortMode, games: Vec<Game>) -> Vec<String> {
        let mut keyed: Vec<_> = games
            .into_iter()
            .map(|game| (SortKey::new(&game, 0), game.title))
            .collect();
        keyed.sort_by(|(a, _), (b, _)| compare(mode, a, b));
        keyed.into_iter().map(|(_, title)| title).collect()
    }

    #[test]
    fn last_played_puts_recent_games_first_and_unplayed_ones_last_by_name() {
        let games = vec![
            game("1", "Beta", None, 0),
            game("2", "Old", Some(100), 0),
            game("3", "Alpha", None, 0),
            game("4", "Recent", Some(500), 0),
        ];

        assert_eq!(
            sorted(SortMode::LastPlayed, games),
            ["Recent", "Old", "Alpha", "Beta"]
        );
    }

    #[test]
    fn name_sorts_ignore_case_and_go_both_ways() {
        let games = || {
            vec![
                game("1", "beta", None, 0),
                game("2", "Alpha", None, 0),
                game("3", "Gamma", None, 0),
            ]
        };

        assert_eq!(
            sorted(SortMode::NameAscending, games()),
            ["Alpha", "beta", "Gamma"]
        );
        assert_eq!(
            sorted(SortMode::NameDescending, games()),
            ["Gamma", "beta", "Alpha"]
        );
    }

    #[test]
    fn recently_added_puts_the_newest_first() {
        let games = vec![game("1", "First", None, 10), game("2", "Second", None, 20)];

        assert_eq!(sorted(SortMode::RecentlyAdded, games), ["Second", "First"]);
    }

    #[test]
    fn equal_games_fall_back_to_their_id_so_the_order_is_stable() {
        let a = SortKey::new(&game("1", "Same", None, 0), 0);
        let b = SortKey::new(&game("2", "Same", None, 0), 0);

        assert_eq!(compare(SortMode::NameAscending, &a, &b), Ordering::Less);
        assert_eq!(compare(SortMode::NameAscending, &b, &a), Ordering::Greater);
    }

    fn shows(filter: &LibraryFilter, game: &Game) -> bool {
        filter.matches(game, &SearchKey::new(&game.title))
    }

    #[test]
    fn the_filter_drops_hidden_missing_and_removed_games() {
        let filter = LibraryFilter::default();
        let visible = game("1", "Visible", None, 0);
        let mut hidden = game("2", "Hidden", None, 0);
        hidden.hidden = true;
        let mut missing = game("3", "Missing", None, 0);
        missing.missing_since = Some(1);
        let mut removed = game("4", "Removed", None, 0);
        removed.user_removed = true;

        assert!(shows(&filter, &visible));
        assert!(!shows(&filter, &hidden));
        assert!(!shows(&filter, &missing));
        assert!(!shows(&filter, &removed));
    }

    #[test]
    fn search_keeps_titles_containing_the_text_in_any_case() {
        let mut filter = LibraryFilter::default();
        filter.set_search("RING");

        assert!(shows(&filter, &game("1", "Elden Ring", None, 0)));
        assert!(!shows(&filter, &game("2", "Dota 2", None, 0)));
    }

    #[test]
    fn changing_the_search_reports_how_the_filter_changed() {
        let mut filter = LibraryFilter::default();

        assert_eq!(filter.set_search("ri"), FilterChange::MoreStrict);
        assert_eq!(filter.set_search("rin"), FilterChange::MoreStrict);
        assert_eq!(filter.set_search("rin"), FilterChange::Unchanged);
        assert_eq!(filter.set_search("r"), FilterChange::LessStrict);
        assert_eq!(filter.set_search("dota"), FilterChange::Different);
    }

    fn sample() -> Vec<Game> {
        let mut installed = game("1", "Installed", Some(10), 0);
        installed.installed = true;
        let mut favourite = game("2", "Favourite", None, 0);
        favourite.favourite = true;
        let mut heroic = Game::new(
            GameId::new(Source::HeroicGog, "3"),
            "Heroic",
            LaunchKind::StoreUri,
            0,
        );
        heroic.installed = true;
        let mut hidden = game("4", "Hidden", Some(20), 0);
        hidden.hidden = true;
        hidden.installed = true;
        let mut missing = game("5", "Missing", None, 0);
        missing.missing_since = Some(1);
        vec![installed, favourite, heroic, hidden, missing]
    }

    fn shown(view: View) -> Vec<String> {
        let mut filter = LibraryFilter::default();
        filter.set_view(view);
        sample()
            .into_iter()
            .filter(|game| shows(&filter, game))
            .map(|game| game.title)
            .collect()
    }

    #[test]
    fn each_view_shows_its_games() {
        assert_eq!(shown(View::All), ["Installed", "Favourite", "Heroic"]);
        assert_eq!(shown(View::Installed), ["Installed", "Heroic"]);
        assert_eq!(shown(View::Favourites), ["Favourite"]);
        assert_eq!(shown(View::PlayHistory), ["Installed"]);
        assert_eq!(shown(View::Source(Source::HeroicGog)), ["Heroic"]);
    }

    #[test]
    fn changing_the_view_is_reported() {
        let mut filter = LibraryFilter::default();

        assert_eq!(filter.set_view(View::All), FilterChange::Unchanged);
        assert_eq!(filter.set_view(View::Installed), FilterChange::MoreStrict);
        assert_eq!(filter.set_view(View::All), FilterChange::LessStrict);
        filter.set_view(View::Installed);
        assert_eq!(filter.set_view(View::Favourites), FilterChange::Different);
    }

    #[test]
    fn counts_cover_only_games_the_views_show() {
        let counts = Counts::of(&sample());

        assert_eq!(counts.get(View::All), 3);
        assert_eq!(counts.get(View::Installed), 2);
        assert_eq!(counts.get(View::Favourites), 1);
        assert_eq!(counts.get(View::PlayHistory), 1);
        assert_eq!(counts.get(View::Source(Source::Steam)), 2);
        assert_eq!(counts.get(View::Source(Source::HeroicGog)), 1);
        assert_eq!(counts.get(View::Source(Source::Lutris)), 0);
    }

    #[test]
    fn loading_pairs_each_game_with_its_best_art() {
        use crate::model::{ArtLayout, ArtOrigin, Artwork};
        use crate::store::Store;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let with_art = game("1", "With Art", None, 0);
        let without_art = game("2", "Without Art", None, 0);
        store.insert_game(&with_art).unwrap();
        store.insert_game(&without_art).unwrap();
        let set = |layout, origin, path: &str| {
            store
                .set_artwork(&Artwork {
                    game_id: with_art.id.clone(),
                    layout,
                    origin,
                    path: path.into(),
                })
                .unwrap();
        };
        set(ArtLayout::Cover, ArtOrigin::Source, "/source-cover.jpg");
        set(ArtLayout::Cover, ArtOrigin::User, "/user-cover.png");
        set(ArtLayout::Icon, ArtOrigin::Source, "/icon.jpg");

        let entries = load(&store).unwrap();

        assert_eq!(entries.len(), 2);
        let first = entries
            .iter()
            .find(|entry| entry.game.id == with_art.id)
            .unwrap();
        assert_eq!(
            first.art.cover.as_deref(),
            Some(std::path::Path::new("/user-cover.png"))
        );
        assert_eq!(
            first.art.icon.as_deref(),
            Some(std::path::Path::new("/icon.jpg"))
        );
        assert_eq!(first.art.hero, None);
        let second = entries
            .iter()
            .find(|entry| entry.game.id == without_art.id)
            .unwrap();
        assert_eq!(second.art, ArtSet::default());
    }

    fn shown_with(filters: Filters, games: &[Game]) -> Vec<String> {
        let mut filter = LibraryFilter::default();
        filter.set_filters(filters);
        games
            .iter()
            .filter(|game| shows(&filter, game))
            .map(|game| game.title.clone())
            .collect()
    }

    fn platform_games() -> Vec<Game> {
        use crate::model::Platform;
        let mut native = game("1", "Native Installed", None, 0);
        native.platform = Some(Platform::Linux);
        native.installed = true;
        let mut windows = game("2", "Windows Favourite", None, 0);
        windows.platform = Some(Platform::Windows);
        windows.favourite = true;
        let mut both = game("3", "Windows Installed Favourite", None, 0);
        both.platform = Some(Platform::Windows);
        both.installed = true;
        both.favourite = true;
        vec![native, windows, both]
    }

    #[test]
    fn filters_of_different_kinds_narrow_together() {
        let games = platform_games();
        let filters = Filters {
            installed: true,
            favourites: true,
            ..Filters::default()
        };

        assert_eq!(shown_with(filters, &games), ["Windows Installed Favourite"]);
    }

    #[test]
    fn filters_of_the_same_kind_widen_each_other() {
        let games = platform_games();
        let both_states = Filters {
            installed: true,
            not_installed: true,
            ..Filters::default()
        };
        let both_platforms = Filters {
            windows: true,
            native: true,
            ..Filters::default()
        };
        let windows = Filters {
            windows: true,
            ..Filters::default()
        };
        let not_installed = Filters {
            not_installed: true,
            ..Filters::default()
        };

        assert_eq!(shown_with(both_states, &games).len(), 3);
        assert_eq!(shown_with(both_platforms, &games).len(), 3);
        assert_eq!(
            shown_with(windows, &games),
            ["Windows Favourite", "Windows Installed Favourite"]
        );
        assert_eq!(shown_with(not_installed, &games), ["Windows Favourite"]);
    }

    #[test]
    fn no_filters_means_no_filtering() {
        assert!(!Filters::default().any());
        assert!(
            Filters {
                native: true,
                ..Filters::default()
            }
            .any()
        );
        assert_eq!(shown_with(Filters::default(), &platform_games()).len(), 3);
    }

    #[test]
    fn search_ignores_accents_and_a_leading_the() {
        let mut filter = LibraryFilter::default();
        filter.set_search("witcher wild");

        assert!(shows(
            &filter,
            &game("1", "The Witcher 3: Wild Hunt", None, 0)
        ));
        filter.set_search("pokemon");
        assert!(shows(&filter, &game("2", "Pokémon Snap", None, 0)));
    }

    #[test]
    fn playtime_sorts_the_most_played_first() {
        let keys = [
            SortKey::new(&game("1", "Little", None, 0), 60),
            SortKey::new(&game("2", "Lots", None, 0), 36_000),
            SortKey::new(&game("3", "None", None, 0), 0),
        ];
        let mut order: Vec<_> = keys.iter().collect();
        order.sort_by(|a, b| compare(SortMode::Playtime, a, b));

        let ids: Vec<_> = order.iter().map(|key| key.id.as_str()).collect();
        assert_eq!(ids, ["steam:2", "steam:1", "steam:3"]);
    }

    #[test]
    fn name_sorts_ignore_a_leading_the() {
        let games = vec![
            game("1", "The Witcher", None, 0),
            game("2", "Valheim", None, 0),
            game("3", "Abzu", None, 0),
        ];

        assert_eq!(
            sorted(SortMode::NameAscending, games),
            ["Abzu", "Valheim", "The Witcher"]
        );
    }

    #[test]
    fn loading_totals_playtime_from_the_source_or_from_sessions() {
        use crate::model::ImportedPlaytime;
        use crate::store::Store;

        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let imported = game("1", "Imported", None, 0);
        let tracked = game("2", "Tracked", None, 0);
        let unplayed = game("3", "Unplayed", None, 0);
        for game in [&imported, &tracked, &unplayed] {
            store.insert_game(game).unwrap();
        }
        store
            .set_imported_playtime(&ImportedPlaytime {
                game_id: imported.id.clone(),
                source: Source::Steam,
                total_secs: 7_200,
                last_played_at: None,
            })
            .unwrap();
        store.add_play_session(&tracked.id, 0, 600).unwrap();
        store.add_play_session(&tracked.id, 1_000, 1_900).unwrap();

        let entries = load(&store).unwrap();

        let playtime = |id: &GameId| {
            entries
                .iter()
                .find(|entry| entry.game.id == *id)
                .unwrap()
                .playtime_secs
        };
        assert_eq!(playtime(&imported.id), 7_200);
        assert_eq!(playtime(&tracked.id), 1_500);
        assert_eq!(playtime(&unplayed.id), 0);
    }
}
