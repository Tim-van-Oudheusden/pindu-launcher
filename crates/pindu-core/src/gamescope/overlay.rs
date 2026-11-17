//! Quick Access over a running game: the launcher's overlay window, which gamescope
//! composites above the game in `--steam` mode, as plain data. The window carries
//! `STEAM_OVERLAY` from the start and is open or closed by its opacity and
//! `STEAM_INPUT_FOCUS`; it never carries `STEAM_GAME`, so `GAMESCOPE_FOCUSED_APP_GFX` stays
//! the game's while it is open. Tested against gamescope 3.17.0.

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

/// The overlay window's properties while Quick Access is `open` over a game, or closed:
/// always an overlay, fully opaque and holding input while open, see-through and leaving
/// input to the game while closed.
pub fn overlay_props(open: bool) -> OverlayProps {
    OverlayProps {
        steam_overlay: 1,
        opacity: if open { u32::MAX } else { 0 },
        input_focus: u32::from(open),
    }
}

/// Whether Quick Access is open over the game after `intent`, from `open`: Misc1 opens or
/// closes it, and Guide closes it as it brings the launcher to the front, where Quick
/// Access opens inside the main window instead.
pub fn open_after(open: bool, intent: InGameIntent) -> bool {
    match intent {
        InGameIntent::ToggleQuickAccess => !open,
        InGameIntent::ShowLauncher => false,
    }
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
    fn misc1_flips_quick_access_and_guide_closes_it_on_the_way_to_the_launcher() {
        assert!(open_after(false, InGameIntent::ToggleQuickAccess));
        assert!(!open_after(true, InGameIntent::ToggleQuickAccess));
        assert!(!open_after(true, InGameIntent::ShowLauncher));
        assert!(!open_after(false, InGameIntent::ShowLauncher));
    }
}
