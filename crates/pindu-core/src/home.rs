//! Fullscreen Home: its cards' size for each tile shape a design can choose and the art
//! each shows, the details row of the focused game or section, the first screen's row as
//! a design starts it, the rows of the Shelves layout and the moves between them, where
//! the categories are (a Categories tile and screen, or rows of their own), the preview
//! of the focused section, and what Home's animated background does.

use std::path::PathBuf;

use crate::gamepad::Direction;
use crate::library::{ArtSet, HistoryGroup, history_group};
use crate::model::{Game, GameId, Source, Timestamp};
use crate::sections::{Section, SectionId};
use crate::themes::{HomeStart, ThemeLayout, TileShape};

/// How far a design may scale a focused card (`transform: scale()`) before its art blurs:
/// cards keep their size in code, as CSS can grow a widget but not shrink it, and their
/// art is decoded this much larger.
pub const ART_HEADROOM: f64 = 1.6;

/// The size of Home's cards, the Library tile's too, in the shape a design chose, with
/// its shorter side `size` pixels when the design sets it (`ThemeLayout::home_tile_size`).
pub fn card_size(shape: TileShape, size: Option<i32>) -> (i32, i32) {
    let (width, height) = match shape {
        TileShape::Landscape => (320, 150),
        TileShape::Square => (200, 200),
        TileShape::Portrait => (200, 300),
    };
    let Some(size) = size else {
        return (width, height);
    };
    let shorter = f64::from(width.min(height));
    let scaled = |side: i32| (f64::from(side) * f64::from(size) / shorter).round() as i32;
    (scaled(width), scaled(height))
}

/// The device pixels to decode a card's art at, for a card of `card` logical pixels on a
/// display with `scale_factor`, with room for a design to scale it up.
pub fn art_pixels((width, height): (i32, i32), scale_factor: i32) -> (u32, u32) {
    let pixels = |side: i32| (f64::from(side * scale_factor.max(1)) * ART_HEADROOM).ceil() as u32;
    (pixels(width), pixels(height))
}

/// The art a Home card shows of its game: art of the card's shape where there is some, so
/// every card in a row looks alike; a game without art shows the card's title alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardArt {
    /// The picture to show, fitted to the card as covers are.
    pub art: Option<PathBuf>,
    /// Crop the art to fill the card, whatever its shape.
    pub crop: bool,
}

/// What a card of `shape` shows of a game with `art`: square cards their square art, or
/// else the cover (or banner) cropped to square, and the icon only when there is nothing
/// else, as for a desktop launcher entry.
pub fn card_art(shape: TileShape, art: &ArtSet) -> CardArt {
    match shape {
        TileShape::Landscape => CardArt {
            art: art.hero.clone().or_else(|| art.cover.clone()),
            crop: false,
        },
        TileShape::Square => CardArt {
            art: art
                .square
                .clone()
                .or_else(|| art.cover.clone())
                .or_else(|| art.hero.clone())
                .or_else(|| art.icon.clone()),
            crop: true,
        },
        TileShape::Portrait => CardArt {
            art: art.cover.clone().or_else(|| art.hero.clone()),
            crop: false,
        },
    }
}

/// One item of the details row a design can show under the focused game's logo or title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Detail {
    /// Its playtime in seconds, at least a minute.
    Playtime(i64),
    /// When it was last played: its Play history group (today, this week, earlier), and
    /// the time, for a date on an earlier day.
    LastPlayed {
        /// The Play history group.
        group: HistoryGroup,
        /// When it was last played.
        at: Timestamp,
    },
    /// Where it comes from.
    Source(Source),
    /// How many games a section of Home's first screen holds.
    Games(u32),
    /// How many categories the Categories tile leads to.
    Categories(u32),
}

/// The details of `game` with `playtime_secs` played, at `now` and `utc_offset` seconds
/// east of UTC: playtime, last played and source, leaving out what it has no data for.
pub fn details(game: &Game, playtime_secs: i64, now: Timestamp, utc_offset: i64) -> Vec<Detail> {
    let playtime = (playtime_secs >= 60).then_some(Detail::Playtime(playtime_secs));
    let last_played = game.last_played_at.map(|at| Detail::LastPlayed {
        group: history_group(at, now, utc_offset),
        at,
    });
    playtime
        .into_iter()
        .chain(last_played)
        .chain([Detail::Source(game.source())])
        .collect()
}

/// The details of a section of Home's first screen: how many games it holds.
pub fn section_details(section: &Section) -> Vec<Detail> {
    let count = u32::try_from(section.games.len()).unwrap_or(u32::MAX);
    vec![Detail::Games(count)]
}

/// The details of the Categories tile, which leads to `categories`: how many there are.
pub fn categories_details(categories: &[Section]) -> Vec<Detail> {
    let count = u32::try_from(categories.len()).unwrap_or(u32::MAX);
    vec![Detail::Categories(count)]
}

/// The row of the Categories screen, in the layouts with two screens: a tile for each of
/// `categories`, in order (by name).
pub fn categories_row(categories: &[Section]) -> Vec<SectionId> {
    categories.iter().map(|category| category.id).collect()
}

/// An item of the row on Home's first screen, before the Library tile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RowItem {
    /// A game's card.
    Game(GameId),
    /// The line between the game cards and the section tiles.
    Divider,
    /// A section's tile.
    Section(SectionId),
}

/// The row of Home's first screen as a design starts it. With the sections: a tile for
/// each of `sections`, in order. With the games: a card for each Recently Played game, a
/// divider, then a tile for every other section; Recently Played itself gets no tile.
/// The Categories tile follows Installed, or where Installed would be, while the
/// library has `categories`.
pub fn first_screen_row(
    start: HomeStart,
    sections: &[Section],
    categories: &[Section],
) -> Vec<RowItem> {
    let tiles = |sections: &[Section]| -> Vec<RowItem> {
        let mut tiles: Vec<RowItem> = sections
            .iter()
            .filter(|section| {
                start == HomeStart::Sections || section.id != SectionId::RecentlyPlayed
            })
            .map(|section| RowItem::Section(section.id))
            .collect();
        if !categories.is_empty() {
            // Before the launchers and collections, whose number varies.
            let at = tiles
                .iter()
                .position(|tile| {
                    matches!(
                        tile,
                        RowItem::Section(SectionId::Source(_) | SectionId::Collection(_))
                    )
                })
                .unwrap_or(tiles.len());
            tiles.insert(at, RowItem::Section(SectionId::Categories));
        }
        tiles
    };
    if start == HomeStart::Sections {
        return tiles(sections);
    }
    let mut row: Vec<RowItem> = sections
        .iter()
        .filter(|section| section.id == SectionId::RecentlyPlayed)
        .flat_map(|section| section.games.iter().cloned().map(RowItem::Game))
        .collect();
    let tiles = tiles(sections);
    if !row.is_empty() && !tiles.is_empty() {
        row.push(RowItem::Divider);
    }
    row.extend(tiles);
    row
}

/// A row of the Shelves layout: a section with the games it shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    /// The section the row stands for.
    pub id: SectionId,
    /// The collection's name; `None` for the other sections, which the shell names.
    pub title: Option<String>,
    /// The games the row shows, in order.
    pub games: Vec<GameId>,
    /// How many games the section holds in all.
    pub total: usize,
}

/// How many game cards a row of the Shelves layout holds at most; a section with more
/// ends in See All.
pub const SHELF_LIMIT: usize = 20;

/// How many categories the Shelves and Explore layouts give a row of their own: those
/// with the most games. The others share the More Categories row.
pub const HOME_CATEGORIES: usize = 8;

/// The rows of the Shelves layout: one for each of `sections`, in order, then one for
/// each of the `HOME_CATEGORIES` `categories` with the most games, most games first, with
/// its first `SHELF_LIMIT` games.
pub fn shelves(sections: &[Section], categories: &[Section]) -> Vec<Shelf> {
    sections
        .iter()
        .chain(categories_on_home(categories))
        .map(|section| Shelf {
            id: section.id,
            title: section.title.clone(),
            games: section.games.iter().take(SHELF_LIMIT).cloned().collect(),
            total: section.games.len(),
        })
        .collect()
}

/// The `HOME_CATEGORIES` of `categories` with the most games, most games first, those
/// with as many in the order they are listed (by name).
fn categories_on_home(categories: &[Section]) -> Vec<&Section> {
    let mut by_size: Vec<&Section> = categories.iter().collect();
    by_size.sort_by_key(|category| std::cmp::Reverse(category.games.len()));
    by_size.truncate(HOME_CATEGORIES);
    by_size
}

/// The More Categories row's tiles, the last of the categories' rows in the Shelves and
/// Explore layouts: the categories that get no row of their own, by name; none while
/// every category has a row.
pub fn more_categories(categories: &[Section]) -> Vec<SectionId> {
    let on_home: Vec<SectionId> = categories_on_home(categories)
        .iter()
        .map(|category| category.id)
        .collect();
    categories
        .iter()
        .map(|category| category.id)
        .filter(|id| !on_home.contains(id))
        .collect()
}

/// A row of the Explore layout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExploreRow {
    /// The carousel of the Recently Played games, in order.
    Featured(Vec<GameId>),
    /// A tile for each of these sections, in order; the shell adds the Library tile.
    Tiles(Vec<SectionId>),
    /// A section as a row of its games.
    Shelf(Shelf),
    /// The More Categories row: a tile for each of these categories, which opens the
    /// library at it (`more_categories`).
    MoreCategories(Vec<SectionId>),
}

/// The rows of the Explore layout: the Recently Played games, when there are any; a row
/// of tiles for every other section, there even without one, and one Categories tile
/// after them while there are `categories`; then a shelf for each of those sections and
/// the categories with the most games (`shelves`), in order, and the More Categories row
/// for the other categories, if any (`more_categories`).
pub fn explore_rows(sections: &[Section], categories: &[Section]) -> Vec<ExploreRow> {
    let (recent, others): (Vec<Section>, Vec<Section>) = sections
        .iter()
        .cloned()
        .partition(|section| section.id == SectionId::RecentlyPlayed);
    let mut rows: Vec<ExploreRow> = recent
        .into_iter()
        .filter(|section| !section.games.is_empty())
        .map(|section| ExploreRow::Featured(section.games))
        .collect();
    let tile = (!categories.is_empty()).then_some(SectionId::Categories);
    rows.push(ExploreRow::Tiles(
        others
            .iter()
            .map(|section| section.id)
            .chain(tile)
            .collect(),
    ));
    rows.extend(
        shelves(&others, categories)
            .into_iter()
            .map(ExploreRow::Shelf),
    );
    let more = more_categories(categories);
    if !more.is_empty() {
        rows.push(ExploreRow::MoreCategories(more));
    }
    rows
}

/// The shape and size of the cards in the Shelves row of `section` under `layout`: wide
/// cards 130 pixels tall for Recently Played, the design's tiles for every other row.
pub fn shelf_shape(section: SectionId, layout: ThemeLayout) -> (TileShape, (i32, i32)) {
    match section {
        SectionId::RecentlyPlayed => (
            TileShape::Landscape,
            card_size(TileShape::Landscape, Some(130)),
        ),
        _ => (
            layout.home_tile,
            card_size(layout.home_tile, layout.home_tile_size),
        ),
    }
}

/// Where a move lands in the Shelves layout, or along Home's row in the other layouts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShelfStep {
    /// On the item at this place in the same row.
    Item(usize),
    /// In this row, on the item that last had focus there.
    Row(usize),
    /// Nowhere: focus stays where it is.
    Stay,
    /// Out of the rows, upwards, to the top bar.
    Leave,
}

/// Where `direction` leads from the item at `(row, item)` in the Shelves layout, whose
/// rows hold `rows` items each: left and right move along the row and stop at its ends,
/// down and up change row, down stops at the last row and up leaves from the first.
pub fn shelf_step((row, item): (usize, usize), direction: Direction, rows: &[usize]) -> ShelfStep {
    let length = rows.get(row).copied().unwrap_or(0);
    match direction {
        Direction::Left if item > 0 => ShelfStep::Item(item - 1),
        Direction::Right if item + 1 < length => ShelfStep::Item(item + 1),
        Direction::Left | Direction::Right => ShelfStep::Stay,
        Direction::Down if row + 1 < rows.len() => ShelfStep::Row(row + 1),
        Direction::Down => ShelfStep::Stay,
        Direction::Up if row > 0 => ShelfStep::Row(row - 1),
        Direction::Up => ShelfStep::Leave,
    }
}

/// Where `direction` leads from the item at `item` of Home's row of `length` items, in
/// the layouts with two screens, on either screen: as a row of the Shelves layout that
/// is the only one, left and right move along it and stop at its ends, down stays and
/// up leaves for the top bar.
pub fn row_step(item: usize, direction: Direction, length: usize) -> ShelfStep {
    shelf_step((0, item), direction, &[length])
}

/// A button on a page of the Explore layout's featured row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeaturedButton {
    /// Play, or Stop while the game runs.
    Play,
    /// Details, which opens the game page.
    Details,
}

/// Where Left or Right leads from `button` on page `page` of the featured row's `pages`:
/// Right from Play to Details, then to the next page's Play; Left from Details to Play,
/// then to the previous page's Play. `None` where focus stays: at either end, and for Up
/// and Down, which change row (`shelf_step`).
pub fn featured_step(
    page: usize,
    button: FeaturedButton,
    direction: Direction,
    pages: usize,
) -> Option<(usize, FeaturedButton)> {
    match (direction, button) {
        (Direction::Right, FeaturedButton::Play) => Some((page, FeaturedButton::Details)),
        (Direction::Right, FeaturedButton::Details) if page + 1 < pages => {
            Some((page + 1, FeaturedButton::Play))
        }
        (Direction::Left, FeaturedButton::Details) => Some((page, FeaturedButton::Play)),
        (Direction::Left, FeaturedButton::Play) if page > 0 => {
            Some((page - 1, FeaturedButton::Play))
        }
        _ => None,
    }
}

/// How many covers the preview of the focused section shows at most.
pub const PREVIEW_SIZE: usize = 6;

/// The preview of the focused section a design can show on the first screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Preview {
    /// The games whose covers show, in order.
    pub games: Vec<GameId>,
    /// How many games the section holds beyond them.
    pub more: usize,
}

/// The preview of `section`: its first `PREVIEW_SIZE` games and the number of the rest.
pub fn preview(section: &Section) -> Preview {
    let games: Vec<GameId> = section.games.iter().take(PREVIEW_SIZE).cloned().collect();
    Preview {
        more: section.games.len() - games.len(),
        games,
    }
}

/// What Home's animated background, behind its first screen, does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    /// Plays on, muted and looped.
    Play,
    /// Holds the frame it is on.
    Pause,
    /// Holds its first frame.
    FirstFrame,
    /// Shows no video at all: Home's plain background colour stands in for it, with no
    /// stars or waves behind the text.
    Hidden,
}

/// Where the window is, as far as Home's background is concerned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BackgroundContext {
    /// The window is in fullscreen mode.
    pub fullscreen: bool,
    /// Home is the shown page, on its first screen (the sections).
    pub first_screen: bool,
    /// A game we started runs.
    pub game_running: bool,
    /// The window is hidden or minimized.
    pub hidden: bool,
    /// Pindu moves less (`accessibility::Accessibility::reduces_motion`): the desktop's
    /// animations are off, the system asks for reduced motion, or Reduce Motion is on.
    pub reduced_motion: bool,
    /// The system asks for high contrast (`adw::StyleManager:high-contrast`).
    pub high_contrast: bool,
    /// Art covers the background on the screen shown (the focused game's, or a section's
    /// first game's), or the layout is one screen of rows
    /// (`themes::HomeLayout::one_screen`: Shelves and Explore), which never shows it.
    pub covered: bool,
}

/// What Home's background does in `context`: it shows nothing under high contrast, holds
/// its first frame outside fullscreen mode and under reduced motion, pauses behind a
/// section's games, art that covers it, a running game or a hidden window, and plays
/// behind the first screen.
pub fn background_playback(context: BackgroundContext) -> Playback {
    if context.high_contrast {
        Playback::Hidden
    } else if !context.fullscreen || context.reduced_motion {
        Playback::FirstFrame
    } else if !context.first_screen || context.covered || context.game_running || context.hidden {
        Playback::Pause
    } else {
        Playback::Play
    }
}

/// Home's fullscreen background, which a design that offers the choice
/// (`ThemeLayout::home_background_choice`) lets the user pick in Quick Access.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Background {
    /// Red ribbons on a dark background.
    #[default]
    RedRibbons,
    /// The original blue Home animation.
    BlueStation,
}

/// The videos in `home/`, smallest first: device pixel size, Red Ribbons, Blue Station.
const BACKGROUND_VIDEOS: [((i32, i32), &str, &str); 3] = [
    (
        (1280, 720),
        "red-ribbons-720p60.webm",
        "background-720p.webm",
    ),
    (
        (1920, 1080),
        "red-ribbons-1080p60.webm",
        "background-1080p.webm",
    ),
    (
        (2560, 1440),
        "red-ribbons-1440p60.webm",
        "background-1440p.webm",
    ),
];

impl Background {
    /// The background named by `value`; an unknown value uses Red Ribbons.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "blue-station" => Self::BlueStation,
            _ => Self::default(),
        }
    }

    /// The smallest video for this background that fills `screen` in device pixels,
    /// or the largest if none fills it. An unknown screen uses 1080p.
    pub fn video(self, screen: Option<(i32, i32)>) -> &'static str {
        let &(_, red, blue) = screen.map_or(&BACKGROUND_VIDEOS[1], |(width, height)| {
            BACKGROUND_VIDEOS
                .iter()
                .find(|((w, h), _, _)| *w >= width && *h >= height)
                .unwrap_or(&BACKGROUND_VIDEOS[BACKGROUND_VIDEOS.len() - 1])
        });
        match self {
            Self::RedRibbons => red,
            Self::BlueStation => blue,
        }
    }
}

/// The background Home shows when the `fullscreen-background` setting holds `setting`:
/// the chosen one where the design in force offers the choice (`offered`), else the
/// default, whatever the setting holds.
pub fn background_in_force(offered: bool, setting: &str) -> Background {
    if offered {
        Background::from_setting(setting)
    } else {
        Background::default()
    }
}

/// The GSettings key choosing Home's background.
pub const BACKGROUND_KEY: &str = "fullscreen-background";

/// The GSettings key of Categories on Home: while it is off, Home shows no Categories
/// tile, no category's row and no More Categories row, as for a library without a genre.
pub const CATEGORIES_KEY: &str = "fullscreen-home-categories";

/// The GSettings key of Home's background blur, a `Strength`'s setting.
pub const BLUR_KEY: &str = "fullscreen-background-blur";
/// The GSettings key of Home's background dim, a `Strength`'s setting.
pub const DIM_KEY: &str = "fullscreen-background-dim";

/// How much Home's background (its animated background and the focused game's art) is
/// blurred or dimmed, as Quick Access sets it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Strength {
    /// Not at all.
    #[default]
    Off,
    /// A little.
    Low,
    /// A lot.
    High,
}

impl Strength {
    /// Every strength, weakest first.
    pub const ALL: [Self; 3] = [Self::Off, Self::Low, Self::High];

    /// The value the setting holds for it.
    pub fn setting(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Low => "low",
            Self::High => "high",
        }
    }

    /// The strength the setting `value` holds; `Off` for an unknown one.
    pub fn from_setting(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|strength| strength.setting() == value)
            .unwrap_or_default()
    }
}

/// Every class `background_classes` gives Home.
pub const BACKGROUND_CLASSES: [&str; 4] = ["blur-low", "blur-high", "dim-low", "dim-high"];

/// The style classes Home carries for its background's `blur` and `dim`, which
/// `style.css` and designs give their look.
pub fn background_classes(blur: Strength, dim: Strength) -> Vec<&'static str> {
    let blur = match blur {
        Strength::Off => None,
        Strength::Low => Some("blur-low"),
        Strength::High => Some("blur-high"),
    };
    let dim = match dim {
        Strength::Off => None,
        Strength::Low => Some("dim-low"),
        Strength::High => Some("dim-high"),
    };
    blur.into_iter().chain(dim).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LaunchKind;
    use crate::sections::CategoryId;

    const ON_FIRST_SCREEN: BackgroundContext = BackgroundContext {
        fullscreen: true,
        first_screen: true,
        game_running: false,
        hidden: false,
        reduced_motion: false,
        high_contrast: false,
        covered: false,
    };

    #[test]
    fn the_background_choice_selects_the_right_video_for_the_display() {
        assert_eq!(
            Background::default().video(None),
            "red-ribbons-1080p60.webm"
        );
        for (setting, screen, expected) in [
            ("red-ribbons", (1280, 720), "red-ribbons-720p60.webm"),
            ("red-ribbons", (1920, 1080), "red-ribbons-1080p60.webm"),
            ("red-ribbons", (3840, 2160), "red-ribbons-1440p60.webm"),
            ("blue-station", (1280, 720), "background-720p.webm"),
            ("unknown", (1280, 800), "red-ribbons-1080p60.webm"),
        ] {
            assert_eq!(
                Background::from_setting(setting).video(Some(screen)),
                expected,
                "{setting} on {screen:?}"
            );
        }
    }

    #[test]
    fn the_background_is_the_smallest_video_that_fills_the_screen() {
        for (screen, file) in [
            ((1280, 720), "background-720p.webm"),
            ((1280, 800), "background-1080p.webm"),
            ((1920, 1080), "background-1080p.webm"),
            ((1920, 1200), "background-1440p.webm"),
            ((2560, 1440), "background-1440p.webm"),
        ] {
            assert_eq!(
                Background::BlueStation.video(Some(screen)),
                file,
                "{screen:?}"
            );
        }
    }

    #[test]
    fn a_screen_bigger_than_every_video_gets_the_largest() {
        for screen in [(3840, 2160), (1080, 1920)] {
            assert_eq!(
                Background::BlueStation.video(Some(screen)),
                "background-1440p.webm",
                "{screen:?}"
            );
        }
    }

    #[test]
    fn an_unknown_screen_gets_the_1080p_background() {
        assert_eq!(Background::BlueStation.video(None), "background-1080p.webm");
    }

    #[test]
    fn every_background_video_ships_and_is_installed() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let meson = std::fs::read_to_string(data.join("meson.build")).unwrap();

        for (_, red, blue) in BACKGROUND_VIDEOS {
            for file in [red, blue] {
                assert!(data.join("home").join(file).is_file(), "{file} is missing");
                assert!(
                    meson.contains(&format!("'home' / '{file}'")),
                    "{file} is not installed"
                );
            }
        }
    }

    #[test]
    fn each_background_choice_in_the_schema_picks_its_own_background() {
        let schema = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/io.github.tim_van_oudheusden.pindu-launcher.gschema.xml.in"),
        )
        .unwrap();
        let key = &schema[schema
            .find(&format!("<key name=\"{BACKGROUND_KEY}\""))
            .expect("the schema has the background key")..];
        let choices = &key[..key.find("</choices>").unwrap()];

        let picked: Vec<_> = choices
            .split("<choice value=\"")
            .skip(1)
            .map(|choice| Background::from_setting(&choice[..choice.find('"').unwrap()]))
            .collect();

        assert_eq!(picked, [Background::RedRibbons, Background::BlueStation]);
    }

    #[test]
    fn the_background_plays_behind_the_first_screen() {
        assert_eq!(background_playback(ON_FIRST_SCREEN), Playback::Play);
    }

    #[test]
    fn the_background_pauses_behind_a_section_a_game_or_a_hidden_window() {
        for context in [
            BackgroundContext {
                first_screen: false,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                game_running: true,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                hidden: true,
                ..ON_FIRST_SCREEN
            },
        ] {
            assert_eq!(background_playback(context), Playback::Pause, "{context:?}");
        }
    }

    #[test]
    fn the_background_holds_its_first_frame_outside_fullscreen_and_under_reduced_motion() {
        for context in [
            BackgroundContext {
                fullscreen: false,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                reduced_motion: true,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                fullscreen: false,
                first_screen: false,
                ..ON_FIRST_SCREEN
            },
        ] {
            assert_eq!(
                background_playback(context),
                Playback::FirstFrame,
                "{context:?}"
            );
        }
    }

    #[test]
    fn under_high_contrast_the_background_shows_nothing_whatever_else_holds() {
        for context in [
            ON_FIRST_SCREEN,
            BackgroundContext {
                reduced_motion: true,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                fullscreen: false,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                first_screen: false,
                ..ON_FIRST_SCREEN
            },
            BackgroundContext {
                game_running: true,
                hidden: true,
                ..ON_FIRST_SCREEN
            },
        ] {
            let context = BackgroundContext {
                high_contrast: true,
                ..context
            };
            assert_eq!(
                background_playback(context),
                Playback::Hidden,
                "{context:?}"
            );
        }
    }

    #[test]
    fn background_strengths_come_from_their_settings() {
        assert_eq!(Strength::from_setting("off"), Strength::Off);
        assert_eq!(Strength::from_setting("low"), Strength::Low);
        assert_eq!(Strength::from_setting("high"), Strength::High);
        assert_eq!(Strength::from_setting("max"), Strength::Off);
    }

    #[test]
    fn blur_and_dim_become_home_s_style_classes() {
        use Strength::*;
        assert!(background_classes(Off, Off).is_empty());
        assert_eq!(background_classes(Low, Off), ["blur-low"]);
        assert_eq!(background_classes(High, Off), ["blur-high"]);
        assert_eq!(background_classes(Off, Low), ["dim-low"]);
        assert_eq!(background_classes(Low, High), ["blur-low", "dim-high"]);
        for class in background_classes(High, High) {
            assert!(BACKGROUND_CLASSES.contains(&class), "{class}");
        }
    }

    #[test]
    fn each_shape_has_its_card_size() {
        assert_eq!(card_size(TileShape::Landscape, None), (320, 150));
        assert_eq!(card_size(TileShape::Square, None), (200, 200));
        assert_eq!(card_size(TileShape::Portrait, None), (200, 300));
    }

    #[test]
    fn a_design_sets_the_shorter_side_and_the_shape_keeps_its_aspect() {
        assert_eq!(card_size(TileShape::Square, Some(112)), (112, 112));
        assert_eq!(card_size(TileShape::Landscape, Some(150)), (320, 150));
        assert_eq!(card_size(TileShape::Landscape, Some(112)), (239, 112));
        assert_eq!(card_size(TileShape::Portrait, Some(112)), (112, 168));
    }

    #[test]
    fn art_is_decoded_for_a_card_a_design_scales_up() {
        assert_eq!(art_pixels((320, 150), 1), (512, 240));
        assert_eq!(art_pixels((200, 300), 2), (640, 960));
        assert_eq!(art_pixels((201, 201), 1), (322, 322));
    }

    #[test]
    fn landscape_cards_show_the_hero_then_the_cover() {
        let art = ArtSet {
            cover: Some("cover.png".into()),
            hero: Some("hero.png".into()),
            logo: Some("logo.png".into()),
            icon: Some("icon.png".into()),
            square: Some("square.png".into()),
        };
        let without_hero = ArtSet {
            hero: None,
            ..art.clone()
        };

        let shown = card_art(TileShape::Landscape, &art);
        let fallback = card_art(TileShape::Landscape, &without_hero);

        assert_eq!(
            shown,
            CardArt {
                art: Some("hero.png".into()),
                crop: false,
            }
        );
        assert_eq!(fallback.art, Some("cover.png".into()));
    }

    #[test]
    fn square_cards_show_square_art_or_else_the_cover_cropped_and_the_icon_only_as_a_last_resort() {
        let art = ArtSet {
            cover: Some("cover.png".into()),
            hero: Some("hero.png".into()),
            logo: None,
            icon: Some("icon.png".into()),
            square: Some("square.png".into()),
        };
        let without_square = ArtSet {
            square: None,
            ..art.clone()
        };
        let only_icon = ArtSet {
            icon: Some("icon.png".into()),
            ..ArtSet::default()
        };

        assert_eq!(
            card_art(TileShape::Square, &art),
            CardArt {
                art: Some("square.png".into()),
                crop: true,
            }
        );
        assert_eq!(
            card_art(TileShape::Square, &without_square),
            CardArt {
                art: Some("cover.png".into()),
                crop: true,
            }
        );
        assert_eq!(
            card_art(TileShape::Square, &only_icon),
            CardArt {
                art: Some("icon.png".into()),
                crop: true,
            }
        );
    }

    #[test]
    fn portrait_cards_show_the_cover_then_the_hero() {
        let art = ArtSet {
            cover: Some("cover.png".into()),
            hero: Some("hero.png".into()),
            logo: None,
            icon: None,
            square: Some("square.png".into()),
        };
        let without_cover = ArtSet {
            cover: None,
            ..art.clone()
        };

        let shown = card_art(TileShape::Portrait, &art);
        let fallback = card_art(TileShape::Portrait, &without_cover);

        assert_eq!(shown.art, Some("cover.png".into()));
        assert!(!shown.crop);
        assert_eq!(fallback.art, Some("hero.png".into()));
    }

    const DAY: i64 = 86_400;
    const NOW: i64 = 1_800_000_000;

    fn played(last_played: Option<i64>) -> Game {
        let mut game = Game::new(
            GameId::new(Source::Steam, "1"),
            "Celeste",
            LaunchKind::StoreUri,
            0,
        );
        game.last_played_at = last_played;
        game
    }

    #[test]
    fn details_show_playtime_last_played_and_source() {
        let game = played(Some(NOW - 3 * DAY));

        let shown = details(&game, 12 * 3600, NOW, 0);

        assert_eq!(
            shown,
            [
                Detail::Playtime(12 * 3600),
                Detail::LastPlayed {
                    group: HistoryGroup::ThisWeek,
                    at: NOW - 3 * DAY,
                },
                Detail::Source(Source::Steam),
            ]
        );
    }

    #[test]
    fn details_leave_out_what_is_missing() {
        // Under a minute reads as no playtime, as on the game's page.
        let game = played(None);

        let shown = details(&game, 59, NOW, 0);

        assert_eq!(shown, [Detail::Source(Source::Steam)]);
    }

    #[test]
    fn last_played_follows_the_local_day() {
        // Played at 23:30 local time (UTC+2) and now 00:30 local the next day, though
        // both fall on one day in UTC.
        let at = NOW - NOW.rem_euclid(DAY) - 30 * 60 - 2 * 3600 + DAY;
        let now = at + 3600;
        let game = played(Some(at));

        let shown = details(&game, 0, now, 2 * 3600);

        assert_eq!(
            shown[0],
            Detail::LastPlayed {
                group: HistoryGroup::ThisWeek,
                at,
            }
        );
    }

    #[test]
    fn a_section_details_how_many_games_it_holds() {
        let section = Section {
            id: SectionId::Installed,
            title: None,
            games: ["a", "b", "c"]
                .map(|id| GameId::new(Source::Steam, id))
                .to_vec(),
        };

        assert_eq!(section_details(&section), [Detail::Games(3)]);
    }

    /// A section of `count` games, named `<section>-<n>`.
    fn section(id: SectionId, count: usize) -> Section {
        Section {
            id,
            title: None,
            games: (0..count)
                .map(|n| GameId::new(Source::Steam, &format!("{}-{n}", id.name())))
                .collect(),
        }
    }

    #[test]
    fn the_first_screen_s_row_is_a_tile_for_each_section() {
        let sections = [
            section(SectionId::RecentlyPlayed, 2),
            section(SectionId::Favourites, 1),
            section(SectionId::Source(Source::Steam), 3),
        ];

        let row = first_screen_row(HomeStart::Sections, &sections, &[]);

        assert_eq!(
            row,
            [
                RowItem::Section(SectionId::RecentlyPlayed),
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Source(Source::Steam)),
            ]
        );
    }

    #[test]
    fn a_row_that_starts_with_games_shows_the_recent_ones_then_the_other_sections() {
        let recent = section(SectionId::RecentlyPlayed, 2);
        let sections = [
            recent.clone(),
            section(SectionId::Favourites, 1),
            section(SectionId::Source(Source::Steam), 3),
        ];

        let row = first_screen_row(HomeStart::Games, &sections, &[]);

        assert_eq!(
            row,
            [
                RowItem::Game(recent.games[0].clone()),
                RowItem::Game(recent.games[1].clone()),
                RowItem::Divider,
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Source(Source::Steam)),
            ]
        );
    }

    #[test]
    fn a_row_that_starts_with_games_is_the_tiles_alone_without_recent_games() {
        let sections = [
            section(SectionId::Favourites, 1),
            section(SectionId::Installed, 4),
        ];

        let row = first_screen_row(HomeStart::Games, &sections, &[]);

        assert_eq!(
            row,
            [
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Installed),
            ]
        );
    }

    #[test]
    fn a_row_of_recent_games_alone_has_no_divider() {
        let recent = section(SectionId::RecentlyPlayed, 1);

        let row = first_screen_row(HomeStart::Games, std::slice::from_ref(&recent), &[]);

        assert_eq!(row, [RowItem::Game(recent.games[0].clone())]);
    }

    /// The category `genre`, holding `count` games.
    fn category(genre: &str, count: usize) -> Section {
        Section {
            title: Some(genre.to_owned()),
            ..section(SectionId::Category(CategoryId::of(genre)), count)
        }
    }

    #[test]
    fn the_categories_tile_follows_installed_whatever_the_row_starts_with() {
        let recent = section(SectionId::RecentlyPlayed, 1);
        let sections = [
            recent.clone(),
            section(SectionId::Favourites, 1),
            section(SectionId::Installed, 2),
            section(SectionId::Source(Source::Steam), 3),
            section(SectionId::Collection(1), 1),
        ];
        let categories = [category("Arcade", 1), category("Brawler", 2)];

        assert_eq!(
            first_screen_row(HomeStart::Sections, &sections, &categories),
            [
                RowItem::Section(SectionId::RecentlyPlayed),
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Installed),
                RowItem::Section(SectionId::Categories),
                RowItem::Section(SectionId::Source(Source::Steam)),
                RowItem::Section(SectionId::Collection(1)),
            ]
        );
        assert_eq!(
            first_screen_row(HomeStart::Games, &sections, &categories),
            [
                RowItem::Game(recent.games[0].clone()),
                RowItem::Divider,
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Installed),
                RowItem::Section(SectionId::Categories),
                RowItem::Section(SectionId::Source(Source::Steam)),
                RowItem::Section(SectionId::Collection(1)),
            ]
        );
    }

    #[test]
    fn the_categories_tile_stands_where_installed_would_and_not_without_categories() {
        let sections = [
            section(SectionId::Favourites, 1),
            section(SectionId::Source(Source::Lutris), 2),
        ];
        let categories = [category("Cozy", 1)];

        for start in HomeStart::ALL {
            assert_eq!(
                first_screen_row(start, &sections, &categories),
                [
                    RowItem::Section(SectionId::Favourites),
                    RowItem::Section(SectionId::Categories),
                    RowItem::Section(SectionId::Source(Source::Lutris)),
                ],
                "{start:?}"
            );
            // A library without a genre, or the Categories on Home switch off.
            assert_eq!(
                first_screen_row(start, &sections, &[]),
                [
                    RowItem::Section(SectionId::Favourites),
                    RowItem::Section(SectionId::Source(Source::Lutris)),
                ],
                "{start:?}"
            );
        }
        // At the end of a row of sections that has none after it.
        assert_eq!(
            first_screen_row(HomeStart::Sections, &sections[..1], &categories),
            [
                RowItem::Section(SectionId::Favourites),
                RowItem::Section(SectionId::Categories),
            ]
        );
    }

    #[test]
    fn there_is_a_shelf_for_each_section_in_order() {
        let favourites = section(SectionId::Favourites, 3);
        let collection = Section {
            title: Some("Co-op".to_owned()),
            ..section(SectionId::Collection(4), 1)
        };

        let shelves = shelves(&[favourites.clone(), collection.clone()], &[]);

        assert_eq!(
            shelves,
            [
                Shelf {
                    id: SectionId::Favourites,
                    title: None,
                    games: favourites.games,
                    total: 3,
                },
                Shelf {
                    id: SectionId::Collection(4),
                    title: Some("Co-op".to_owned()),
                    games: collection.games,
                    total: 1,
                },
            ]
        );
    }

    /// Ten categories, listed by name, holding 1 to 25 games.
    fn ten_categories() -> Vec<Section> {
        [
            ("Alchemy", 3),
            ("Bullet Hell", 25),
            ("Cards", 1),
            ("Dungeon", 5),
            ("Exploration", 5),
            ("Fishing", 2),
            ("Gardening", 7),
            ("Heist", 1),
            ("Idle", 9),
            ("Jigsaw", 4),
        ]
        .into_iter()
        .map(|(genre, count)| category(genre, count))
        .collect()
    }

    #[test]
    fn the_eight_categories_with_the_most_games_are_shelves_after_the_sections() {
        let collection = Section {
            title: Some("Co-op".to_owned()),
            ..section(SectionId::Collection(4), 1)
        };
        let categories = ten_categories();

        let shelves = shelves(std::slice::from_ref(&collection), &categories);

        assert_eq!(HOME_CATEGORIES, 8);
        let rows: Vec<(Option<&str>, usize)> = shelves
            .iter()
            .map(|shelf| (shelf.title.as_deref(), shelf.total))
            .collect();
        // Most games first; Dungeon and Exploration tie, by name; Cards and Heist,
        // with fewest, are left for the More Categories row.
        assert_eq!(
            rows,
            [
                (Some("Co-op"), 1),
                (Some("Bullet Hell"), 25),
                (Some("Idle"), 9),
                (Some("Gardening"), 7),
                (Some("Dungeon"), 5),
                (Some("Exploration"), 5),
                (Some("Jigsaw"), 4),
                (Some("Alchemy"), 3),
                (Some("Fishing"), 2),
            ]
        );
        // A category's shelf is any section's: its first games, all counted.
        assert_eq!(shelves[1].id, categories[1].id);
        assert_eq!(shelves[1].games, categories[1].games[..SHELF_LIMIT]);
    }

    #[test]
    fn the_categories_without_a_shelf_are_more_categories_by_name() {
        let categories = ten_categories();

        assert_eq!(
            more_categories(&categories),
            [categories[2].id, categories[7].id]
        );
        // A tile each for those after the eighth, by name, however many games they hold.
        let mut eleven = categories.clone();
        eleven.insert(0, category("Aardvark", 1));
        assert_eq!(
            more_categories(&eleven),
            [eleven[0].id, eleven[3].id, eleven[8].id]
        );
        // Eight or fewer all have a shelf: no More Categories row.
        assert_eq!(more_categories(&categories[..8]), []);
        assert_eq!(more_categories(&[]), []);
    }

    #[test]
    fn the_categories_tile_details_how_many_categories_and_each_category_its_games() {
        let categories = ten_categories();

        assert_eq!(categories_details(&categories), [Detail::Categories(10)]);
        assert_eq!(
            categories_details(&categories[..1]),
            [Detail::Categories(1)]
        );
        assert_eq!(section_details(&categories[1]), [Detail::Games(25)]);
    }

    #[test]
    fn categories_on_home_is_a_setting_that_starts_on() {
        let schema = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../data/io.github.tim_van_oudheusden.pindu-launcher.gschema.xml.in"),
        )
        .unwrap();
        let key = format!("<key name=\"{CATEGORIES_KEY}\" type=\"b\">");

        let after = schema.split_once(&key).map(|(_, after)| after);
        let default = after.and_then(|after| after.split_once("</default>"));

        assert!(
            default.is_some_and(|(start, _)| start.trim_end().ends_with("<default>true")),
            "{CATEGORIES_KEY} is missing or not on by default"
        );
    }

    #[test]
    fn the_categories_screen_is_a_tile_for_each_category_by_name() {
        let categories = ten_categories();

        let row = categories_row(&categories);

        // Every one of them, not only the eight with shelves, as they are listed.
        let listed: Vec<SectionId> = categories.iter().map(|category| category.id).collect();
        assert_eq!(row, listed);
        assert_eq!(categories_row(&[]), []);
    }

    #[test]
    fn a_shelf_shows_its_first_games_and_counts_them_all() {
        let full = section(SectionId::Installed, SHELF_LIMIT);
        let large = section(SectionId::Source(Source::Steam), SHELF_LIMIT + 5);

        let shelves = shelves(&[full.clone(), large.clone()], &[]);

        assert_eq!(SHELF_LIMIT, 20);
        assert_eq!(shelves[0].games, full.games);
        assert_eq!(shelves[0].total, 20);
        assert_eq!(shelves[1].games, large.games[..20]);
        assert_eq!(shelves[1].total, 25);
    }

    #[test]
    fn the_recently_played_shelf_is_landscape_and_the_others_take_the_design_s_shape() {
        let layout = ThemeLayout {
            home_tile: TileShape::Portrait,
            home_tile_size: Some(132),
            ..ThemeLayout::default()
        };

        assert_eq!(
            shelf_shape(SectionId::RecentlyPlayed, layout),
            (TileShape::Landscape, (277, 130))
        );
        for other in [
            SectionId::Favourites,
            SectionId::Source(Source::Lutris),
            SectionId::Collection(1),
        ] {
            assert_eq!(
                shelf_shape(other, layout),
                (TileShape::Portrait, (132, 198)),
                "{other:?}"
            );
        }
        assert_eq!(
            shelf_shape(SectionId::Installed, ThemeLayout::default()),
            (TileShape::Landscape, (320, 150))
        );
    }

    #[test]
    fn a_preview_holds_a_section_s_first_six_games_and_counts_the_rest() {
        assert_eq!(PREVIEW_SIZE, 6);
        for (count, shown, more) in [(0, 0, 0), (4, 4, 0), (6, 6, 0), (12, 6, 6)] {
            let section = section(SectionId::Favourites, count);

            let preview = preview(&section);

            assert_eq!(preview.games, section.games[..shown], "{count} games");
            assert_eq!(preview.more, more, "{count} games");
        }
    }

    #[test]
    fn the_background_is_the_chosen_one_only_where_the_design_offers_the_choice() {
        for setting in ["red-ribbons", "blue-station", "unknown", ""] {
            assert_eq!(
                background_in_force(false, setting),
                Background::RedRibbons,
                "{setting}"
            );
        }
        assert_eq!(
            background_in_force(true, "blue-station"),
            Background::BlueStation
        );
        assert_eq!(
            background_in_force(true, "red-ribbons"),
            Background::RedRibbons
        );
    }

    #[test]
    fn the_background_pauses_while_art_covers_it() {
        let covered = BackgroundContext {
            covered: true,
            ..ON_FIRST_SCREEN
        };

        assert_eq!(background_playback(covered), Playback::Pause);
        // The rules before it still hold.
        for (context, expected) in [
            (
                BackgroundContext {
                    reduced_motion: true,
                    ..covered
                },
                Playback::FirstFrame,
            ),
            (
                BackgroundContext {
                    fullscreen: false,
                    ..covered
                },
                Playback::FirstFrame,
            ),
            (
                BackgroundContext {
                    high_contrast: true,
                    ..covered
                },
                Playback::Hidden,
            ),
        ] {
            assert_eq!(background_playback(context), expected, "{context:?}");
        }
    }

    #[test]
    fn left_and_right_move_along_a_shelf_and_stay_at_its_ends() {
        // Three rows of 3, 1 and 2 items.
        let rows = [3, 1, 2];

        assert_eq!(
            shelf_step((0, 0), Direction::Right, &rows),
            ShelfStep::Item(1)
        );
        assert_eq!(
            shelf_step((0, 2), Direction::Left, &rows),
            ShelfStep::Item(1)
        );
        assert_eq!(shelf_step((0, 0), Direction::Left, &rows), ShelfStep::Stay);
        assert_eq!(shelf_step((0, 2), Direction::Right, &rows), ShelfStep::Stay);
        assert_eq!(shelf_step((1, 0), Direction::Right, &rows), ShelfStep::Stay);
    }

    #[test]
    fn up_and_down_change_shelf_and_up_from_the_first_leaves_for_the_top_bar() {
        let rows = [3, 1, 2];

        assert_eq!(
            shelf_step((0, 2), Direction::Down, &rows),
            ShelfStep::Row(1)
        );
        assert_eq!(
            shelf_step((1, 0), Direction::Down, &rows),
            ShelfStep::Row(2)
        );
        assert_eq!(shelf_step((2, 1), Direction::Down, &rows), ShelfStep::Stay);
        assert_eq!(shelf_step((2, 1), Direction::Up, &rows), ShelfStep::Row(1));
        assert_eq!(shelf_step((1, 0), Direction::Up, &rows), ShelfStep::Row(0));
        assert_eq!(shelf_step((0, 1), Direction::Up, &rows), ShelfStep::Leave);
    }

    #[test]
    fn home_row_stays_at_its_ends_and_up_leaves_for_the_top_bar() {
        assert_eq!(row_step(0, Direction::Right, 3), ShelfStep::Item(1));
        assert_eq!(row_step(2, Direction::Left, 3), ShelfStep::Item(1));
        assert_eq!(row_step(2, Direction::Right, 3), ShelfStep::Stay);
        assert_eq!(row_step(0, Direction::Left, 3), ShelfStep::Stay);
        assert_eq!(row_step(0, Direction::Right, 1), ShelfStep::Stay);
        assert_eq!(row_step(1, Direction::Up, 3), ShelfStep::Leave);
        assert_eq!(row_step(1, Direction::Down, 3), ShelfStep::Stay);
    }

    #[test]
    fn explore_starts_with_the_recent_games_then_the_tiles_then_a_shelf_each() {
        let recent = section(SectionId::RecentlyPlayed, 2);
        let favourites = section(SectionId::Favourites, 3);
        let installed = section(SectionId::Installed, 4);

        let rows = explore_rows(
            &[recent.clone(), favourites.clone(), installed.clone()],
            &[],
        );

        assert_eq!(
            rows,
            [
                ExploreRow::Featured(recent.games),
                ExploreRow::Tiles(vec![SectionId::Favourites, SectionId::Installed]),
                ExploreRow::Shelf(shelves(std::slice::from_ref(&favourites), &[]).remove(0)),
                ExploreRow::Shelf(shelves(std::slice::from_ref(&installed), &[]).remove(0)),
            ]
        );
    }

    #[test]
    fn explore_has_no_featured_row_without_recent_games() {
        let favourites = section(SectionId::Favourites, 1);

        for sections in [
            vec![favourites.clone()],
            vec![section(SectionId::RecentlyPlayed, 0), favourites.clone()],
        ] {
            assert_eq!(
                explore_rows(&sections, &[]),
                [
                    ExploreRow::Tiles(vec![SectionId::Favourites]),
                    ExploreRow::Shelf(shelves(std::slice::from_ref(&favourites), &[]).remove(0)),
                ]
            );
        }
    }

    #[test]
    fn an_empty_library_explores_the_tiles_row_alone() {
        assert_eq!(explore_rows(&[], &[]), [ExploreRow::Tiles(Vec::new())]);
    }

    #[test]
    fn explore_gives_the_categories_one_tile_then_their_shelves_after_the_sections() {
        let favourites = section(SectionId::Favourites, 2);
        let collection = Section {
            title: Some("Co-op".to_owned()),
            ..section(SectionId::Collection(4), 1)
        };
        let sections = [favourites.clone(), collection.clone()];
        let categories = ten_categories();

        let rows = explore_rows(&sections, &categories);

        let shelves = shelves(&sections, &categories);
        let expected: Vec<ExploreRow> = [ExploreRow::Tiles(vec![
            SectionId::Favourites,
            SectionId::Collection(4),
            SectionId::Categories,
        ])]
        .into_iter()
        .chain(shelves.into_iter().map(ExploreRow::Shelf))
        .chain([ExploreRow::MoreCategories(more_categories(&categories))])
        .collect();
        assert_eq!(rows, expected);
        // The category shelves follow the collection's.
        assert!(matches!(&rows[2], ExploreRow::Shelf(shelf) if shelf.id == collection.id));
        assert!(matches!(&rows[3], ExploreRow::Shelf(shelf) if shelf.id == categories[1].id));
        // Eight categories or fewer: a shelf each, and no More Categories row.
        let few = explore_rows(&sections, &categories[..3]);
        assert_eq!(few.len(), 1 + 2 + 3);
        assert!(
            !few.iter()
                .any(|row| matches!(row, ExploreRow::MoreCategories(_)))
        );
    }

    #[test]
    fn the_featured_row_steps_through_play_and_details_and_turns_the_page() {
        use FeaturedButton::{Details, Play};
        let step = |page, button, direction| featured_step(page, button, direction, 3);

        assert_eq!(step(0, Play, Direction::Right), Some((0, Details)));
        assert_eq!(step(0, Details, Direction::Right), Some((1, Play)));
        assert_eq!(step(1, Details, Direction::Left), Some((1, Play)));
        assert_eq!(step(1, Play, Direction::Left), Some((0, Play)));
        // Focus stays at either end.
        assert_eq!(step(0, Play, Direction::Left), None);
        assert_eq!(step(2, Details, Direction::Right), None);
        assert_eq!(step(2, Play, Direction::Right), Some((2, Details)));
    }
}
