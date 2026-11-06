//! Quick Access's Controls: every fullscreen intent a controller asks for, grouped, with
//! the button that asks for it on the active controller's [`Layout`] (after any confirm
//! and back swap) and the keys that stand in for it. Both come from the mappings the
//! input itself goes through (`gamepad`'s buttons and holds, `keys`' shortcuts), so the
//! list cannot drift from them.

use crate::gamepad::{self, Button, Intent, Layout};
use crate::keys::{self, Chord};

/// A group of the list, in the order it shows them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    /// Moving focus, selecting and going back.
    Moving,
    /// Searching, the library's tabs and its sort order.
    Library,
    /// The menu drawer, Quick Access and a game's options.
    Panels,
    /// What works while a game starts: holding B to cancel it.
    Launching,
}

impl Group {
    pub const ALL: [Self; 4] = [Self::Moving, Self::Library, Self::Panels, Self::Launching];
}

/// One row of the list: an intent, the button that asks for it and the keys that stand in
/// for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Control {
    pub intent: Intent,
    /// The button on the active controller; `None` without one.
    pub button: Option<Button>,
    /// Whether the button is held down rather than pressed.
    pub held: bool,
    pub keys: Vec<Chord>,
}

/// A group with its rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub group: Group,
    pub controls: Vec<Control>,
}

/// The list for a controller with `layout`, or for the keyboard alone without one, when
/// it leaves out what only a controller does. Groups without rows are left out.
pub fn controls(layout: Option<Layout>) -> Vec<Section> {
    let rows: Vec<(Group, Control)> = gamepad::intents()
        .into_iter()
        .map(|intent| {
            let control = Control {
                intent,
                button: layout.map(|layout| layout.button(intent)),
                held: intent.held_from().is_some(),
                keys: keys::stand_ins(intent),
            };
            (group(intent), control)
        })
        .filter(|(_, control)| control.button.is_some() || !control.keys.is_empty())
        .collect();
    Group::ALL
        .into_iter()
        .map(|group| Section {
            group,
            controls: rows
                .iter()
                .filter(|(of, _)| *of == group)
                .map(|(_, control)| control.clone())
                .collect(),
        })
        .filter(|section| !section.controls.is_empty())
        .collect()
}

/// The group an intent shows in.
fn group(intent: Intent) -> Group {
    match intent {
        Intent::Navigate(_) | Intent::Activate | Intent::Back => Group::Moving,
        Intent::Search
        | Intent::PreviousTab
        | Intent::NextTab
        | Intent::PreviousSort
        | Intent::NextSort => Group::Library,
        Intent::Menu | Intent::Home | Intent::QuickAccess | Intent::Options => Group::Panels,
        Intent::BackHeld => Group::Launching,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::{
        ControllerSettings, DeviceId, DeviceKind, Direction, GamepadEvent, HOLD_DURATION, Intents,
        Pad, Prompts, Swap,
    };
    use std::time::Duration;

    const PAD: DeviceId = DeviceId(1);

    fn listed(sections: &[Section]) -> Vec<Intent> {
        sections
            .iter()
            .flat_map(|section| &section.controls)
            .map(|control| control.intent)
            .collect()
    }

    fn control(sections: &[Section], intent: Intent) -> Option<&Control> {
        sections
            .iter()
            .flat_map(|section| &section.controls)
            .find(|control| control.intent == intent)
    }

    fn button(intents: &mut Intents, button: Button, pressed: bool) -> Option<Intent> {
        let event = GamepadEvent::Button {
            device: PAD,
            button,
            pressed,
        };
        intents.handle(&event, Duration::ZERO)
    }

    /// Every intent the gamepad service asks for on a pad of `kind`: each button pressed
    /// and released, each held past [`HOLD_DURATION`], and the stick pushed each way.
    fn emitted(kind: DeviceKind) -> Vec<Intent> {
        let mut intents = Intents::new();
        let pad = Pad {
            name: String::new(),
            kind,
            ids: None,
            rumble: false,
        };
        intents.handle(
            &GamepadEvent::Connected { device: PAD, pad },
            Duration::ZERO,
        );
        let mut emitted = Vec::new();
        for pressed in gamepad::BUTTONS {
            emitted.extend(button(&mut intents, pressed, true));
            emitted.extend(intents.repeat(HOLD_DURATION));
            button(&mut intents, pressed, false);
        }
        for (axis, value) in [
            (gamepad::Axis::LeftX, -1.0),
            (gamepad::Axis::LeftX, 1.0),
            (gamepad::Axis::LeftY, -1.0),
            (gamepad::Axis::LeftY, 1.0),
        ] {
            for value in [value, 0.0] {
                let event = GamepadEvent::Axis {
                    device: PAD,
                    axis,
                    value,
                };
                emitted.extend(intents.handle(&event, Duration::ZERO));
            }
        }
        emitted
    }

    #[test]
    fn every_intent_the_gamepad_service_asks_for_is_listed() {
        for kind in DeviceKind::ALL {
            let shown = controls(Some(Layout::detected(kind)));
            let listed = listed(&shown);
            let emitted = emitted(kind);
            assert!(emitted.contains(&Intent::BackHeld), "{kind:?}");
            for intent in emitted {
                assert!(listed.contains(&intent), "{kind:?}: {intent:?} is missing");
            }
        }
    }

    #[test]
    fn each_intent_is_listed_once() {
        let listed = listed(&controls(Some(Layout::detected(DeviceKind::Xbox))));
        for (index, intent) in listed.iter().enumerate() {
            assert!(!listed[..index].contains(intent), "{intent:?} twice");
        }
    }

    #[test]
    fn holding_back_to_cancel_a_launch_is_listed_under_launching() {
        let shown = controls(Some(Layout::detected(DeviceKind::Xbox)));
        let launching = shown
            .iter()
            .find(|section| section.group == Group::Launching)
            .expect("a Launching group");

        assert_eq!(
            launching.controls,
            [Control {
                intent: Intent::BackHeld,
                button: Some(Button::East),
                held: true,
                keys: Vec::new(),
            }]
        );
    }

    #[test]
    fn groups_come_moving_library_panels_launching() {
        let shown = controls(Some(Layout::detected(DeviceKind::Xbox)));
        let groups: Vec<Group> = shown.iter().map(|section| section.group).collect();

        assert_eq!(groups, Group::ALL);
        let moving = &shown[0].controls;
        assert!(
            moving
                .iter()
                .any(|c| c.intent == Intent::Navigate(Direction::Up))
        );
        assert!(moving.iter().any(|c| c.intent == Intent::Activate));
        assert!(moving.iter().any(|c| c.intent == Intent::Back));
        assert!(
            shown[1]
                .controls
                .iter()
                .any(|c| c.intent == Intent::NextSort)
        );
        assert!(
            shown[2]
                .controls
                .iter()
                .any(|c| c.intent == Intent::QuickAccess)
        );
    }

    #[test]
    fn a_nintendo_layout_pad_confirms_on_the_east_and_goes_back_on_the_south() {
        let shown = controls(Some(Layout::detected(DeviceKind::Nintendo)));

        let button = |intent| control(&shown, intent).and_then(|control| control.button);
        assert_eq!(button(Intent::Activate), Some(Button::East));
        assert_eq!(button(Intent::Back), Some(Button::South));
        assert_eq!(button(Intent::BackHeld), Some(Button::South));
    }

    #[test]
    fn turning_the_swap_off_puts_confirm_back_on_the_south() {
        let settings = ControllerSettings {
            prompts: Prompts::Auto,
            swap: Swap::Off,
        };
        let shown = controls(Some(Layout::resolve(DeviceKind::Nintendo, settings)));

        let button = |intent| control(&shown, intent).and_then(|control| control.button);
        assert_eq!(button(Intent::Activate), Some(Button::South));
        assert_eq!(button(Intent::Back), Some(Button::East));
    }

    #[test]
    fn each_row_carries_the_keys_that_stand_in_for_it() {
        let shown = controls(Some(Layout::detected(DeviceKind::Xbox)));

        for intent in listed(&shown) {
            let keys = &control(&shown, intent).expect("listed").keys;
            assert_eq!(*keys, keys::stand_ins(intent), "{intent:?}");
        }
        let options = control(&shown, Intent::Options).expect("Options");
        assert_eq!(options.button, Some(Button::Select));
        assert_eq!(options.keys, [Chord::plain("Menu"), Chord::shift("F10")]);
    }

    #[test]
    fn without_a_controller_only_what_the_keyboard_does_shows_without_buttons() {
        let shown = controls(None);
        let listed = listed(&shown);

        assert!(!listed.contains(&Intent::BackHeld));
        assert!(!listed.contains(&Intent::Home), "only Guide asks for it");
        assert!(listed.contains(&Intent::Back));
        assert!(listed.contains(&Intent::QuickAccess));
        assert!(
            !shown
                .iter()
                .any(|section| section.group == Group::Launching),
            "an empty group is left out"
        );
        for section in &shown {
            for control in &section.controls {
                assert_eq!(control.button, None, "{:?}", control.intent);
                assert!(!control.keys.is_empty(), "{:?}", control.intent);
            }
        }
    }
}
