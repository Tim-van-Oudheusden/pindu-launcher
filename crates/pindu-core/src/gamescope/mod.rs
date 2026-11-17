//! gamescope's `--steam` focus protocol, as plain data: who the launcher has to be inside a
//! `--steam` gamescope, which app IDs its focus list names, which windows get which app ID,
//! and who is in front. The X11 side lives in the UI crate's `gamescope_service`; this
//! module never touches a display.
//!
//! In `--steam` mode gamescope shows only windows with an app ID (`STEAM_GAME`), and only
//! when the root's `GAMESCOPECTRL_BASELAYER_APPID` names one of them. The launcher takes
//! Steam's place there: its main window is app [`STEAM_APP_ID`], it writes the list, and it
//! tags the windows of the games it starts. Tested against gamescope 3.17.0.

use std::ffi::{c_long, c_ulong};

use crate::gamepad::Button;
use crate::model::{GameId, Source};
use crate::sources::sha256::sha256_hex;

mod store_launch;
pub use store_launch::{STORE_LAUNCH_GRACE, STORE_LAUNCH_TIMEOUT, StoreLaunch};

/// The app ID gamescope knows as Steam, which the launcher's main window claims.
pub const STEAM_APP_ID: u32 = 769;

/// The bit that marks an app ID as not Steam's: Steam's own app IDs, and the ones gamescope
/// special-cases, all stay below it, as Steam's non-Steam shortcuts do.
const NOT_STEAM: u32 = 1 << 31;

/// Whether gamescope runs in `--steam` mode, from the root's `GAMESCOPE_FOCUSED_APP`
/// property as read (`None` when the root has none). gamescope writes it only in
/// `--steam` mode, empty before the first window maps, and never otherwise.
pub fn steam_mode(focused_app: Option<&[u32]>) -> bool {
    focused_app.is_some()
}

/// The app ID gamescope sees for a game: a Steam game's own Steam app ID, which Steam's
/// reaper gives its windows anyway, and for any other game a stable ID from its
/// [`GameId`] with the high bit set, which keeps it away from 0, [`STEAM_APP_ID`] and every
/// Steam app ID.
pub fn app_id(game: &GameId) -> u32 {
    if game.source() == Source::Steam
        && let Ok(id) = game.native_id().parse::<u32>()
        && id != 0
        && id < NOT_STEAM
    {
        return id;
    }
    let digest = sha256_hex(game.as_str().as_bytes());
    let hashed = u32::from_str_radix(&digest[..8], 16).expect("a SHA-256 digest is hex");
    hashed | NOT_STEAM
}

/// Who gamescope shows as the base layer: the launcher, or a game by its app ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Front {
    /// The launcher's main window.
    #[default]
    Launcher,
    /// The game with this app ID (see [`app_id`]).
    Game(u32),
}

/// The value of `GAMESCOPECTRL_BASELAYER_APPID`, gamescope's focus priority list, for
/// `front` while the games with the app IDs `running` run: `[game, 769]` with a game in
/// front, so the launcher shows once it is gone, and `[769, games…]` with the launcher in
/// front, so it shows without the list naming nothing on screen.
pub fn base_layer(front: Front, running: &[u32]) -> Vec<u32> {
    match front {
        Front::Game(game) => vec![game, STEAM_APP_ID],
        Front::Launcher => std::iter::once(STEAM_APP_ID)
            .chain(running.iter().copied())
            .collect(),
    }
}

/// A game the launcher expects a window from, as [`tag_for_window`] and [`front`] match
/// it: one we started that runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExpectedGame {
    /// The app ID its windows get (see [`app_id`]).
    pub app_id: u32,
    /// The window class it runs with, `pindu-<hash>`, which SDL games give their windows.
    pub window_class: String,
    /// The process ID of the game we started, when it runs in our PID namespace, so a
    /// window's `_NET_WM_PID` can name it.
    pub pid: Option<u32>,
}

impl ExpectedGame {
    /// The game with `id`, started as the process `pid` when we know it.
    pub fn new(id: &GameId, pid: Option<u32>) -> Self {
        Self {
            app_id: app_id(id),
            window_class: crate::launch::window_class(id),
            pid,
        }
    }
}

/// An X11 window's identifier, as Xlib has it.
pub type Xid = c_ulong;

/// What the launcher reads from a top-level window another client mapped, to decide its
/// app ID.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowFacts {
    /// The window.
    pub xid: Xid,
    /// The instance and class names of its `WM_CLASS`.
    pub wm_class: Vec<String>,
    /// Its `_NET_WM_PID`, when it has one.
    pub pid: Option<u32>,
    /// Its `STEAM_GAME`, when it has one already: Steam's reaper, or an earlier tag.
    pub steam_game: Option<u32>,
    /// Whether it is override-redirect: a menu, tooltip or splash that is never a base
    /// layer.
    pub override_redirect: bool,
    /// Its width in pixels.
    pub width: u32,
    /// Its height in pixels.
    pub height: u32,
}

/// The app ID to set as `STEAM_GAME` on a window that just mapped, or `None` to leave it
/// alone. A window whose `WM_CLASS` is an expected game's class is that game's; then one
/// whose `_NET_WM_PID` is an expected game's process; then, while exactly one game is
/// expected, any other top-level of another client, as gamescope-fg does. A window that
/// already has a non-zero app ID is never retagged, so Steam's reaper wins; the launcher's
/// own windows (`own`), override-redirect windows and one-pixel helpers are never tagged.
pub fn tag_for_window(window: &WindowFacts, expected: &[ExpectedGame], own: &[Xid]) -> Option<u32> {
    let tagged = window.steam_game.is_some_and(|id| id != 0);
    // gamescope's own rule for windows that are never shown: one pixel by one.
    let useless = window.width <= 1 && window.height <= 1;
    if tagged || useless || window.override_redirect || own.contains(&window.xid) {
        return None;
    }
    let by_class = expected
        .iter()
        .find(|game| window.wm_class.contains(&game.window_class));
    let by_pid = || {
        expected
            .iter()
            .find(|game| game.pid.is_some() && game.pid == window.pid)
    };
    let only = || match expected {
        [game] => Some(game),
        _ => None,
    };
    by_class
        .or_else(by_pid)
        .or_else(only)
        .map(|game| game.app_id)
}

/// Who is in front, from the root's `GAMESCOPE_FOCUSED_APP_GFX`: the app ID of the base
/// layer, which stays the game's while an overlay holds input (unlike
/// `GAMESCOPE_FOCUSED_APP`, which follows input). An expected game is in front when it is
/// the base layer; otherwise the launcher counts as in front, the launcher itself, nothing
/// yet, or an app it did not start.
pub fn front(focused_app_gfx: Option<u32>, expected: &[ExpectedGame]) -> Front {
    match focused_app_gfx {
        Some(id) if expected.iter().any(|game| game.app_id == id) => Front::Game(id),
        _ => Front::Launcher,
    }
}

/// What a controller button does while a game is in front, where the game gets every
/// button too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InGameIntent {
    /// Bring the launcher to the front, as Guide does over a game in Steam.
    ShowLauncher,
}

/// What `button` asks of the launcher while a game is in front: Guide brings the launcher
/// to the front; every other button is the game's.
pub fn in_game_intent(button: Button) -> Option<InGameIntent> {
    match button {
        Button::Guide => Some(InGameIntent::ShowLauncher),
        _ => None,
    }
}

/// A CARDINAL as Xlib wants it written: format-32 property data is an array of C `long`.
pub fn cardinal_to_long(value: u32) -> c_long {
    // `c_long` is 64 bits on every target the launcher ships for; Xlib sends the low 32.
    i64::from(value) as c_long
}

/// A CARDINAL as Xlib reads it back: a C `long` that comes back sign-extended when the
/// value has its high bit set, as every non-Steam [`app_id`] does.
pub fn cardinal_from_long(value: c_long) -> u32 {
    // Only the low 32 bits are the value; the rest is the sign extension.
    value as u32
}

/// The instance and class names of a `WM_CLASS` property as read: two strings, each ending
/// in a NUL byte. Names that are empty or not UTF-8 are left out.
pub fn wm_class_names(property: &[u8]) -> Vec<String> {
    property
        .split(|&byte| byte == 0)
        .filter(|name| !name.is_empty())
        .filter_map(|name| std::str::from_utf8(name).ok())
        .map(str::to_owned)
        .collect()
}

/// Who the launcher wants in front once the games it started change from `before` to
/// `running` (app IDs): a game that just started, as Play brings it up; the launcher once
/// the game it wanted in front is gone; otherwise still `wanted`.
pub fn front_after_sessions(wanted: Front, before: &[u32], running: &[u32]) -> Front {
    if let Some(&started) = running.iter().rfind(|id| !before.contains(id)) {
        return Front::Game(started);
    }
    match wanted {
        Front::Game(id) if !running.contains(&id) => Front::Launcher,
        _ => wanted,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_mode_follows_the_focused_app_property() {
        assert!(steam_mode(Some(&[])));
        assert!(steam_mode(Some(&[769])));
        assert!(!steam_mode(None));
    }

    #[test]
    fn a_steam_game_keeps_its_steam_app_id() {
        let game: GameId = "steam:620".parse().unwrap();
        assert_eq!(app_id(&game), 620);
    }

    #[test]
    fn other_games_get_a_stable_id_with_the_high_bit_set() {
        let celeste: GameId = "lutris:celeste".parse().unwrap();
        let hades: GameId = "heroic-epic:hades".parse().unwrap();
        assert_eq!(app_id(&celeste), app_id(&"lutris:celeste".parse().unwrap()));
        assert_ne!(app_id(&celeste), app_id(&hades));
        for id in [app_id(&celeste), app_id(&hades)] {
            assert_ne!(id & (1 << 31), 0, "{id:#x}");
        }
    }

    #[test]
    fn a_steam_id_that_is_no_number_is_hashed_like_any_other_game() {
        let game: GameId = "steam:not-a-number".parse().unwrap();
        assert_ne!(app_id(&game) & (1 << 31), 0);
    }

    #[test]
    fn the_base_layer_puts_the_game_in_front_first_and_the_launcher_after_it() {
        assert_eq!(base_layer(Front::Game(7), &[5, 7]), [7, STEAM_APP_ID]);
    }

    #[test]
    fn with_the_launcher_in_front_the_base_layer_lists_it_then_the_games() {
        assert_eq!(base_layer(Front::Launcher, &[5, 7]), [STEAM_APP_ID, 5, 7]);
        assert_eq!(base_layer(Front::Launcher, &[]), [STEAM_APP_ID]);
    }

    fn expected(id: &str, pid: Option<u32>) -> ExpectedGame {
        ExpectedGame::new(&id.parse().unwrap(), pid)
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
    fn a_window_with_a_games_class_gets_its_app_id() {
        let celeste = expected("lutris:celeste", None);
        let hades = expected("heroic-epic:hades", None);
        let facts = WindowFacts {
            wm_class: vec![hades.window_class.clone(), hades.window_class.clone()],
            ..window(0x40_0001)
        };
        assert_eq!(
            tag_for_window(&facts, &[celeste, hades.clone()], &[]),
            Some(hades.app_id)
        );
    }

    #[test]
    fn a_window_of_a_games_process_gets_its_app_id() {
        let celeste = expected("lutris:celeste", Some(4100));
        let hades = expected("heroic-epic:hades", Some(4200));
        let facts = WindowFacts {
            wm_class: vec!["hades".to_owned(), "Hades".to_owned()],
            pid: Some(4200),
            ..window(0x40_0001)
        };
        assert_eq!(
            tag_for_window(&facts, &[celeste, hades.clone()], &[]),
            Some(hades.app_id)
        );
    }

    #[test]
    fn with_one_game_expected_any_other_top_level_is_its() {
        let hades = expected("heroic-epic:hades", None);
        let facts = WindowFacts {
            wm_class: vec!["hades.exe".to_owned(), "hades.exe".to_owned()],
            ..window(0x40_0001)
        };
        assert_eq!(
            tag_for_window(&facts, std::slice::from_ref(&hades), &[]),
            Some(hades.app_id)
        );
        let celeste = expected("lutris:celeste", None);
        assert_eq!(tag_for_window(&facts, &[celeste, hades], &[]), None);
    }

    #[test]
    fn tagged_own_override_redirect_and_one_pixel_windows_are_left_alone() {
        let hades = expected("heroic-epic:hades", Some(4200));
        let games = std::slice::from_ref(&hades);
        let matching = WindowFacts {
            wm_class: vec![hades.window_class.clone()],
            pid: Some(4200),
            ..window(0x40_0001)
        };
        let tagged = WindowFacts {
            steam_game: Some(1_145_360),
            ..matching.clone()
        };
        assert_eq!(tag_for_window(&tagged, games, &[]), None);
        let untagged_zero = WindowFacts {
            steam_game: Some(0),
            ..matching.clone()
        };
        assert_eq!(
            tag_for_window(&untagged_zero, games, &[]),
            Some(hades.app_id)
        );
        assert_eq!(tag_for_window(&matching, games, &[0x40_0001]), None);
        let menu = WindowFacts {
            override_redirect: true,
            ..matching.clone()
        };
        assert_eq!(tag_for_window(&menu, games, &[]), None);
        let helper = WindowFacts {
            width: 1,
            height: 1,
            ..matching
        };
        assert_eq!(tag_for_window(&helper, games, &[]), None);
    }

    #[test]
    fn nothing_is_tagged_while_no_game_is_expected() {
        assert_eq!(tag_for_window(&window(0x40_0001), &[], &[]), None);
    }

    #[test]
    fn an_expected_game_as_the_base_layer_is_in_front() {
        let hades = expected("heroic-epic:hades", None);
        let games = std::slice::from_ref(&hades);
        assert_eq!(front(Some(hades.app_id), games), Front::Game(hades.app_id));
        assert_eq!(front(Some(STEAM_APP_ID), games), Front::Launcher);
        assert_eq!(front(None, games), Front::Launcher);
        assert_eq!(front(Some(1_145_360), games), Front::Launcher);
    }

    #[test]
    fn in_game_only_guide_brings_the_launcher_to_the_front() {
        assert_eq!(
            in_game_intent(Button::Guide),
            Some(InGameIntent::ShowLauncher)
        );
        for button in [
            Button::South,
            Button::East,
            Button::Start,
            Button::Select,
            Button::RightStick,
            Button::Misc1,
        ] {
            assert_eq!(in_game_intent(button), None, "{button:?}");
        }
    }

    #[test]
    fn a_cardinal_with_the_high_bit_survives_xlibs_c_long() {
        let id = app_id(&"lutris:celeste".parse().unwrap());
        assert_eq!(cardinal_from_long(cardinal_to_long(id)), id);
        assert_eq!(
            cardinal_from_long(cardinal_to_long(STEAM_APP_ID)),
            STEAM_APP_ID
        );
        // Xlib hands a 32-bit value with the high bit set back sign-extended.
        assert_eq!(cardinal_from_long(-0x7edc_ba99), 0x8123_4567);
    }

    #[test]
    fn wm_class_gives_its_instance_and_class_names() {
        assert_eq!(
            wm_class_names(b"pindu-0123456789ab\0pindu-0123456789ab\0"),
            ["pindu-0123456789ab", "pindu-0123456789ab"]
        );
        assert_eq!(
            wm_class_names(b"hades.exe\0Hades\0"),
            ["hades.exe", "Hades"]
        );
        // Some clients leave out the last NUL, or the instance name.
        assert_eq!(wm_class_names(b"\0Game"), ["Game"]);
        assert!(wm_class_names(b"").is_empty());
    }

    #[test]
    fn a_game_that_starts_comes_to_the_front_and_the_launcher_when_it_ends() {
        assert_eq!(
            front_after_sessions(Front::Launcher, &[], &[5]),
            Front::Game(5)
        );
        assert_eq!(
            front_after_sessions(Front::Launcher, &[5], &[5, 7]),
            Front::Game(7)
        );
        assert_eq!(
            front_after_sessions(Front::Game(7), &[5, 7], &[5]),
            Front::Launcher
        );
        // Another game ending leaves the front alone, the launcher or a game.
        assert_eq!(
            front_after_sessions(Front::Game(7), &[5, 7], &[7]),
            Front::Game(7)
        );
        assert_eq!(
            front_after_sessions(Front::Launcher, &[5, 7], &[7]),
            Front::Launcher
        );
    }
}
