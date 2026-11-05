//! Choosing and placing game artwork.

use crate::model::{ArtLayout, ArtOrigin, Artwork};

/// The cover aspect ratio, width over height.
pub const COVER_RATIO: f32 = 2.0 / 3.0;
/// How far from [`COVER_RATIO`] art may be and still fill a cover with a slight crop.
pub const FILL_TOLERANCE: f32 = 0.1;
/// Share of the box's width an icon takes when it stands in for art.
pub const ICON_SCALE: f32 = 0.5;

/// The image to show for a layout: the user's choice, then the source's own art, then
/// SteamGridDB, then a generated placeholder. `prefer_steamgriddb` puts SteamGridDB before
/// the source's art; the user's choice always comes first.
pub fn best(artwork: &[Artwork], layout: ArtLayout, prefer_steamgriddb: bool) -> Option<&Artwork> {
    let rank = |origin| match origin {
        ArtOrigin::User => 0,
        ArtOrigin::Source if prefer_steamgriddb => 2,
        ArtOrigin::Source => 1,
        ArtOrigin::SteamGridDb if prefer_steamgriddb => 1,
        ArtOrigin::SteamGridDb => 2,
        ArtOrigin::Generated => 3,
    };
    artwork
        .iter()
        .filter(|art| art.layout == layout)
        .min_by_key(|art| rank(art.origin))
}

/// A rectangle in a tile's coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// How to draw art of one shape into a box of another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Placement {
    /// Scale to cover the box and crop the overflow evenly.
    Fill(Rect),
    /// Draw a blurred copy covering the box, then the art scaled to fit inside it.
    FitOnBlur { background: Rect, foreground: Rect },
}

/// Places `art_width`×`art_height` art in a `box_width`×`box_height` cover: it fills when
/// its ratio is within [`FILL_TOLERANCE`] of [`COVER_RATIO`], else it fits on a blurred copy.
pub fn place(art_width: f32, art_height: f32, box_width: f32, box_height: f32) -> Placement {
    let cover = scaled(art_width, art_height, box_width, box_height, f32::max);
    if (art_width / art_height - COVER_RATIO).abs() <= FILL_TOLERANCE {
        Placement::Fill(cover)
    } else {
        Placement::FitOnBlur {
            background: cover,
            foreground: scaled(art_width, art_height, box_width, box_height, f32::min),
        }
    }
}

/// Places `art_width`×`art_height` art to cover a `box_width`×`box_height` box, whatever
/// its ratio, cropping the overflow evenly.
pub fn crop(art_width: f32, art_height: f32, box_width: f32, box_height: f32) -> Rect {
    scaled(art_width, art_height, box_width, box_height, f32::max)
}

/// How many times larger than drawn art may be before plain linear filtering aliases.
const LINEAR_SHRINK_LIMIT: f32 = 2.0;

/// Whether `art_width`×`art_height` art drawn into `drawn_width`×`drawn_height` device
/// pixels needs mipmaps (trilinear filtering): only art shrunk more than twice does.
/// GTK picks the mipmap level from the drawn size before a CSS transform, so art a design
/// scales up would show that smaller level enlarged; without mipmaps it stays sharp.
pub fn needs_mipmaps(art_width: f32, art_height: f32, drawn_width: f32, drawn_height: f32) -> bool {
    let shrink = f32::max(art_width / drawn_width, art_height / drawn_height);
    shrink > LINEAR_SHRINK_LIMIT
}

/// The art scaled by `pick` of the two axis scales and centred in the box.
fn scaled(
    art_width: f32,
    art_height: f32,
    box_width: f32,
    box_height: f32,
    pick: fn(f32, f32) -> f32,
) -> Rect {
    let scale = pick(box_width / art_width, box_height / art_height);
    let (width, height) = (art_width * scale, art_height * scale);
    Rect {
        x: (box_width - width) / 2.0,
        y: (box_height - height) / 2.0,
        width,
        height,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ArtLayout, ArtOrigin, Artwork, GameId, Source};

    fn art(layout: ArtLayout, origin: ArtOrigin, path: &str) -> Artwork {
        Artwork {
            game_id: GameId::new(Source::Steam, "1"),
            layout,
            origin,
            path: path.into(),
        }
    }

    #[test]
    fn steamgriddb_can_come_before_source_art_but_never_before_the_users_choice() {
        let all = [
            art(ArtLayout::Cover, ArtOrigin::Source, "/source"),
            art(ArtLayout::Cover, ArtOrigin::SteamGridDb, "/sgdb"),
            art(ArtLayout::Cover, ArtOrigin::User, "/user"),
        ];

        assert_eq!(
            best(&all[..2], ArtLayout::Cover, true)
                .unwrap()
                .path
                .to_str(),
            Some("/sgdb")
        );
        assert_eq!(
            best(&all, ArtLayout::Cover, true).unwrap().path.to_str(),
            Some("/user")
        );
    }

    #[test]
    fn the_users_choice_wins_then_source_art_then_steamgriddb_then_generated() {
        let all = [
            art(ArtLayout::Cover, ArtOrigin::Generated, "/generated"),
            art(ArtLayout::Cover, ArtOrigin::SteamGridDb, "/sgdb"),
            art(ArtLayout::Cover, ArtOrigin::Source, "/source"),
            art(ArtLayout::Cover, ArtOrigin::User, "/user"),
            art(ArtLayout::Hero, ArtOrigin::Source, "/hero"),
        ];

        assert_eq!(
            best(&all, ArtLayout::Cover, false).unwrap().path.to_str(),
            Some("/user")
        );
        assert_eq!(
            best(&all[..3], ArtLayout::Cover, false)
                .unwrap()
                .path
                .to_str(),
            Some("/source")
        );
        assert_eq!(
            best(&all[..2], ArtLayout::Cover, false)
                .unwrap()
                .path
                .to_str(),
            Some("/sgdb")
        );
        assert_eq!(
            best(&all[..1], ArtLayout::Cover, false)
                .unwrap()
                .path
                .to_str(),
            Some("/generated")
        );
        assert!(best(&all, ArtLayout::Logo, false).is_none());
    }

    #[test]
    fn art_within_two_to_three_plus_or_minus_a_tenth_fills_the_tile() {
        // 600×900 is exactly 2:3; Heroic's 3:4 art (0.75) and 0.57 are inside the band.
        for (width, height) in [(600.0, 900.0), (300.0, 400.0), (513.0, 900.0)] {
            assert!(
                matches!(place(width, height, 200.0, 300.0), Placement::Fill(_)),
                "{width}×{height}"
            );
        }
        // Just outside the band on either side.
        for (width, height) in [(780.0, 1000.0), (560.0, 1000.0)] {
            assert!(
                matches!(
                    place(width, height, 200.0, 300.0),
                    Placement::FitOnBlur { .. }
                ),
                "{width}×{height}"
            );
        }
    }

    #[test]
    fn filled_art_is_cropped_evenly_on_the_long_side() {
        let Placement::Fill(rect) = place(300.0, 400.0, 200.0, 300.0) else {
            panic!("3:4 art fills");
        };

        // Scaled to cover 200×300: 225×300, cropped 12.5 px left and right.
        assert_eq!(
            rect,
            Rect {
                x: -12.5,
                y: 0.0,
                width: 225.0,
                height: 300.0
            }
        );
    }

    #[test]
    fn square_and_landscape_art_fits_on_a_blurred_copy() {
        let Placement::FitOnBlur {
            background,
            foreground,
        } = place(460.0, 215.0, 200.0, 300.0)
        else {
            panic!("landscape art fits");
        };

        // The blurred copy covers the tile; the art itself fits its width, centred.
        assert_eq!(background.height, 300.0);
        assert!(background.width >= 200.0);
        assert_eq!(foreground.width, 200.0);
        assert!((foreground.height - 200.0 * 215.0 / 460.0).abs() < 0.001);
        assert!((foreground.y - (300.0 - foreground.height) / 2.0).abs() < 0.001);
        assert!(matches!(
            place(512.0, 512.0, 200.0, 300.0),
            Placement::FitOnBlur { .. }
        ));
    }

    #[test]
    fn cropped_art_fills_any_box() {
        // A 2:3 cover in a square card, cropped evenly top and bottom.
        let rect = crop(600.0, 900.0, 200.0, 200.0);

        assert_eq!((rect.x, rect.width), (0.0, 200.0));
        assert!((rect.height - 300.0).abs() < 0.001);
        assert!((rect.y + 50.0).abs() < 0.001);
    }

    #[test]
    fn art_decoded_for_a_card_a_design_scales_up_is_drawn_without_mipmaps() {
        // A 224-pixel card's art, decoded 1.6 times larger for a focused card.
        assert!(!needs_mipmaps(359.0, 359.0, 224.0, 224.0));
    }

    #[test]
    fn art_shrunk_more_than_twice_is_drawn_with_mipmaps() {
        // A full-size cover in a library tile, and art at exactly twice the drawn size.
        assert!(needs_mipmaps(600.0, 900.0, 200.0, 300.0));
        assert!(!needs_mipmaps(448.0, 448.0, 224.0, 224.0));
        assert!(needs_mipmaps(449.0, 300.0, 224.0, 150.0));
    }
}
