//! The Steam client's own windows during a Steam store launch, which it asks its questions
//! in: a sign-in, install, launch option or cloud conflict prompt. In a `--steam` gamescope
//! the Steam client runs as Big Picture, one `steamwebhelper` window whose prompts show
//! inside it. It tags its windows `STEAM_GAME = 769`, the launcher's app ID, and the
//! launcher pins its main window while the game it wants in front has no window (see
//! [`BaseLayerClaim`](super::BaseLayerClaim)), so without this rule they never show and a
//! launch that waits on the user's answer waits with nothing on screen to act on. Steam also
//! shows its own window while it starts the game, which the launcher's starting screen
//! covers instead, so a window counts only once it has stayed open for a while.

use std::time::{Duration, Instant};

use super::{BaseLayerClaim, STEAM_APP_ID, WindowFacts, Xid};

/// How long one of the Steam client's windows stays open during a Steam store launch
/// before the launcher shows it. Steam's own "starting game" window closes before then; a
/// prompt that waits on the user's answer does not.
pub const STEAM_PROMPT_DELAY: Duration = Duration::from_secs(5);

impl BaseLayerClaim {
    /// The claim with `steam_window`, the Steam client's window a store launch waits on
    /// (see [`StoreLaunch::steam_window`](super::StoreLaunch::steam_window)), pinned in
    /// place of the launcher's main window.
    #[must_use]
    pub fn showing(self, steam_window: Option<Xid>) -> Self {
        // Only where the launcher holds the base layer: the game's window wins otherwise.
        match (self.window, steam_window) {
            (Some(_), Some(xid)) => Self {
                window: Some(xid),
                ..self
            },
            _ => self,
        }
    }
}

/// The Steam client's windows that are open while a Steam store launch waits for its game,
/// the ones open when it started first, in the order they mapped, each with when it did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SteamPrompts {
    mapped: Vec<(Xid, Instant)>,
}

impl SteamPrompts {
    /// Counts `window`, which another client mapped or had open when the launch started at
    /// `now`, when it is the Steam client's (`own` holds the launcher's windows).
    pub(crate) fn mapped(&mut self, window: &WindowFacts, own: &[Xid], now: Instant) {
        // gamescope's own rule for windows that are never shown: one pixel by one.
        let useless = window.width <= 1 && window.height <= 1;
        let steams = window.steam_game == Some(STEAM_APP_ID);
        if steams && !useless && !window.override_redirect && !own.contains(&window.xid) {
            self.mapped.push((window.xid, now));
        }
    }

    /// Forgets `xid` once it is unmapped.
    pub(crate) fn unmapped(&mut self, xid: Xid) {
        self.mapped.retain(|&(mapped, _)| mapped != xid);
    }

    /// Whether any of them is open, shown or not yet.
    pub(crate) fn any(&self) -> bool {
        !self.mapped.is_empty()
    }

    /// The newest of them that has stayed open for [`STEAM_PROMPT_DELAY`] at `now`, which
    /// the launcher shows.
    pub(crate) fn newest(&self, now: Instant) -> Option<Xid> {
        self.mapped
            .iter()
            .rev()
            .find(|&&(_, since)| since + STEAM_PROMPT_DELAY <= now)
            .map(|&(xid, _)| xid)
    }

    /// When the next of them still hidden at `now` shows.
    pub(crate) fn next_reveal(&self, now: Instant) -> Option<Instant> {
        self.mapped
            .iter()
            .map(|&(_, since)| since + STEAM_PROMPT_DELAY)
            .filter(|&shows| shows > now)
            .min()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::gamescope::{
        BaseLayerClaim, Front, STEAM_APP_ID, STEAM_PROMPT_DELAY, STORE_LAUNCH_TIMEOUT, StoreLaunch,
        WindowFacts, Xid,
    };

    fn launch(at: Instant) -> StoreLaunch {
        StoreLaunch::new(&"steam:646570".parse().unwrap(), at)
    }

    /// A window the Steam client mapped: `steamwebhelper`, tagged as app 769 by Steam.
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
    fn a_steam_window_shows_once_it_has_stayed_open_for_the_delay() {
        let start = Instant::now();
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        let mapped = start + Duration::from_secs(1);
        spire.window_mapped(&steam_window(0x220_0035), &[], mapped);
        // Steam's own window that shows the game starting stays behind the launcher.
        assert_eq!(spire.steam_window(front, mapped), None);
        let shown = mapped + STEAM_PROMPT_DELAY;
        assert_eq!(spire.reveal_at(mapped), Some(shown));
        assert_eq!(
            spire.steam_window(front, shown - Duration::from_millis(1)),
            None
        );
        // Still open: a prompt that waits on the user's answer.
        assert_eq!(spire.steam_window(front, shown), Some(0x220_0035));
        assert_eq!(spire.reveal_at(shown), None);
    }

    #[test]
    fn a_steam_window_that_closes_before_the_delay_never_shows() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[], at(1));
        // No time-out while it is open, shown yet or not.
        assert_eq!(spire.deadline(), None);
        spire.window_unmapped(0x220_0035, at(3));
        assert_eq!(spire.reveal_at(at(3)), None);
        assert_eq!(spire.steam_window(front, at(10)), None);
        assert_eq!(spire.deadline(), Some(at(3) + STORE_LAUNCH_TIMEOUT));
    }

    #[test]
    fn only_the_steam_clients_own_top_levels_count() {
        let start = Instant::now();
        let mut spire = launch(start);
        let front = Front::Game(646_570);
        // The launcher's own window, also app 769.
        spire.window_mapped(&steam_window(0x60_0004), &[0x60_0004], start);
        // A window with another app ID, or none yet (the store launch tags those).
        let game = WindowFacts {
            steam_game: Some(646_570),
            ..steam_window(0x200_0009)
        };
        spire.window_mapped(&game, &[], start);
        let untagged = WindowFacts {
            steam_game: None,
            ..steam_window(0x200_000a)
        };
        spire.window_mapped(&untagged, &[], start);
        // Menus, tooltips and one-pixel helpers are never a base layer.
        let menu = WindowFacts {
            override_redirect: true,
            ..steam_window(0x220_0040)
        };
        spire.window_mapped(&menu, &[], start);
        let helper = WindowFacts {
            width: 1,
            height: 1,
            ..steam_window(0x160_0001)
        };
        spire.window_mapped(&helper, &[], start);
        assert_eq!(spire.steam_window(front, start + STEAM_PROMPT_DELAY), None);
        assert_eq!(spire.reveal_at(start), None);
        assert!(spire.deadline().is_some());
    }

    #[test]
    fn the_steam_window_shows_only_while_the_launch_is_wanted_in_front() {
        let start = Instant::now();
        let shown = start + STEAM_PROMPT_DELAY;
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[], start);
        assert_eq!(spire.steam_window(Front::Launcher, shown), None);
        assert_eq!(spire.steam_window(Front::Game(620), shown), None);
    }

    #[test]
    fn once_the_games_window_shows_the_steam_windows_are_left_to_the_list() {
        let start = Instant::now();
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[], start);
        spire.focusable_apps_changed(&[STEAM_APP_ID, 646_570], start);
        assert_eq!(spire.steam_window(front, start + STEAM_PROMPT_DELAY), None);
        assert_eq!(spire.reveal_at(start), None);
        // The game ends; Steam's cloud sync window, or its library, is not the launch's.
        let after = start + Duration::from_secs(600);
        spire.focusable_apps_changed(&[STEAM_APP_ID], after);
        spire.window_mapped(&steam_window(0x220_0050), &[], after);
        assert_eq!(spire.steam_window(front, after + STEAM_PROMPT_DELAY), None);
    }

    #[test]
    fn the_newest_steam_window_shown_and_still_mapped_is_shown() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        // Steam's library, then its install prompt over it.
        spire.window_mapped(&steam_window(0x220_0035), &[], at(0));
        spire.window_mapped(&steam_window(0x220_0060), &[], at(3));
        assert_eq!(spire.steam_window(front, at(5)), Some(0x220_0035));
        assert_eq!(spire.reveal_at(at(5)), Some(at(3) + STEAM_PROMPT_DELAY));
        assert_eq!(spire.steam_window(front, at(8)), Some(0x220_0060));
        spire.window_unmapped(0x220_0060, at(9));
        assert_eq!(spire.steam_window(front, at(9)), Some(0x220_0035));
        spire.window_unmapped(0x220_0035, at(9));
        assert_eq!(spire.steam_window(front, at(9)), None);
    }

    #[test]
    fn the_launch_waits_for_the_users_answer_then_for_the_game_again() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[], start);
        // Signing in takes as long as it takes.
        assert_eq!(spire.deadline(), None);
        assert!(!spire.is_over(at(600)));
        // Answered: the game gets the full time-out from then on.
        spire.window_unmapped(0x220_0035, at(600));
        assert_eq!(spire.deadline(), Some(at(600) + STORE_LAUNCH_TIMEOUT));
    }

    #[test]
    fn the_claim_pins_the_steam_window_in_the_launchers_place() {
        const MAIN: Xid = 0x60_0004;
        let front = Front::Game(646_570);
        let waiting = BaseLayerClaim::new(front, &[646_570], &[STEAM_APP_ID], Some(MAIN));
        let shown = waiting.clone().showing(Some(0x220_0035));
        assert_eq!(shown.window, Some(0x220_0035));
        assert_eq!(shown.app_ids, waiting.app_ids);
        assert_eq!(waiting.clone().showing(None), waiting);
        // The game's own window wins once it is there.
        let focusable = [STEAM_APP_ID, 646_570];
        let there = BaseLayerClaim::new(front, &[646_570], &focusable, Some(MAIN));
        assert_eq!(there.clone().showing(Some(0x220_0035)), there);
    }

    #[test]
    fn a_launch_through_another_store_never_shows_the_steam_client() {
        let start = Instant::now();
        let hades = "heroic-epic:hades".parse().unwrap();
        let mut launch = StoreLaunch::new(&hades, start);
        launch.window_mapped(&steam_window(0x220_0035), &[], start);
        let front = Front::Game(launch.app_id());
        assert_eq!(launch.steam_window(front, start + STEAM_PROMPT_DELAY), None);
        assert_eq!(launch.reveal_at(start), None);
    }
}
