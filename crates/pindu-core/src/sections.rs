//! Fullscreen Home's sections, the first screen's tiles: Recently played, Favourites,
//! Installed, one per launcher, then the user's collections, each with its games. The
//! Library tile that ends the row is not a section. Home's categories, the genres the
//! library's games have (`categories`), are sections too, behind one Categories tile or
//! as rows of their own, as the design's layout shows them (`crate::home`).

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::library::{self, SortKey, SortMode, View};
use crate::model::{Collection, Game, GameId, Source};

/// A category of Home, a genre named ignoring case: the same name always gives the same
/// ID while Pindu runs, so a category keeps its ID, and what had focus on it, when the
/// library reloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CategoryId(usize);

/// The genres given an ID so far: each one's ID by its name in lower case, and its name
/// as the library last spelled it, by ID.
#[derive(Default)]
struct Interned {
    ids: HashMap<String, usize>,
    names: Vec<String>,
}

static CATEGORIES: Mutex<Option<Interned>> = Mutex::new(None);

/// Runs `f` on the genres given an ID so far.
fn interned<T>(f: impl FnOnce(&mut Interned) -> T) -> T {
    let mut guard = CATEGORIES.lock().unwrap_or_else(PoisonError::into_inner);
    f(guard.get_or_insert_with(Interned::default))
}

impl CategoryId {
    /// The ID of the category `genre`, spelled as the library spells it now.
    pub fn of(genre: &str) -> Self {
        interned(|interned| {
            let key = genre.to_lowercase();
            let id = match interned.ids.get(&key) {
                Some(&id) => {
                    genre.clone_into(&mut interned.names[id]);
                    id
                }
                None => {
                    interned.names.push(genre.to_owned());
                    interned.ids.insert(key, interned.names.len() - 1);
                    interned.names.len() - 1
                }
            };
            Self(id)
        })
    }

    /// The ID of the category `genre` if it has one, without giving it one.
    fn find(genre: &str) -> Option<Self> {
        interned(|interned| interned.ids.get(&genre.to_lowercase()).copied().map(Self))
    }

    /// The category's genre, as the library last spelled it.
    pub fn genre(self) -> String {
        interned(|interned| interned.names[self.0].clone())
    }
}

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
    /// Home's categories as one tile, which leads to them: the Categories screen of the
    /// layouts with two screens, the categories' rows of the Explore layout.
    Categories,
    /// A category: the games of one genre.
    Category(CategoryId),
}

impl SectionId {
    /// The section's name: its art's file name (`data/home/<name>.svg`) for all but a
    /// collection, which is `collection-<id>`, and a category, `category-<genre>`.
    pub fn name(self) -> String {
        match self {
            Self::RecentlyPlayed => RECENTLY_PLAYED.to_owned(),
            Self::Favourites => FAVOURITES.to_owned(),
            Self::Installed => INSTALLED.to_owned(),
            Self::Source(source) => source.as_str().to_owned(),
            Self::Collection(id) => format!("{COLLECTION_PREFIX}{id}"),
            Self::Categories => CATEGORIES_NAME.to_owned(),
            Self::Category(id) => format!("{CATEGORY_PREFIX}{}", id.genre()),
        }
    }

    /// The section named `name` (see [`Self::name`]).
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            RECENTLY_PLAYED => Some(Self::RecentlyPlayed),
            FAVOURITES => Some(Self::Favourites),
            INSTALLED => Some(Self::Installed),
            CATEGORIES_NAME => Some(Self::Categories),
            _ => {
                if let Some(id) = name.strip_prefix(COLLECTION_PREFIX) {
                    id.parse().ok().map(Self::Collection)
                } else if let Some(genre) = name.strip_prefix(CATEGORY_PREFIX) {
                    CategoryId::find(genre).map(Self::Category)
                } else {
                    name.parse().ok().map(Self::Source)
                }
            }
        }
    }

    /// The class the section's tile carries for designs: `section-` and [`Self::name`],
    /// or `section-collection` for every collection and `section-category` for every
    /// category.
    pub fn css_class(self) -> String {
        match self {
            Self::Collection(_) => "section-collection".to_owned(),
            Self::Category(_) => "section-category".to_owned(),
            _ => format!("section-{}", self.name()),
        }
    }

    /// The library view that shows all of the section's games, where its See All goes:
    /// All Games for a category, which the library narrows to its genre
    /// ([`Self::genre`]), and for the Categories tile.
    pub fn view(self) -> View {
        match self {
            Self::RecentlyPlayed => View::PlayHistory,
            Self::Favourites => View::Favourites,
            Self::Installed => View::Installed,
            Self::Source(source) => View::Source(source),
            Self::Collection(id) => View::Collection(id),
            Self::Categories | Self::Category(_) => View::All,
        }
    }

    /// The genre the library is filtered to (`library::Filters::genre`) when the section
    /// opens there: a category's; `None` for every other section.
    pub fn genre(self) -> Option<String> {
        match self {
            Self::Category(id) => Some(id.genre()),
            _ => None,
        }
    }
}

const RECENTLY_PLAYED: &str = "recently-played";
const FAVOURITES: &str = "favourites";
const INSTALLED: &str = "installed";
const COLLECTION_PREFIX: &str = "collection-";
const CATEGORIES_NAME: &str = "categories";
const CATEGORY_PREFIX: &str = "category-";

/// The two letters a launcher's section tile shows where a design labels its tiles;
/// `None` for a source that is not a launcher, which shows an icon. Not translated.
pub fn monogram(source: Source) -> Option<&'static str> {
    match source {
        Source::Steam => Some("St"),
        Source::HeroicEpic => Some("Ep"),
        Source::HeroicGog => Some("Go"),
        Source::HeroicAmazon => Some("Am"),
        Source::HeroicSideload => Some("Sl"),
        Source::Lutris => Some("Lu"),
        Source::Bottles => Some("Bo"),
        Source::Faugus => Some("Fa"),
        Source::Legendary => Some("Le"),
        Source::Itch => Some("It"),
        Source::Desktop | Source::Manual | Source::External => None,
    }
}

/// The symbolic icon that stands for `source`: a launcher's own mark where Pindu ships one
/// (`pindu-source-*`, `data/icons/sources/`), and GNOME's own icon for the sources that are
/// not a brand. `None` for the launchers whose mark Pindu does not ship (Epic, GOG and
/// Amazon): fullscreen mode's labelled tiles show their monogram, and the desktop window a
/// generic icon.
pub fn source_icon(source: Source) -> Option<&'static str> {
    match source {
        Source::Steam => Some("pindu-source-steam-symbolic"),
        Source::HeroicEpic | Source::HeroicGog | Source::HeroicAmazon => None,
        Source::HeroicSideload => Some("application-x-executable-symbolic"),
        Source::Lutris => Some("pindu-source-lutris-symbolic"),
        Source::Bottles => Some("pindu-source-bottles-symbolic"),
        Source::Faugus => Some("pindu-source-faugus-symbolic"),
        Source::Legendary => Some("pindu-source-legendary-symbolic"),
        Source::Itch => Some("pindu-source-itch-symbolic"),
        Source::Desktop => Some("computer-symbolic"),
        Source::Manual => Some("document-edit-symbolic"),
        Source::External => Some("application-x-addon-symbolic"),
    }
}

/// A section of Home's first screen: what it holds, the user's name for it when it is
/// a collection and the genre as the library spells it when it is a category (the shell
/// names the others), and its games in the order they show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// What the section holds.
    pub id: SectionId,
    /// The collection's name or the category's genre; `None` for the other sections.
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
        in_view(games, view)
            .into_iter()
            .map(|game| game.id.clone())
            .collect()
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

/// The games of `games` that `view` shows, most recently played first, then by name.
fn in_view<'a>(games: &[(&'a Game, &[i64])], view: View) -> Vec<&'a Game> {
    let mut shown: Vec<(SortKey, &Game)> = games
        .iter()
        .filter(|(game, held_by)| view.shows(game, held_by))
        .map(|(game, _)| (SortKey::new(game, 0), *game))
        .collect();
    shown.sort_by(|(a, _), (b, _)| library::compare(SortMode::LastPlayed, a, b));
    shown.into_iter().map(|(_, game)| game).collect()
}

/// Home's categories: every genre of the games the library shows (`library::genres`),
/// hidden and removed games left out, in its order, each with its games in the order of
/// every section, most recently played first, then by name. A game is in each of its
/// genres' categories. `games` are every game with the IDs of the collections holding it.
pub fn categories(games: &[(&Game, &[i64])]) -> Vec<Section> {
    let shown = in_view(games, View::All);
    // Named as the library's own list names them, in the library's order.
    let listed = games
        .iter()
        .filter(|(game, held_by)| View::All.shows(game, held_by))
        .map(|(game, _)| *game);
    library::genres(listed)
        .into_iter()
        .map(|genre| Section {
            id: SectionId::Category(CategoryId::of(&genre)),
            games: shown
                .iter()
                .filter(|game| {
                    game.genres
                        .iter()
                        .any(|own| library::same_genre(own, &genre))
                })
                .map(|game| game.id.clone())
                .collect(),
            title: Some(genre),
        })
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

    /// `game` with `genres`.
    fn with_genres(mut game: Game, genres: &[&str]) -> Game {
        game.genres = genres.iter().map(|&genre| genre.to_owned()).collect();
        game
    }

    #[test]
    fn categories_are_the_genres_by_name_each_with_its_games_and_a_game_in_each_of_its_own() {
        let old = with_genres(game(Source::Steam, "Old", Some(100)), &["Strategy"]);
        let quest = with_genres(
            game(Source::Lutris, "Quest", Some(500)),
            &["RPG", "strategy"],
        );
        let alpha = with_genres(game(Source::Steam, "Alpha", None), &["Strategy"]);
        let plain = game(Source::Steam, "Plain", Some(900));
        let games = [
            (&old, &[][..]),
            (&quest, &[][..]),
            (&alpha, &[][..]),
            (&plain, &[][..]),
        ];

        let categories = categories(&games);

        assert_eq!(
            categories,
            [
                Section {
                    id: SectionId::Category(CategoryId::of("RPG")),
                    title: Some("RPG".to_owned()),
                    games: vec![quest.id.clone()],
                },
                // Spelled as first seen, its games as every section orders them.
                Section {
                    id: SectionId::Category(CategoryId::of("Strategy")),
                    title: Some("Strategy".to_owned()),
                    games: vec![quest.id.clone(), old.id.clone(), alpha.id.clone()],
                },
            ]
        );
    }

    #[test]
    fn hidden_and_removed_games_count_for_no_category() {
        // Genres of their own: every test shares the names given an ID.
        let mut hidden = with_genres(game(Source::Steam, "Hidden", Some(5)), &["Horror", "Shmup"]);
        hidden.hidden = true;
        let mut missing = with_genres(game(Source::Lutris, "Missing", Some(6)), &["Karting"]);
        missing.missing_since = Some(1);
        let mut removed = with_genres(game(Source::Steam, "Removed", None), &["Shmup"]);
        removed.user_removed = true;
        let shown = with_genres(game(Source::Steam, "Shown", None), &["shmup"]);
        let games = [
            (&hidden, &[][..]),
            (&missing, &[][..]),
            (&removed, &[][..]),
            (&shown, &[][..]),
        ];

        let categories_shown = categories(&games);

        assert_eq!(
            categories_shown,
            [Section {
                id: SectionId::Category(CategoryId::of("shmup")),
                title: Some("shmup".to_owned()),
                games: vec![shown.id.clone()],
            }]
        );
        assert!(categories(&[(&hidden, &[][..])]).is_empty());
    }

    #[test]
    fn a_category_keeps_its_id_by_name_over_a_reload_whatever_its_case() {
        let before = with_genres(game(Source::Steam, "A", None), &["Metroidvania"]);
        let after = with_genres(game(Source::Lutris, "B", None), &["METROIDVANIA"]);

        let first = categories(&[(&before, &[][..])]).remove(0).id;
        let reloaded = categories(&[(&after, &[][..])]).remove(0).id;

        assert_eq!(first, reloaded);
        // Named as the library spells it now, and found by that name.
        assert_eq!(reloaded.name(), "category-METROIDVANIA");
        assert_eq!(SectionId::from_name("category-METROIDVANIA"), Some(first));
        assert_eq!(SectionId::from_name("category-metroidvania"), Some(first));
        assert_ne!(first, SectionId::Category(CategoryId::of("Roguelike")));
        assert_eq!(SectionId::from_name("category-never-a-genre-here"), None);
    }

    #[test]
    fn a_category_opens_the_library_at_all_games_with_its_genre() {
        let tactics = SectionId::Category(CategoryId::of("Tactics"));

        assert_eq!(tactics.view(), View::All);
        assert_eq!(tactics.genre().as_deref(), Some("Tactics"));
        for other in [
            SectionId::Installed,
            SectionId::Collection(3),
            SectionId::Categories,
        ] {
            assert_eq!(other.genre(), None, "{other:?}");
        }
    }

    #[test]
    fn the_categories_tile_and_each_category_are_named_and_classed_for_designs() {
        assert_eq!(SectionId::Categories.name(), "categories");
        assert_eq!(
            SectionId::from_name("categories"),
            Some(SectionId::Categories)
        );
        assert_eq!(SectionId::Categories.css_class(), "section-categories");
        for genre in ["Puzzle", "Sports"] {
            assert_eq!(
                SectionId::Category(CategoryId::of(genre)).css_class(),
                "section-category"
            );
        }
    }

    #[test]
    fn the_categories_tile_ships_its_art_as_a_resource_under_home_s_licence() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let resources = std::fs::read_to_string(data.join("resources.gresource.xml")).unwrap();
        let provenance = std::fs::read_to_string(data.join("home/LICENSE")).unwrap();
        let art = format!("home/{}.svg", SectionId::Categories.name());

        assert!(data.join(&art).is_file(), "{art} is missing");
        assert!(
            resources.contains(&format!(">{art}</file>")),
            "{art} is not a resource"
        );
        assert!(provenance.contains("The section tiles (*.svg)"));
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
    fn a_section_has_a_css_class_for_designs_to_tint_its_tile() {
        for (id, class) in [
            (SectionId::RecentlyPlayed, "section-recently-played"),
            (SectionId::Favourites, "section-favourites"),
            (SectionId::Installed, "section-installed"),
            (SectionId::Source(Source::Steam), "section-steam"),
            (SectionId::Source(Source::HeroicEpic), "section-heroic-epic"),
            (SectionId::Collection(7), "section-collection"),
            (SectionId::Collection(12), "section-collection"),
        ] {
            assert_eq!(id.css_class(), class);
        }
    }

    #[test]
    fn each_launcher_has_its_monogram_and_the_others_none() {
        let table = [
            (Source::Steam, Some("St")),
            (Source::HeroicEpic, Some("Ep")),
            (Source::HeroicGog, Some("Go")),
            (Source::HeroicAmazon, Some("Am")),
            (Source::HeroicSideload, Some("Sl")),
            (Source::Lutris, Some("Lu")),
            (Source::Bottles, Some("Bo")),
            (Source::Faugus, Some("Fa")),
            (Source::Legendary, Some("Le")),
            (Source::Itch, Some("It")),
            (Source::Desktop, None),
            (Source::Manual, None),
            (Source::External, None),
        ];

        assert_eq!(table.len(), Source::ALL.len());
        for (source, expected) in table {
            assert_eq!(monogram(source), expected, "{source:?}");
        }
    }

    #[test]
    fn a_source_shows_its_own_icon_where_pindu_ships_one() {
        let table = [
            (Source::Steam, Some("pindu-source-steam-symbolic")),
            (Source::HeroicEpic, None),
            (Source::HeroicGog, None),
            (Source::HeroicAmazon, None),
            (
                Source::HeroicSideload,
                Some("application-x-executable-symbolic"),
            ),
            (Source::Lutris, Some("pindu-source-lutris-symbolic")),
            (Source::Bottles, Some("pindu-source-bottles-symbolic")),
            (Source::Faugus, Some("pindu-source-faugus-symbolic")),
            (Source::Legendary, Some("pindu-source-legendary-symbolic")),
            (Source::Itch, Some("pindu-source-itch-symbolic")),
            (Source::Desktop, Some("computer-symbolic")),
            (Source::Manual, Some("document-edit-symbolic")),
            (Source::External, Some("application-x-addon-symbolic")),
        ];

        assert_eq!(table.len(), Source::ALL.len());
        for (source, expected) in table {
            assert_eq!(source_icon(source), expected, "{source:?}");
        }
    }

    #[test]
    fn every_launcher_icon_pindu_names_ships_in_its_icon_theme_with_its_provenance() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let icons = data.join("icons/sources");
        let provenance = std::fs::read_to_string(icons.join("LICENSE")).unwrap();
        let resources = std::fs::read_to_string(data.join("resources.gresource.xml")).unwrap();

        for &source in Source::ALL {
            let Some(name) = source_icon(source).filter(|name| name.starts_with("pindu-")) else {
                continue;
            };
            let file = format!("{name}.svg");
            assert!(icons.join(&file).is_file(), "{file} is missing");
            assert!(provenance.contains(&file), "{file} has no provenance");
            let alias = format!("alias=\"icons/scalable/actions/{file}\"");
            let path = format!(">icons/sources/{file}</file>");
            assert!(
                resources
                    .lines()
                    .any(|line| line.contains(&alias) && line.contains(&path)),
                "{file} is not in the app's icon theme"
            );
        }
    }

    #[test]
    fn a_section_opens_in_the_library_at_its_view() {
        for (id, view) in [
            (SectionId::RecentlyPlayed, View::PlayHistory),
            (SectionId::Favourites, View::Favourites),
            (SectionId::Installed, View::Installed),
            (
                SectionId::Source(Source::Lutris),
                View::Source(Source::Lutris),
            ),
            (SectionId::Collection(7), View::Collection(7)),
        ] {
            assert_eq!(id.view(), view);
        }
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
