//! gamescope-session-plus: what changes when the launcher is a session's client, started
//! with `--session` by the `sessions.d/pindu` client file. session-plus exports nothing
//! that says so, and the session ends when its client exits, so with the flag closing the
//! window does not quit the app, and the menu drawer's power dock offers Switch to
//! Desktop where the distribution has `os-session-select`. Here too is what the dock
//! holds ([`power_dock`]): in the launcher's own session, the flag's or a `--steam`
//! gamescope's that the display manager started ([`runs_as_own_session`]), it leaves out
//! Exit Pindu Launcher, as quitting would leave nothing on the screen.

use std::collections::BTreeMap;
use std::path::Path;

use crate::launch::Command;
use crate::system::power::{PowerAction, PowerEntry, PowerState};

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

/// The power dock's Switch to Desktop entry.
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

    /// The power dock's Switch to Desktop entry: offered to a session's client where the
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

/// An entry of the menu drawer's power dock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockEntry {
    /// Exit Fullscreen Mode, which leaves fullscreen mode for the desktop window.
    ExitFullscreen,
    /// Switch to Desktop, for gamescope-session-plus's client.
    SwitchToDesktop(SwitchToDesktop),
    /// Exit Pindu Launcher, which quits the app.
    Exit,
    /// Suspend, Restart or Power Off, as the system allows it.
    Power(PowerEntry),
}

/// One of the power dock's two groups.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockGroup {
    /// Leaving the launcher, at the dock's start: Exit Fullscreen Mode, Switch to Desktop
    /// and Exit Pindu Launcher.
    Leaving,
    /// The machine, at the dock's end: Suspend, Restart and Power Off.
    Machine,
}

impl DockEntry {
    /// The group the entry sits in.
    pub fn group(&self) -> DockGroup {
        match self {
            Self::ExitFullscreen | Self::SwitchToDesktop(_) | Self::Exit => DockGroup::Leaving,
            Self::Power(_) => DockGroup::Machine,
        }
    }

    /// Whether the entry can be used. One that cannot stays in the dock, greyed out, and
    /// still takes focus so it can say why: Switch to Desktop where the app may not run
    /// `os-session-select` on the host, and an action the system does not allow, or that
    /// the Flatpak cannot ask for without host access ([`PowerState::sandboxed`]).
    pub fn allowed(&self) -> bool {
        match self {
            Self::ExitFullscreen | Self::Exit => true,
            Self::SwitchToDesktop(switch) => switch.allowed,
            Self::Power(entry) => entry.allowed,
        }
    }
}

/// The power dock's entries, start to end, in two groups ([`DockEntry::group`]). Leaving
/// the launcher: Exit Fullscreen Mode, Switch to Desktop where `switch` offers it, then
/// Exit Pindu Launcher where `exit` says quitting leaves something behind, which is not so
/// in the launcher's own session (see [`runs_as_own_session`]). The machine: Suspend,
/// Restart and Power Off as `power` lists them, including those the system does not allow
/// ([`DockEntry::allowed`]). Over a running game (`over_game`) the drawer has no dock:
/// each of them would leave the game or end the session.
pub fn power_dock(
    power: &PowerState,
    switch: Option<SwitchToDesktop>,
    exit: bool,
    over_game: bool,
) -> Vec<DockEntry> {
    if over_game {
        return Vec::new();
    }
    let switch = switch.map(DockEntry::SwitchToDesktop);
    let exit = exit.then_some(DockEntry::Exit);
    let actions = power.entries.iter().copied().map(DockEntry::Power);
    std::iter::once(DockEntry::ExitFullscreen)
        .chain(switch)
        .chain(exit)
        .chain(actions)
        .collect()
}

/// Where focus lands in `dock` when Up from the top of the menu wraps to it: Suspend,
/// which loses nothing, even when the system does not allow it; without it, the first
/// entry of the machine group; without that group, the dock's first entry. `None` when
/// there is no dock.
pub fn dock_landing(dock: &[DockEntry]) -> Option<usize> {
    let suspend = |entry: &DockEntry| matches!(entry, DockEntry::Power(power) if power.action == PowerAction::Suspend);
    let machine = |entry: &DockEntry| entry.group() == DockGroup::Machine;
    dock.iter()
        .position(suspend)
        .or_else(|| dock.iter().position(machine))
        .or_else(|| (!dock.is_empty()).then_some(0))
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
    use DockGroup::{Leaving, Machine};

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

    fn every_action(allowed: bool) -> PowerState {
        PowerState {
            entries: PowerAction::ALL
                .map(|action| PowerEntry { action, allowed })
                .to_vec(),
            ..PowerState::default()
        }
    }

    fn groups(dock: &[DockEntry]) -> Vec<DockGroup> {
        dock.iter().map(|entry| entry.group()).collect()
    }

    #[test]
    fn a_desktop_session_s_dock_leaves_fullscreen_or_quits_then_holds_the_machine() {
        let dock = power_dock(&every_action(true), None, true, false);
        assert_eq!(
            dock,
            [
                DockEntry::ExitFullscreen,
                DockEntry::Exit,
                DockEntry::Power(allowed(PowerAction::Suspend)),
                DockEntry::Power(allowed(PowerAction::Restart)),
                DockEntry::Power(allowed(PowerAction::PowerOff)),
            ]
        );
        assert_eq!(groups(&dock), [Leaving, Leaving, Machine, Machine, Machine]);
        assert_eq!(dock_landing(&dock), Some(2), "Suspend");
    }

    #[test]
    fn a_session_client_s_dock_offers_switch_to_desktop_where_the_session_has_it() {
        let switch = SwitchToDesktop { allowed: true };
        let dock = power_dock(&every_action(true), Some(switch), false, false);
        assert_eq!(
            dock,
            [
                DockEntry::ExitFullscreen,
                DockEntry::SwitchToDesktop(switch),
                DockEntry::Power(allowed(PowerAction::Suspend)),
                DockEntry::Power(allowed(PowerAction::Restart)),
                DockEntry::Power(allowed(PowerAction::PowerOff)),
            ]
        );
        assert_eq!(groups(&dock), [Leaving, Leaving, Machine, Machine, Machine]);
        assert_eq!(dock_landing(&dock), Some(2), "Suspend");

        let dock = power_dock(&every_action(true), None, false, false);
        assert_eq!(
            dock,
            [
                DockEntry::ExitFullscreen,
                DockEntry::Power(allowed(PowerAction::Suspend)),
                DockEntry::Power(allowed(PowerAction::Restart)),
                DockEntry::Power(allowed(PowerAction::PowerOff)),
            ]
        );
        assert_eq!(groups(&dock), [Leaving, Machine, Machine, Machine]);
        assert_eq!(dock_landing(&dock), Some(1), "Suspend");
    }

    #[test]
    fn the_launcher_s_own_session_has_no_exit_and_lands_on_the_first_machine_action() {
        let power = PowerState {
            entries: vec![
                allowed(PowerAction::Restart),
                allowed(PowerAction::PowerOff),
            ],
            ..PowerState::default()
        };
        let dock = power_dock(&power, None, false, false);
        assert_eq!(
            dock,
            [
                DockEntry::ExitFullscreen,
                DockEntry::Power(allowed(PowerAction::Restart)),
                DockEntry::Power(allowed(PowerAction::PowerOff)),
            ]
        );
        assert_eq!(groups(&dock), [Leaving, Machine, Machine]);
        assert_eq!(dock_landing(&dock), Some(1), "Restart, without Suspend");
    }

    #[test]
    fn without_power_actions_the_dock_holds_only_the_ways_out_and_lands_on_the_first() {
        let none = PowerState::default();
        let dock = power_dock(&none, None, true, false);
        assert_eq!(dock, [DockEntry::ExitFullscreen, DockEntry::Exit]);
        assert_eq!(groups(&dock), [Leaving, Leaving]);
        assert_eq!(dock_landing(&dock), Some(0));

        let dock = power_dock(&none, None, false, false);
        assert_eq!(
            dock,
            [DockEntry::ExitFullscreen],
            "the launcher's own session"
        );
        assert_eq!(dock_landing(&dock), Some(0));
    }

    #[test]
    fn actions_the_system_does_not_offer_stay_in_the_dock_unavailable_and_take_the_wrap() {
        let sandboxed = PowerState {
            sandboxed: true,
            ..every_action(false)
        };
        let switch = SwitchToDesktop { allowed: false };
        let dock = power_dock(&sandboxed, Some(switch), false, false);
        assert_eq!(
            dock.iter().map(DockEntry::allowed).collect::<Vec<_>>(),
            [true, false, false, false, false]
        );
        assert_eq!(groups(&dock), [Leaving, Leaving, Machine, Machine, Machine]);
        assert_eq!(dock_landing(&dock), Some(2), "Suspend, though unavailable");
    }

    #[test]
    fn over_a_running_game_the_drawer_has_no_dock() {
        let switch = SwitchToDesktop { allowed: true };
        let dock = power_dock(&every_action(true), Some(switch), true, true);
        assert!(dock.is_empty());
        assert_eq!(dock_landing(&dock), None);
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
