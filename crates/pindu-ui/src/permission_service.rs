//! Which permission tiers the sandbox has, checked once at start-up.

use std::sync::{LazyLock, OnceLock};

use pindu_core::model::Game;
use pindu_core::permissions;

use crate::config::APP_ID;

/// Whether host spawn is allowed. Read from `/.flatpak-info` once; a change needs a
/// restart, as `flatpak override` only applies to new instances.
pub fn host_spawn_allowed() -> bool {
    static ALLOWED: OnceLock<bool> = OnceLock::new();
    *ALLOWED.get_or_init(|| {
        let info = std::fs::read_to_string("/.flatpak-info").ok();
        permissions::host_spawn_allowed(info.as_deref())
    })
}

/// Whether we run inside Flatpak, as [`in_flatpak`] read it once.
static IN_FLATPAK: OnceLock<bool> = OnceLock::new();

/// Whether we run inside Flatpak, where host commands go through `flatpak-spawn --host`.
pub fn in_flatpak() -> bool {
    *IN_FLATPAK.get_or_init(|| std::path::Path::new("/.flatpak-info").exists())
}

/// Makes [`in_flatpak`] answer `false` from now on. Only the system panel's integration
/// tests call it: they run inside the GNOME SDK's sandbox, as CI does, against a private
/// system bus of their own, where the sandbox's rule of no system bus does not hold.
/// Returns `false`, changing nothing, when [`in_flatpak`] has been read already.
pub fn assume_native() -> bool {
    IN_FLATPAK.set(false).is_ok()
}

/// Whether starting `game` needs a permission the sandbox lacks.
pub fn needs_permission(game: &Game) -> bool {
    permissions::needs_host_spawn(game.launch_kind) && !host_spawn_allowed()
}

/// Where the Flatpak mounts the MangoHud Vulkan-layer extension.
const MANGOHUD_LAYER_DIR: &str = "/app/lib/extensions/vulkan/MangoHud";

/// Whether `game`'s MangoHud switch can work. In Flatpak the extension's folder is read
/// once; installing it needs a restart, as the extension is mounted at start-up.
pub fn mangohud_works(game: &Game) -> bool {
    static LAYER: LazyLock<bool> = LazyLock::new(|| {
        permissions::mangohud_layer_installed(std::path::Path::new(MANGOHUD_LAYER_DIR))
    });
    let in_flatpak = in_flatpak();
    permissions::mangohud_works(game.launch_kind, in_flatpak, in_flatpak && *LAYER)
}

/// The command that grants host spawn to this app.
pub fn override_command() -> String {
    permissions::host_override_command(APP_ID)
}
