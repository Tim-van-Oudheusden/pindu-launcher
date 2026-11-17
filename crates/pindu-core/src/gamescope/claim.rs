//! The launcher's claim on gamescope's base layer, held against the Steam client. A Steam
//! client that starts inside a launcher session (a Steam game started from the launcher)
//! tags its own windows as app 769, as the launcher's main window is, and writes the
//! root's `GAMESCOPECTRL_BASELAYER_APPID` and `GAMESCOPECTRL_BASELAYER_WINDOW` itself. Among
//! windows of one app ID gamescope shows the one mapped last, so Steam's window would win
//! wherever the list names 769. The launcher therefore pins its main window with
//! `GAMESCOPECTRL_BASELAYER_WINDOW` whenever gamescope should show the launcher, and writes
//! its claim back when another client changes either property.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use super::{Front, Xid, base_layer};

/// How often the launcher writes its claim back within [`RECLAIM_PERIOD`] at most, so it
/// never fights a client that rewrites the root on every change in a loop. The Steam client
/// wrote it up to three times within a second, when it started and when a game started or
/// ended.
pub const RECLAIM_LIMIT: usize = 10;

/// The period [`RECLAIM_LIMIT`] counts in.
pub const RECLAIM_PERIOD: Duration = Duration::from_secs(10);

/// What the launcher writes on gamescope's root to decide the base layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BaseLayerClaim {
    /// The value of `GAMESCOPECTRL_BASELAYER_APPID` (see [`base_layer`]).
    pub app_ids: Vec<u32>,
    /// The window `GAMESCOPECTRL_BASELAYER_WINDOW` pins, which gamescope shows before
    /// anything the list names; `None` leaves the base layer to the list.
    pub window: Option<Xid>,
}

impl BaseLayerClaim {
    /// The claim for `front` while the games with the app IDs `running` run, when the
    /// root's `GAMESCOPE_FOCUSABLE_APPS` lists `focusable_apps` and the launcher's main
    /// window is `main_window` (`None` before it is realized).
    pub fn new(
        front: Front,
        running: &[u32],
        focusable_apps: &[u32],
        main_window: Option<Xid>,
    ) -> Self {
        // gamescope falls back to 769 while the game has no window: that has to be ours.
        let window = match front {
            Front::Game(game) if focusable_apps.contains(&game) => None,
            _ => main_window,
        };
        Self {
            app_ids: base_layer(front, running),
            window,
        }
    }

    /// Whether the root still holds the claim, from its `GAMESCOPECTRL_BASELAYER_APPID`
    /// and `GAMESCOPECTRL_BASELAYER_WINDOW` as read (`None` when the root has no such
    /// property). A pinned window of 0 is no pin, as gamescope reads it.
    pub fn is_held(&self, app_ids: Option<&[u32]>, window: Option<Xid>) -> bool {
        let pinned = window.filter(|&xid| xid != 0);
        app_ids == Some(self.app_ids.as_slice()) && pinned == self.window
    }
}

/// The times the launcher wrote its claim back after another client changed it.
#[derive(Debug, Clone, Default)]
pub struct Reclaims {
    recent: VecDeque<Instant>,
}

impl Reclaims {
    /// Whether the launcher may write its claim back at `now`, which it then counts:
    /// [`RECLAIM_LIMIT`] times within [`RECLAIM_PERIOD`].
    pub fn allow(&mut self, now: Instant) -> bool {
        while self
            .recent
            .front()
            .is_some_and(|&then| now.saturating_duration_since(then) >= RECLAIM_PERIOD)
        {
            self.recent.pop_front();
        }
        if self.recent.len() >= RECLAIM_LIMIT {
            return false;
        }
        self.recent.push_back(now);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamescope::STEAM_APP_ID;

    const MAIN: Xid = 0x60_0004;

    #[test]
    fn with_the_launcher_in_front_the_claim_pins_its_main_window() {
        let claim = BaseLayerClaim::new(Front::Launcher, &[620], &[STEAM_APP_ID], Some(MAIN));
        assert_eq!(claim.app_ids, [STEAM_APP_ID, 620]);
        assert_eq!(claim.window, Some(MAIN));
    }

    #[test]
    fn with_a_game_in_front_whose_window_is_there_the_list_decides() {
        let focusable = [STEAM_APP_ID, 620];
        let claim = BaseLayerClaim::new(Front::Game(620), &[620], &focusable, Some(MAIN));
        assert_eq!(claim.app_ids, [620, STEAM_APP_ID]);
        assert_eq!(claim.window, None);
    }

    #[test]
    fn with_a_game_in_front_that_has_no_window_the_launcher_stays_pinned() {
        // The Steam client's own 769 window, mapped after ours, would show otherwise.
        let claim = BaseLayerClaim::new(Front::Game(620), &[], &[STEAM_APP_ID], Some(MAIN));
        assert_eq!(claim.app_ids, [620, STEAM_APP_ID]);
        assert_eq!(claim.window, Some(MAIN));
    }

    #[test]
    fn a_claim_another_client_changed_is_no_longer_held() {
        let claim = BaseLayerClaim::new(Front::Launcher, &[], &[STEAM_APP_ID], Some(MAIN));
        assert!(claim.is_held(Some(&[STEAM_APP_ID]), Some(MAIN)));
        // What the Steam client wrote when it started inside the launcher's session.
        assert!(!claim.is_held(Some(&[413_091, STEAM_APP_ID]), Some(MAIN)));
        assert!(!claim.is_held(Some(&[STEAM_APP_ID]), Some(0)));
        assert!(!claim.is_held(None, None));
    }

    #[test]
    fn an_unpinned_claim_is_held_by_a_window_of_0_or_none() {
        let claim = BaseLayerClaim::new(Front::Game(620), &[620], &[620], Some(MAIN));
        assert!(claim.is_held(Some(&[620, STEAM_APP_ID]), Some(0)));
        assert!(claim.is_held(Some(&[620, STEAM_APP_ID]), None));
        assert!(!claim.is_held(Some(&[620, STEAM_APP_ID]), Some(MAIN)));
    }

    #[test]
    fn the_claim_is_written_back_a_few_times_per_period_at_most() {
        let start = Instant::now();
        let mut reclaims = Reclaims::default();
        for _ in 0..RECLAIM_LIMIT {
            assert!(reclaims.allow(start));
        }
        assert!(!reclaims.allow(start + Duration::from_secs(1)));
        assert!(reclaims.allow(start + RECLAIM_PERIOD));
    }
}
