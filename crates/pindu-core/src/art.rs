//! Choosing and placing game artwork.

use crate::model::{ArtLayout, ArtOrigin, Artwork};

/// The cover aspect ratio, width over height.
pub const COVER_RATIO: f32 = 2.0 / 3.0;
/// How far from [`COVER_RATIO`] art may be and still fill a cover with a slight crop.
pub const FILL_TOLERANCE: f32 = 0.1;

/// The image to show for a layout: the user's choice, then the source's own art, then
/// SteamGridDB, then a generated placeholder.
pub fn best(artwork: &[Artwork], layout: ArtLayout) -> Option<&Artwork> {
    let rank = |origin| match origin {
        ArtOrigin::User => 0,
        ArtOrigin::Source => 1,
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
    fn the_users_choice_wins_then_source_art_then_steamgriddb_then_generated() {
        let all = [
            art(ArtLayout::Cover, ArtOrigin::Generated, "/generated"),
            art(ArtLayout::Cover, ArtOrigin::SteamGridDb, "/sgdb"),
            art(ArtLayout::Cover, ArtOrigin::Source, "/source"),
            art(ArtLayout::Cover, ArtOrigin::User, "/user"),
            art(ArtLayout::Hero, ArtOrigin::Source, "/hero"),
        ];

        assert_eq!(
            best(&all, ArtLayout::Cover).unwrap().path.to_str(),
            Some("/user")
        );
        assert_eq!(
            best(&all[..3], ArtLayout::Cover).unwrap().path.to_str(),
            Some("/source")
        );
        assert_eq!(
            best(&all[..2], ArtLayout::Cover).unwrap().path.to_str(),
            Some("/sgdb")
        );
        assert_eq!(
            best(&all[..1], ArtLayout::Cover).unwrap().path.to_str(),
            Some("/generated")
        );
        assert!(best(&all, ArtLayout::Logo).is_none());
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
}
