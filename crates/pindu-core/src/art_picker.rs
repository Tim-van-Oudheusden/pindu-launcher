//! Change Artwork: the kinds of art a game's art is changed in, in the order both shells
//! show them, and when fullscreen mode's grid of SteamGridDB images loads its next page.

use crate::model::ArtLayout;

/// The kinds of art, in the order the desktop window's list and fullscreen mode's tabs
/// show them.
pub const KINDS: [ArtLayout; 5] = [
    ArtLayout::Cover,
    ArtLayout::Hero,
    ArtLayout::Logo,
    ArtLayout::Icon,
    ArtLayout::Square,
];

/// The kind after `current`, or before it, which the shoulder buttons switch to: none
/// past either end, as with the library's tabs.
pub fn step_kind(current: ArtLayout, forward: bool) -> Option<ArtLayout> {
    let index = KINDS.iter().position(|&kind| kind == current)?;
    let next = if forward {
        index + 1
    } else {
        index.checked_sub(1)?
    };
    KINDS.get(next).copied()
}

/// Whether fullscreen mode's grid of `loaded` images, `columns` to a row, loads its next
/// page with the image at `focused` focused: once focus reaches the last two rows, while
/// there is a `next_page`. Scrolling to the bottom loads it too.
pub fn loads_more(
    focused: Option<usize>,
    loaded: usize,
    columns: usize,
    next_page: Option<u32>,
) -> bool {
    let (Some(focused), Some(_)) = (focused, next_page) else {
        return false;
    };
    focused + 2 * columns.max(1) >= loaded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shoulders_step_through_the_kinds_and_stop_at_either_end() {
        assert_eq!(KINDS[0], ArtLayout::Cover);
        assert_eq!(step_kind(ArtLayout::Cover, true), Some(ArtLayout::Hero));
        assert_eq!(step_kind(ArtLayout::Hero, false), Some(ArtLayout::Cover));
        assert_eq!(step_kind(ArtLayout::Cover, false), None);
        assert_eq!(step_kind(KINDS[KINDS.len() - 1], true), None);
    }

    #[test]
    fn the_next_page_loads_once_focus_reaches_the_last_two_rows() {
        // 50 images in rows of 5: rows 0-9, the last two start at 40.
        assert!(!loads_more(Some(39), 50, 5, Some(1)));
        assert!(loads_more(Some(40), 50, 5, Some(1)));
        assert!(loads_more(Some(49), 50, 5, Some(1)));
        // Not past the last page.
        assert!(!loads_more(Some(49), 50, 5, None));
        // A short result loads more at once; nothing focused waits for focus.
        assert!(loads_more(Some(0), 4, 5, Some(1)));
        assert!(!loads_more(None, 50, 5, Some(1)));
    }
}
