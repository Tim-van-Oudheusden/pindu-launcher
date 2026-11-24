//! The launcher's overlay window over a running game, which gamescope composites above the
//! game in `--steam` mode, as plain data: what it shows (the menu drawer or Quick Access)
//! and the properties that open and close it. The window carries `STEAM_OVERLAY` from the
//! start and is open or closed by its opacity and `STEAM_INPUT_FOCUS`; it never carries
//! `STEAM_GAME`, so `GAMESCOPE_FOCUSED_APP_GFX` stays the game's while it is open. Tested
//! against gamescope 3.17.0.

use super::InGameIntent;

/// The CARDINALs gamescope reads from the overlay window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayProps {
    /// `STEAM_OVERLAY`: 1 makes the window an overlay rather than a base layer candidate.
    pub steam_overlay: u32,
    /// `_NET_WM_WINDOW_OPACITY`: 0 leaves the layer out of composition, so the game keeps
    /// direct scan-out.
    pub opacity: u32,
    /// `STEAM_INPUT_FOCUS`: 1 sends the keyboard and pointer to the overlay, 0 leaves them
    /// with the game.
    pub input_focus: u32,
}

/// The overlay window's properties while it is `open` over a game, or closed: always an
/// overlay, fully opaque and holding input while open, see-through and leaving input to
/// the game while closed.
pub fn overlay_props(open: bool) -> OverlayProps {
    OverlayProps {
        steam_overlay: 1,
        opacity: if open { u32::MAX } else { 0 },
        input_focus: u32::from(open),
    }
}

/// The overlay window's properties while it shows a notice, such as how to leave Steam
/// Big Picture: drawn, but leaving input to the window under it.
pub fn notice_props() -> OverlayProps {
    OverlayProps {
        steam_overlay: 1,
        opacity: u32::MAX,
        input_focus: 0,
    }
}

/// What the overlay window shows over a game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverGame {
    /// The menu drawer, on the left, as Guide opens Steam's menu over a game.
    Menu,
    /// Quick Access, on the right.
    QuickAccess,
}

/// What the overlay window shows over the game after `intent`, from `shown`: Guide opens or
/// closes the menu, the Quick Access button opens or closes Quick Access, and either one
/// turns from the other panel to its own.
pub fn shown_after(shown: Option<OverGame>, intent: InGameIntent) -> Option<OverGame> {
    let wanted = match intent {
        InGameIntent::ToggleMenu => OverGame::Menu,
        InGameIntent::ToggleQuickAccess => OverGame::QuickAccess,
    };
    (shown != Some(wanted)).then_some(wanted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_closed_overlay_is_see_through_and_leaves_input_to_the_game() {
        assert_eq!(
            overlay_props(false),
            OverlayProps {
                steam_overlay: 1,
                opacity: 0,
                input_focus: 0,
            }
        );
    }

    #[test]
    fn an_open_overlay_is_opaque_and_takes_input() {
        assert_eq!(
            overlay_props(true),
            OverlayProps {
                steam_overlay: 1,
                opacity: 0xffff_ffff,
                input_focus: 1,
            }
        );
    }

    #[test]
    fn a_notice_shows_over_big_picture_and_leaves_it_the_input() {
        assert_eq!(
            notice_props(),
            OverlayProps {
                steam_overlay: 1,
                opacity: 0xffff_ffff,
                input_focus: 0,
            }
        );
    }

    #[test]
    fn guide_opens_and_closes_the_menu_over_the_game() {
        use OverGame::*;
        assert_eq!(shown_after(None, InGameIntent::ToggleMenu), Some(Menu));
        assert_eq!(shown_after(Some(Menu), InGameIntent::ToggleMenu), None);
        // Over Quick Access, Guide turns to the menu, as Steam's button does.
        assert_eq!(
            shown_after(Some(QuickAccess), InGameIntent::ToggleMenu),
            Some(Menu)
        );
    }

    #[test]
    fn the_quick_access_button_opens_and_closes_quick_access_over_the_game() {
        use OverGame::*;
        let toggle = InGameIntent::ToggleQuickAccess;
        assert_eq!(shown_after(None, toggle), Some(QuickAccess));
        assert_eq!(shown_after(Some(QuickAccess), toggle), None);
        assert_eq!(shown_after(Some(Menu), toggle), Some(QuickAccess));
    }
}
