//! The Steam client's own windows during a Steam store launch, which it asks its questions
//! in: a sign-in, install, launch option or cloud conflict prompt. In a `--steam` gamescope
//! the Steam client runs as Big Picture, one `steamwebhelper` window whose prompts show
//! inside it. It tags its windows `STEAM_GAME = 769`, the launcher's app ID, and the
//! launcher pins its main window while the game it wants in front has no window (see
//! [`BaseLayerClaim`](super::BaseLayerClaim)), so without this rule they never show and a
//! launch that waits on the user's answer waits with nothing on screen to act on.

use super::{BaseLayerClaim, STEAM_APP_ID, WindowFacts, Xid};

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
/// the ones open when it started first, in the order they mapped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct SteamPrompts {
    mapped: Vec<Xid>,
}

impl SteamPrompts {
    /// Counts `window`, which another client just mapped or had open when the launch
    /// started, when it is the Steam client's (`own` holds the launcher's windows).
    pub(crate) fn mapped(&mut self, window: &WindowFacts, own: &[Xid]) {
        // gamescope's own rule for windows that are never shown: one pixel by one.
        let useless = window.width <= 1 && window.height <= 1;
        let steams = window.steam_game == Some(STEAM_APP_ID);
        if steams && !useless && !window.override_redirect && !own.contains(&window.xid) {
            self.mapped.push(window.xid);
        }
    }

    /// Forgets `xid` once it is unmapped.
    pub(crate) fn unmapped(&mut self, xid: Xid) {
        self.mapped.retain(|&mapped| mapped != xid);
    }

    /// The newest of them, which the launcher shows.
    pub(crate) fn newest(&self) -> Option<Xid> {
        self.mapped.last().copied()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::gamescope::{
        BaseLayerClaim, Front, STEAM_APP_ID, STORE_LAUNCH_TIMEOUT, StoreLaunch, WindowFacts, Xid,
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
    fn a_steam_window_that_maps_while_the_launch_waits_is_shown() {
        let mut spire = launch(Instant::now());
        spire.window_mapped(&steam_window(0x220_0035), &[]);
        assert_eq!(spire.steam_window(Front::Game(646_570)), Some(0x220_0035));
    }

    #[test]
    fn only_the_steam_clients_own_top_levels_count() {
        let mut spire = launch(Instant::now());
        let front = Front::Game(646_570);
        // The launcher's own window, also app 769.
        spire.window_mapped(&steam_window(0x60_0004), &[0x60_0004]);
        // A window with another app ID, or none yet (the store launch tags those).
        let game = WindowFacts {
            steam_game: Some(646_570),
            ..steam_window(0x200_0009)
        };
        spire.window_mapped(&game, &[]);
        let untagged = WindowFacts {
            steam_game: None,
            ..steam_window(0x200_000a)
        };
        spire.window_mapped(&untagged, &[]);
        // Menus, tooltips and one-pixel helpers are never a base layer.
        let menu = WindowFacts {
            override_redirect: true,
            ..steam_window(0x220_0040)
        };
        spire.window_mapped(&menu, &[]);
        let helper = WindowFacts {
            width: 1,
            height: 1,
            ..steam_window(0x160_0001)
        };
        spire.window_mapped(&helper, &[]);
        assert_eq!(spire.steam_window(front), None);
    }

    #[test]
    fn the_steam_window_shows_only_while_the_launch_is_wanted_in_front() {
        let mut spire = launch(Instant::now());
        spire.window_mapped(&steam_window(0x220_0035), &[]);
        assert_eq!(spire.steam_window(Front::Launcher), None);
        assert_eq!(spire.steam_window(Front::Game(620)), None);
    }

    #[test]
    fn once_the_games_window_shows_the_steam_windows_are_left_to_the_list() {
        let start = Instant::now();
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[]);
        spire.focusable_apps_changed(&[STEAM_APP_ID, 646_570], start);
        assert_eq!(spire.steam_window(front), None);
        // The game ends; Steam's cloud sync window, or its library, is not the launch's.
        let after = start + Duration::from_secs(600);
        spire.focusable_apps_changed(&[STEAM_APP_ID], after);
        spire.window_mapped(&steam_window(0x220_0050), &[]);
        assert_eq!(spire.steam_window(front), None);
    }

    #[test]
    fn the_newest_steam_window_still_mapped_is_shown() {
        let start = Instant::now();
        let front = Front::Game(646_570);
        let mut spire = launch(start);
        // Steam's library, then its install prompt over it.
        spire.window_mapped(&steam_window(0x220_0035), &[]);
        spire.window_mapped(&steam_window(0x220_0060), &[]);
        assert_eq!(spire.steam_window(front), Some(0x220_0060));
        spire.window_unmapped(0x220_0060, start);
        assert_eq!(spire.steam_window(front), Some(0x220_0035));
        spire.window_unmapped(0x220_0035, start);
        assert_eq!(spire.steam_window(front), None);
    }

    #[test]
    fn the_launch_waits_for_the_users_answer_then_for_the_game_again() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut spire = launch(start);
        spire.window_mapped(&steam_window(0x220_0035), &[]);
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
        let hades = "heroic-epic:hades".parse().unwrap();
        let mut launch = StoreLaunch::new(&hades, Instant::now());
        launch.window_mapped(&steam_window(0x220_0035), &[]);
        assert_eq!(launch.steam_window(Front::Game(launch.app_id())), None);
    }
}
