//! Fullscreen Home: its cards' size for each tile shape a design can choose and the art
//! each shows, the details row of the focused game or section, the first screen's row as
//! a design starts it, the rows of the Shelves layout and the moves between them, the
//! preview of the focused section, and what Home's animated background does.

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
pub fn first_screen_row(start: HomeStart, sections: &[Section]) -> Vec<RowItem> {
    let tiles = |sections: &[Section]| -> Vec<RowItem> {
        sections
            .iter()
            .filter(|section| {
                start == HomeStart::Sections || section.id != SectionId::RecentlyPlayed
            })
            .map(|section| RowItem::Section(section.id))
            .collect()
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

/// The rows of the Shelves layout: one for each of `sections`, in order, with its first
/// `SHELF_LIMIT` games.
pub fn shelves(sections: &[Section]) -> Vec<Shelf> {
    sections
        .iter()
        .map(|section| Shelf {
            id: section.id,
            title: section.title.clone(),
            games: section.games.iter().take(SHELF_LIMIT).cloned().collect(),
            total: section.games.len(),
        })
        .collect()
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
    /// first game's), or the layout is Shelves, which never shows it.
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

        let row = first_screen_row(HomeStart::Sections, &sections);

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

        let row = first_screen_row(HomeStart::Games, &sections);

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

        let row = first_screen_row(HomeStart::Games, &sections);

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

        let row = first_screen_row(HomeStart::Games, std::slice::from_ref(&recent));

        assert_eq!(row, [RowItem::Game(recent.games[0].clone())]);
    }

    #[test]
    fn there_is_a_shelf_for_each_section_in_order() {
        let favourites = section(SectionId::Favourites, 3);
        let collection = Section {
            title: Some("Co-op".to_owned()),
            ..section(SectionId::Collection(4), 1)
        };

        let shelves = shelves(&[favourites.clone(), collection.clone()]);

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

    #[test]
    fn a_shelf_shows_its_first_games_and_counts_them_all() {
        let full = section(SectionId::Installed, SHELF_LIMIT);
        let large = section(SectionId::Source(Source::Steam), SHELF_LIMIT + 5);

        let shelves = shelves(&[full.clone(), large.clone()]);

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
}
