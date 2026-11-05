//! Fullscreen mode's launch overlay and the way back to the game's page. Play shows an
//! overlay that matches the route: the launcher's name for a store game we hand off, and
//! "Starting…", which a held B cancels, for a game we run. It goes once the game or its
//! launcher comes to the front, the game ends, a dialog covers it, or after a while. When
//! the window has focus again (or the game ends while it has focus), the shell returns to
//! the game's page with focus on Play, or on Stop while the game still runs.

use std::time::Duration;

use crate::hints::Launching;
use crate::launch::{self, Route};
use crate::model::{Game, GameId, LaunchKind};

/// How long "Opening {launcher}…" stays after the hand-off when the launcher does not
/// come to the front.
pub const HAND_OFF_LINGER: Duration = Duration::from_secs(3);
/// How long "Starting…" stays after the game started when its window does not come to
/// the front.
pub const START_LINGER: Duration = Duration::from_secs(10);

/// How Play starts a game, as the overlay tells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch {
    /// Handed to its launcher (a store game), which cannot be called back.
    HandOff,
    /// Run by us, which a held B cancels.
    Run,
}

impl Launch {
    /// How Play starts `game`, or `None` when it cannot start it and shows no overlay.
    pub fn of(game: &Game) -> Option<Self> {
        match game.launch_kind {
            // A store game's URI needs no launch config.
            LaunchKind::StoreUri => matches!(launch::route(game, None), Some(Route::StoreUri(_)))
                .then_some(Self::HandOff),
            LaunchKind::HostCommand | LaunchKind::DesktopEntry | LaunchKind::Proton => {
                Some(Self::Run)
            }
            LaunchKind::RuntimeNative => None,
        }
    }
}

/// Something that happens to a launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// Play was pressed for the game.
    Play(GameId, Launch),
    /// The launch reported back (its toast): handed off, started, or failed.
    Reported,
    /// A game we run started or ended (its Playing state).
    Playing(GameId, bool),
    /// The window gained or lost focus to the game or its launcher.
    Focus(bool),
    /// The overlay has been up for its linger time.
    Lingered,
    /// A dialog covers the page.
    Interrupted,
    /// B was held down.
    CancelHeld,
}

/// What the shell does after an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Sends [`Event::Lingered`] after this long.
    Linger(Duration),
    /// Stops the game.
    Stop(GameId),
    /// Shows the game's page with focus on its Play (or Stop) button.
    Return(GameId),
}

/// The launch under way and the return to its page.
#[derive(Debug, Default)]
pub struct LaunchOverlay {
    pending: Option<Pending>,
    shown: Launching,
}

#[derive(Debug)]
struct Pending {
    game: GameId,
    launch: Launch,
    /// Its Playing state is on.
    running: bool,
    /// The window lost focus since Play.
    away: bool,
}

impl LaunchOverlay {
    /// What the overlay shows.
    pub fn shown(&self) -> Launching {
        self.shown
    }

    /// The game the overlay or the return is for.
    pub fn game(&self) -> Option<&GameId> {
        self.pending.as_ref().map(|pending| &pending.game)
    }

    /// Follows `event`, and says what the shell does next.
    pub fn handle(&mut self, event: Event) -> Option<Effect> {
        if let Event::Play(game, launch) = event {
            self.shown = match launch {
                Launch::HandOff => Launching::HandingOff,
                Launch::Run => Launching::Starting,
            };
            self.pending = Some(Pending {
                game,
                launch,
                running: false,
                away: false,
            });
            return None;
        }
        let pending = self.pending.as_mut()?;
        match event {
            Event::Play(..) => unreachable!("handled above"),
            Event::Reported if self.shown == Launching::No => None,
            // A game we run that is not running after its launch did not start.
            Event::Reported if pending.launch == Launch::Run && !pending.running => {
                self.finish();
                None
            }
            Event::Reported => Some(Effect::Linger(match pending.launch {
                Launch::HandOff => HAND_OFF_LINGER,
                Launch::Run => START_LINGER,
            })),
            Event::Playing(game, _) if game != pending.game => None,
            Event::Playing(_, true) => {
                pending.running = true;
                None
            }
            Event::Playing(_, false) => {
                pending.running = false;
                self.shown = Launching::No;
                if pending.away {
                    // The return waits for the window to have focus again.
                    return None;
                }
                let game = pending.game.clone();
                self.finish();
                Some(Effect::Return(game))
            }
            Event::Focus(false) => {
                pending.away = true;
                self.shown = Launching::No;
                None
            }
            Event::Focus(true) if !pending.away => None,
            Event::Focus(true) => {
                pending.away = false;
                let game = pending.game.clone();
                // A running game is returned from each time; anything else once.
                if !pending.running {
                    self.finish();
                }
                Some(Effect::Return(game))
            }
            Event::Lingered => {
                self.shown = Launching::No;
                // A launcher that never came up is no longer waited for.
                if pending.launch == Launch::HandOff && !pending.away {
                    self.finish();
                }
                None
            }
            Event::Interrupted => {
                self.shown = Launching::No;
                None
            }
            Event::CancelHeld if self.shown == Launching::Starting => {
                self.shown = Launching::No;
                Some(Effect::Stop(pending.game.clone()))
            }
            Event::CancelHeld => None,
        }
    }

    fn finish(&mut self) {
        self.pending = None;
        self.shown = Launching::No;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;

    fn id() -> GameId {
        GameId::new(Source::Manual, "celeste")
    }

    fn game(source: Source, kind: LaunchKind) -> Game {
        Game::new(GameId::new(source, "1"), "Celeste", kind, 0)
    }

    #[test]
    fn store_games_are_handed_off_and_games_we_start_are_run() {
        assert_eq!(
            Launch::of(&game(Source::Steam, LaunchKind::StoreUri)),
            Some(Launch::HandOff)
        );
        for kind in [
            LaunchKind::HostCommand,
            LaunchKind::DesktopEntry,
            LaunchKind::Proton,
        ] {
            assert_eq!(
                Launch::of(&game(Source::Manual, kind)),
                Some(Launch::Run),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn games_that_cannot_start_show_no_overlay() {
        assert_eq!(
            Launch::of(&game(Source::Manual, LaunchKind::RuntimeNative)),
            None
        );
        // A store game with no URI of its own and no launcher to build one.
        assert_eq!(
            Launch::of(&game(Source::Desktop, LaunchKind::StoreUri)),
            None
        );
    }

    #[test]
    fn a_hand_off_shows_until_the_launcher_comes_up_then_returns_once() {
        let mut overlay = LaunchOverlay::default();

        assert_eq!(overlay.handle(Event::Play(id(), Launch::HandOff)), None);
        assert_eq!(overlay.shown(), Launching::HandingOff);
        assert_eq!(
            overlay.handle(Event::Reported),
            Some(Effect::Linger(HAND_OFF_LINGER))
        );
        assert_eq!(overlay.shown(), Launching::HandingOff);

        assert_eq!(overlay.handle(Event::Focus(false)), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Focus(true)),
            Some(Effect::Return(id()))
        );

        assert_eq!(overlay.handle(Event::Focus(false)), None);
        assert_eq!(overlay.handle(Event::Focus(true)), None);
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn a_hand_off_the_launcher_never_takes_over_goes_after_lingering() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::HandOff));
        overlay.handle(Event::Reported);

        assert_eq!(overlay.handle(Event::Lingered), None);
        assert_eq!(overlay.shown(), Launching::No);
        // Coming back from somewhere else later is not a return from the game.
        overlay.handle(Event::Focus(false));
        assert_eq!(overlay.handle(Event::Focus(true)), None);
    }

    #[test]
    fn a_game_we_run_returns_to_its_page_when_it_ends() {
        let mut overlay = LaunchOverlay::default();

        overlay.handle(Event::Play(id(), Launch::Run));
        assert_eq!(overlay.shown(), Launching::Starting);
        assert_eq!(overlay.handle(Event::Playing(id(), true)), None);
        assert_eq!(
            overlay.handle(Event::Reported),
            Some(Effect::Linger(START_LINGER))
        );
        assert_eq!(overlay.shown(), Launching::Starting);

        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn a_game_in_front_returns_to_its_page_each_time_the_window_has_focus_again() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));
        overlay.handle(Event::Reported);

        assert_eq!(overlay.handle(Event::Focus(false)), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Focus(true)),
            Some(Effect::Return(id()))
        );
        assert_eq!(overlay.handle(Event::Focus(false)), None);
        assert_eq!(
            overlay.handle(Event::Focus(true)),
            Some(Effect::Return(id()))
        );

        // It ends while another window has focus: the return waits for ours.
        overlay.handle(Event::Focus(false));
        assert_eq!(overlay.handle(Event::Playing(id(), false)), None);
        assert_eq!(
            overlay.handle(Event::Focus(true)),
            Some(Effect::Return(id()))
        );
        overlay.handle(Event::Focus(false));
        assert_eq!(overlay.handle(Event::Focus(true)), None);
    }

    #[test]
    fn a_game_we_run_that_did_not_start_drops_the_overlay() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));

        assert_eq!(overlay.handle(Event::Reported), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn a_held_b_stops_a_game_we_start_and_returns_when_it_ends() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));

        assert_eq!(overlay.handle(Event::CancelHeld), Some(Effect::Stop(id())));
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
    }

    #[test]
    fn a_hand_off_cannot_be_cancelled() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::HandOff));

        assert_eq!(overlay.handle(Event::CancelHeld), None);
        assert_eq!(overlay.shown(), Launching::HandingOff);
    }

    #[test]
    fn other_games_starting_and_ending_change_nothing() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        let other = GameId::new(Source::Manual, "other");

        assert_eq!(overlay.handle(Event::Playing(other.clone(), true)), None);
        assert_eq!(overlay.handle(Event::Playing(other, false)), None);
        assert_eq!(overlay.shown(), Launching::Starting);
    }

    #[test]
    fn a_dialog_covers_the_overlay_but_keeps_the_return() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));

        assert_eq!(overlay.handle(Event::Interrupted), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), Some(&id()));
        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
    }

    #[test]
    fn nothing_shows_before_play() {
        let mut overlay = LaunchOverlay::default();

        for event in [
            Event::Reported,
            Event::Focus(false),
            Event::Focus(true),
            Event::Lingered,
            Event::CancelHeld,
        ] {
            assert_eq!(overlay.handle(event), None);
        }
        assert_eq!(overlay.shown(), Launching::No);
    }
}
