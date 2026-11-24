//! Steam Big Picture opened from Home in a `--steam` gamescope. The Steam client shows Big
//! Picture in its own `steamwebhelper` window, tagged app 769 as the launcher's main window
//! is, and starts its games itself. While the session runs the launcher steps aside: it
//! shows Steam's window in its own place, leaves the base layer to a game Steam started,
//! and takes back the front once Big Picture's window has closed, or when the user asks to
//! leave ([`leave_big_picture`]): under gamescope Steam's window closes only when Steam
//! quits.

use std::time::{Duration, Instant};

use super::steam_prompts::SteamPrompts;
use super::{
    BaseLayerClaim, STEAM_APP_ID, STORE_LAUNCH_GRACE, STORE_LAUNCH_TIMEOUT, WindowFacts, Xid,
};
use crate::model::{GameId, Source};

/// How long the launcher waits for Big Picture's window before it gives up on it.
pub const BIG_PICTURE_TIMEOUT: Duration = STORE_LAUNCH_TIMEOUT;

/// The class of the Steam client's web helper, whose windows show Steam's sign-in and Big
/// Picture; the small windows Steam's updater shows before them are of another.
const STEAM_WEB_HELPER: &str = "steamwebhelper";

/// How long the session outlives the Steam client's last window, for a window that closes
/// a while before another opens: on a cold start, Steam's update or sign-in window before
/// Big Picture's, as for a store launch.
pub const BIG_PICTURE_GRACE: Duration = STORE_LAUNCH_GRACE;

/// How long the launcher's overlay says how to leave Big Picture once its window first
/// shows in a session, 8 seconds.
pub const BIG_PICTURE_NOTICE: Duration = Duration::from_secs(8);

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
    /// Whether Steam has never run: its set-up screen does the waiting, and Steam's
    /// updater's windows stay behind it (see [`Self::first_start`]).
    first_start: bool,
    /// When the Steam client's first window showed, which starts the notice of how to
    /// leave.
    shown_at: Option<Instant>,
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
            first_start: false,
            shown_at: None,
        }
    }

    /// The session of a Steam that has never run (`launch::SteamSession::first_start`):
    /// it sets itself up for minutes before it shows its sign-in. The Setting Up Steam
    /// screen waits in front meanwhile, so the session has no time-out of its own until a
    /// window shows, and only the web helper's window, the sign-in, takes the launcher's
    /// place: the updater's small windows stay behind the screen.
    #[must_use]
    pub fn first_start(self) -> Self {
        Self {
            first_start: true,
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
            Phase::Waiting(_) if self.first_start => None,
            Phase::Waiting(since) => Some(since + BIG_PICTURE_TIMEOUT),
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

    /// Until when the launcher says how to leave Big Picture: [`BIG_PICTURE_NOTICE`] from
    /// the Steam client's first window in the session; `None` before it shows.
    pub fn notice_until(&self) -> Option<Instant> {
        self.shown_at.map(|shown| shown + BIG_PICTURE_NOTICE)
    }

    /// Whether the launcher says how to leave Big Picture at `now` (see
    /// [`Self::notice_until`]): once a session, while Big Picture has just shown.
    pub fn shows_notice(&self, now: Instant) -> bool {
        self.notice_until().is_some_and(|until| now < until)
    }

    /// Follows a top-level window another client mapped at `now`, or that was there when
    /// the session started (`own` holds the launcher's windows): one of the Steam client's
    /// shows Big Picture, or on a first start its web helper's shows the sign-in.
    pub fn window_mapped(&mut self, window: &WindowFacts, own: &[Xid], now: Instant) {
        let updater = !window
            .wm_class
            .iter()
            .any(|class| class == STEAM_WEB_HELPER);
        if self.first_start && self.is_opening() && updater {
            return;
        }
        self.steam.mapped(window, own, now);
        if self.steam.latest().is_some() {
            self.phase = Phase::Shown;
            self.shown_at.get_or_insert(now);
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

/// What the launcher does when asked to leave Steam Big Picture, by the `leave-big-picture`
/// action or the chord ([`LEAVE_BIG_PICTURE_CHORD`](crate::gamepad::LEAVE_BIG_PICTURE_CHORD)).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leave {
    /// There is no session: nothing changes.
    Nothing,
    /// The session ends and the launcher comes back to the front with the controller. The
    /// Steam client keeps running behind it, so its downloads go on and Big Picture opens
    /// again at once.
    ToLauncher,
    /// As [`Self::ToLauncher`], while the game Steam started, a Steam game, keeps running
    /// behind the launcher as a game handed to its store: it has a Playing Now card whose
    /// Return brings it back to the front and whose Stop exits it, and the launcher stays
    /// in front when it ends.
    ToLauncherWithGame(GameId),
}

/// What leaving `session`, the Big Picture session while there is one, does on request. A
/// game Steam started that is a Steam game, whose app ID is its Steam app ID as
/// [`app_id`](super::app_id) gives it, stays playing behind the launcher; any other, such
/// as a shortcut Steam added, is left to Steam.
pub fn leave_big_picture(session: Option<&BigPicture>) -> Leave {
    let Some(session) = session else {
        return Leave::Nothing;
    };
    match session.game {
        Some(app) => {
            let id = GameId::new(Source::Steam, &app.to_string());
            if super::app_id(&id) == app {
                Leave::ToLauncherWithGame(id)
            } else {
                Leave::ToLauncher
            }
        }
        None => Leave::ToLauncher,
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
    fn steams_first_start_waits_as_long_as_its_set_up_screen() {
        let start = Instant::now();
        let session = BigPicture::new(start, &[]).first_start();
        // The Setting Up Steam screen times out, or the user cancels it.
        assert_eq!(session.deadline(), None);
        assert!(!session.is_over(start + Duration::from_secs(3600)));
    }

    #[test]
    fn steams_own_windows_stay_behind_the_set_up_screen_until_its_sign_in() {
        let start = Instant::now();
        let mut session = BigPicture::new(start, &[]).first_start();
        // Steam's updater shows its progress in a small window of its own.
        let updater = WindowFacts {
            wm_class: vec!["steam".to_owned(), "Steam".to_owned()],
            width: 400,
            height: 100,
            ..steam_window(0x220_0010)
        };
        session.window_mapped(&updater, &[MAIN], start);
        assert!(session.is_opening());
        assert_eq!(session.claim(Some(MAIN)).window, Some(MAIN));
        // The sign-in is Steam's web helper's.
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], start);
        assert!(!session.is_opening());
        assert_eq!(session.claim(Some(MAIN)).window, Some(0x220_0035));
        // Once Steam is there, its other windows show as in any session.
        session.window_mapped(&updater, &[MAIN], start);
        assert_eq!(session.claim(Some(MAIN)).window, Some(0x220_0010));
    }

    #[test]
    fn how_to_leave_shows_once_for_a_while_after_big_picture_shows() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut session = BigPicture::new(start, &[]);
        // Not while Big Picture opens.
        assert!(!session.shows_notice(start));
        session.window_mapped(&steam_window(0x220_0035), &[], at(5));
        assert!(session.shows_notice(at(5)));
        assert_eq!(session.notice_until(), Some(at(5) + BIG_PICTURE_NOTICE));
        assert!(session.shows_notice(at(5) + BIG_PICTURE_NOTICE - Duration::from_millis(1)));
        assert!(!session.shows_notice(at(5) + BIG_PICTURE_NOTICE));
        // Another window of Steam's later says nothing again.
        session.window_unmapped(0x220_0035, at(60));
        session.window_mapped(&steam_window(0x220_0070), &[], at(61));
        assert_eq!(session.notice_until(), Some(at(5) + BIG_PICTURE_NOTICE));
        assert!(!session.shows_notice(at(61)));
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

    #[test]
    fn leaving_outside_a_session_changes_nothing() {
        assert_eq!(leave_big_picture(None), Leave::Nothing);
    }

    #[test]
    fn leaving_big_picture_brings_the_launcher_back() {
        let now = Instant::now();
        let mut session = BigPicture::new(now, &[]);
        // Leaving while Big Picture still opens gives up on it.
        assert_eq!(leave_big_picture(Some(&session)), Leave::ToLauncher);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], now);
        assert_eq!(leave_big_picture(Some(&session)), Leave::ToLauncher);
    }

    #[test]
    fn leaving_with_a_game_steam_started_keeps_it_playing_behind_the_launcher() {
        const BALATRO: u32 = 2_379_780;
        let now = Instant::now();
        let mut session = BigPicture::new(now, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], now);
        session.focusable_apps_changed(&[STEAM_APP_ID, BALATRO], now);
        session.window_unmapped(0x220_0035, now);
        assert_eq!(
            leave_big_picture(Some(&session)),
            Leave::ToLauncherWithGame(GameId::new(Source::Steam, "2379780"))
        );
        // The game ended: the launcher alone.
        session.focusable_apps_changed(&[STEAM_APP_ID], now);
        assert_eq!(leave_big_picture(Some(&session)), Leave::ToLauncher);
    }

    #[test]
    fn a_non_steam_game_steam_started_is_left_to_steam() {
        // A shortcut Steam added runs under an app ID that is no Steam game's.
        const SHORTCUT: u32 = 0xa1b2_c3d4;
        let now = Instant::now();
        let mut session = BigPicture::new(now, &[]);
        session.window_mapped(&steam_window(0x220_0035), &[MAIN], now);
        session.focusable_apps_changed(&[STEAM_APP_ID, SHORTCUT], now);
        assert_eq!(leave_big_picture(Some(&session)), Leave::ToLauncher);
    }
}
