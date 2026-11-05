//! Fullscreen mode's keyboard shortcuts for what a controller does beyond moving and
//! activating, so the mode works without one: F10 opens the menu drawer like Start,
//! Ctrl+2 (Big Picture's key) opens Quick Access like R3, the Menu key or Shift+F10 opens
//! the focused game's options like View, Ctrl+Page Up and Ctrl+Page Down switch the
//! library's tabs like LB and RB, and with Shift held too they cycle its sort order like
//! LT and RT.

use crate::gamepad::Intent;

/// The modifiers held with a key.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub shift: bool,
    pub alt: bool,
}

/// What a key press does in fullscreen mode, by the key's name as GDK has it ("F10",
/// "Menu", "Page_Up"); `None` leaves the key to the focused widget.
pub fn fullscreen_intent(key: &str, modifiers: Modifiers) -> Option<Intent> {
    let Modifiers {
        control,
        shift,
        alt,
    } = modifiers;
    if alt {
        return None;
    }
    match (key, control, shift) {
        ("F10", false, false) => Some(Intent::Menu),
        ("2", true, false) => Some(Intent::QuickAccess),
        ("Menu", false, false) | ("F10", false, true) => Some(Intent::Options),
        ("Page_Up", true, false) => Some(Intent::PreviousTab),
        ("Page_Down", true, false) => Some(Intent::NextTab),
        ("Page_Up", true, true) => Some(Intent::PreviousSort),
        ("Page_Down", true, true) => Some(Intent::NextSort),
        _ => None,
    }
}

/// What Esc does in fullscreen mode once nothing nearer the focus (a menu, a dialog, the
/// search entry, a page to go back from) took it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Escape {
    /// Back from a section's games to Home's first screen, as B does.
    ShowSections,
    /// Leave fullscreen mode for the desktop window.
    LeaveFullscreen,
}

/// What Esc does in fullscreen mode, as Home shows a section's games or not.
pub fn escape(shows_home_section: bool) -> Escape {
    if shows_home_section {
        Escape::ShowSections
    } else {
        Escape::LeaveFullscreen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NONE: Modifiers = Modifiers {
        control: false,
        shift: false,
        alt: false,
    };

    #[test]
    fn f10_opens_the_menu_drawer() {
        assert_eq!(fullscreen_intent("F10", NONE), Some(Intent::Menu));
    }

    #[test]
    fn control_2_opens_quick_access() {
        let control = Modifiers {
            control: true,
            ..NONE
        };
        assert_eq!(fullscreen_intent("2", control), Some(Intent::QuickAccess));
        assert_eq!(fullscreen_intent("2", NONE), None, "typing a 2");
    }

    #[test]
    fn the_menu_key_and_shift_f10_open_the_game_options() {
        let shift = Modifiers {
            shift: true,
            ..NONE
        };
        assert_eq!(fullscreen_intent("Menu", NONE), Some(Intent::Options));
        assert_eq!(fullscreen_intent("F10", shift), Some(Intent::Options));
    }

    #[test]
    fn control_page_up_and_down_switch_tabs() {
        let control = Modifiers {
            control: true,
            ..NONE
        };
        assert_eq!(
            fullscreen_intent("Page_Up", control),
            Some(Intent::PreviousTab)
        );
        assert_eq!(
            fullscreen_intent("Page_Down", control),
            Some(Intent::NextTab)
        );
    }

    #[test]
    fn control_shift_page_up_and_down_cycle_the_sort_order() {
        let control_shift = Modifiers {
            control: true,
            shift: true,
            ..NONE
        };
        assert_eq!(
            fullscreen_intent("Page_Up", control_shift),
            Some(Intent::PreviousSort)
        );
        assert_eq!(
            fullscreen_intent("Page_Down", control_shift),
            Some(Intent::NextSort)
        );
    }

    #[test]
    fn other_keys_and_combinations_are_left_to_the_focused_widget() {
        let alt = Modifiers { alt: true, ..NONE };
        let control = Modifiers {
            control: true,
            ..NONE
        };
        assert_eq!(fullscreen_intent("Page_Up", NONE), None);
        assert_eq!(fullscreen_intent("F10", alt), None);
        assert_eq!(fullscreen_intent("Menu", control), None);
        assert_eq!(fullscreen_intent("Return", NONE), None);
        assert_eq!(fullscreen_intent("Escape", NONE), None);
    }

    #[test]
    fn escape_on_a_home_sections_games_goes_back_to_homes_first_screen() {
        assert_eq!(escape(true), Escape::ShowSections);
    }

    #[test]
    fn escape_anywhere_else_leaves_fullscreen_mode() {
        assert_eq!(escape(false), Escape::LeaveFullscreen);
    }
}
