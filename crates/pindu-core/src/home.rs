//! Fullscreen Home's cards: their size for each tile shape a design can choose, and the
//! art each shows.

use std::path::PathBuf;

use crate::art::ICON_SCALE;
use crate::library::ArtSet;
use crate::themes::TileShape;

/// How far a design may scale a focused card (`transform: scale()`) before its art blurs:
/// cards keep their size in code, as CSS can grow a widget but not shrink it, and their
/// art is decoded this much larger.
pub const ART_HEADROOM: f64 = 1.6;

/// The size of Home's cards, the Library tile's too, in the shape a design chose.
pub fn card_size(shape: TileShape) -> (i32, i32) {
    match shape {
        TileShape::Landscape => (320, 150),
        TileShape::Square => (200, 200),
        TileShape::Portrait => (200, 300),
    }
}

/// The device pixels to decode a card's art at, for a card of `card` logical pixels on a
/// display with `scale_factor`, with room for a design to scale it up.
pub fn art_pixels((width, height): (i32, i32), scale_factor: i32) -> (u32, u32) {
    let pixels = |side: i32| (f64::from(side * scale_factor.max(1)) * ART_HEADROOM).ceil() as u32;
    (pixels(width), pixels(height))
}

/// The art a Home card shows of its game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardArt {
    /// The picture to show, fitted to the card as covers are.
    pub art: Option<PathBuf>,
    /// The icon to show instead: when there is no `art`, or first if `icon_first`.
    pub icon: Option<PathBuf>,
    /// Show the icon when it is large enough (`icon_large_enough`), the art otherwise.
    pub icon_first: bool,
    /// Crop the art to fill the card, whatever its shape.
    pub crop: bool,
}

/// What a card of `shape` shows of a game with `art`.
pub fn card_art(shape: TileShape, art: &ArtSet) -> CardArt {
    match shape {
        TileShape::Landscape => CardArt {
            art: art.hero.clone().or_else(|| art.cover.clone()),
            icon: art.icon.clone(),
            icon_first: false,
            crop: false,
        },
        TileShape::Square => CardArt {
            art: art.cover.clone().or_else(|| art.hero.clone()),
            icon: art.icon.clone(),
            icon_first: true,
            crop: true,
        },
        TileShape::Portrait => CardArt {
            art: art.cover.clone().or_else(|| art.hero.clone()),
            icon: art.icon.clone(),
            icon_first: false,
            crop: false,
        },
    }
}

/// Whether an icon of `icon` pixels is large enough to stand for its game on a card
/// `card_width` device pixels wide, where it draws at `art::ICON_SCALE` of the width: it is
/// never enlarged.
pub fn icon_large_enough((width, height): (u32, u32), card_width: u32) -> bool {
    width.min(height) as f32 >= card_width as f32 * ICON_SCALE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shape_has_its_card_size() {
        assert_eq!(card_size(TileShape::Landscape), (320, 150));
        assert_eq!(card_size(TileShape::Square), (200, 200));
        assert_eq!(card_size(TileShape::Portrait), (200, 300));
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
                icon: Some("icon.png".into()),
                icon_first: false,
                crop: false,
            }
        );
        assert_eq!(fallback.art, Some("cover.png".into()));
    }

    #[test]
    fn square_cards_show_a_large_icon_or_else_the_cover_cropped() {
        let art = ArtSet {
            cover: Some("cover.png".into()),
            hero: Some("hero.png".into()),
            logo: None,
            icon: Some("icon.png".into()),
        };

        let shown = card_art(TileShape::Square, &art);

        assert_eq!(
            shown,
            CardArt {
                art: Some("cover.png".into()),
                icon: Some("icon.png".into()),
                icon_first: true,
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
        };
        let without_cover = ArtSet {
            cover: None,
            ..art.clone()
        };

        let shown = card_art(TileShape::Portrait, &art);
        let fallback = card_art(TileShape::Portrait, &without_cover);

        assert_eq!(shown.art, Some("cover.png".into()));
        assert!(!shown.icon_first && !shown.crop);
        assert_eq!(fallback.art, Some("hero.png".into()));
    }

    #[test]
    fn an_icon_is_large_enough_when_it_is_not_enlarged() {
        // Icons draw half as wide as the card.
        assert!(icon_large_enough((256, 256), 320));
        assert!(icon_large_enough((160, 200), 320));
        assert!(!icon_large_enough((128, 128), 320));
        assert!(!icon_large_enough((32, 32), 200));
    }
}
