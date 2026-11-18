//! A store launch that is expected in front: a game the launcher handed to its store
//! launcher (Steam, Heroic, Lutris, Bottles, Faugus) through a URI. The launcher does not
//! track such a game, so in a `--steam` gamescope it follows the game's app ID in the
//! root's `GAMESCOPE_FOCUSABLE_APPS` instead, from the moment the URI opens until a window
//! with the ID has come and gone, or until a time-out passes without any.

use std::time::{Duration, Instant};

use super::steam_prompts::SteamPrompts;
use super::{ExpectedGame, Front, WindowFacts, Xid};
use crate::model::{GameId, Source};

/// How long a store launch waits for its first window before the launcher gives up on it.
pub const STORE_LAUNCH_TIMEOUT: Duration = Duration::from_secs(60);

/// How long a store launch outlives its last window, for the store launcher's window that
/// closes just before the game's opens (Steam's update window, Lutris's): its windows still
/// get its app ID, and one that shows again brings the game back.
pub const STORE_LAUNCH_GRACE: Duration = Duration::from_secs(10);

/// How long the game of a store launch still plays after its last window left, for a game
/// that closes one window and opens the next. After that it has exited, and the launcher
/// comes back to the front.
pub const STORE_EXIT_GRACE: Duration = Duration::from_secs(2);

/// A store launch that is expected in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLaunch {
    id: GameId,
    game: ExpectedGame,
    phase: Phase,
    /// Whether the Steam client launches the game, so its windows may be the launch's;
    /// not once the user gave up on the launch.
    by_steam: bool,
    /// The Steam client's windows that are open while the launch waits.
    steam: SteamPrompts,
}

/// Where a store launch is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No window with its app ID yet, since the URI opened.
    Waiting(Instant),
    /// A window with its app ID is there.
    Shown,
    /// Its last window left at this time.
    Left(Instant),
}

impl StoreLaunch {
    /// The launch of the game with `id`, whose URI opened at `now`.
    pub fn new(id: &GameId, now: Instant) -> Self {
        Self {
            id: id.clone(),
            game: ExpectedGame::new(id, None),
            phase: Phase::Waiting(now),
            by_steam: id.source() == Source::Steam,
            steam: SteamPrompts::default(),
        }
    }

    /// Whether the launch is over at `now`.
    pub fn is_over(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }

    /// When the launch is over unless a window with its app ID shows first, so the
    /// launcher knows when to look again; `None` while one is there, or while the Steam
    /// client has a window open the launch may wait on, shown yet or not.
    pub fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Waiting(_) if self.steam.any() => None,
            Phase::Waiting(since) => Some(since + STORE_LAUNCH_TIMEOUT),
            Phase::Shown => None,
            Phase::Left(since) => Some(since + STORE_LAUNCH_GRACE),
        }
    }

    /// When one of the Steam client's windows open at `now` while the launch waits has
    /// stayed open for [`STEAM_PROMPT_DELAY`](super::STEAM_PROMPT_DELAY) and shows (see
    /// [`Self::steam_window`]), so the launcher looks again then; `None` when none waits to
    /// show.
    pub fn reveal_at(&self, now: Instant) -> Option<Instant> {
        match self.phase {
            Phase::Waiting(_) => self.steam.next_reveal(now),
            Phase::Shown | Phase::Left(_) => None,
        }
    }

    /// When the launcher looks at the launch again after `now`: when its game exits, a
    /// little after its last window left, and then at its [`deadline`](Self::deadline).
    pub fn next_check(&self, now: Instant) -> Option<Instant> {
        match self.phase {
            Phase::Left(since) if now < since + STORE_EXIT_GRACE => Some(since + STORE_EXIT_GRACE),
            _ => self.deadline(),
        }
    }

    /// The app ID the game's windows get (see [`app_id`](super::app_id)).
    pub fn app_id(&self) -> u32 {
        self.game.app_id
    }

    /// The game the store launcher was handed.
    pub fn game(&self) -> &GameId {
        &self.id
    }

    /// Whether the game plays at `now`: a window with its app ID has shown, and has not been
    /// gone for [`STORE_EXIT_GRACE`] since the last one left. Its store launcher runs it, not
    /// the launcher.
    pub fn is_playing(&self, now: Instant) -> bool {
        !matches!(self.phase, Phase::Waiting(_)) && !self.has_exited(now)
    }

    /// Whether the game has exited at `now`: its last window left [`STORE_EXIT_GRACE`] ago
    /// or more. The launch is not over yet, so a window that shows again brings it back.
    pub fn has_exited(&self, now: Instant) -> bool {
        matches!(self.phase, Phase::Left(since) if now >= since + STORE_EXIT_GRACE)
    }

    /// Follows the root's `GAMESCOPE_FOCUSABLE_APPS` as it changed at `now`: the app IDs of
    /// the windows gamescope could show. Returns whether the game came back: a window with
    /// its app ID shows again after the game exited, so the game is wanted in front again.
    pub fn focusable_apps_changed(&mut self, apps: &[u32], now: Instant) -> bool {
        let there = apps.contains(&self.game.app_id);
        let back = there && self.has_exited(now);
        self.phase = match self.phase {
            _ if there => Phase::Shown,
            Phase::Shown => Phase::Left(now),
            phase => phase,
        };
        back
    }

    /// The game as [`front`](super::front) matches it.
    pub fn expected(&self) -> &ExpectedGame {
        &self.game
    }

    /// The app ID to set as `STEAM_GAME` on a window that just mapped while this launch is
    /// expected, as [`tag_for_window`](super::tag_for_window) decides it with the games we
    /// started that run (`tracked`): their windows by class or process stay theirs, and any
    /// other untagged top-level of another client is this launch's, the store launcher's
    /// own windows as much as the game's.
    pub fn tag_for_window(
        &self,
        window: &WindowFacts,
        tracked: &[ExpectedGame],
        own: &[Xid],
    ) -> Option<u32> {
        // With the launch among them, more than one game is expected, so only a class or
        // process match counts; the launch has neither, so it matches none of them.
        let all: Vec<ExpectedGame> = tracked
            .iter()
            .chain(std::iter::once(&self.game))
            .cloned()
            .collect();
        super::tag_for_window(window, &all, own)
            .or_else(|| super::tag_for_window(window, std::slice::from_ref(&self.game), own))
    }

    /// Whether a game we started replaces this launch, once the games we started change
    /// from `before` to `running` (app IDs): one that just started is the launch the user
    /// made last. A later store launch replaces it by taking its place.
    pub fn replaced_by_sessions(&self, before: &[u32], running: &[u32]) -> bool {
        running.iter().any(|id| !before.contains(id))
    }

    /// Follows a top-level window another client mapped at `now`, or that was there when
    /// the launch started (`own` holds the launcher's windows): one of the Steam client's,
    /// while a Steam launch waits for its game, is shown once it has stayed open for
    /// [`STEAM_PROMPT_DELAY`](super::STEAM_PROMPT_DELAY), until the game's window comes
    /// (see [`Self::steam_window`]).
    pub fn window_mapped(&mut self, window: &WindowFacts, own: &[Xid], now: Instant) {
        if self.by_steam && matches!(self.phase, Phase::Waiting(_)) {
            self.steam.mapped(window, own, now);
        }
    }

    /// Follows a top-level window that was unmapped at `now`: once the last of the Steam
    /// client's windows the launch waits on is gone, the game gets the full time-out again.
    pub fn window_unmapped(&mut self, xid: Xid, now: Instant) {
        let was_open = self.steam.any();
        self.steam.unmapped(xid);
        if was_open
            && !self.steam.any()
            && let Phase::Waiting(_) = self.phase
        {
            self.phase = Phase::Waiting(now);
        }
    }

    /// The Steam client's window to pin as the base layer at `now` with `front` wanted in
    /// front: the newest one that has stayed open for
    /// [`STEAM_PROMPT_DELAY`](super::STEAM_PROMPT_DELAY) while a Steam launch waits for its
    /// game, so a prompt the launch needs answered shows instead of the launcher, and
    /// Steam's own window that shows the game starting does not.
    pub fn steam_window(&self, front: Front, now: Instant) -> Option<Xid> {
        let waiting = matches!(self.phase, Phase::Waiting(_));
        let wanted = front == Front::Game(self.game.app_id);
        self.steam.newest(now).filter(|_| waiting && wanted)
    }

    /// The user gave up on the launch at `now`, as Cancel on the starting screen does: the
    /// Steam client's windows are left to it from then on, and its game gets the full
    /// time-out again. Steam may still start the game, whose window then plays as before.
    pub fn give_up(&mut self, now: Instant) {
        self.by_steam = false;
        self.steam = SteamPrompts::default();
        if let Phase::Waiting(_) = self.phase {
            self.phase = Phase::Waiting(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn launch(at: Instant) -> StoreLaunch {
        StoreLaunch::new(&"heroic-epic:hades".parse().unwrap(), at)
    }

    #[test]
    fn a_launch_without_any_window_is_over_after_the_time_out() {
        let start = Instant::now();
        let hades = launch(start);
        assert!(!hades.is_over(start));
        assert!(!hades.is_over(start + STORE_LAUNCH_TIMEOUT - Duration::from_secs(1)));
        assert!(hades.is_over(start + STORE_LAUNCH_TIMEOUT));
    }

    #[test]
    fn a_launch_whose_window_shows_outlasts_the_time_out() {
        let start = Instant::now();
        let mut hades = launch(start);
        hades.focusable_apps_changed(&[769], start + Duration::from_secs(5));
        hades.focusable_apps_changed(&[769, hades.app_id()], start + Duration::from_secs(20));
        assert!(!hades.is_over(start + STORE_LAUNCH_TIMEOUT * 10));
    }

    #[test]
    fn a_launch_is_over_a_little_after_its_last_window_left() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut hades = launch(start);
        let id = hades.app_id();
        hades.focusable_apps_changed(&[769, id], at(20));
        // The store launcher's window closes, and the game's opens soon after.
        hades.focusable_apps_changed(&[769], at(30));
        assert!(!hades.is_over(at(31)));
        hades.focusable_apps_changed(&[769, id], at(35));
        assert!(!hades.is_over(at(35) + STORE_LAUNCH_GRACE));
        // The game ends.
        hades.focusable_apps_changed(&[769], at(3600));
        assert!(!hades.is_over(at(3600)));
        assert!(hades.is_over(at(3600) + STORE_LAUNCH_GRACE));
    }

    #[test]
    fn the_deadline_is_the_time_out_then_none_then_the_grace() {
        let start = Instant::now();
        let mut hades = launch(start);
        let id = hades.app_id();
        assert_eq!(hades.deadline(), Some(start + STORE_LAUNCH_TIMEOUT));
        hades.focusable_apps_changed(&[769, id], start + Duration::from_secs(5));
        assert_eq!(hades.deadline(), None);
        let left = start + Duration::from_secs(50);
        hades.focusable_apps_changed(&[769], left);
        assert_eq!(hades.deadline(), Some(left + STORE_LAUNCH_GRACE));
    }

    #[test]
    fn a_launch_plays_from_its_first_window_until_it_is_over() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut hades = launch(start);
        let id = hades.app_id();
        assert!(!hades.is_playing(at(0)));
        hades.focusable_apps_changed(&[769, id], at(5));
        assert!(hades.is_playing(at(5)));
        // Between the store launcher's window and the game's, it still plays.
        hades.focusable_apps_changed(&[769], at(30));
        assert!(hades.is_playing(at(30)));
        assert_eq!(
            hades.game(),
            &"heroic-epic:hades".parse::<GameId>().unwrap()
        );
    }

    #[test]
    fn a_game_stops_playing_a_little_after_its_last_window_left() {
        let start = Instant::now();
        let mut hades = launch(start);
        let id = hades.app_id();
        hades.focusable_apps_changed(&[769, id], start);
        let left = start + Duration::from_secs(600);
        hades.focusable_apps_changed(&[769], left);
        assert!(hades.is_playing(left + STORE_EXIT_GRACE - Duration::from_millis(1)));
        assert!(!hades.is_playing(left + STORE_EXIT_GRACE));
    }

    #[test]
    fn a_launch_outlives_its_exited_game_until_the_long_grace() {
        let start = Instant::now();
        let mut hades = launch(start);
        let id = hades.app_id();
        hades.focusable_apps_changed(&[769, id], start);
        assert!(!hades.has_exited(start));
        let left = start + Duration::from_secs(600);
        hades.focusable_apps_changed(&[769], left);
        assert!(!hades.has_exited(left));
        let exited = left + STORE_EXIT_GRACE;
        assert!(hades.has_exited(exited));
        assert!(!hades.is_over(exited));
        assert!(!hades.is_over(left + STORE_LAUNCH_GRACE - Duration::from_millis(1)));
        assert!(hades.is_over(left + STORE_LAUNCH_GRACE));
    }

    #[test]
    fn a_window_back_before_the_launch_is_over_plays_again() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut hades = launch(start);
        let id = hades.app_id();
        assert!(!hades.focusable_apps_changed(&[769, id], at(5)));
        // The game closes its window and opens the next before it exits.
        assert!(!hades.focusable_apps_changed(&[769], at(600)));
        assert!(!hades.focusable_apps_changed(&[769, id], at(601)));
        assert!(hades.is_playing(at(601) + STORE_EXIT_GRACE));
        // The store launcher hands over to the game once the launcher took the front back.
        hades.focusable_apps_changed(&[769], at(700));
        assert!(!hades.is_playing(at(705)));
        assert!(hades.focusable_apps_changed(&[769, id], at(705)));
        assert!(hades.is_playing(at(705)));
        assert!(!hades.is_over(at(700) + STORE_LAUNCH_GRACE));
    }

    #[test]
    fn the_launch_is_looked_at_when_its_game_exits_then_when_it_is_over() {
        let start = Instant::now();
        let mut hades = launch(start);
        let id = hades.app_id();
        assert_eq!(hades.next_check(start), Some(start + STORE_LAUNCH_TIMEOUT));
        hades.focusable_apps_changed(&[769, id], start);
        assert_eq!(hades.next_check(start), None);
        let left = start + Duration::from_secs(600);
        hades.focusable_apps_changed(&[769], left);
        let exited = left + STORE_EXIT_GRACE;
        assert_eq!(hades.next_check(left), Some(exited));
        assert_eq!(
            hades.next_check(exited - Duration::from_millis(1)),
            Some(exited)
        );
        assert_eq!(hades.next_check(exited), Some(left + STORE_LAUNCH_GRACE));
    }

    fn window(xid: Xid) -> WindowFacts {
        WindowFacts {
            xid,
            width: 1280,
            height: 720,
            ..WindowFacts::default()
        }
    }

    #[test]
    fn other_clients_untagged_windows_are_the_launchs() {
        let hades = launch(Instant::now());
        let celeste = ExpectedGame::new(&"lutris:celeste".parse().unwrap(), Some(4100));
        let heroic = WindowFacts {
            wm_class: vec!["heroic".to_owned(), "Heroic".to_owned()],
            ..window(0x40_0001)
        };
        assert_eq!(
            hades.tag_for_window(&heroic, &[], &[]),
            Some(hades.app_id())
        );
        // With a game we started running too, its windows stay its own.
        let tracked = std::slice::from_ref(&celeste);
        assert_eq!(
            hades.tag_for_window(&heroic, tracked, &[]),
            Some(hades.app_id())
        );
        let celestes = WindowFacts {
            pid: Some(4100),
            ..window(0x40_0002)
        };
        assert_eq!(
            hades.tag_for_window(&celestes, tracked, &[]),
            Some(celeste.app_id)
        );
        // Tagged windows, ours and override-redirect ones are left alone.
        let steam_game = WindowFacts {
            steam_game: Some(1_145_360),
            ..heroic.clone()
        };
        assert_eq!(hades.tag_for_window(&steam_game, &[], &[]), None);
        assert_eq!(hades.tag_for_window(&heroic, &[], &[0x40_0001]), None);
        let menu = WindowFacts {
            override_redirect: true,
            ..heroic
        };
        assert_eq!(hades.tag_for_window(&menu, &[], &[]), None);
    }

    #[test]
    fn a_game_we_start_later_replaces_the_launch_and_one_that_ends_does_not() {
        let hades = launch(Instant::now());
        assert!(hades.replaced_by_sessions(&[], &[5]));
        assert!(hades.replaced_by_sessions(&[5], &[5, 7]));
        assert!(!hades.replaced_by_sessions(&[5, 7], &[5]));
        assert!(!hades.replaced_by_sessions(&[5], &[5]));
    }
}
