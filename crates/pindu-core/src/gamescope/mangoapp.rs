//! MangoApp, MangoHud's overlay for gamescope, driven by the Performance Overlay switch.
//!
//! gamescope-session-plus starts `mangoapp`, and so does `gamescope --steam --mangoapp`,
//! with `MANGOHUD_CONFIGFILE` naming a file that holds `no_display`. Steam switches the
//! overlay by rewriting that file, and the launcher does in Steam's place: the switch of
//! the game in front, whenever that game or its switch changes. MangoApp hides itself
//! while app 769, the launcher, is in front. Where the file is out of reach, games keep
//! MangoHud of their own, as outside gamescope.

use std::ffi::OsStr;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use super::Front;
use crate::model::LaunchConfig;

/// The MangoHud config text for the Performance Overlay switch: off hides the overlay, on
/// shows MangoHud's preset 2, the horizontal bar.
pub fn config_text(overlay_on: bool) -> &'static str {
    if overlay_on {
        "preset=2\n"
    } else {
        "no_display\n"
    }
}

/// The MangoHud config file the launcher drives the session's MangoApp through, when it
/// can: gamescope runs in `--steam` mode, `MANGOHUD_CONFIGFILE` (as `config_file`) names a
/// file, and the launcher can write it. MangoApp creates no file of its own, so one that
/// does not exist here is out of the launcher's reach: in another sandbox's `/tmp`, say.
pub fn session_config_file(steam_mode: bool, config_file: Option<&OsStr>) -> Option<PathBuf> {
    if !steam_mode {
        return None;
    }
    let path = PathBuf::from(config_file?);
    // Opening for writing neither creates, truncates nor modifies the file.
    OpenOptions::new().write(true).open(&path).ok()?;
    Some(path)
}

/// Writes the config text for the Performance Overlay switch (see [`config_text`]) into the
/// config file at `path`, in place: MangoApp watches that file with inotify for changes and
/// for its deletion, so a new file renamed over it would go unnoticed. MangoApp waits a
/// moment after a change before it reads the file, so it never sees it half written.
pub fn write_config(path: &Path, overlay_on: bool) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(path)?
        .write_all(config_text(overlay_on).as_bytes())
}

/// The launch config a game starts with while the session's MangoApp shows its Performance
/// Overlay (see [`session_config_file`]): no MangoHud of its own, neither the `mangohud`
/// wrapper nor `MANGOHUD=1`, which would draw a second one. The stored switch stays, and
/// drives MangoApp instead.
pub fn launch_config(config: &LaunchConfig) -> LaunchConfig {
    LaunchConfig {
        mangohud: false,
        ..config.clone()
    }
}

/// Whether the Performance Overlay switch of the game with app ID `game` goes into the
/// config file with `front` in front: only the game in front's does. Nothing is written
/// while the launcher is in front, because MangoApp hides itself then, and the game's
/// switch is written again once a game comes back.
pub fn writes_for(front: Front, game: u32) -> bool {
    front == Front::Game(game)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_switch_off_hides_the_overlay() {
        assert_eq!(config_text(false), "no_display\n");
    }

    #[test]
    fn the_switch_on_shows_the_horizontal_bar() {
        assert_eq!(config_text(true), "preset=2\n");
    }

    /// A config file holding `no_display`, as gamescope and the session create it.
    fn config_file() -> tempfile::NamedTempFile {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), "no_display\n").unwrap();
        file
    }

    #[test]
    fn a_writable_config_file_in_steam_mode_is_the_session_s_overlay() {
        let file = config_file();
        let path = file.path().as_os_str();
        assert_eq!(
            session_config_file(true, Some(path)).as_deref(),
            Some(file.path())
        );
    }

    #[test]
    fn outside_steam_mode_there_is_no_session_overlay() {
        let file = config_file();
        assert_eq!(
            session_config_file(false, Some(file.path().as_os_str())),
            None
        );
    }

    #[test]
    fn a_config_file_out_of_reach_is_no_session_overlay() {
        let folder = tempfile::tempdir().unwrap();
        let missing = folder.path().join("mangohud.XXXXXXXX");
        assert_eq!(session_config_file(true, Some(missing.as_os_str())), None);
    }

    #[test]
    fn without_a_config_file_there_is_no_session_overlay() {
        assert_eq!(session_config_file(true, None), None);
        assert_eq!(session_config_file(true, Some(OsStr::new(""))), None);
    }

    #[test]
    fn a_read_only_config_file_is_no_session_overlay() {
        use std::os::unix::fs::MetadataExt;

        let file = config_file();
        let mut permissions = file.as_file().metadata().unwrap().permissions();
        permissions.set_readonly(true);
        file.as_file().set_permissions(permissions).unwrap();
        // We created the file, so it is ours: root writes it despite its mode, and then
        // the launcher really can drive MangoApp through it.
        let root = file.as_file().metadata().unwrap().uid() == 0;
        let found = session_config_file(true, Some(file.path().as_os_str()));
        assert_eq!(found.as_deref(), root.then_some(file.path()));
    }

    #[test]
    fn a_config_path_nobody_can_write_is_no_session_overlay() {
        // Not even root opens a folder for writing.
        let folder = tempfile::tempdir().unwrap();
        assert_eq!(
            session_config_file(true, Some(folder.path().as_os_str())),
            None
        );
    }

    #[test]
    fn the_switch_rewrites_the_config_file_in_place() {
        use std::os::unix::fs::MetadataExt;

        let file = config_file();
        let inode = file.path().metadata().unwrap().ino();
        write_config(file.path(), true).unwrap();
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "preset=2\n");
        write_config(file.path(), false).unwrap();
        assert_eq!(
            std::fs::read_to_string(file.path()).unwrap(),
            "no_display\n"
        );
        // MangoApp watches the file it was given, which a new file in its place is not.
        assert_eq!(file.path().metadata().unwrap().ino(), inode);
    }

    #[test]
    fn under_the_session_overlay_games_start_without_mangohud_of_their_own() {
        use crate::launch::{Route, route};
        use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Source};

        let config = LaunchConfig {
            executable: "/opt/celeste/Celeste".into(),
            mangohud: true,
            gamemode: true,
            ..LaunchConfig::default()
        };
        let game = |kind| Game::new(GameId::new(Source::Manual, "celeste"), "Celeste", kind, 0);

        let host = route(
            &game(LaunchKind::HostCommand),
            Some(&launch_config(&config)),
        );
        let Some(Route::HostSpawn(command)) = host else {
            panic!("no host route");
        };
        assert_eq!(command.argv, ["gamemoderun", "/opt/celeste/Celeste"]);

        let runtime = route(
            &game(LaunchKind::RuntimeNative),
            Some(&launch_config(&config)),
        );
        let Some(Route::Runtime(command)) = runtime else {
            panic!("no runtime route");
        };
        assert!(!command.environment.contains_key("MANGOHUD"));

        let proton = route(&game(LaunchKind::Proton), Some(&launch_config(&config)));
        let Some(Route::Proton(config)) = proton else {
            panic!("no Proton route");
        };
        // `proton::command` sets `MANGOHUD=1` for a config with it on.
        assert!(!config.mangohud && config.gamemode);
    }

    #[test]
    fn only_the_switch_of_the_game_in_front_is_written() {
        assert!(writes_for(Front::Game(42), 42));
        assert!(!writes_for(Front::Game(42), 7));
        // MangoApp hides itself while the launcher is in front.
        assert!(!writes_for(Front::Launcher, 42));
    }
}
