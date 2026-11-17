//! gamescope-session-plus: what changes when the launcher is a session's client, started
//! with `--session` by the `sessions.d/pindu` client file. session-plus exports nothing
//! that says so, and the session ends when its client exits, so with the flag closing the
//! window does not quit the app, and the menu drawer's Power level offers Switch to
//! Desktop where the distribution has `os-session-select`.

use std::collections::BTreeMap;
use std::path::Path;

use crate::launch::Command;
use crate::system::power::PowerState;

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

/// Whether the menu drawer has its Power entry: where logind offers an action (`power`),
/// or Switch to Desktop is offered, as it can be in the Flatpak, which has no system bus.
pub fn power_shown(power: &PowerState, switch: Option<SwitchToDesktop>) -> bool {
    power.shown() || switch.is_some()
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

    #[test]
    fn switch_to_desktop_alone_shows_the_power_entry() {
        let none = PowerState::default();
        let restart = PowerState {
            entries: vec![PowerEntry {
                action: PowerAction::Restart,
                allowed: true,
            }],
        };
        let switch = Some(SwitchToDesktop { allowed: false });
        assert!(!power_shown(&none, None));
        assert!(power_shown(&none, switch));
        assert!(power_shown(&restart, None));
        assert!(power_shown(&restart, switch));
    }
}
