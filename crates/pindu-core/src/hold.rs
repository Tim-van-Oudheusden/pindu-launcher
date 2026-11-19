//! Hold to confirm in fullscreen mode: with the option on, the confirm button activates
//! Power Off, Restart, Forget and Stop only once held down for [`HOLD_DURATION`], with the
//! hold's progress shown meanwhile. Letting go early, or any other button, ends the hold
//! with nothing done. The keyboard and the pointer act at once, as before.

use std::time::Duration;

pub use crate::gamepad::HOLD_DURATION;
use crate::gamepad::Intent;

/// The `fullscreen-hold-to-confirm` setting: whether the destructive actions need a hold.
pub const HOLD_TO_CONFIRM_KEY: &str = "fullscreen-hold-to-confirm";

/// An action that needs a hold while the option is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Destructive {
    /// Power Off, in the menu drawer's Power.
    PowerOff,
    /// Restart, in the menu drawer's Power.
    Restart,
    /// Forget a network or a Bluetooth device, in Quick Access.
    Forget,
    /// Stop a running game.
    Stop,
}

/// The actions that need a hold, by the name of the action their widgets activate.
const ACTIONS: [(&str, Destructive); 6] = [
    ("win.power-off", Destructive::PowerOff),
    ("win.power-restart", Destructive::Restart),
    ("win.network-forget", Destructive::Forget),
    ("win.bluetooth-forget", Destructive::Forget),
    ("app.stop", Destructive::Stop),
    ("win.playing-stop", Destructive::Stop),
];

impl Destructive {
    /// The destructive action a widget activating `action` (such as `win.power-off`) does,
    /// if it does one.
    pub fn of_action(action: &str) -> Option<Self> {
        ACTIONS
            .into_iter()
            .find(|&(name, _)| name == action)
            .map(|(_, destructive)| destructive)
    }
}

/// What an intent does under [`HoldToConfirm`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Act on the intent as usual; a hold under way has ended.
    Act,
    /// Show the hold's progress on the focused widget and do nothing else yet.
    Hold(Destructive),
}

/// Where a hold stands, from [`HoldToConfirm::tick`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tick {
    /// No hold is under way.
    Idle,
    /// The hold is under way, this far along, from 0 to 1.
    Holding(f64),
    /// The button was held long enough: do the action.
    Confirmed(Destructive),
    /// The button was let go early: the hold ended with nothing done.
    LetGo,
}

/// Follows a hold to confirm from the confirm button's press until it completes or ends.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HoldToConfirm {
    holding: Option<Destructive>,
}

impl HoldToConfirm {
    /// What `intent` does, with `focus` the destructive action of the focused widget, if
    /// it does one, and `enabled` whether the option is on.
    pub fn intent(&mut self, intent: Intent, focus: Option<Destructive>, enabled: bool) -> Press {
        self.holding = (intent == Intent::Activate && enabled)
            .then_some(focus)
            .flatten();
        self.holding.map_or(Press::Act, Press::Hold)
    }

    /// Where the hold stands, with `held_for` how long the confirm button has been held
    /// down ([`Intents::held_for`](crate::gamepad::Intents::held_for)), `None` once let go.
    pub fn tick(&mut self, held_for: Option<Duration>) -> Tick {
        let Some(destructive) = self.holding else {
            return Tick::Idle;
        };
        let Some(held_for) = held_for else {
            self.holding = None;
            return Tick::LetGo;
        };
        if held_for >= HOLD_DURATION {
            self.holding = None;
            return Tick::Confirmed(destructive);
        }
        Tick::Holding(held_for.as_secs_f64() / HOLD_DURATION.as_secs_f64())
    }

    /// The action whose hold is under way, if one is.
    pub fn holding(&self) -> Option<Destructive> {
        self.holding
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Direction;

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn the_destructive_actions_are_found_by_their_action_names() {
        assert_eq!(
            Destructive::of_action("win.power-off"),
            Some(Destructive::PowerOff)
        );
        assert_eq!(
            Destructive::of_action("win.power-restart"),
            Some(Destructive::Restart)
        );
        for forget in ["win.network-forget", "win.bluetooth-forget"] {
            assert_eq!(Destructive::of_action(forget), Some(Destructive::Forget));
        }
        for stop in ["app.stop", "win.playing-stop"] {
            assert_eq!(Destructive::of_action(stop), Some(Destructive::Stop));
        }
        for other in ["app.play", "win.power-suspend", "win.network-join", ""] {
            assert_eq!(Destructive::of_action(other), None, "{other}");
        }
    }

    #[test]
    fn with_the_option_off_a_destructive_action_acts_at_once() {
        let mut hold = HoldToConfirm::default();

        let press = hold.intent(Intent::Activate, Some(Destructive::PowerOff), false);

        assert_eq!(press, Press::Act);
        assert_eq!(hold.holding(), None);
    }

    #[test]
    fn with_the_option_on_the_confirm_button_on_a_destructive_action_starts_a_hold() {
        let mut hold = HoldToConfirm::default();

        let press = hold.intent(Intent::Activate, Some(Destructive::Stop), true);

        assert_eq!(press, Press::Hold(Destructive::Stop));
        assert_eq!(hold.holding(), Some(Destructive::Stop));
    }

    #[test]
    fn with_the_option_on_anything_else_acts_at_once() {
        let mut hold = HoldToConfirm::default();

        assert_eq!(hold.intent(Intent::Activate, None, true), Press::Act);
        assert_eq!(
            hold.intent(Intent::Back, Some(Destructive::Forget), true),
            Press::Act
        );
        assert_eq!(hold.holding(), None);
    }

    #[test]
    fn a_hold_shows_its_progress_and_confirms_once_the_button_is_held_long_enough() {
        let mut hold = HoldToConfirm::default();
        hold.intent(Intent::Activate, Some(Destructive::PowerOff), true);

        assert_eq!(hold.tick(Some(ms(0))), Tick::Holding(0.0));
        assert_eq!(hold.tick(Some(HOLD_DURATION / 4)), Tick::Holding(0.25));
        assert_eq!(
            hold.tick(Some(HOLD_DURATION)),
            Tick::Confirmed(Destructive::PowerOff)
        );
        assert_eq!(hold.holding(), None);
        assert_eq!(hold.tick(Some(HOLD_DURATION * 2)), Tick::Idle);
    }

    #[test]
    fn letting_go_early_ends_the_hold_with_nothing_done() {
        let mut hold = HoldToConfirm::default();
        hold.intent(Intent::Activate, Some(Destructive::Restart), true);

        assert_eq!(hold.tick(Some(ms(500))), Tick::Holding(0.5));
        assert_eq!(hold.tick(None), Tick::LetGo);
        assert_eq!(hold.holding(), None);
        assert_eq!(hold.tick(Some(HOLD_DURATION)), Tick::Idle);
    }

    #[test]
    fn any_other_intent_ends_the_hold() {
        let mut hold = HoldToConfirm::default();
        for intent in [
            Intent::Navigate(Direction::Down),
            Intent::Back,
            Intent::Menu,
        ] {
            hold.intent(Intent::Activate, Some(Destructive::Forget), true);

            assert_eq!(hold.intent(intent, None, true), Press::Act, "{intent:?}");
            assert_eq!(hold.tick(Some(HOLD_DURATION)), Tick::Idle, "{intent:?}");
        }
    }
}
