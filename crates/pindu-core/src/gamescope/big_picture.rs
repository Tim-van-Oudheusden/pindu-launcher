//! Steam Big Picture opened from Home in a `--steam` gamescope. The Steam client shows Big
//! Picture in its own `steamwebhelper` window, tagged app 769 as the launcher's main window
//! is, and starts its games itself. While the session runs the launcher steps aside: it
//! shows Steam's window in its own place, leaves the base layer to a game Steam started,
//! and takes back the front once Big Picture's window has closed.

use std::time::{Duration, Instant};

use super::steam_prompts::SteamPrompts;
use super::{
    BaseLayerClaim, STEAM_APP_ID, STORE_LAUNCH_GRACE, STORE_LAUNCH_TIMEOUT, WindowFacts, Xid,
};

/// How long the launcher waits for Big Picture's window before it gives up on it.
pub const BIG_PICTURE_TIMEOUT: Duration = STORE_LAUNCH_TIMEOUT;

/// How long the launcher waits for the window of a Steam that has never run, 10 minutes:
/// its first start downloads the client's update before it shows a window, which can take
/// minutes.
pub const BIG_PICTURE_FIRST_START_TIMEOUT: Duration = Duration::from_secs(600);

/// How long the session outlives the Steam client's last window, for a window that closes
/// a while before another opens: on a cold start, Steam's update or sign-in window before
/// Big Picture's, as for a store launch.
pub const BIG_PICTURE_GRACE: Duration = STORE_LAUNCH_GRACE;

/// Steam Big Picture, opened from Home, in front of the launcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BigPicture {
    phase: Phase,
    /// The Steam client's windows that are open, in the order they mapped.
    steam: SteamPrompts,
    /// The app ID of a game the Steam client started, while gamescope could show it.
    game: Option<u32>,
    /// The app IDs that are never Steam's game (see [`Self::new`]).
    before: Vec<u32>,
    /// How long it waits for the Steam client's first window.
    timeout: Duration,
}

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// No window of the Steam client yet, since the hand-off went out at this time.
    Waiting(Instant),
    /// A window of the Steam client is there.
    Shown,
    /// The Steam client's last window left at this time.
    Left(Instant),
}

impl BigPicture {
    /// The session of Big Picture, whose hand-off to Steam went out at `now`. `before`
    /// holds the app IDs that are not Steam's game: the apps gamescope could show then,
    /// and the games the launcher runs or handed to their store.
    pub fn new(now: Instant, before: &[u32]) -> Self {
        Self {
            phase: Phase::Waiting(now),
            steam: SteamPrompts::default(),
            game: None,
            before: before.to_vec(),
            timeout: BIG_PICTURE_TIMEOUT,
        }
    }

    /// The session waits [`BIG_PICTURE_FIRST_START_TIMEOUT`] for the Steam client's first
    /// window: Steam has never run (`launch::SteamSession::first_start`).
    #[must_use]
    pub fn first_start(self) -> Self {
        Self {
            timeout: BIG_PICTURE_FIRST_START_TIMEOUT,
            ..self
        }
    }

    /// Whether the session is over at `now`.
    pub fn is_over(&self, now: Instant) -> bool {
        self.deadline().is_some_and(|deadline| now >= deadline)
    }

    /// When the session is over unless the Steam client's window shows first, so the
    /// launcher knows when to look again; `None` while one is there, or a game Steam
    /// started.
    pub fn deadline(&self) -> Option<Instant> {
        match self.phase {
            Phase::Waiting(since) => Some(since + self.timeout),
            Phase::Shown => None,
            Phase::Left(_) if self.game.is_some() => None,
            Phase::Left(since) => Some(since + BIG_PICTURE_GRACE),
        }
    }

    /// Whether Big Picture is still opening: the Steam client has shown no window yet.
    pub fn is_opening(&self) -> bool {
        matches!(self.phase, Phase::Waiting(_))
    }

    /// Whether Steam has the controller: its window or a game it started is in the
    /// launcher's place. While Big Picture opens, and once its last window has left with no
    /// game of its own playing, the launcher's window is in front and the controller is its
    /// own.
    pub fn has_controller(&self) -> bool {
        self.phase == Phase::Shown || self.game.is_some()
    }

    /// Follows a top-level window another client mapped at `now`, or that was there when
    /// the session started (`own` holds the launcher's windows): one of the Steam client's
    /// shows Big Picture.
    pub fn window_mapped(&mut self, window: &WindowFacts, own: &[Xid], now: Instant) {
        self.steam.mapped(window, own, now);
        if self.steam.latest().is_some() {
            self.phase = Phase::Shown;
        }
    }

    /// Follows a top-level window that was unmapped at `now`: once the Steam client's
    /// last window is gone, the user has left Big Picture.
    pub fn window_unmapped(&mut self, xid: Xid, now: Instant) {
        self.steam.unmapped(xid);
        if self.phase == Phase::Shown && self.steam.latest().is_none() {
            self.phase = Phase::Left(now);
        }
    }

    /// What the launcher claims on the root while the session runs, with its main window
    /// `main_window`: a game Steam started first, with nothing pinned, so it shows;
    /// otherwise the Steam client's newest window pinned in the launcher's place, or the
    /// launcher's own while there is none.
    pub fn claim(&self, main_window: Option<Xid>) -> BaseLayerClaim {
        match self.game {
            Some(game) => BaseLayerClaim {
                app_ids: vec![game, STEAM_APP_ID],
                window: None,
            },
            None => BaseLayerClaim {
                app_ids: vec![STEAM_APP_ID],
                window: self.steam.latest().or(main_window),
            },
        }
    }

    /// Follows the root's `GAMESCOPE_FOCUSABLE_APPS` as it changed at `now`, the app IDs
    /// of the windows gamescope could show: one other than Steam's own, and not among the
    /// ones there before, is a game Steam started. Without the Steam client's window, the
    /// grace starts when the game ends.
    pub fn focusable_apps_changed(&mut self, apps: &[u32], now: Instant) {
        let had_game = self.game.is_some();
        self.game = apps
            .iter()
            .copied()
            .find(|app| ![STEAM_APP_ID, 0].contains(app) && !self.before.contains(app));
        if had_game && self.game.is_none() && matches!(self.phase, Phase::Left(_)) {
            self.phase = Phase::Left(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamescope::{STEAM_APP_ID, WindowFacts, Xid};

    /// Big Picture's window: `steamwebhelper`, tagged as app 769 by Steam.
    fn steam_window(xid: Xid) -> WindowFacts {
        WindowFacts {
            xid,
            wm_class: vec!["steamwebhelper".to_owned(), "steam".to_owned()],
            pid: Some(557_997),
            steam_game: Some(STEAM_APP_ID),
            override_redirect: false,
            width: 1280,
            height: 720,
        }
    }

    #[test]
    fn a_session_without_a_steam_window_is_over_after_the_time_out() {
        let start = Instant::now();
        let session = BigPicture::new(start, &[]);
        assert!(!session.is_over(start + BIG_PICTURE_TIMEOUT - Duration::from_secs(1)));
        assert!(session.is_over(start + BIG_PICTURE_TIMEOUT));
    }

    #[test]
    fn steams_first_start_gets_longer_to_show_a_window() {
        let start = Instant::now();
        let session = BigPicture::new(start, &[]).first_start();
        assert!(!session.is_over(start + BIG_PICTURE_TIMEOUT));
        assert!(!session.is_over(start + BIG_PICTURE_FIRST_START_TIMEOUT - Duration::from_secs(1)));
        assert!(session.is_over(start + BIG_PICTURE_FIRST_START_TIMEOUT));
    }

    #[test]
    fn the_steam_clients_window_opens_the_session_past_the_time_out() {
        let start = Instant::now();
        let mut session = BigPicture::new(start, &[]);
        assert!(session.is_opening());
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        assert!(!session.is_opening());
        assert_eq!(session.deadline(), None);
        assert!(!session.is_over(start + BIG_PICTURE_TIMEOUT * 10));
    }

    #[test]
    fn the_session_is_over_a_little_after_the_steam_clients_last_window_left() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        session.window_mapped(&steam_window(0x220_0060), &[], start);
        session.window_unmapped(0x220_0060, at(30));
        assert_eq!(session.deadline(), None);
        // The user leaves Big Picture.
        session.window_unmapped(0x220_0035, at(600));
        assert!(!session.is_opening());
        assert_eq!(session.deadline(), Some(at(600) + BIG_PICTURE_GRACE));
        assert!(!session.is_over(at(601)));
        assert!(session.is_over(at(600) + BIG_PICTURE_GRACE));
    }

    #[test]
    fn the_launcher_has_the_controller_back_as_soon_as_steams_last_window_left() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        assert!(session.has_controller());
        // Exit Steam: its window closes, and the launcher's is in front for the grace.
        session.window_unmapped(0x220_0035, at(600));
        assert!(!session.is_over(at(601)));
        assert!(!session.has_controller());
    }

    const MAIN: Xid = 0x60_0004;

    #[test]
    fn the_claim_pins_the_newest_steam_window_in_the_launchers_place() {
        let start = Instant::now();
        let mut session = BigPicture::new(start, &[]);
        // While Big Picture opens, the launcher says so.
        let waiting = session.claim(Some(MAIN));
        assert_eq!(waiting.app_ids, [STEAM_APP_ID]);
        assert_eq!(waiting.window, Some(MAIN));
        // The launcher's own window and menus are not Big Picture's.
        session.window_mapped(&steam_window(MAIN), &[MAIN], start);
        let menu = WindowFacts {
            override_redirect: true,
            ..steam_window(0x220_0040)
        };
        session.window_mapped(&menu, &[MAIN], start);
        assert_eq!(session.claim(Some(MAIN)), waiting);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], start);
        session.window_mapped(&steam_window(0x220_0060), &[MAIN], start);
        let shown = session.claim(Some(MAIN));
        assert_eq!(shown.app_ids, [STEAM_APP_ID]);
        assert_eq!(shown.window, Some(0x220_0060));
    }

    #[test]
    fn a_game_steam_started_comes_first_and_steams_window_once_it_ends() {
        const BALATRO: u32 = 2_379_780;
        let now = Instant::now();
        let mut session = BigPicture::new(now, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], now);
        session.focusable_apps_changed(&[STEAM_APP_ID, BALATRO], now);
        let playing = session.claim(Some(MAIN));
        assert_eq!(playing.app_ids, [BALATRO, STEAM_APP_ID]);
        assert_eq!(playing.window, None);
        // Big Picture's own windows, or none at all, leave the front to Steam's window.
        session.focusable_apps_changed(&[STEAM_APP_ID], now);
        assert_eq!(session.claim(Some(MAIN)).window, Some(0x220_0035));
        session.focusable_apps_changed(&[], now);
        assert_eq!(session.claim(Some(MAIN)).app_ids, [STEAM_APP_ID]);
    }

    #[test]
    fn a_steam_window_that_maps_within_the_grace_keeps_the_session() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        session.window_unmapped(0x220_0035, at(60));
        session.window_mapped(&steam_window(0x220_0070), &[], at(61));
        assert_eq!(session.deadline(), None);
        assert!(!session.is_over(at(3600)));
    }

    #[test]
    fn a_game_steam_started_keeps_the_session_until_it_ends() {
        const BALATRO: u32 = 2_379_780;
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        session.focusable_apps_changed(&[STEAM_APP_ID, BALATRO], at(5));
        // Steam hides its window while the game plays.
        session.window_unmapped(0x220_0035, at(10));
        assert_eq!(session.deadline(), None);
        assert!(!session.is_over(at(600)));
        session.focusable_apps_changed(&[STEAM_APP_ID], at(900));
        assert_eq!(session.deadline(), Some(at(900) + BIG_PICTURE_GRACE));
    }

    #[test]
    fn a_game_steam_started_keeps_the_controller_until_it_ends() {
        const BALATRO: u32 = 2_379_780;
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[], start);
        session.focusable_apps_changed(&[STEAM_APP_ID, BALATRO], at(5));
        // Steam hides its window while the game plays.
        session.window_unmapped(0x220_0035, at(10));
        assert!(session.has_controller());
        session.focusable_apps_changed(&[STEAM_APP_ID], at(900));
        assert!(!session.has_controller());
    }

    #[test]
    fn steams_update_window_closing_a_while_before_big_picture_opens_keeps_the_session() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        // A cold start: Steam's update window, then Big Picture's some seconds later.
        session.window_mapped(&steam_window(0x220_0010), &[], start);
        session.window_unmapped(0x220_0010, at(20));
        assert!(!session.is_over(at(25)));
        session.window_mapped(&steam_window(0x220_0035), &[], at(25));
        assert_eq!(session.deadline(), None);
    }

    #[test]
    fn apps_there_before_big_picture_opened_are_not_steams_game() {
        // A game we run, and one Steam ran before, with windows when Big Picture opens.
        const CELESTE: u32 = 0x8000_1234;
        const BALATRO: u32 = 2_379_780;
        const HADES: u32 = 1_145_360;
        let now = Instant::now();
        let mut session = BigPicture::new(now, &[STEAM_APP_ID, CELESTE, BALATRO]);
        session.focusable_apps_changed(&[STEAM_APP_ID, CELESTE, BALATRO], now);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], now);
        let shown = session.claim(Some(MAIN));
        assert_eq!(shown.app_ids, [STEAM_APP_ID]);
        assert_eq!(shown.window, Some(0x220_0035));
        // A game started from Big Picture is Steam's.
        session.focusable_apps_changed(&[STEAM_APP_ID, CELESTE, BALATRO, HADES], now);
        assert_eq!(session.claim(Some(MAIN)).app_ids, [HADES, STEAM_APP_ID]);
    }
}
