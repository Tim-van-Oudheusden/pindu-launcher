//! Fullscreen Home's sections, the first screen's tiles: Recently played, Favourites,
//! Installed, one per launcher, then the user's collections, each with its games. The
//! Library tile that ends the row is not a section.

use crate::library::{self, SortKey, SortMode, View};
use crate::model::{Collection, Game, GameId, Source};

/// Which games a section holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SectionId {
    /// The "Jump back in" games: pins, then the most recently played.
    RecentlyPlayed,
    /// The user's favourites.
    Favourites,
    /// Installed games.
    Installed,
    /// The games of one launcher.
    Source(Source),
    /// A user collection, by ID.
    Collection(i64),
}

impl SectionId {
    /// The section's name: its art's file name (`data/home/<name>.svg`) for all but a
    /// collection, which is `collection-<id>`.
    pub fn name(self) -> String {
        match self {
            Self::RecentlyPlayed => RECENTLY_PLAYED.to_owned(),
            Self::Favourites => FAVOURITES.to_owned(),
            Self::Installed => INSTALLED.to_owned(),
            Self::Source(source) => source.as_str().to_owned(),
            Self::Collection(id) => format!("{COLLECTION_PREFIX}{id}"),
        }
    }

    /// The section named `name` (see [`Self::name`]).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            RECENTLY_PLAYED => Some(Self::RecentlyPlayed),
            FAVOURITES => Some(Self::Favourites),
            INSTALLED => Some(Self::Installed),
            _ => match name.strip_prefix(COLLECTION_PREFIX) {
                Some(id) => id.parse().ok().map(Self::Collection),
                None => name.parse().ok().map(Self::Source),
            },
        }
    }
}

const RECENTLY_PLAYED: &str = "recently-played";
const FAVOURITES: &str = "favourites";
const INSTALLED: &str = "installed";
const COLLECTION_PREFIX: &str = "collection-";

/// A section of Home's first screen: what it holds, the user's name for it when it is
/// a collection (the shell names the others), and its games in the order they show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// What the section holds.
    pub id: SectionId,
    /// The collection's name; `None` for the other sections.
    pub title: Option<String>,
    /// The section's games, in the order they show.
    pub games: Vec<GameId>,
}

/// Home's sections, in order, leaving out the empty ones; `games` are every game with the
/// IDs of the collections holding it, `collections` the user's, in their order. Recently
/// played keeps the "Jump back in" order; the others hold their library view's games,
/// most recently played first, then by name.
pub fn sections(games: &[(&Game, &[i64])], collections: &[Collection]) -> Vec<Section> {
    let recent = library::jump_back_in(games.iter().map(|(game, _)| *game))
        .into_iter()
        .map(|game| game.id.clone())
        .collect();
    let in_view = |view: View| -> Vec<GameId> {
        let mut shown: Vec<(SortKey, &Game)> = games
            .iter()
            .filter(|(game, held_by)| view.shows(game, held_by))
            .map(|(game, _)| (SortKey::new(game, 0), *game))
            .collect();
        shown.sort_by(|(a, _), (b, _)| library::compare(SortMode::LastPlayed, a, b));
        shown.into_iter().map(|(_, game)| game.id.clone()).collect()
    };
    let section = |id, title, games| Section { id, title, games };
    [
        section(SectionId::RecentlyPlayed, None, recent),
        section(SectionId::Favourites, None, in_view(View::Favourites)),
        section(SectionId::Installed, None, in_view(View::Installed)),
    ]
    .into_iter()
    .chain(Source::ALL.iter().map(|&source| {
        section(
            SectionId::Source(source),
            None,
            in_view(View::Source(source)),
        )
    }))
    .chain(collections.iter().map(|collection| {
        section(
            SectionId::Collection(collection.id),
            Some(collection.name.clone()),
            in_view(View::Collection(collection.id)),
        )
    }))
    .filter(|section| !section.games.is_empty())
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LaunchKind;

    fn game(source: Source, id: &str, last_played_at: Option<i64>) -> Game {
        let mut game = Game::new(GameId::new(source, id), id, LaunchKind::StoreUri, 0);
        game.last_played_at = last_played_at;
        game
    }

    fn collection(id: i64, name: &str) -> Collection {
        Collection {
            id,
            name: name.to_owned(),
            position: id,
        }
    }

    fn ids(sections: &[Section]) -> Vec<SectionId> {
        sections.iter().map(|section| section.id).collect()
    }

    #[test]
    fn sections_come_in_home_order() {
        let mut played = game(Source::Lutris, "played", Some(10));
        played.installed = true;
        let mut favourite = game(Source::Steam, "favourite", None);
        favourite.favourite = true;
        let epic = game(Source::HeroicEpic, "epic", None);
        let games = [
            (&played, &[2][..]),
            (&favourite, &[1][..]),
            (&epic, &[][..]),
        ];

        let sections = sections(&games, &[collection(2, "RPGs"), collection(1, "Co-op")]);

        assert_eq!(
            ids(&sections),
            [
                SectionId::RecentlyPlayed,
                SectionId::Favourites,
                SectionId::Installed,
                SectionId::Source(Source::Steam),
                SectionId::Source(Source::HeroicEpic),
                SectionId::Source(Source::Lutris),
                SectionId::Collection(2),
                SectionId::Collection(1),
            ]
        );
        // Only a collection has a title of its own: the user's name for it.
        let titles: Vec<Option<&str>> = sections
            .iter()
            .map(|section| section.title.as_deref())
            .collect();
        assert!(titles[..6].iter().all(Option::is_none));
        assert_eq!(titles[6..], [Some("RPGs"), Some("Co-op")]);
    }

    #[test]
    fn empty_sections_and_hidden_or_missing_games_are_left_out() {
        let mut hidden = game(Source::Steam, "hidden", Some(5));
        hidden.hidden = true;
        hidden.favourite = true;
        let mut missing = game(Source::Lutris, "missing", Some(6));
        missing.missing_since = Some(1);
        missing.installed = true;
        let bottles = game(Source::Bottles, "bottles", None);
        let games = [
            (&hidden, &[1][..]),
            (&missing, &[1][..]),
            (&bottles, &[][..]),
        ];
        let empty = Collection {
            id: 1,
            name: "Empty".to_owned(),
            position: 0,
        };

        let sections = sections(&games, &[empty]);

        assert_eq!(
            sections,
            [Section {
                id: SectionId::Source(Source::Bottles),
                title: None,
                games: vec![bottles.id.clone()],
            }]
        );
    }

    #[test]
    fn a_section_shows_its_most_recently_played_games_first_then_the_rest_by_name() {
        let beta = game(Source::Steam, "Beta", None);
        let old = game(Source::Steam, "Old", Some(100));
        let alpha = game(Source::Steam, "Alpha", None);
        let recent = game(Source::Steam, "Recent", Some(500));
        let games = [
            (&beta, &[][..]),
            (&old, &[][..]),
            (&alpha, &[][..]),
            (&recent, &[][..]),
        ];

        let sections = sections(&games, &[]);
        let steam = sections
            .iter()
            .find(|section| section.id == SectionId::Source(Source::Steam))
            .unwrap();

        assert_eq!(
            steam.games,
            [&recent.id, &old.id, &alpha.id, &beta.id].map(Clone::clone)
        );
    }

    #[test]
    fn a_section_is_named_by_its_art_id_and_found_by_it() {
        let named = [
            (SectionId::RecentlyPlayed, "recently-played"),
            (SectionId::Favourites, "favourites"),
            (SectionId::Installed, "installed"),
            (SectionId::Source(Source::Steam), "steam"),
            (SectionId::Source(Source::HeroicEpic), "heroic-epic"),
            (SectionId::Collection(7), "collection-7"),
        ];

        for (id, name) in named {
            assert_eq!(id.name(), name);
            assert_eq!(SectionId::from_name(name), Some(id));
        }
        assert_eq!(SectionId::from_name("collection-x"), None);
        assert_eq!(SectionId::from_name("library"), None);
    }

    #[test]
    fn every_section_but_a_collection_ships_its_art_as_a_resource() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let resources = std::fs::read_to_string(data.join("resources.gresource.xml")).unwrap();
        let ids = [
            SectionId::RecentlyPlayed,
            SectionId::Favourites,
            SectionId::Installed,
        ]
        .into_iter()
        .chain(Source::ALL.iter().map(|&source| SectionId::Source(source)));

        for id in ids {
            let art = format!("home/{}.svg", id.name());
            assert!(data.join(&art).is_file(), "{art} is missing");
            assert!(
                resources.contains(&format!(">{art}</file>")),
                "{art} is not a resource"
            );
        }
    }
}
