//! Permission tiers: what the sandbox lets us do, and which games need more than it has.
//!
//! Store launches go through the OpenURI portal and always work. Starting a game on the
//! host (desktop entries, Flatpak games, Faugus games, games added by hand) needs host
//! spawn: talking to `org.freedesktop.Flatpak`, which the user grants with
//! `flatpak override`. The manifest never asks for it.

use crate::model::LaunchKind;

/// The D-Bus name host spawn talks to.
const FLATPAK_BUS_NAME: &str = "org.freedesktop.Flatpak";

/// Whether host spawn is allowed, judged from the sandbox's `/.flatpak-info` (`None`
/// outside Flatpak, where we start games directly). Overrides the user applied are part
/// of that file.
pub fn host_spawn_allowed(flatpak_info: Option<&str>) -> bool {
    let Some(info) = flatpak_info else {
        return true;
    };
    let mut group = "";
    for line in info.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = name;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let granted = match (group, key.trim()) {
            ("Session Bus Policy", FLATPAK_BUS_NAME) => matches!(value.trim(), "talk" | "own"),
            ("Context", "sockets") => value.split(';').any(|socket| socket == "session-bus"),
            _ => false,
        };
        if granted {
            return true;
        }
    }
    false
}

/// Whether starting a game of this launch kind needs host spawn.
pub fn needs_host_spawn(kind: LaunchKind) -> bool {
    matches!(kind, LaunchKind::HostCommand | LaunchKind::DesktopEntry)
}

/// The command that grants host spawn to the app.
pub fn host_override_command(app_id: &str) -> String {
    format!("flatpak override --user --talk-name={FLATPAK_BUS_NAME} {app_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outside_flatpak_host_spawn_is_always_there() {
        assert!(host_spawn_allowed(None));
    }

    #[test]
    fn the_base_sandbox_has_no_host_spawn() {
        let info = "[Application]\nname=io.example.App\n\n\
                    [Context]\nshared=network;ipc;\nsockets=x11;wayland;\n";

        assert!(!host_spawn_allowed(Some(info)));
    }

    #[test]
    fn talking_to_flatpak_on_the_session_bus_grants_host_spawn() {
        let with = |policy: &str| {
            format!(
                "[Application]\nname=io.example.App\n\n\
                 [Session Bus Policy]\norg.freedesktop.Notifications=talk\n\
                 org.freedesktop.Flatpak={policy}\n"
            )
        };

        assert!(host_spawn_allowed(Some(&with("talk"))));
        assert!(host_spawn_allowed(Some(&with("own"))));
        assert!(!host_spawn_allowed(Some(&with("see"))));
        assert!(!host_spawn_allowed(Some(&with("none"))));
    }

    #[test]
    fn full_session_bus_access_grants_host_spawn() {
        let info = "[Context]\nsockets=wayland;session-bus;\n";

        assert!(host_spawn_allowed(Some(info)));
    }

    #[test]
    fn only_games_we_start_on_the_host_need_host_spawn() {
        assert!(needs_host_spawn(LaunchKind::HostCommand));
        assert!(needs_host_spawn(LaunchKind::DesktopEntry));
        assert!(!needs_host_spawn(LaunchKind::StoreUri));
        assert!(!needs_host_spawn(LaunchKind::Proton));
        assert!(!needs_host_spawn(LaunchKind::RuntimeNative));
    }

    #[test]
    fn the_override_command_names_the_app() {
        assert_eq!(
            host_override_command("io.example.App"),
            "flatpak override --user --talk-name=org.freedesktop.Flatpak io.example.App"
        );
    }
}
