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

/// The command that runs a Windows game: `umu-run` with the game, given its Proton build
/// as `PROTONPATH` and its prefix as `WINEPREFIX`. The build and prefix in its launch
/// config win; without them it gets the newest build and its own folder under `prefixes`.
/// The user's environment wins over ours.
pub fn command(
    game: &Game,
    config: &LaunchConfig,
    builds: &[ProtonBuild],
    prefixes: &Path,
) -> Result<Command, ProtonError> {
    let build = match &config.proton_version {
        Some(name) => builds
            .iter()
            .find(|build| &build.name == name)
            .ok_or_else(|| ProtonError::Missing(name.clone()))?,
        None => builds.first().ok_or(ProtonError::NoBuilds)?,
    };
    let prefix = config
        .prefix_path
        .clone()
        .unwrap_or_else(|| prefixes.join(game.id.native_id()));
    let mut environment = launch::session_environment(game);
    environment.insert("PROTONPATH".to_owned(), path_text(&build.path));
    environment.insert("WINEPREFIX".to_owned(), path_text(&prefix));
    environment.extend(config.environment.clone());
    let mut argv = vec!["umu-run".to_owned(), path_text(&config.executable)];
    argv.extend(config.arguments.iter().cloned());
    Ok(Command {
        argv,
        environment,
        working_dir: config.working_dir.clone(),
    })
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

    #[test]
    fn games_run_umu_with_the_newest_build_and_a_prefix_of_their_own() {
        let builds = [build("GE-Proton11-3"), build("GE-Proton10-34")];

        let command = command(&game(), &config(), &builds, Path::new(PREFIXES)).unwrap();

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

        let command = command(&game(), &config, &builds, Path::new(PREFIXES)).unwrap();

        assert_eq!(command.environment["PROTONPATH"], "/tools/GE-Proton10-34");
        assert_eq!(command.environment["WINEPREFIX"], "/games/Celeste/prefix");
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
        let prefixes = Path::new("/prefixes");

        assert_eq!(
            command(&game, &LaunchConfig::default(), &[], prefixes),
            Err(ProtonError::NoBuilds)
        );
        assert_eq!(
            command(&game, &chosen, &[other], prefixes),
            Err(ProtonError::Missing("GE-Proton9-1".into()))
        );
    }
}
