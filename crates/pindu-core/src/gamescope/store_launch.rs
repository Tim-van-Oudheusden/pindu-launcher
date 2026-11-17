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
/// closes just before the game's opens (Steam's update window, Lutris's).
pub const STORE_LAUNCH_GRACE: Duration = Duration::from_secs(10);

/// A store launch that is expected in front.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreLaunch {
    game: ExpectedGame,
    phase: Phase,
    /// Whether the Steam client launches the game, so its windows may be the launch's.
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
    /// client shows a window the launch waits on.
    pub fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Waiting(_) if self.steam.newest().is_some() => None,
            Phase::Waiting(since) => Some(since + STORE_LAUNCH_TIMEOUT),
            Phase::Shown => None,
            Phase::Left(since) => Some(since + STORE_LAUNCH_GRACE),
        }
    }

    /// The app ID the game's windows get (see [`app_id`](super::app_id)).
    pub fn app_id(&self) -> u32 {
        self.game.app_id
    }

    /// Follows the root's `GAMESCOPE_FOCUSABLE_APPS` as it changed at `now`: the app IDs of
    /// the windows gamescope could show.
    pub fn focusable_apps_changed(&mut self, apps: &[u32], now: Instant) {
        let there = apps.contains(&self.game.app_id);
        self.phase = match self.phase {
            _ if there => Phase::Shown,
            Phase::Shown => Phase::Left(now),
            phase => phase,
        };
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

    /// Follows a top-level window another client mapped, or that was there when the launch
    /// started (`own` holds the launcher's windows): one of the Steam client's, while a
    /// Steam launch waits for its game, is shown until the game's window comes (see
    /// [`Self::steam_window`]).
    pub fn window_mapped(&mut self, window: &WindowFacts, own: &[Xid]) {
        if self.by_steam && matches!(self.phase, Phase::Waiting(_)) {
            self.steam.mapped(window, own);
        }
    }

    /// Follows a top-level window that was unmapped at `now`: once the last of the Steam
    /// client's windows the launch waits on is gone, the game gets the full time-out again.
    pub fn window_unmapped(&mut self, xid: Xid, now: Instant) {
        let was_showing = self.steam.newest().is_some();
        self.steam.unmapped(xid);
        if was_showing
            && self.steam.newest().is_none()
            && let Phase::Waiting(_) = self.phase
        {
            self.phase = Phase::Waiting(now);
        }
    }

    /// The Steam client's window to pin as the base layer with `front` wanted in front: the
    /// newest one open while a Steam launch waits for its game, so a prompt the launch needs
    /// answered shows instead of the launcher.
    pub fn steam_window(&self, front: Front) -> Option<Xid> {
        let waiting = matches!(self.phase, Phase::Waiting(_));
        let wanted = front == Front::Game(self.game.app_id);
        self.steam.newest().filter(|_| waiting && wanted)
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
