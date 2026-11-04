//! Ordering and matching of library games, shared by both shells' list models.

use std::cmp::{Ordering, Reverse};

use std::collections::HashMap;

use crate::model::{Game, Source, Timestamp};

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
}

/// What sorting needs from a game, computed once so comparisons stay cheap in big
/// libraries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SortKey {
    name: String,
    last_played_at: Option<Timestamp>,
    added_at: Timestamp,
    id: String,
}

impl SortKey {
    pub fn new(game: &Game) -> Self {
        Self {
            name: game.sort_title.to_lowercase(),
            last_played_at: game.last_played_at,
            added_at: game.added_at,
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
    }
}

/// A game title prepared for searching, computed once per title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchKey(String);

impl SearchKey {
    pub fn new(title: &str) -> Self {
        Self(title.to_lowercase())
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

/// Which games the library views show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    view: View,
    search: String,
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

    /// Shows only games whose title contains `text`, ignoring case; empty shows all.
    pub fn set_search(&mut self, text: &str) -> FilterChange {
        let text = text.to_lowercase();
        let change = if text == self.search {
            FilterChange::Unchanged
        } else if text.contains(&self.search) {
            FilterChange::MoreStrict
        } else if self.search.contains(&text) {
            FilterChange::LessStrict
        } else {
            FilterChange::Different
        };
        self.search = text;
        change
    }

    /// Whether a game shows: in the library, not hidden, in the view and matching the
    /// search.
    pub fn matches(&self, game: &Game, key: &SearchKey) -> bool {
        game.in_library()
            && !game.hidden
            && self.view.includes(game)
            && key.0.contains(&self.search)
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
            .map(|game| (SortKey::new(&game), game.title))
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
        let a = SortKey::new(&game("1", "Same", None, 0));
        let b = SortKey::new(&game("2", "Same", None, 0));

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
}
