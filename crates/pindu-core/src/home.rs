//! Fullscreen Home's cards: their size for each tile shape a design can choose, the art
//! each shows, and the details row of the focused game or section.

use std::path::PathBuf;

use crate::library::{ArtSet, HistoryGroup, history_group};
use crate::model::{Game, Source, Timestamp};
use crate::sections::Section;
use crate::themes::TileShape;

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

/// The art a Home card shows of its game. Cards never show icons, so every card in a row
/// looks alike; a game without art shows the card's title alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardArt {
    /// The picture to show, fitted to the card as covers are.
    pub art: Option<PathBuf>,
    /// Crop the art to fill the card, whatever its shape.
    pub crop: bool,
}

/// What a card of `shape` shows of a game with `art`: square cards their square art, or
/// else the cover (or banner) cropped to square.
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
                .or_else(|| art.hero.clone()),
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
    LastPlayed { group: HistoryGroup, at: Timestamp },
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

/// What Home's animated background, behind its first screen, does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Playback {
    /// Plays on, muted and looped.
    Play,
    /// Holds the frame it is on.
    Pause,
    /// Holds its first frame.
    FirstFrame,
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
    /// The system asks for reduced motion.
    pub reduced_motion: bool,
}

/// What Home's background does in `context`: it holds its first frame outside fullscreen
/// mode and under reduced motion, pauses behind a section's games, a running game or a
/// hidden window, and plays behind the first screen.
pub fn background_playback(context: BackgroundContext) -> Playback {
    if !context.fullscreen || context.reduced_motion {
        Playback::FirstFrame
    } else if !context.first_screen || context.game_running || context.hidden {
        Playback::Pause
    } else {
        Playback::Play
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GameId, LaunchKind};
    use crate::sections::SectionId;

    const ON_FIRST_SCREEN: BackgroundContext = BackgroundContext {
        fullscreen: true,
        first_screen: true,
        game_running: false,
        hidden: false,
        reduced_motion: false,
    };

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
    fn square_cards_show_square_art_or_else_the_cover_cropped_and_never_the_icon() {
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
                art: None,
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
}
