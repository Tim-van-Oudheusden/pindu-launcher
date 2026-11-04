//! Ordering and matching of library games, shared by both shells' list models.

use std::cmp::{Ordering, Reverse};

use crate::model::{Game, Timestamp};

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

/// Which games the library views show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LibraryFilter {
    search: String,
}

impl LibraryFilter {
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

    /// Whether a game shows: in the library, not hidden, and matching the search.
    pub fn matches(&self, game: &Game, key: &SearchKey) -> bool {
        game.in_library() && !game.hidden && key.0.contains(&self.search)
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
}
