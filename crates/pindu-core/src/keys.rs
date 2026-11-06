//! Fullscreen mode's keyboard shortcuts for what a controller does beyond moving and
//! activating, so the mode works without one: F10 opens the menu drawer like Start,
//! Ctrl+2 (Big Picture's key) opens Quick Access like R3, the Menu key or Shift+F10 opens
//! the focused game's options like View, Ctrl+Page Up and Ctrl+Page Down switch the
//! library's tabs like LB and RB, and with Shift held too they cycle its sort order like
//! LT and RT. Alongside them, the keys GTK and the window handle themselves (the arrows,
//! Enter, Esc and Ctrl+F) stand in for the rest, which Quick Access's Controls lists.

use crate::gamepad::{Direction, Intent};

/// A key with the modifiers held with it, by the key's name as GDK has it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub key: &'static str,
    pub control: bool,
    pub shift: bool,
}

impl Chord {
    pub const fn plain(key: &'static str) -> Self {
        Self {
            key,
            control: false,
            shift: false,
        }
    }

    pub const fn control(key: &'static str) -> Self {
        Self {
            control: true,
            ..Self::plain(key)
        }
    }

    pub const fn shift(key: &'static str) -> Self {
        Self {
            shift: true,
            ..Self::plain(key)
        }
    }

    pub const fn control_shift(key: &'static str) -> Self {
        Self {
            control: true,
            shift: true,
            ..Self::plain(key)
        }
    }

    /// The chord as `gtk_accelerator_parse` and shortcut triggers read it.
    pub fn accelerator(self) -> String {
        let control = if self.control { "<Control>" } else { "" };
        let shift = if self.shift { "<Shift>" } else { "" };
        format!("{control}{shift}{}", self.key)
    }
}

/// Esc, which goes back in fullscreen mode, and closes a panel over it.
pub const ESCAPE: Chord = Chord::plain("Escape");
/// Ctrl+F, which starts a search.
pub const SEARCH: Chord = Chord::control("f");

/// The keys [`fullscreen_intent`] turns into intents.
const SHORTCUTS: [(Chord, Intent); 8] = [
    (Chord::plain("F10"), Intent::Menu),
    (Chord::control("2"), Intent::QuickAccess),
    (Chord::plain("Menu"), Intent::Options),
    (Chord::shift("F10"), Intent::Options),
    (Chord::control("Page_Up"), Intent::PreviousTab),
    (Chord::control("Page_Down"), Intent::NextTab),
    (Chord::control_shift("Page_Up"), Intent::PreviousSort),
    (Chord::control_shift("Page_Down"), Intent::NextSort),
];

/// The keys that do in fullscreen mode what an intent does without becoming one: GTK
/// moves focus with the arrows and activates with Enter, and the window binds Esc and
/// Ctrl+F.
const WIDGET_KEYS: [(Chord, Intent); 7] = [
    (Chord::plain("Up"), Intent::Navigate(Direction::Up)),
    (Chord::plain("Down"), Intent::Navigate(Direction::Down)),
    (Chord::plain("Left"), Intent::Navigate(Direction::Left)),
    (Chord::plain("Right"), Intent::Navigate(Direction::Right)),
    (Chord::plain("Return"), Intent::Activate),
    (ESCAPE, Intent::Back),
    (SEARCH, Intent::Search),
];

/// The keys that stand in for `intent` in fullscreen mode, shortcuts first; none for an
/// intent only a controller asks for, such as holding B to cancel a launch.
pub fn stand_ins(intent: Intent) -> Vec<Chord> {
    SHORTCUTS
        .into_iter()
        .chain(WIDGET_KEYS)
        .filter(|&(_, stands_for)| stands_for == intent)
        .map(|(chord, _)| chord)
        .collect()
}

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
    SHORTCUTS
        .into_iter()
        .find(|(chord, _)| (chord.key, chord.control, chord.shift) == (key, control, shift))
        .map(|(_, intent)| intent)
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
    fn every_shortcut_asks_for_the_intent_it_stands_in_for() {
        for (chord, intent) in SHORTCUTS {
            let modifiers = Modifiers {
                control: chord.control,
                shift: chord.shift,
                alt: false,
            };
            assert_eq!(
                fullscreen_intent(chord.key, modifiers),
                Some(intent),
                "{chord:?}"
            );
        }
    }

    #[test]
    fn stand_ins_are_the_shortcuts_then_the_keys_gtk_and_the_window_handle() {
        use crate::gamepad::Direction;

        assert_eq!(
            stand_ins(Intent::Options),
            [Chord::plain("Menu"), Chord::shift("F10")]
        );
        assert_eq!(
            stand_ins(Intent::NextSort),
            [Chord::control_shift("Page_Down")]
        );
        assert_eq!(stand_ins(Intent::Back), [ESCAPE]);
        assert_eq!(stand_ins(Intent::Search), [SEARCH]);
        assert_eq!(stand_ins(Intent::Activate), [Chord::plain("Return")]);
        assert_eq!(
            stand_ins(Intent::Navigate(Direction::Left)),
            [Chord::plain("Left")]
        );
        assert_eq!(stand_ins(Intent::BackHeld), []);
    }

    #[test]
    fn a_chord_is_written_as_a_gtk_accelerator() {
        assert_eq!(Chord::plain("F10").accelerator(), "F10");
        assert_eq!(SEARCH.accelerator(), "<Control>f");
        assert_eq!(
            Chord::control_shift("Page_Up").accelerator(),
            "<Control><Shift>Page_Up"
        );
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
