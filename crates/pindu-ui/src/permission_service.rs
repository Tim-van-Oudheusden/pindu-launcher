//! Which permission tiers the sandbox has, checked once at start-up.

use std::sync::OnceLock;

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

/// Whether starting `game` needs a permission the sandbox lacks.
pub fn needs_permission(game: &Game) -> bool {
    permissions::needs_host_spawn(game.launch_kind) && !host_spawn_allowed()
}

/// The command that grants host spawn to this app.
pub fn override_command() -> String {
    permissions::host_override_command(APP_ID)
}
