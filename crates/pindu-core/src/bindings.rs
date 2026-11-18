//! Which button asks for which of fullscreen mode's functions: the binding table the
//! controller's intents, the hints and Quick Access's Controls all read (through each
//! pad's [`Layout`](crate::gamepad::Layout)), and which the menu drawer's Controls rebinds.
//! The face buttons, the shoulders and triggers, View, Menu and R3 take any function
//! ([`BINDABLE`], [`FUNCTIONS`]). Guide, the Steam Deck's "…", the D-pad and the left stick
//! stay fixed, so the launcher can always be navigated. Binding a function to a button
//! swaps it with the function that button had, so every function keeps a button. The
//! table is kept in the [`BINDINGS_KEY`] setting.

use crate::gamepad::{BUTTONS, Button, Direction, Intent};

/// The GSettings key that keeps the bindings: each button that can be bound, by its name,
/// with the name of the function it asks for (see [`Bindings::setting`]). Empty for the
/// defaults.
pub const BINDINGS_KEY: &str = "button-bindings";

/// The buttons a function can be bound to, by their position on an Xbox pad, in the order
/// hints prefer them when two ask for the same: options show on View, which the spec
/// names for it, rather than on the west face button.
pub const BINDABLE: [Button; 11] = [
    Button::South,
    Button::East,
    Button::North,
    Button::Select,
    Button::West,
    Button::Start,
    Button::RightStick,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
];

/// The functions a button can be bound to, in the order the drawer's Controls lists them.
pub const FUNCTIONS: [Intent; 10] = [
    Intent::Activate,
    Intent::Back,
    Intent::Search,
    Intent::Options,
    Intent::Menu,
    Intent::QuickAccess,
    Intent::PreviousTab,
    Intent::NextTab,
    Intent::PreviousSort,
    Intent::NextSort,
];

/// The function of each button in [`BINDABLE`] by default: both View and the west face
/// button open a game's options.
const DEFAULTS: [Intent; 11] = [
    Intent::Activate,
    Intent::Back,
    Intent::Search,
    Intent::Options,
    Intent::Options,
    Intent::Menu,
    Intent::QuickAccess,
    Intent::PreviousTab,
    Intent::NextTab,
    Intent::PreviousSort,
    Intent::NextSort,
];

/// The function each button asks for, by the button's position on an Xbox pad (before
/// [`Layout`](crate::gamepad::Layout) swaps confirm and back, if it does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bindings([Intent; 11]);

impl Default for Bindings {
    fn default() -> Self {
        Self(DEFAULTS)
    }
}

impl Bindings {
    /// The intent `button` asks for: its bound function, or the fixed intent of Guide, the
    /// Steam Deck's "…" and the D-pad.
    pub fn intent(self, button: Button) -> Intent {
        match (index(button), fixed_intent(button)) {
            (Some(at), _) => self.0[at],
            (None, Some(intent)) => intent,
            (None, None) => unreachable!("every button is bindable or fixed"),
        }
    }

    /// The button that asks for `intent`, the first in the order hints prefer the buttons;
    /// `None` for an intent no button asks for when pressed (a held one).
    pub fn button(self, intent: Intent) -> Option<Button> {
        BUTTONS
            .into_iter()
            .find(|&button| self.intent(button) == intent)
    }

    /// The bindings with `function` on `button`, and the function `button` had on the
    /// button `function` had (see [`Self::button`]). A button that cannot be bound, or an
    /// intent that is not one of the [`FUNCTIONS`], leaves them as they are.
    pub fn bind(self, function: Intent, button: Button) -> Self {
        let from = self.button(function).and_then(index);
        match (FUNCTIONS.contains(&function), index(button), from) {
            (true, Some(to), Some(from)) => {
                let mut table = self.0;
                table.swap(to, from);
                Self(table)
            }
            _ => self,
        }
    }

    /// The value of the [`BINDINGS_KEY`] setting: each button that can be bound with its
    /// function, by name; empty for the defaults.
    pub fn setting(self) -> Vec<(&'static str, &'static str)> {
        if self == Self::default() {
            return Vec::new();
        }
        BINDABLE
            .into_iter()
            .zip(self.0)
            .filter_map(|(button, function)| Some((button_name(button), function_name(function)?)))
            .collect()
    }

    /// The bindings a [`BINDINGS_KEY`] value holds, over the defaults. Names it does not
    /// know are left out, and a value that leaves a function without a button, written by
    /// hand or by another version, reads as the defaults.
    pub fn from_setting<'a>(entries: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut table = DEFAULTS;
        for (button, function) in entries {
            let at = BINDABLE
                .iter()
                .position(|&known| button_name(known) == button);
            let function = function_named(function);
            if let (Some(at), Some(function)) = (at, function) {
                table[at] = function;
            }
        }
        if FUNCTIONS.iter().all(|function| table.contains(function)) {
            Self(table)
        } else {
            Self::default()
        }
    }
}

/// Where `button` sits in [`BINDABLE`], if it can be bound.
fn index(button: Button) -> Option<usize> {
    BINDABLE.iter().position(|&bindable| bindable == button)
}

/// The intent of a button that cannot be bound; `None` for one that can.
fn fixed_intent(button: Button) -> Option<Intent> {
    let navigate = |direction| Some(Intent::Navigate(direction));
    match button {
        Button::Guide => Some(Intent::Home),
        Button::Misc1 => Some(Intent::SystemQuickAccess),
        Button::DpadUp => navigate(Direction::Up),
        Button::DpadDown => navigate(Direction::Down),
        Button::DpadLeft => navigate(Direction::Left),
        Button::DpadRight => navigate(Direction::Right),
        _ => None,
    }
}

/// The name a button goes by in the setting.
fn button_name(button: Button) -> &'static str {
    match button {
        Button::South => "south",
        Button::East => "east",
        Button::West => "west",
        Button::North => "north",
        Button::LeftShoulder => "left-shoulder",
        Button::RightShoulder => "right-shoulder",
        Button::LeftTrigger => "left-trigger",
        Button::RightTrigger => "right-trigger",
        Button::Select => "select",
        Button::Start => "start",
        Button::Guide => "guide",
        Button::RightStick => "right-stick",
        Button::Misc1 => "misc1",
        Button::DpadUp => "dpad-up",
        Button::DpadDown => "dpad-down",
        Button::DpadLeft => "dpad-left",
        Button::DpadRight => "dpad-right",
    }
}

/// The name a function goes by in the setting and in the drawer's `win.bind-button`
/// action; `None` for an intent that is not one of the [`FUNCTIONS`].
pub fn function_name(function: Intent) -> Option<&'static str> {
    Some(match function {
        Intent::Activate => "activate",
        Intent::Back => "back",
        Intent::Search => "search",
        Intent::Options => "options",
        Intent::Menu => "menu",
        Intent::QuickAccess => "quick-access",
        Intent::PreviousTab => "previous-tab",
        Intent::NextTab => "next-tab",
        Intent::PreviousSort => "previous-sort",
        Intent::NextSort => "next-sort",
        Intent::Navigate(_) | Intent::BackHeld | Intent::Home | Intent::SystemQuickAccess => {
            return None;
        }
    })
}

/// The function that goes by `name` (see [`function_name`]), if one does.
pub fn function_named(name: &str) -> Option<Intent> {
    FUNCTIONS
        .into_iter()
        .find(|&function| function_name(function) == Some(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn by_default_each_button_asks_for_what_it_always_has() {
        let bindings = Bindings::default();
        let cases = [
            (Button::South, Intent::Activate),
            (Button::East, Intent::Back),
            (Button::North, Intent::Search),
            (Button::West, Intent::Options),
            (Button::Select, Intent::Options),
            (Button::Start, Intent::Menu),
            (Button::RightStick, Intent::QuickAccess),
            (Button::LeftShoulder, Intent::PreviousTab),
            (Button::RightShoulder, Intent::NextTab),
            (Button::LeftTrigger, Intent::PreviousSort),
            (Button::RightTrigger, Intent::NextSort),
            (Button::Guide, Intent::Home),
            (Button::Misc1, Intent::SystemQuickAccess),
            (Button::DpadUp, Intent::Navigate(Direction::Up)),
            (Button::DpadDown, Intent::Navigate(Direction::Down)),
            (Button::DpadLeft, Intent::Navigate(Direction::Left)),
            (Button::DpadRight, Intent::Navigate(Direction::Right)),
        ];
        assert_eq!(cases.len(), BUTTONS.len(), "every button has a case");

        for (button, intent) in cases {
            assert_eq!(bindings.intent(button), intent, "{button:?}");
        }
        assert_eq!(bindings.button(Intent::Options), Some(Button::Select));
    }

    #[test]
    fn binding_a_function_swaps_it_with_the_one_the_button_had() {
        let bindings = Bindings::default().bind(Intent::Search, Button::South);

        assert_eq!(bindings.intent(Button::South), Intent::Search);
        assert_eq!(bindings.intent(Button::North), Intent::Activate);
        assert_eq!(bindings.button(Intent::Search), Some(Button::South));
        assert_eq!(bindings.button(Intent::Activate), Some(Button::North));
    }

    #[test]
    fn binding_options_moves_it_off_view_and_leaves_the_west_button_with_it() {
        let bindings = Bindings::default().bind(Intent::Options, Button::LeftTrigger);

        assert_eq!(bindings.intent(Button::LeftTrigger), Intent::Options);
        assert_eq!(bindings.intent(Button::Select), Intent::PreviousSort);
        assert_eq!(bindings.intent(Button::West), Intent::Options);
    }

    #[test]
    fn binding_a_function_to_a_button_it_has_changes_nothing() {
        let bindings = Bindings::default();

        assert_eq!(bindings.bind(Intent::Back, Button::East), bindings);
        assert_eq!(bindings.bind(Intent::Options, Button::West), bindings);
    }

    #[test]
    fn guide_the_dots_button_and_the_d_pad_stay_fixed() {
        let bindings = Bindings::default();
        for button in [
            Button::Guide,
            Button::Misc1,
            Button::DpadUp,
            Button::DpadDown,
            Button::DpadLeft,
            Button::DpadRight,
        ] {
            assert_eq!(
                bindings.bind(Intent::Activate, button),
                bindings,
                "{button:?}"
            );
        }
        for intent in [
            Intent::Home,
            Intent::SystemQuickAccess,
            Intent::BackHeld,
            Intent::Navigate(Direction::Up),
        ] {
            assert_eq!(bindings.bind(intent, Button::South), bindings, "{intent:?}");
        }
    }

    #[test]
    fn every_function_keeps_a_button_whatever_is_bound_where() {
        let mut bindings = Bindings::default();
        for (step, button) in BINDABLE.into_iter().cycle().take(40).enumerate() {
            let function = FUNCTIONS[(step * 7) % FUNCTIONS.len()];
            bindings = bindings.bind(function, button);

            assert_eq!(bindings.intent(button), function, "step {step}");
            for function in FUNCTIONS {
                assert!(
                    bindings.button(function).is_some(),
                    "step {step}: {function:?} lost its button"
                );
            }
        }
    }

    #[test]
    fn the_defaults_are_kept_as_an_empty_setting() {
        assert_eq!(Bindings::default().setting(), []);
        assert_eq!(Bindings::from_setting([]), Bindings::default());
    }

    #[test]
    fn bindings_read_back_from_their_setting() {
        let bindings = Bindings::default()
            .bind(Intent::Search, Button::South)
            .bind(Intent::Menu, Button::RightTrigger);
        let setting = bindings.setting();

        assert!(setting.contains(&("south", "search")), "{setting:?}");
        assert!(setting.contains(&("right-trigger", "menu")), "{setting:?}");
        assert_eq!(Bindings::from_setting(setting), bindings);
    }

    #[test]
    fn a_setting_with_unknown_names_reads_over_the_defaults() {
        let bindings =
            Bindings::from_setting([("south", "search"), ("north", "activate"), ("x", "back")]);

        assert_eq!(
            bindings,
            Bindings::default().bind(Intent::Search, Button::South)
        );
        assert_eq!(
            Bindings::from_setting([("guide", "menu"), ("south", "jump")]),
            Bindings::default()
        );
    }

    #[test]
    fn a_setting_that_leaves_a_function_without_a_button_reads_as_the_defaults() {
        assert_eq!(
            Bindings::from_setting([("east", "activate")]),
            Bindings::default(),
            "Back would have no button"
        );
    }

    #[test]
    fn function_names_read_back() {
        for function in FUNCTIONS {
            let name = function_name(function).expect("a function has a name");
            assert_eq!(function_named(name), Some(function));
        }
        assert_eq!(function_name(Intent::Home), None);
        assert_eq!(function_named("home"), None);
    }
}
