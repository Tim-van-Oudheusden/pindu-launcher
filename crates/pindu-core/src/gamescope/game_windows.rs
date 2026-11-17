//! A running game's windows, as gamescope lists them, and the one the user picks from the
//! menu drawer over the game. Many games open through a launcher of their own, so a game
//! can have two windows; gamescope shows the one mapped last. The root's
//! `GAMESCOPE_FOCUSABLE_WINDOWS` lists every window it could show, and the launcher pins the
//! picked one with `GAMESCOPECTRL_BASELAYER_WINDOW` while the game is wanted in front.

use super::{BaseLayerClaim, Xid};

/// A window gamescope could show, from the root's `GAMESCOPE_FOCUSABLE_WINDOWS`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusableWindow {
    /// The window.
    pub xid: Xid,
    /// Its app ID (`STEAM_GAME`, or the one gamescope derived).
    pub app_id: u32,
}

/// The windows in `GAMESCOPE_FOCUSABLE_WINDOWS` as read: `[window, app ID, process]` for
/// each, in gamescope's order. An incomplete last one is left out.
pub fn focusable_windows(property: &[u32]) -> Vec<FocusableWindow> {
    let (windows, _incomplete) = property.as_chunks::<3>();
    windows
        .iter()
        .map(|&[xid, app_id, _pid]| FocusableWindow {
            xid: Xid::from(xid),
            app_id,
        })
        .collect()
}

/// The windows of the game with `app_id` among `windows`, in gamescope's order.
pub fn game_windows(windows: &[FocusableWindow], app_id: u32) -> Vec<Xid> {
    windows
        .iter()
        .filter(|window| window.app_id == app_id)
        .map(|window| window.xid)
        .collect()
}

/// The window of a running game the user picked to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PickedWindow {
    /// The game's app ID.
    pub app_id: u32,
    /// The window.
    pub xid: Xid,
}

impl BaseLayerClaim {
    /// The claim with `picked` pinned, while gamescope shows its game rather than the
    /// launcher (the list decides, no window is pinned) and `windows` still lists it with
    /// that game's app ID.
    #[must_use]
    pub fn picking(self, picked: Option<PickedWindow>, windows: &[FocusableWindow]) -> Self {
        let wanted = self.app_ids.first().copied();
        let pin = picked.filter(|picked| {
            self.window.is_none()
                && wanted == Some(picked.app_id)
                && game_windows(windows, picked.app_id).contains(&picked.xid)
        });
        match pin {
            Some(picked) => Self {
                window: Some(picked.xid),
                ..self
            },
            None => self,
        }
    }
}

/// A window's title from its `_NET_WM_NAME` (UTF-8) and its `WM_NAME` as read, the first
/// that says something; `None` when neither does.
pub fn window_title(net_wm_name: Option<&[u8]>, wm_name: Option<&[u8]>) -> Option<String> {
    [net_wm_name, wm_name]
        .into_iter()
        .flatten()
        .map(|name| {
            String::from_utf8_lossy(name)
                .trim_matches(['\0', ' '])
                .to_owned()
        })
        .find(|title| !title.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamescope::{BaseLayerClaim, Front, STEAM_APP_ID};

    const GAME: u32 = 0x8000_1234;
    const MAIN: Xid = 0x60_0004;

    #[test]
    fn the_focusable_windows_are_triplets_of_window_app_and_process() {
        let property = [0x40_0001, GAME, 4100, 0x60_0004, STEAM_APP_ID, 2, 0x50_0001];
        assert_eq!(
            focusable_windows(&property),
            [
                FocusableWindow {
                    xid: 0x40_0001,
                    app_id: GAME,
                },
                FocusableWindow {
                    xid: 0x60_0004,
                    app_id: STEAM_APP_ID,
                },
            ]
        );
    }

    #[test]
    fn a_games_windows_are_the_ones_with_its_app_id() {
        let windows = focusable_windows(&[
            0x40_0001,
            GAME,
            4100,
            0x60_0004,
            STEAM_APP_ID,
            2,
            0x40_0007,
            GAME,
            4101,
        ]);
        assert_eq!(game_windows(&windows, GAME), [0x40_0001, 0x40_0007]);
    }

    #[test]
    fn a_picked_window_is_pinned_while_its_game_is_wanted_in_front_and_it_is_there() {
        let windows = focusable_windows(&[0x40_0001, GAME, 4100, 0x40_0007, GAME, 4101]);
        let picked = Some(PickedWindow {
            app_id: GAME,
            xid: 0x40_0001,
        });
        let claim = |front| {
            BaseLayerClaim::new(front, &[GAME], &[GAME, STEAM_APP_ID], Some(MAIN))
                .picking(picked, &windows)
        };
        assert_eq!(claim(Front::Game(GAME)).window, Some(0x40_0001));
        // The launcher in front pins the launcher, not the picked window.
        assert_eq!(claim(Front::Launcher).window, Some(MAIN));
        // Gone, it leaves the base layer to the list again.
        let gone = focusable_windows(&[0x40_0007, GAME, 4101]);
        let claim = BaseLayerClaim::new(Front::Game(GAME), &[GAME], &[GAME], Some(MAIN))
            .picking(picked, &gone);
        assert_eq!(claim.window, None);
    }

    #[test]
    fn a_windows_title_is_its_net_wm_name_then_its_wm_name() {
        assert_eq!(
            window_title(Some("Balatro".as_bytes()), Some(b"steam_app")),
            Some("Balatro".to_owned())
        );
        assert_eq!(
            window_title(None, Some(b"Launcher\0")),
            Some("Launcher".to_owned())
        );
        assert_eq!(window_title(Some(b""), Some(b" ")), None);
        assert_eq!(window_title(None, None), None);
    }
}
