//! Who the controller belongs to once the launcher has started a game. Window focus alone
//! cannot say (#130): in a nested gamescope the launcher's window keeps focus there while
//! the game plays on the desktop, and on a desktop a game that "is ready" may never take
//! focus. So once a game starts, the controller is the game's, and the launcher acts on
//! nothing but Guide until it gets the controller back.

use crate::gamepad::Intent;

/// Whether the controller is the game's or the launcher's, as games start and end and the
/// launcher's window loses and regains focus.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GameControl {
    /// The game has the controller.
    game_has_it: bool,
    /// A store game has it, whose end the launcher never sees.
    store: bool,
    /// How many games the launcher runs and tracks.
    running: usize,
    /// The launcher's window lost focus since the game took the controller.
    left: bool,
}

impl GameControl {
    /// The games the launcher runs and tracks changed: `running` says how many run now. A
    /// game that started takes the controller, and the last one ending gives it back
    /// unless a store game has it. `focused` is whether the launcher's window has focus.
    pub fn sessions_changed(&mut self, running: usize, focused: bool) {
        if running > self.running {
            self.game_has_it = true;
            self.left = !focused;
        } else if running == 0 && !self.store {
            self.game_has_it = false;
        }
        self.running = running;
    }

    /// The launcher handed a game to its store's launcher, which runs it out of the
    /// launcher's sight. `focused` is whether the launcher's window has focus.
    pub fn store_launch_opened(&mut self, focused: bool) {
        self.game_has_it = true;
        self.store = true;
        self.left = !focused;
    }

    /// Whether the launcher acts on `intent` from a controller: on every one while it has
    /// the controller, and on Guide alone while the game has it.
    pub fn acts_on(self, intent: Intent) -> bool {
        !self.game_has_it || intent == Intent::Home
    }

    /// The user called the launcher back, with Guide or the keyboard: the controller is
    /// its again.
    pub fn give_back(&mut self) {
        self.game_has_it = false;
        self.store = false;
    }

    /// The launcher's window gained (`focused`) or lost focus. Focus that comes back after
    /// the window lost it means the user went back to the launcher: the controller is its
    /// again.
    pub fn focus_changed(&mut self, focused: bool) {
        if !self.game_has_it {
            return;
        }
        if !focused {
            self.left = true;
        } else if self.left {
            self.give_back();
        }
    }

    /// Whether the game has the controller, so the launcher waits for Guide.
    pub fn game_has_controller(self) -> bool {
        self.game_has_it
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Direction;

    #[test]
    fn a_game_the_launcher_starts_takes_the_controller() {
        let mut control = GameControl::default();

        control.sessions_changed(1, true);

        assert!(!control.acts_on(Intent::Activate));
        assert!(!control.acts_on(Intent::Navigate(Direction::Up)));
        assert!(!control.acts_on(Intent::Menu));
    }

    #[test]
    fn guide_still_reaches_the_launcher_while_the_game_has_the_controller() {
        let mut control = GameControl::default();
        control.sessions_changed(1, true);

        assert!(control.acts_on(Intent::Home));
    }

    #[test]
    fn without_a_game_the_launcher_acts_on_every_button() {
        let control = GameControl::default();

        assert!(control.acts_on(Intent::Activate));
        assert!(control.acts_on(Intent::Home));
    }

    #[test]
    fn calling_the_launcher_back_gives_it_the_controller() {
        let mut control = GameControl::default();
        control.sessions_changed(1, true);

        control.give_back();

        assert!(control.acts_on(Intent::Activate));
    }

    #[test]
    fn only_a_game_that_starts_takes_the_controller_back_from_the_launcher() {
        let mut control = GameControl::default();
        control.sessions_changed(1, true);
        control.give_back();

        // The same game still runs; a second one starting takes the controller again.
        control.sessions_changed(1, true);
        assert!(control.acts_on(Intent::Activate));
        control.sessions_changed(2, true);
        assert!(!control.acts_on(Intent::Activate));
    }

    #[test]
    fn the_last_game_ending_gives_the_launcher_the_controller() {
        let mut control = GameControl::default();
        control.sessions_changed(2, true);

        control.sessions_changed(1, true);
        assert!(!control.acts_on(Intent::Activate));
        control.sessions_changed(0, true);
        assert!(control.acts_on(Intent::Activate));
    }

    #[test]
    fn a_game_handed_to_its_store_takes_the_controller() {
        let mut control = GameControl::default();

        control.store_launch_opened(true);

        assert!(!control.acts_on(Intent::Activate));
    }

    #[test]
    fn a_store_game_keeps_the_controller_when_the_games_the_launcher_runs_end() {
        // The launcher never sees a store game end, so only calling the launcher back
        // gives the controller back.
        let mut control = GameControl::default();
        control.store_launch_opened(true);
        control.sessions_changed(1, true);

        control.sessions_changed(0, true);

        assert!(!control.acts_on(Intent::Activate));
    }

    #[test]
    fn the_window_regaining_focus_after_losing_it_gives_the_launcher_the_controller() {
        let mut control = GameControl::default();
        control.store_launch_opened(true);

        // Focus that only says the window still has it hands nothing back.
        control.focus_changed(true);
        assert!(!control.acts_on(Intent::Activate));
        control.focus_changed(false);
        assert!(!control.acts_on(Intent::Activate));
        control.focus_changed(true);
        assert!(control.acts_on(Intent::Activate));
    }

    #[test]
    fn a_game_that_took_focus_before_the_launcher_heard_of_it_gives_the_controller_back_on_refocus()
    {
        // Steam's window can take focus before the portal says the link opened, and a game
        // window before its session is counted.
        for start in [
            |control: &mut GameControl| control.store_launch_opened(false),
            |control: &mut GameControl| control.sessions_changed(1, false),
        ] {
            let mut control = GameControl::default();
            start(&mut control);

            control.focus_changed(true);

            assert!(control.acts_on(Intent::Activate));
        }
    }

    #[test]
    fn says_whether_the_game_has_the_controller() {
        let mut control = GameControl::default();
        assert!(!control.game_has_controller());

        control.store_launch_opened(true);

        assert!(control.game_has_controller());
    }
}
