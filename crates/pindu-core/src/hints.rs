//! The fullscreen hint bar: what each button does right now for the focused page and
//! widget, with the glyph that shows the button on the active controller.

use crate::gamepad::{Button, DeviceKind, Intent};
use crate::panels::{DrawerLevel, OptionsLevel, Panel};

/// What kind of widget has focus, as far as the A button is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// Nothing has focus, so A does nothing.
    Nothing,
    /// A cover or card that opens its game.
    Game,
    /// The game page's Play button.
    Play,
    /// The game page's Stop button, while the game runs.
    Stop,
    /// A switch, check box or toggle button.
    Toggle,
    /// Any other widget that activates.
    Other,
}

/// Where the user is: the focused widget, an open dialog, a page to go back to, whether
/// the page searches, whether it has tabs and a sort to step through, whether a game has
/// options, whether Start opens the menu drawer, an open panel, and whether a launch is
/// under way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Context {
    pub focus: Focus,
    /// A dialog is open, which B closes.
    pub dialog: bool,
    /// A page lies below the shown one, which B goes back to.
    pub page_below: bool,
    /// The page searches the library (Home and the library), which Y starts.
    pub search: bool,
    /// The page has tabs, which the shoulder buttons switch, and a sort order, which the
    /// triggers cycle (fullscreen mode's library).
    pub tabs: bool,
    /// A game has focus (a cover or card) or the page is a game's: View opens its options.
    pub options: bool,
    /// Start opens the menu drawer (fullscreen mode).
    pub menu: bool,
    /// The menu drawer or the options sheet is open, at a level.
    pub panel: Option<Panel>,
    /// The launch overlay is up.
    pub launching: Launching,
}

/// What the launch overlay shows, as far as the buttons are concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Launching {
    /// No launch overlay.
    #[default]
    No,
    /// A store game going to its launcher, which cannot be called back.
    HandingOff,
    /// A game we run starting, which holding B cancels.
    Starting,
}

/// What a button does, as the hint bar names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Open,
    Play,
    Stop,
    Toggle,
    Select,
    Back,
    Close,
    Search,
    /// Switch to the tab before or after.
    Tabs,
    /// Cycle the sort order back or forward.
    Sort,
    /// Open the options for the page's game.
    Options,
    /// Hold the button to cancel the launch.
    HoldToCancel,
    /// Open the menu drawer.
    Menu,
}

/// One hint: the button, by its position on the pad, and what it does. A pair of buttons
/// that step two ways (the shoulders, the triggers) shares one hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hint {
    pub button: Button,
    /// The button that steps the other way.
    pub pair: Option<Button>,
    pub action: Action,
}

/// The hints for `context` on a controller of `kind`, confirm first.
pub fn hints(context: Context, kind: DeviceKind) -> Vec<Hint> {
    // The launch overlay takes every button: only a held B, for a game we start.
    match context.launching {
        Launching::No => {}
        Launching::HandingOff => return Vec::new(),
        Launching::Starting => {
            return vec![Hint {
                button: kind.button(Intent::Back),
                pair: None,
                action: Action::HoldToCancel,
            }];
        }
    }
    let activate = match context.focus {
        Focus::Nothing => None,
        Focus::Game => Some(Action::Open),
        Focus::Play => Some(Action::Play),
        Focus::Stop => Some(Action::Stop),
        Focus::Toggle => Some(Action::Toggle),
        Focus::Other => Some(Action::Select),
    };
    // A panel over the page closes on B, or steps back from one of its lists; the page's
    // own buttons wait for it, as they wait for a dialog.
    let back = match context.panel {
        _ if context.dialog => Some(Action::Close),
        Some(Panel::Drawer(DrawerLevel::Main) | Panel::Options(OptionsLevel::Main)) => {
            Some(Action::Close)
        }
        Some(_) => Some(Action::Back),
        None => context.page_below.then_some(Action::Back),
    };
    let page = !context.dialog && context.panel.is_none();
    // Y searches the page under a dialog, so it waits for the dialog to close; so do the
    // page's tabs, sort, options and menu.
    let search = (context.search && page).then_some(Action::Search);
    let options = (context.options && page).then_some(Action::Options);
    let menu = (context.menu && page).then_some(Action::Menu);
    let tabs = context.tabs && page;
    let single = |intent, action: Option<Action>| {
        action.map(|action| Hint {
            button: kind.button(intent),
            pair: None,
            action,
        })
    };
    let pair = |back, forward, action| {
        tabs.then(|| Hint {
            button: kind.button(back),
            pair: Some(kind.button(forward)),
            action,
        })
    };
    [
        single(Intent::Activate, activate),
        single(Intent::Back, back),
        single(Intent::Search, search),
        single(Intent::Options, options),
        single(Intent::Menu, menu),
        pair(Intent::PreviousTab, Intent::NextTab, Action::Tabs),
        pair(Intent::PreviousSort, Intent::NextSort, Action::Sort),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The icon name of the glyph for `button` on a controller of `kind`: the layout's own
/// label (a letter or shape) drawn at the button's position, in our neutral style.
/// Unknown pads look like Xbox pads, as libmanette maps them.
pub fn glyph(kind: DeviceKind, button: Button) -> &'static str {
    use DeviceKind::{Generic, Nintendo, PlayStation, SteamDeck, Xbox};
    match (kind, button) {
        (PlayStation, Button::South) => "pindu-glyph-south-cross-symbolic",
        (PlayStation, Button::East) => "pindu-glyph-east-circle-symbolic",
        (PlayStation, Button::West) => "pindu-glyph-west-square-symbolic",
        (PlayStation, Button::North) => "pindu-glyph-north-triangle-symbolic",
        (Nintendo, Button::East) => "pindu-glyph-east-a-symbolic",
        (Nintendo, Button::South) => "pindu-glyph-south-b-symbolic",
        (Nintendo, Button::North) => "pindu-glyph-north-x-symbolic",
        (Nintendo, Button::West) => "pindu-glyph-west-y-symbolic",
        (Xbox | SteamDeck | Generic, Button::South) => "pindu-glyph-south-a-symbolic",
        (Xbox | SteamDeck | Generic, Button::East) => "pindu-glyph-east-b-symbolic",
        (Xbox | SteamDeck | Generic, Button::West) => "pindu-glyph-west-x-symbolic",
        (Xbox | SteamDeck | Generic, Button::North) => "pindu-glyph-north-y-symbolic",
        (Xbox | Generic, Button::LeftShoulder) => "pindu-glyph-lb-symbolic",
        (Xbox | Generic, Button::RightShoulder) => "pindu-glyph-rb-symbolic",
        (PlayStation | SteamDeck, Button::LeftShoulder) => "pindu-glyph-l1-symbolic",
        (PlayStation | SteamDeck, Button::RightShoulder) => "pindu-glyph-r1-symbolic",
        (Nintendo, Button::LeftShoulder) => "pindu-glyph-l-symbolic",
        (Nintendo, Button::RightShoulder) => "pindu-glyph-r-symbolic",
        (Xbox | Generic, Button::LeftTrigger) => "pindu-glyph-lt-symbolic",
        (Xbox | Generic, Button::RightTrigger) => "pindu-glyph-rt-symbolic",
        (PlayStation | SteamDeck, Button::LeftTrigger) => "pindu-glyph-l2-symbolic",
        (PlayStation | SteamDeck, Button::RightTrigger) => "pindu-glyph-r2-symbolic",
        (Nintendo, Button::LeftTrigger) => "pindu-glyph-zl-symbolic",
        (Nintendo, Button::RightTrigger) => "pindu-glyph-zr-symbolic",
        (Nintendo, Button::Select) => "pindu-glyph-minus-symbolic",
        (Nintendo, Button::Start) => "pindu-glyph-plus-symbolic",
        (_, Button::Select) => "pindu-glyph-view-symbolic",
        (_, Button::Start) => "pindu-glyph-menu-symbolic",
        (_, Button::Guide) => "pindu-glyph-home-symbolic",
        (_, Button::DpadUp) => "pindu-glyph-dpad-up-symbolic",
        (_, Button::DpadDown) => "pindu-glyph-dpad-down-symbolic",
        (_, Button::DpadLeft) => "pindu-glyph-dpad-left-symbolic",
        (_, Button::DpadRight) => "pindu-glyph-dpad-right-symbolic",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn context(focus: Focus) -> Context {
        Context {
            focus,
            dialog: false,
            page_below: false,
            search: false,
            tabs: false,
            options: false,
            menu: false,
            panel: None,
            launching: Launching::No,
        }
    }

    #[test]
    fn fullscreen_pages_open_the_menu_with_start_and_a_focused_cover_s_options_with_view() {
        let home = Context {
            search: true,
            options: true,
            menu: true,
            ..context(Focus::Game)
        };
        assert_eq!(
            actions(home, DeviceKind::Xbox),
            [
                (Button::South, Action::Open),
                (Button::North, Action::Search),
                (Button::Select, Action::Options),
                (Button::Start, Action::Menu)
            ]
        );
        let dialog_over_home = Context {
            dialog: true,
            ..home
        };
        assert_eq!(
            actions(dialog_over_home, DeviceKind::Xbox),
            [(Button::South, Action::Open), (Button::East, Action::Close)]
        );
    }

    #[test]
    fn an_open_panel_shows_only_a_and_b_which_closes_it_or_steps_back_a_level() {
        let library = Context {
            page_below: true,
            search: true,
            tabs: true,
            options: true,
            menu: true,
            ..context(Focus::Other)
        };
        let drawer = Context {
            panel: Some(Panel::Drawer(DrawerLevel::Main)),
            ..library
        };
        assert_eq!(
            actions(drawer, DeviceKind::Xbox),
            [
                (Button::South, Action::Select),
                (Button::East, Action::Close)
            ]
        );
        let settings = Context {
            focus: Focus::Toggle,
            panel: Some(Panel::Drawer(DrawerLevel::Settings)),
            ..library
        };
        assert_eq!(
            actions(settings, DeviceKind::Nintendo),
            [
                (Button::East, Action::Toggle),
                (Button::South, Action::Back)
            ]
        );
        let collections = Context {
            focus: Focus::Toggle,
            panel: Some(Panel::Options(OptionsLevel::Collections)),
            ..library
        };
        assert_eq!(
            actions(collections, DeviceKind::Xbox),
            [
                (Button::South, Action::Toggle),
                (Button::East, Action::Back)
            ]
        );
        // A dialog opened from a panel closes first.
        let dialog_over_sheet = Context {
            dialog: true,
            ..collections
        };
        assert_eq!(
            actions(dialog_over_sheet, DeviceKind::Xbox),
            [
                (Button::South, Action::Toggle),
                (Button::East, Action::Close)
            ]
        );
    }

    fn actions(context: Context, kind: DeviceKind) -> Vec<(Button, Action)> {
        hints(context, kind)
            .into_iter()
            .map(|hint| (hint.button, hint.action))
            .collect()
    }

    #[test]
    fn the_fullscreen_library_switches_tabs_with_the_shoulders_and_sorts_with_the_triggers() {
        let library = Context {
            page_below: true,
            search: true,
            tabs: true,
            ..context(Focus::Game)
        };
        let shown: Vec<_> = hints(library, DeviceKind::Xbox)
            .into_iter()
            .map(|hint| (hint.button, hint.pair, hint.action))
            .collect();
        assert_eq!(
            shown,
            [
                (Button::South, None, Action::Open),
                (Button::East, None, Action::Back),
                (Button::North, None, Action::Search),
                (
                    Button::LeftShoulder,
                    Some(Button::RightShoulder),
                    Action::Tabs
                ),
                (
                    Button::LeftTrigger,
                    Some(Button::RightTrigger),
                    Action::Sort
                ),
            ]
        );
    }

    #[test]
    fn an_open_dialog_keeps_the_shoulders_and_triggers_from_the_library() {
        let dialog_over_library = Context {
            dialog: true,
            tabs: true,
            ..context(Focus::Other)
        };
        assert_eq!(
            actions(dialog_over_library, DeviceKind::Xbox),
            [
                (Button::South, Action::Select),
                (Button::East, Action::Close)
            ]
        );
    }

    #[test]
    fn a_focused_cover_on_the_first_page_only_opens() {
        assert_eq!(
            actions(context(Focus::Game), DeviceKind::Xbox),
            [(Button::South, Action::Open)]
        );
    }

    #[test]
    fn a_page_with_one_below_goes_back() {
        let game_page = Context {
            page_below: true,
            ..context(Focus::Play)
        };
        assert_eq!(
            actions(game_page, DeviceKind::Xbox),
            [(Button::South, Action::Play), (Button::East, Action::Back)]
        );
    }

    #[test]
    fn an_open_dialog_closes_before_the_page_goes_back() {
        let preferences = Context {
            dialog: true,
            page_below: true,
            ..context(Focus::Toggle)
        };
        assert_eq!(
            actions(preferences, DeviceKind::Xbox),
            [
                (Button::South, Action::Toggle),
                (Button::East, Action::Close)
            ]
        );
    }

    #[test]
    fn nintendo_pads_confirm_on_the_east_button_and_go_back_on_the_south() {
        let game_page = Context {
            page_below: true,
            ..context(Focus::Play)
        };
        assert_eq!(
            actions(game_page, DeviceKind::Nintendo),
            [(Button::East, Action::Play), (Button::South, Action::Back)]
        );
    }

    #[test]
    fn home_and_the_library_search_with_the_north_button() {
        let home = Context {
            search: true,
            ..context(Focus::Game)
        };
        assert_eq!(
            actions(home, DeviceKind::Xbox),
            [
                (Button::South, Action::Open),
                (Button::North, Action::Search)
            ]
        );
        let library = Context {
            page_below: true,
            ..home
        };
        assert_eq!(
            actions(library, DeviceKind::Nintendo),
            [
                (Button::East, Action::Open),
                (Button::South, Action::Back),
                (Button::North, Action::Search)
            ]
        );
    }

    #[test]
    fn an_open_dialog_keeps_y_from_searching() {
        let preferences_over_home = Context {
            dialog: true,
            search: true,
            ..context(Focus::Toggle)
        };
        assert_eq!(
            actions(preferences_over_home, DeviceKind::Xbox),
            [
                (Button::South, Action::Toggle),
                (Button::East, Action::Close)
            ]
        );
    }

    #[test]
    fn the_game_page_opens_its_options_with_view() {
        let game_page = Context {
            page_below: true,
            options: true,
            ..context(Focus::Play)
        };
        assert_eq!(
            actions(game_page, DeviceKind::Xbox),
            [
                (Button::South, Action::Play),
                (Button::East, Action::Back),
                (Button::Select, Action::Options)
            ]
        );
        let dialog_over_game_page = Context {
            dialog: true,
            ..game_page
        };
        assert_eq!(
            actions(dialog_over_game_page, DeviceKind::Xbox),
            [(Button::South, Action::Play), (Button::East, Action::Close)]
        );
    }

    #[test]
    fn a_game_we_start_cancels_with_a_held_b_and_nothing_else() {
        let starting = Context {
            page_below: true,
            options: true,
            launching: Launching::Starting,
            ..context(Focus::Play)
        };
        assert_eq!(
            actions(starting, DeviceKind::Xbox),
            [(Button::East, Action::HoldToCancel)]
        );
        assert_eq!(
            actions(starting, DeviceKind::Nintendo),
            [(Button::South, Action::HoldToCancel)]
        );
    }

    #[test]
    fn a_store_hand_off_has_no_hints() {
        let opening = Context {
            page_below: true,
            options: true,
            launching: Launching::HandingOff,
            ..context(Focus::Play)
        };
        assert_eq!(actions(opening, DeviceKind::Xbox), []);
    }

    #[test]
    fn glyphs_show_each_layout_s_label_at_the_button_s_position() {
        let cases = [
            (
                DeviceKind::Xbox,
                Button::South,
                "pindu-glyph-south-a-symbolic",
            ),
            (
                DeviceKind::Nintendo,
                Button::East,
                "pindu-glyph-east-a-symbolic",
            ),
            (
                DeviceKind::Nintendo,
                Button::North,
                "pindu-glyph-north-x-symbolic",
            ),
            (
                DeviceKind::PlayStation,
                Button::South,
                "pindu-glyph-south-cross-symbolic",
            ),
            (
                DeviceKind::SteamDeck,
                Button::LeftShoulder,
                "pindu-glyph-l1-symbolic",
            ),
            (
                DeviceKind::Generic,
                Button::West,
                "pindu-glyph-west-x-symbolic",
            ),
        ];
        for (kind, button, icon) in cases {
            assert_eq!(glyph(kind, button), icon, "{kind:?} {button:?}");
        }
    }

    #[test]
    fn every_glyph_is_an_icon_in_the_app_resources() {
        let data = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data");
        let resources = std::fs::read_to_string(data.join("resources.gresource.xml")).unwrap();
        let kinds = [
            DeviceKind::Xbox,
            DeviceKind::PlayStation,
            DeviceKind::Nintendo,
            DeviceKind::SteamDeck,
            DeviceKind::Generic,
        ];
        for kind in kinds {
            for button in crate::gamepad::BUTTONS {
                let file = format!("icons/scalable/actions/{}.svg", glyph(kind, button));
                assert!(data.join(&file).is_file(), "{file} is missing");
                assert!(resources.contains(&file), "{file} is not a resource");
            }
        }
    }
}
