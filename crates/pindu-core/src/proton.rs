//! Proton builds, and running Windows games through umu and Proton inside the sandbox.
//!
//! Builds come from Steam's `compatibilitytools.d` folders (native, then Flatpak) and our
//! own tools folder. Nothing is downloaded here: umu only gets a build that is already on
//! the machine.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::launch::{self, Command};
use crate::model::{Game, LaunchConfig, Source};
use crate::sources::location::{HostDirs, Location};
use crate::sources::vdf;

/// The folder under a Steam root that holds compatibility tools.
const COMPAT_TOOLS: &str = "compatibilitytools.d";
/// The GameMode client library, from the `gamemode` module of our Flatpak.
const LIBGAMEMODE: &str = "libgamemode.so.0";

/// A Proton build on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtonBuild {
    /// The tool's internal name, as Steam keys it: what a launch config stores.
    pub name: String,
    /// The name its `compatibilitytool.vdf` gives for menus.
    pub display_name: String,
    /// The build's folder, which umu takes as `PROTONPATH`.
    pub path: PathBuf,
    /// When it was built, from the timestamp its `version` file starts with.
    built_at: Option<i64>,
}

/// The folders searched for Proton builds, in order: each Steam root's
/// `compatibilitytools.d` (native, then Flatpak), then our own tools folder.
pub fn tool_dirs(dirs: &HostDirs, ours: &Path) -> Vec<PathBuf> {
    let steam = Location::of(Source::Steam, dirs).expect("Steam has data folders");
    steam
        .candidates()
        .iter()
        .map(|root| root.join(COMPAT_TOOLS))
        .chain([ours.to_owned()])
        .collect()
}

/// Every Proton build in `dirs`, newest first. A build found in several folders counts
/// once, from the first folder searched.
pub fn discover(dirs: &[PathBuf]) -> Vec<ProtonBuild> {
    let mut seen = HashSet::new();
    let mut builds: Vec<ProtonBuild> = dirs
        .iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flat_map(|entries| {
            let mut paths: Vec<PathBuf> = entries.flatten().map(|entry| entry.path()).collect();
            paths.sort();
            paths
        })
        .filter_map(|path| read_build(&path))
        .filter(|build| seen.insert(build.name.clone()))
        .collect();
    builds.sort_by(|a, b| {
        b.built_at
            .cmp(&a.built_at)
            .then_with(|| a.name.cmp(&b.name))
    });
    builds
}

/// A tool folder that holds a Proton build: its manifest names the `proton` layer and
/// its `proton` script is there, so half-unpacked downloads and other tools are skipped.
fn read_build(path: &Path) -> Option<ProtonBuild> {
    let manifest =
        vdf::parse_text(&std::fs::read_to_string(path.join("toolmanifest.vdf")).ok()?).ok()?;
    let layer = manifest
        .path(&["manifest", "compatmanager_layer_name"])?
        .as_str()?;
    if layer != "proton" || !path.join("proton").is_file() {
        return None;
    }
    let folder = path.file_name()?.to_string_lossy().into_owned();
    let (name, display_name) = compat_tool_names(path).unwrap_or((folder.clone(), folder));
    let built_at = std::fs::read_to_string(path.join("version"))
        .ok()
        .and_then(|text| text.split_whitespace().next()?.parse().ok());
    Some(ProtonBuild {
        name,
        display_name,
        path: path.to_owned(),
        built_at,
    })
}

/// The internal and display names of the first tool in `compatibilitytool.vdf`.
fn compat_tool_names(path: &Path) -> Option<(String, String)> {
    let text = std::fs::read_to_string(path.join("compatibilitytool.vdf")).ok()?;
    let root = vdf::parse_text(&text).ok()?;
    let (name, tool) = root
        .path(&["compatibilitytools", "compat_tools"])?
        .entries()
        .first()?;
    let display_name = tool.str("display_name").unwrap_or(name);
    Some((name.clone(), display_name.to_owned()))
}

/// Why a Proton game cannot start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtonError {
    /// There is no Proton build on this machine.
    NoBuilds,
    /// The build the game's launch config names is no longer on this machine.
    Missing(String),
}

impl fmt::Display for ProtonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoBuilds => f.write_str("no Proton build found"),
            Self::Missing(name) => write!(f, "{name} is not installed"),
        }
    }
}

impl std::error::Error for ProtonError {}

/// What Windows games get when their launch config names nothing of its own: the
/// Preferences' Windows games page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Defaults {
    /// The Proton build, by its internal name; `None` for the newest one at each start.
    pub proton_version: Option<String>,
    /// The folder that holds games' prefixes of their own.
    pub prefixes: PathBuf,
    /// Whether new games start with MangoHud on.
    pub mangohud: bool,
    /// Whether new games start with GameMode on.
    pub gamemode: bool,
}

/// The command that runs a Windows game: `umu-run` with the game, given its Proton build
/// as `PROTONPATH` and its prefix as `WINEPREFIX`. The build and prefix in its launch
/// config win; without them it gets the default build and its own folder in the default
/// prefixes folder. The user's environment wins over ours.
pub fn command(
    game: &Game,
    config: &LaunchConfig,
    builds: &[ProtonBuild],
    defaults: &Defaults,
) -> Result<Command, ProtonError> {
    let chosen = config
        .proton_version
        .as_deref()
        .or(defaults.proton_version.as_deref());
    let build = pick_build(chosen, builds)?;
    let prefix = config
        .prefix_path
        .clone()
        .unwrap_or_else(|| defaults.prefixes.join(game.id.native_id()));
    let mut environment = launch::session_environment(game);
    environment.insert("PROTONPATH".to_owned(), path_text(&build.path));
    environment.insert("WINEPREFIX".to_owned(), path_text(&prefix));
    if config.mangohud {
        // The MangoHud Vulkan-layer extension turns on with this inside the Steam Runtime
        // container too; the `mangohud` script is not on our `PATH`, and DXVK and VKD3D
        // run games on Vulkan anyway.
        environment.insert("MANGOHUD".to_owned(), "1".to_owned());
    }
    environment.extend(config.environment.clone());
    if config.gamemode {
        // `gamemoderun` preloads libgamemodeauto, which loads libgamemode by name.
        // pressure-vessel brings only preloaded libraries into the Steam Runtime
        // container, so libgamemode is preloaded too, or the game itself never asks the
        // GameMode portal.
        let preload = match environment.get("LD_PRELOAD") {
            Some(own) => format!("{LIBGAMEMODE}:{own}"),
            None => LIBGAMEMODE.to_owned(),
        };
        environment.insert("LD_PRELOAD".to_owned(), preload);
    }
    // The wrappers, outermost first, then `gamemoderun`, wrap umu, which starts the game
    // in the Steam Runtime container through the Flatpak portal.
    let mut argv = config.wrappers.clone();
    if config.gamemode {
        argv.push("gamemoderun".to_owned());
    }
    argv.extend(["umu-run".to_owned(), path_text(&config.executable)]);
    argv.extend(config.arguments.iter().cloned());
    Ok(Command {
        argv,
        environment,
        working_dir: config.working_dir.clone(),
    })
}

/// The command that runs a game's installer through umu, in the prefix the game will
/// use, with the named Proton build or the default one. It runs in the installer's
/// folder, and with umu's default game ID, as there is no game yet.
pub fn installer_command(
    installer: &Path,
    proton_version: Option<&str>,
    prefix: &Path,
    builds: &[ProtonBuild],
    defaults: &Defaults,
) -> Result<Command, ProtonError> {
    let build = pick_build(
        proton_version.or(defaults.proton_version.as_deref()),
        builds,
    )?;
    let environment = [
        ("GAMEID", "umu-default".to_owned()),
        ("PROTONPATH", path_text(&build.path)),
        ("WINEPREFIX", path_text(prefix)),
    ]
    .into_iter()
    .map(|(key, value)| (key.to_owned(), value))
    .collect();
    Ok(Command {
        argv: vec!["umu-run".to_owned(), path_text(installer)],
        environment,
        working_dir: installer.parent().map(Path::to_owned),
    })
}

/// The build named, or the newest one when none is.
fn pick_build<'a>(
    name: Option<&str>,
    builds: &'a [ProtonBuild],
) -> Result<&'a ProtonBuild, ProtonError> {
    match name {
        Some(name) => builds
            .iter()
            .find(|build| build.name == name)
            .ok_or_else(|| ProtonError::Missing(name.to_owned())),
        None => builds.first().ok_or(ProtonError::NoBuilds),
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, GameId, LaunchConfig, LaunchKind};

    const PREFIXES: &str = "/home/me/Games/pindu-launcher/prefixes";

    fn build(name: &str) -> ProtonBuild {
        ProtonBuild {
            name: name.to_owned(),
            display_name: name.to_owned(),
            path: Path::new("/tools").join(name),
            built_at: None,
        }
    }

    fn game() -> Game {
        Game::new(
            GameId::new(Source::Manual, "celeste-1a2b"),
            "Celeste",
            LaunchKind::Proton,
            0,
        )
    }

    fn config() -> LaunchConfig {
        LaunchConfig {
            executable: "/games/Celeste/Celeste.exe".into(),
            arguments: vec!["--windowed".into()],
            working_dir: Some("/games/Celeste".into()),
            environment: [("DXVK_HUD".to_owned(), "fps".to_owned())].into(),
            ..LaunchConfig::default()
        }
    }

    /// Preferences as they ship: the newest build, prefixes in the games folder.
    fn defaults() -> Defaults {
        Defaults {
            proton_version: None,
            prefixes: PREFIXES.into(),
            mangohud: false,
            gamemode: false,
        }
    }

    #[test]
    fn games_run_umu_with_the_newest_build_and_a_prefix_of_their_own() {
        let builds = [build("GE-Proton11-3"), build("GE-Proton10-34")];

        let command = command(&game(), &config(), &builds, &defaults()).unwrap();

        assert_eq!(
            command.argv,
            ["umu-run", "/games/Celeste/Celeste.exe", "--windowed"]
        );
        assert_eq!(command.environment["PROTONPATH"], "/tools/GE-Proton11-3");
        assert_eq!(
            command.environment["WINEPREFIX"],
            format!("{PREFIXES}/celeste-1a2b")
        );
        assert!(command.environment["GAMEID"].starts_with("umu-pindu-"));
        assert_eq!(command.environment["DXVK_HUD"], "fps");
        assert_eq!(command.working_dir, Some("/games/Celeste".into()));
    }

    #[test]
    fn the_build_and_prefix_in_the_launch_config_win() {
        let builds = [build("GE-Proton11-3"), build("GE-Proton10-34")];
        let config = LaunchConfig {
            proton_version: Some("GE-Proton10-34".into()),
            prefix_path: Some("/games/Celeste/prefix".into()),
            ..config()
        };

        let command = command(&game(), &config, &builds, &defaults()).unwrap();

        assert_eq!(command.environment["PROTONPATH"], "/tools/GE-Proton10-34");
        assert_eq!(command.environment["WINEPREFIX"], "/games/Celeste/prefix");
    }

    #[test]
    fn games_without_a_build_or_prefix_of_their_own_get_the_defaults() {
        let builds = [build("GE-Proton11-3"), build("GE-Proton10-34")];
        let defaults = Defaults {
            proton_version: Some("GE-Proton10-34".into()),
            prefixes: "/data/prefixes".into(),
            ..defaults()
        };

        let command = command(&game(), &config(), &builds, &defaults).unwrap();

        assert_eq!(command.environment["PROTONPATH"], "/tools/GE-Proton10-34");
        assert_eq!(
            command.environment["WINEPREFIX"],
            "/data/prefixes/celeste-1a2b"
        );
        let gone = Defaults {
            proton_version: Some("GE-Proton9-1".into()),
            ..defaults
        };
        assert_eq!(
            super::command(&game(), &config(), &builds, &gone),
            Err(ProtonError::Missing("GE-Proton9-1".into()))
        );
    }

    #[test]
    fn wrappers_and_gamemode_go_around_umu_and_mangohud_is_switched_on_for_vulkan() {
        let builds = [build("GE-Proton11-3")];
        let config = LaunchConfig {
            wrappers: vec!["taskset".into(), "-c".into(), "0-3".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        };

        let command = command(&game(), &config, &builds, &defaults()).unwrap();
        let plain = super::command(&game(), &self::config(), &builds, &defaults()).unwrap();

        assert_eq!(
            command.argv,
            [
                "taskset",
                "-c",
                "0-3",
                "gamemoderun",
                "umu-run",
                "/games/Celeste/Celeste.exe",
                "--windowed"
            ]
        );
        assert_eq!(command.environment["MANGOHUD"], "1");
        assert!(!plain.environment.contains_key("MANGOHUD"));
    }

    #[test]
    fn gamemode_also_preloads_libgamemode_so_the_game_in_the_runtime_container_finds_it() {
        let builds = [build("GE-Proton11-3")];
        let on = LaunchConfig {
            gamemode: true,
            ..config()
        };
        let with_own_preload = LaunchConfig {
            environment: [("LD_PRELOAD".to_owned(), "libfoo.so".to_owned())].into(),
            ..on.clone()
        };

        let command = command(&game(), &on, &builds, &defaults()).unwrap();
        let own = super::command(&game(), &with_own_preload, &builds, &defaults()).unwrap();
        let plain = super::command(&game(), &config(), &builds, &defaults()).unwrap();

        assert_eq!(command.environment["LD_PRELOAD"], "libgamemode.so.0");
        assert_eq!(own.environment["LD_PRELOAD"], "libgamemode.so.0:libfoo.so");
        assert!(!plain.environment.contains_key("LD_PRELOAD"));
    }

    #[test]
    fn a_game_without_proton_or_with_its_chosen_build_gone_names_the_problem() {
        let game = Game::new(GameId::new(Source::Manual, "x"), "X", LaunchKind::Proton, 0);
        let chosen = LaunchConfig {
            proton_version: Some("GE-Proton9-1".into()),
            ..LaunchConfig::default()
        };
        let other = ProtonBuild {
            name: "GE-Proton11-3".into(),
            display_name: "GE-Proton11-3".into(),
            path: "/tools/GE-Proton11-3".into(),
            built_at: None,
        };
        let defaults = Defaults {
            prefixes: "/prefixes".into(),
            ..defaults()
        };

        assert_eq!(
            command(&game, &LaunchConfig::default(), &[], &defaults),
            Err(ProtonError::NoBuilds)
        );
        assert_eq!(
            command(&game, &chosen, &[other], &defaults),
            Err(ProtonError::Missing("GE-Proton9-1".into()))
        );
    }

    #[test]
    fn an_installer_runs_in_the_prefix_it_installs_into() {
        let builds = [build("GE-Proton11-3"), build("GE-Proton10-34")];
        let installer = Path::new("/run/user/1000/doc/9f8e/setup_celeste.exe");
        let prefix = Path::new(PREFIXES).join("new-prefix");

        let command = installer_command(
            installer,
            Some("GE-Proton10-34"),
            &prefix,
            &builds,
            &defaults(),
        )
        .unwrap();

        assert_eq!(command.argv, ["umu-run", installer.to_str().unwrap()]);
        assert_eq!(command.environment["PROTONPATH"], "/tools/GE-Proton10-34");
        assert_eq!(command.environment["WINEPREFIX"], path_text(&prefix));
        // No game yet, so no fixes for one.
        assert_eq!(command.environment["GAMEID"], "umu-default");
        assert_eq!(command.working_dir, Some("/run/user/1000/doc/9f8e".into()));
        assert_eq!(
            installer_command(installer, None, &prefix, &[], &defaults()),
            Err(ProtonError::NoBuilds)
        );
        let pinned = Defaults {
            proton_version: Some("GE-Proton10-34".into()),
            ..defaults()
        };
        let by_default = installer_command(installer, None, &prefix, &builds, &pinned).unwrap();
        assert_eq!(
            by_default.environment["PROTONPATH"],
            "/tools/GE-Proton10-34"
        );
    }
}
