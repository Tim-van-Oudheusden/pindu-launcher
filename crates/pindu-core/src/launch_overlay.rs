//! Fullscreen mode's starting screen and the way back to the game's page. Play covers the
//! game's page with a starting screen that matches the route: the launcher's name for a
//! store game we hand off, and "Starting…", which a held B cancels, for a game we run.
//! After [`CANCEL_AFTER`] it offers Cancel, which stops a game we run and gives up on a
//! store launch. It goes once the game or its launcher comes to the front, the game ends, a
//! dialog covers it, or after a while, even when a hand-off never reports back. When the
//! window has focus again (or the game ends while it has focus), the shell returns to the
//! game's page with focus on Play, or on Stop while the game still runs.
//!
//! In a `--steam` gamescope the screen follows gamescope instead of the window's focus and
//! timers: it stays until the game is in front, the game ends or the store launch is over,
//! and the return waits for the launcher to be back in front. A Steam client's window
//! shown in the launcher's place leaves the launcher in front, so it does not end it.

use std::time::Duration;

use crate::gamescope::{self, Front};
use crate::hints::Launching;
use crate::launch::{self, Route};
use crate::model::{Game, GameId, LaunchKind};

/// How long "Opening {launcher}…" stays after the hand-off when the launcher does not
/// come to the front.
pub const HAND_OFF_LINGER: Duration = Duration::from_secs(3);
/// How long "Opening {launcher}…" waits for the hand-off to report back. A launcher the
/// desktop cannot reach may never answer (#427), and the overlay takes every button.
pub const HAND_OFF_TIMEOUT: Duration = Duration::from_secs(10);
/// How long "Starting…" stays after the game started when its window does not come to
/// the front.
pub const START_LINGER: Duration = Duration::from_secs(10);
/// How long after Play the starting screen offers Cancel; the shell sends
/// [`Event::CancelDue`] then.
pub const CANCEL_AFTER: Duration = Duration::from_secs(5);

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
            LaunchKind::HostCommand
            | LaunchKind::DesktopEntry
            | LaunchKind::Proton
            | LaunchKind::RuntimeNative => Some(Self::Run),
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
    /// [`CANCEL_AFTER`] has passed since Play.
    CancelDue,
    /// The starting screen's Cancel was activated.
    Cancel,
    /// gamescope shows this in front (in a `--steam` gamescope).
    Front(Front),
    /// The store launch is over, or never began: the launcher no longer wants the game in
    /// front (in a `--steam` gamescope).
    StoreLaunchOver,
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
    /// Gives up on the store launch of the game, bringing the launcher back to the front,
    /// and shows the game's page with focus on its Play button.
    GiveUp(GameId),
}

/// The launch under way and the return to its page.
#[derive(Debug, Default)]
pub struct LaunchOverlay {
    pending: Option<Pending>,
    shown: Launching,
    /// It follows gamescope rather than the window's focus and timers.
    gamescope: bool,
}

#[derive(Debug)]
struct Pending {
    game: GameId,
    launch: Launch,
    /// Its Playing state is on.
    running: bool,
    /// The window lost focus since Play, or in a `--steam` gamescope, the game came to
    /// the front.
    away: bool,
    /// The starting screen offers Cancel.
    cancel: bool,
}

impl LaunchOverlay {
    /// The starting screen in a `--steam` gamescope, which follows who gamescope shows.
    pub fn in_gamescope() -> Self {
        Self {
            gamescope: true,
            ..Self::default()
        }
    }

    /// Whether it follows gamescope rather than the window's focus and timers.
    pub fn follows_gamescope(&self) -> bool {
        self.gamescope
    }

    /// What the starting screen shows, as far as the buttons are concerned.
    pub fn shown(&self) -> Launching {
        match self.pending.as_ref() {
            Some(pending) if pending.cancel && self.shown != Launching::No => Launching::Cancel,
            _ => self.shown,
        }
    }

    /// How the launch on the starting screen starts, while it shows.
    pub fn launch(&self) -> Option<Launch> {
        self.pending
            .as_ref()
            .filter(|_| self.shown != Launching::No)
            .map(|pending| pending.launch)
    }

    /// The game the starting screen or the return is for.
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
                cancel: false,
            });
            // A game we run always reports back, and a held B cancels it; a hand-off
            // that never does is not waited for past its timeout. In a `--steam`
            // gamescope, gamescope and the store launch say when it is through.
            let times_out = launch == Launch::HandOff && !self.gamescope;
            return times_out.then_some(Effect::Linger(HAND_OFF_TIMEOUT));
        }
        let pending = self.pending.as_mut()?;
        let cancel_shown = pending.cancel && self.shown != Launching::No;
        match event {
            Event::Play(..) => unreachable!("handled above"),
            Event::Reported if self.shown == Launching::No => None,
            // A game we run that is not running after its launch did not start.
            Event::Reported if pending.launch == Launch::Run && !pending.running => {
                self.finish();
                None
            }
            Event::Reported if self.gamescope => None,
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
            Event::Focus(_) | Event::Lingered if self.gamescope => None,
            Event::Front(_) | Event::StoreLaunchOver if !self.gamescope => None,
            Event::Focus(false) => {
                pending.away = true;
                self.shown = Launching::No;
                None
            }
            Event::Front(Front::Game(app_id)) if app_id == gamescope::app_id(&pending.game) => {
                pending.away = true;
                self.shown = Launching::No;
                None
            }
            Event::Focus(true) | Event::Front(Front::Launcher) if !pending.away => None,
            Event::Focus(true) | Event::Front(Front::Launcher) => {
                pending.away = false;
                let game = pending.game.clone();
                // A running game is returned from each time; anything else once.
                if !pending.running {
                    self.finish();
                }
                Some(Effect::Return(game))
            }
            // Another game in front.
            Event::Front(Front::Game(_)) => None,
            Event::StoreLaunchOver if pending.launch == Launch::Run => None,
            Event::StoreLaunchOver => {
                let (game, away) = (pending.game.clone(), pending.away);
                self.finish();
                // Once the game was in front, the launcher's return to the front did.
                (!away).then_some(Effect::Return(game))
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
            Event::CancelDue => {
                pending.cancel = true;
                None
            }
            Event::Cancel | Event::CancelHeld if cancel_shown => {
                let game = pending.game.clone();
                match pending.launch {
                    Launch::Run => {
                        self.shown = Launching::No;
                        Some(Effect::Stop(game))
                    }
                    Launch::HandOff => {
                        self.finish();
                        Some(Effect::GiveUp(game))
                    }
                }
            }
            Event::CancelHeld if self.shown == Launching::Starting => {
                self.shown = Launching::No;
                Some(Effect::Stop(pending.game.clone()))
            }
            Event::Cancel | Event::CancelHeld => None,
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
            LaunchKind::RuntimeNative,
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
        // A store game with no URI of its own and no launcher to build one.
        assert_eq!(
            Launch::of(&game(Source::Desktop, LaunchKind::StoreUri)),
            None
        );
    }

    #[test]
    fn a_hand_off_shows_until_the_launcher_comes_up_then_returns_once() {
        let mut overlay = LaunchOverlay::default();

        assert_eq!(
            overlay.handle(Event::Play(id(), Launch::HandOff)),
            Some(Effect::Linger(HAND_OFF_TIMEOUT))
        );
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
    fn a_hand_off_that_never_reports_back_goes_after_its_timeout() {
        let mut overlay = LaunchOverlay::default();

        assert_eq!(
            overlay.handle(Event::Play(id(), Launch::HandOff)),
            Some(Effect::Linger(HAND_OFF_TIMEOUT))
        );
        assert_eq!(overlay.handle(Event::Lingered), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), None);
        // A report that comes after all changes nothing.
        assert_eq!(overlay.handle(Event::Reported), None);
        assert_eq!(overlay.shown(), Launching::No);
    }

    #[test]
    fn a_game_we_run_returns_to_its_page_when_it_ends() {
        let mut overlay = LaunchOverlay::default();

        // A game we run reports back however slowly it starts, so no timeout drops it.
        assert_eq!(overlay.handle(Event::Play(id(), Launch::Run)), None);
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
    fn a_game_we_run_still_returns_after_its_starting_screen_lingers() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));
        overlay.handle(Event::Reported);

        assert_eq!(overlay.handle(Event::Lingered), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), Some(&id()));
        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
    }

    #[test]
    fn a_hand_off_whose_launcher_came_up_still_returns_after_its_timeout() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::HandOff));
        overlay.handle(Event::Focus(false));

        assert_eq!(overlay.handle(Event::Lingered), None);
        assert_eq!(
            overlay.handle(Event::Focus(true)),
            Some(Effect::Return(id()))
        );
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

    #[test]
    fn cancel_shows_after_its_delay_and_stops_a_game_we_run() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        assert_eq!(overlay.handle(Event::Cancel), None);
        assert_eq!(overlay.shown(), Launching::Starting);

        assert_eq!(overlay.handle(Event::CancelDue), None);
        assert_eq!(overlay.shown(), Launching::Cancel);
        assert_eq!(overlay.launch(), Some(Launch::Run));
        assert_eq!(overlay.handle(Event::Cancel), Some(Effect::Stop(id())));
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
    }

    #[test]
    fn cancel_shows_after_its_delay_and_gives_up_on_a_hand_off() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::HandOff));
        overlay.handle(Event::Reported);

        assert_eq!(overlay.handle(Event::CancelDue), None);
        assert_eq!(overlay.shown(), Launching::Cancel);
        assert_eq!(overlay.launch(), Some(Launch::HandOff));
        // A held B does what Cancel does once it shows.
        assert_eq!(
            overlay.handle(Event::CancelHeld),
            Some(Effect::GiveUp(id()))
        );
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn cancel_is_not_offered_once_the_starting_screen_is_gone() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));
        overlay.handle(Event::Focus(false));

        assert_eq!(overlay.handle(Event::CancelDue), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.launch(), None);
        assert_eq!(overlay.handle(Event::Cancel), None);
        assert_eq!(overlay.handle(Event::CancelHeld), None);
    }

    #[test]
    fn outside_gamescope_its_front_and_store_launch_change_nothing() {
        let mut overlay = LaunchOverlay::default();
        overlay.handle(Event::Play(id(), Launch::HandOff));
        let app_id = gamescope::app_id(&id());

        assert_eq!(overlay.handle(Event::Front(Front::Game(app_id))), None);
        assert_eq!(overlay.handle(Event::Front(Front::Launcher)), None);
        assert_eq!(overlay.shown(), Launching::HandingOff);
    }

    #[test]
    fn in_gamescope_a_hand_off_shows_until_the_game_is_in_front() {
        let mut overlay = LaunchOverlay::in_gamescope();
        let app_id = gamescope::app_id(&id());

        // No timers: gamescope says when the game is there.
        assert_eq!(overlay.handle(Event::Play(id(), Launch::HandOff)), None);
        assert_eq!(overlay.handle(Event::Reported), None);
        // Focus follows the Steam client's windows and gamescope's own, not the launch.
        assert_eq!(overlay.handle(Event::Focus(false)), None);
        assert_eq!(overlay.handle(Event::Focus(true)), None);
        // A Steam prompt shown in the launcher's place leaves the launcher in front.
        assert_eq!(overlay.handle(Event::Front(Front::Launcher)), None);
        assert_eq!(overlay.handle(Event::Front(Front::Game(620))), None);
        assert_eq!(overlay.shown(), Launching::HandingOff);
        overlay.handle(Event::CancelDue);
        assert_eq!(overlay.shown(), Launching::Cancel);

        assert_eq!(overlay.handle(Event::Front(Front::Game(app_id))), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Front(Front::Launcher)),
            Some(Effect::Return(id()))
        );
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn in_gamescope_a_store_launch_that_is_over_returns_to_the_page() {
        let mut overlay = LaunchOverlay::in_gamescope();
        overlay.handle(Event::Play(id(), Launch::HandOff));
        overlay.handle(Event::Reported);

        assert_eq!(
            overlay.handle(Event::StoreLaunchOver),
            Some(Effect::Return(id()))
        );
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn in_gamescope_a_store_launch_over_after_the_game_was_in_front_returns_once() {
        let mut overlay = LaunchOverlay::in_gamescope();
        let app_id = gamescope::app_id(&id());
        overlay.handle(Event::Play(id(), Launch::HandOff));
        overlay.handle(Event::Front(Front::Game(app_id)));

        assert_eq!(overlay.handle(Event::StoreLaunchOver), None);
        assert_eq!(overlay.game(), None);
        assert_eq!(overlay.handle(Event::Front(Front::Launcher)), None);
    }

    #[test]
    fn in_gamescope_a_game_we_run_returns_each_time_the_launcher_is_back_in_front() {
        let mut overlay = LaunchOverlay::in_gamescope();
        let app_id = gamescope::app_id(&id());

        assert_eq!(overlay.handle(Event::Play(id(), Launch::Run)), None);
        overlay.handle(Event::Playing(id(), true));
        assert_eq!(overlay.handle(Event::Reported), None);
        assert_eq!(overlay.shown(), Launching::Starting);
        // Its store launch, if any, is not this one's.
        assert_eq!(overlay.handle(Event::StoreLaunchOver), None);

        assert_eq!(overlay.handle(Event::Front(Front::Game(app_id))), None);
        assert_eq!(overlay.shown(), Launching::No);
        assert_eq!(
            overlay.handle(Event::Front(Front::Launcher)),
            Some(Effect::Return(id()))
        );
        overlay.handle(Event::Front(Front::Game(app_id)));
        // It ends while in front: the return waits for the launcher to be back.
        assert_eq!(overlay.handle(Event::Playing(id(), false)), None);
        assert_eq!(
            overlay.handle(Event::Front(Front::Launcher)),
            Some(Effect::Return(id()))
        );
        assert_eq!(overlay.game(), None);
    }

    #[test]
    fn in_gamescope_a_game_we_run_that_ends_before_it_shows_returns_at_once() {
        let mut overlay = LaunchOverlay::in_gamescope();
        overlay.handle(Event::Play(id(), Launch::Run));
        overlay.handle(Event::Playing(id(), true));
        overlay.handle(Event::Reported);
        overlay.handle(Event::CancelDue);

        assert_eq!(overlay.handle(Event::Cancel), Some(Effect::Stop(id())));
        assert_eq!(
            overlay.handle(Event::Playing(id(), false)),
            Some(Effect::Return(id()))
        );
    }
}
