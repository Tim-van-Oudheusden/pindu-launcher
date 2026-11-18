//! gamescope-session-plus: what changes when the launcher is a session's client, started
//! with `--session` by the `sessions.d/pindu` client file. session-plus exports nothing
//! that says so, and the session ends when its client exits, so with the flag closing the
//! window does not quit the app, and the menu drawer's Power level offers Switch to
//! Desktop where the distribution has `os-session-select`. Here too is what the Power
//! level lists ([`power_level`]): in the launcher's own session, the flag's or a `--steam`
//! gamescope's that the display manager started ([`runs_as_own_session`]), it leaves out
//! Exit Pindu Launcher, as quitting would leave nothing on the screen.

use std::collections::BTreeMap;
use std::path::Path;

use crate::launch::Command;
use crate::system::power::{PowerEntry, PowerState};

/// Where the distribution keeps `os-session-select`, on the host.
const OS_SESSION_SELECT: &str = "/usr/lib/os-session-select";

/// What the launcher knows about the session it runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Session {
    /// `--session`: this process is gamescope-session-plus's client.
    pub client: bool,
    /// The distribution has `os-session-select` (see [`has_os_session_select`]).
    pub os_session_select: bool,
    /// The app may run commands on the host: always outside Flatpak, and inside it with
    /// the host-spawn override (see [`crate::permissions::host_spawn_allowed`]).
    pub host_spawn: bool,
}

/// The Power level's Switch to Desktop entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchToDesktop {
    /// Whether the app may run `os-session-select`; when not, the entry shows insensitive
    /// and says why.
    pub allowed: bool,
}

impl Session {
    /// Whether closing the fullscreen window quits the app, as closing an app's last window
    /// does: not for a session's client, whose exit would end the session.
    pub fn closing_quits(&self) -> bool {
        !self.client
    }

    /// The Power level's Switch to Desktop entry: offered to a session's client where the
    /// distribution has `os-session-select`, and allowed where the app may run it on the
    /// host.
    pub fn switch_to_desktop(&self) -> Option<SwitchToDesktop> {
        (self.client && self.os_session_select).then_some(SwitchToDesktop {
            allowed: self.host_spawn,
        })
    }
}

/// Whether the distribution has `os-session-select`, its script that picks the session
/// to return to, under the host's `/usr` (`host_usr`: `/run/host/usr` inside Flatpak,
/// through `host-os:ro`).
pub fn has_os_session_select(host_usr: &Path) -> bool {
    host_usr.join("lib/os-session-select").is_file()
}

/// The command Switch to Desktop runs on the host: `os-session-select desktop`, which
/// makes the desktop the session to return to and, on most distributions, ends this one.
pub fn switch_to_desktop_command() -> Command {
    Command {
        argv: vec![OS_SESSION_SELECT.to_owned(), "desktop".to_owned()],
        environment: BTreeMap::new(),
        working_dir: None,
    }
}

/// Whether Switch to Desktop asks first: while a game we started runs, as Restart does,
/// since ending the session ends the game and its unsaved progress is lost.
pub fn asks_before_switching(game_running: bool) -> bool {
    game_running
}

/// An entry of the menu drawer's Power level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerLevelEntry {
    /// Suspend, Restart or Power Off, as the system allows it.
    Power(PowerEntry),
    /// Switch to Desktop, for gamescope-session-plus's client.
    SwitchToDesktop(SwitchToDesktop),
    /// Exit Pindu Launcher, which quits the app.
    Exit,
}

/// The Power level's entries, top to bottom: Suspend, Restart and Power Off as `power`
/// lists them, Switch to Desktop where `switch` offers it, then Exit Pindu Launcher where
/// `exit` says quitting leaves something behind, which is not so in the launcher's own
/// session (see [`runs_as_own_session`]). The drawer has its Power entry while the level
/// lists anything.
pub fn power_level(
    power: &PowerState,
    switch: Option<SwitchToDesktop>,
    exit: bool,
) -> Vec<PowerLevelEntry> {
    let actions = power.entries.iter().copied().map(PowerLevelEntry::Power);
    let switch = switch.map(PowerLevelEntry::SwitchToDesktop);
    let exit = exit.then_some(PowerLevelEntry::Exit);
    actions.chain(switch).chain(exit).collect()
}

/// Whether the launcher runs as its own login session, where nothing is left once it
/// quits: as gamescope-session-plus's client (`client`), or in a `--steam` gamescope
/// (`steam_gamescope`) that the display manager started as the login session, whose
/// desktop name (`XDG_SESSION_DESKTOP`, else `DESKTOP_SESSION`, read through `var`) names
/// gamescope or Pindu. A gamescope nested in a desktop sets `XDG_CURRENT_DESKTOP` as well,
/// so that one cannot tell them apart.
pub fn runs_as_own_session(
    client: bool,
    steam_gamescope: bool,
    var: impl Fn(&str) -> Option<String>,
) -> bool {
    if client {
        return true;
    }
    if !steam_gamescope {
        return false;
    }
    let desktop = ["XDG_SESSION_DESKTOP", "DESKTOP_SESSION"]
        .into_iter()
        .filter_map(var)
        .find(|name| !name.is_empty())
        .unwrap_or_default()
        .to_lowercase();
    desktop.contains("gamescope") || desktop.contains("pindu")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system::power::{PowerAction, PowerEntry};

    #[test]
    fn closing_the_window_quits_only_outside_a_session() {
        assert!(Session::default().closing_quits());
        let session = Session {
            client: true,
            ..Session::default()
        };
        assert!(!session.closing_quits());
    }

    #[test]
    fn os_session_select_is_found_in_the_hosts_lib() {
        let usr = tempfile::tempdir().unwrap();
        assert!(!has_os_session_select(usr.path()));
        std::fs::create_dir(usr.path().join("lib")).unwrap();
        std::fs::write(usr.path().join("lib/os-session-select"), "#!/bin/sh\n").unwrap();
        assert!(has_os_session_select(usr.path()));
    }

    #[test]
    fn switch_to_desktop_needs_the_flag_and_os_session_select() {
        let session = |client, os_session_select| Session {
            client,
            os_session_select,
            host_spawn: true,
        };
        assert_eq!(session(false, false).switch_to_desktop(), None);
        assert_eq!(session(false, true).switch_to_desktop(), None);
        assert_eq!(session(true, false).switch_to_desktop(), None);
        assert_eq!(
            session(true, true).switch_to_desktop(),
            Some(SwitchToDesktop { allowed: true })
        );
    }

    #[test]
    fn switch_to_desktop_is_greyed_out_without_host_spawn() {
        let session = Session {
            client: true,
            os_session_select: true,
            host_spawn: false,
        };
        assert_eq!(
            session.switch_to_desktop(),
            Some(SwitchToDesktop { allowed: false })
        );
    }

    #[test]
    fn switch_to_desktop_runs_os_session_select_on_the_host_as_an_argument_list() {
        let command = switch_to_desktop_command();
        assert_eq!(command.argv, ["/usr/lib/os-session-select", "desktop"]);
        assert!(command.environment.is_empty());
        assert_eq!(
            command.flatpak_spawn_argv(),
            [
                "flatpak-spawn",
                "--host",
                "--",
                "/usr/lib/os-session-select",
                "desktop"
            ]
        );
    }

    #[test]
    fn switching_to_the_desktop_asks_first_only_while_a_game_runs() {
        assert!(asks_before_switching(true));
        assert!(!asks_before_switching(false));
    }

    fn allowed(action: PowerAction) -> PowerEntry {
        PowerEntry {
            action,
            allowed: true,
        }
    }

    #[test]
    fn the_power_level_lists_the_actions_then_switch_to_desktop_then_exit() {
        let power = PowerState {
            entries: PowerAction::ALL.map(allowed).to_vec(),
            ..PowerState::default()
        };
        let switch = SwitchToDesktop { allowed: true };
        assert_eq!(
            power_level(&power, Some(switch), true),
            [
                PowerLevelEntry::Power(allowed(PowerAction::Suspend)),
                PowerLevelEntry::Power(allowed(PowerAction::Restart)),
                PowerLevelEntry::Power(allowed(PowerAction::PowerOff)),
                PowerLevelEntry::SwitchToDesktop(switch),
                PowerLevelEntry::Exit,
            ]
        );
        assert_eq!(
            power_level(&power, None, true),
            [
                PowerLevelEntry::Power(allowed(PowerAction::Suspend)),
                PowerLevelEntry::Power(allowed(PowerAction::Restart)),
                PowerLevelEntry::Power(allowed(PowerAction::PowerOff)),
                PowerLevelEntry::Exit,
            ]
        );
    }

    #[test]
    fn the_launcher_s_own_session_has_no_exit() {
        let power = PowerState {
            entries: vec![allowed(PowerAction::PowerOff)],
            ..PowerState::default()
        };
        assert_eq!(
            power_level(&power, None, false),
            [PowerLevelEntry::Power(allowed(PowerAction::PowerOff))]
        );
    }

    #[test]
    fn exit_alone_still_fills_the_level() {
        assert_eq!(
            power_level(&PowerState::default(), None, true),
            [PowerLevelEntry::Exit]
        );
    }

    #[test]
    fn switch_to_desktop_alone_fills_the_level_and_nothing_at_all_leaves_it_empty() {
        let none = PowerState::default();
        let restart = PowerState {
            entries: vec![allowed(PowerAction::Restart)],
            ..PowerState::default()
        };
        let switch = Some(SwitchToDesktop { allowed: false });
        assert!(power_level(&none, None, false).is_empty());
        assert_eq!(
            power_level(&none, switch, false),
            [PowerLevelEntry::SwitchToDesktop(SwitchToDesktop {
                allowed: false
            })]
        );
        assert!(!power_level(&restart, None, false).is_empty());
        assert!(!power_level(&restart, switch, false).is_empty());
    }

    fn desktop(name: &'static str, value: &'static str) -> impl Fn(&str) -> Option<String> {
        move |asked| (asked == name).then(|| value.to_owned())
    }

    #[test]
    fn the_session_flag_alone_makes_the_launcher_its_own_session() {
        assert!(runs_as_own_session(true, false, |_| None));
        assert!(runs_as_own_session(
            true,
            true,
            desktop("XDG_SESSION_DESKTOP", "gnome")
        ));
        assert!(!runs_as_own_session(false, false, |_| None));
    }

    #[test]
    fn a_steam_gamescope_is_its_own_session_when_the_login_session_names_it() {
        for (name, value) in [
            ("XDG_SESSION_DESKTOP", "pindu-gamescope"),
            ("XDG_SESSION_DESKTOP", "gamescope-session-pindu"),
            ("XDG_SESSION_DESKTOP", "Pindu"),
            ("DESKTOP_SESSION", "gamescope"),
        ] {
            assert!(
                runs_as_own_session(false, true, desktop(name, value)),
                "{name}={value}"
            );
            assert!(
                !runs_as_own_session(false, false, desktop(name, value)),
                "{name}={value} without a --steam gamescope"
            );
        }
    }

    #[test]
    fn a_steam_gamescope_nested_in_a_desktop_is_not_its_own_session() {
        assert!(!runs_as_own_session(
            false,
            true,
            desktop("XDG_SESSION_DESKTOP", "gnome")
        ));
        assert!(!runs_as_own_session(false, true, |_| None));
        let gnome_over_gamescope = |name: &str| match name {
            "XDG_SESSION_DESKTOP" => Some("gnome".to_owned()),
            "DESKTOP_SESSION" => Some("gamescope".to_owned()),
            _ => None,
        };
        assert!(!runs_as_own_session(false, true, gnome_over_gamescope));
        let empty_first = |name: &str| match name {
            "XDG_SESSION_DESKTOP" => Some(String::new()),
            "DESKTOP_SESSION" => Some("pindu-gamescope".to_owned()),
            _ => None,
        };
        assert!(runs_as_own_session(false, true, empty_first));
    }
}
