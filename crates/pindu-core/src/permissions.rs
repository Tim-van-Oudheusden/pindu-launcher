//! Permission tiers: what the sandbox lets us do, and which games need more than it has.
//!
//! Store launches go through the OpenURI portal and always work. Starting a game on the
//! host (desktop entries, Flatpak games, Faugus and Legendary games, games added by hand)
//! needs host spawn: talking to `org.freedesktop.Flatpak`, which the user grants with
//! `flatpak override`. The manifest never asks for it.

use std::path::Path;

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

/// Whether a game's MangoHud switch can work. Windows games and games in the Steam
/// Linux Runtime run inside the sandbox, where `MANGOHUD=1` only turns on the MangoHud
/// Vulkan-layer extension, so in Flatpak they need it installed. Games started on the
/// host, and every game outside Flatpak, run the system's `mangohud`. Store games have
/// no switch, so there is nothing to explain.
pub fn mangohud_works(kind: LaunchKind, in_flatpak: bool, layer_installed: bool) -> bool {
    let in_sandbox = matches!(kind, LaunchKind::Proton | LaunchKind::RuntimeNative);
    !(in_flatpak && in_sandbox) || layer_installed
}

/// Whether the MangoHud Vulkan-layer extension is installed in `root`, where the
/// Flatpak mounts it. The mount folder is there either way, so the layer's manifest
/// (`MangoHud.<arch>.json`, named after the architecture) tells.
pub fn mangohud_layer_installed(root: &Path) -> bool {
    std::fs::read_dir(root.join("share/vulkan/implicit_layer.d")).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
    })
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

    #[test]
    fn in_flatpak_windows_and_runtime_games_need_the_mangohud_layer() {
        for kind in [LaunchKind::Proton, LaunchKind::RuntimeNative] {
            assert!(!mangohud_works(kind, true, false), "{kind:?}");
            assert!(mangohud_works(kind, true, true), "{kind:?}");
        }
    }

    #[test]
    fn games_started_on_the_host_run_its_mangohud_without_the_layer() {
        for kind in [LaunchKind::HostCommand, LaunchKind::DesktopEntry] {
            assert!(mangohud_works(kind, true, false), "{kind:?}");
        }
    }

    #[test]
    fn outside_flatpak_every_game_runs_the_system_s_mangohud() {
        for kind in LaunchKind::ALL {
            assert!(mangohud_works(*kind, false, false), "{kind:?}");
            assert!(mangohud_works(*kind, false, true), "{kind:?}");
        }
    }

    #[test]
    fn store_games_have_no_mangohud_switch_to_explain() {
        assert!(mangohud_works(LaunchKind::StoreUri, true, false));
    }

    #[test]
    fn an_empty_or_missing_extension_folder_has_no_mangohud_layer() {
        let root = tempfile::tempdir().unwrap();
        assert!(!mangohud_layer_installed(root.path()));
        assert!(!mangohud_layer_installed(&root.path().join("missing")));
    }

    #[test]
    fn the_extension_s_vulkan_layer_manifest_means_the_layer_is_installed() {
        let root = tempfile::tempdir().unwrap();
        let layers = root.path().join("share/vulkan/implicit_layer.d");
        std::fs::create_dir_all(&layers).unwrap();
        std::fs::write(layers.join("MangoHud.x86_64.json"), "{}").unwrap();
        assert!(mangohud_layer_installed(root.path()));
    }

    #[test]
    fn a_layer_folder_without_a_manifest_has_no_mangohud_layer() {
        let root = tempfile::tempdir().unwrap();
        let layers = root.path().join("share/vulkan/implicit_layer.d");
        std::fs::create_dir_all(&layers).unwrap();
        std::fs::write(layers.join("README"), "").unwrap();
        assert!(!mangohud_layer_installed(root.path()));
    }
}
