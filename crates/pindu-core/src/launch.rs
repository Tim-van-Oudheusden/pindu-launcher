//! How Play starts a game and how "Open in launcher" reaches its launcher.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::{Game, LaunchConfig, LaunchKind, Source};

/// The way to start a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Hand the game to its launcher through a URI (the OpenURI portal in the sandbox).
    /// Fire and forget: there is no process to follow.
    StoreUri(String),
    /// Run a command on the host as our child (through `flatpak-spawn --host` in the
    /// sandbox), which needs the host-spawn permission.
    HostSpawn(HostCommand),
}

/// A command to run on the host: an argument list, never a shell string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostCommand {
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub working_dir: Option<PathBuf>,
}

impl HostCommand {
    /// The argument list that runs this command on the host from inside Flatpak. The
    /// environment and folder go as options, since `flatpak-spawn` does not forward ours;
    /// there is no `--watch-bus`, so the game stays our child until it exits.
    pub fn flatpak_spawn_argv(&self) -> Vec<String> {
        let mut argv = vec!["flatpak-spawn".to_owned(), "--host".to_owned()];
        if let Some(dir) = &self.working_dir {
            argv.push(format!("--directory={}", dir.display()));
        }
        argv.extend(
            self.environment
                .iter()
                .map(|(key, value)| format!("--env={key}={value}")),
        );
        argv.push("--".to_owned());
        argv.extend(self.argv.iter().cloned());
        argv
    }
}

/// The route Play takes for a game, or `None` when this version cannot start it. Games we
/// start ourselves need their launch config.
pub fn route(game: &Game, config: Option<&LaunchConfig>) -> Option<Route> {
    match game.launch_kind {
        LaunchKind::StoreUri => store_uri(game).map(Route::StoreUri),
        LaunchKind::HostCommand | LaunchKind::DesktopEntry => {
            config.map(|config| Route::HostSpawn(host_command(config)))
        }
        LaunchKind::Proton | LaunchKind::RuntimeNative => None,
    }
}

/// The command for a launch config: the wrappers (outermost first), then `gamemoderun`
/// and `mangohud` when switched on, then the game. Each wrapper is one argument.
fn host_command(config: &LaunchConfig) -> HostCommand {
    let mut argv: Vec<String> = config.wrappers.clone();
    if config.gamemode {
        argv.push("gamemoderun".to_owned());
    }
    if config.mangohud {
        argv.push("mangohud".to_owned());
    }
    argv.push(config.executable.to_string_lossy().into_owned());
    argv.extend(config.arguments.iter().cloned());
    HostCommand {
        argv,
        environment: config.environment.clone(),
        working_dir: config.working_dir.clone(),
    }
}

fn store_uri(game: &Game) -> Option<String> {
    if let Some(uri) = &game.launch_uri {
        return Some(uri.clone());
    }
    let id = game.id.native_id();
    match game.source() {
        Source::Steam if game.steam_launch_dialog => Some(format!("steam://launch/{id}/dialog")),
        Source::Steam => Some(format!("steam://rungameid/{id}")),
        Source::Lutris => Some(format!("lutris:rungameid/{id}")),
        source => {
            let runner = heroic_runner(source)?;
            Some(format!(
                "heroic://launch?appName={}&runner={runner}",
                percent_encode(id)
            ))
        }
    }
}

/// A URI that brings up the game's launcher: Steam's page for the game, or Heroic
/// itself, which has no URI for a game's page.
pub fn launcher_uri(game: &Game) -> Option<String> {
    match game.source() {
        Source::Steam => Some(format!("steam://nav/games/details/{}", game.id.native_id())),
        source if heroic_runner(source).is_some() => Some("heroic://ping".to_owned()),
        _ => None,
    }
}

fn heroic_runner(source: Source) -> Option<&'static str> {
    Some(match source {
        Source::HeroicEpic => "legendary",
        Source::HeroicGog => "gog",
        Source::HeroicAmazon => "nile",
        Source::HeroicSideload => "sideload",
        _ => return None,
    })
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Source};

    fn host_game(kind: LaunchKind) -> Game {
        Game::new(GameId::new(Source::Desktop, "celeste"), "Celeste", kind, 0)
    }

    fn config() -> LaunchConfig {
        LaunchConfig {
            executable: "/opt/Celeste Game/Celeste".into(),
            arguments: vec!["--fullscreen".into()],
            working_dir: Some("/opt/Celeste Game".into()),
            environment: [("SDL_VIDEODRIVER".to_owned(), "wayland".to_owned())].into(),
            ..LaunchConfig::default()
        }
    }

    #[test]
    fn desktop_entries_and_host_commands_spawn_their_argv_on_the_host() {
        for kind in [LaunchKind::DesktopEntry, LaunchKind::HostCommand] {
            assert_eq!(
                route(&host_game(kind), Some(&config())),
                Some(Route::HostSpawn(HostCommand {
                    argv: vec!["/opt/Celeste Game/Celeste".into(), "--fullscreen".into()],
                    environment: [("SDL_VIDEODRIVER".to_owned(), "wayland".to_owned())].into(),
                    working_dir: Some("/opt/Celeste Game".into()),
                })),
                "{kind}"
            );
        }
    }

    #[test]
    fn a_host_game_without_a_command_has_no_route() {
        assert_eq!(route(&host_game(LaunchKind::DesktopEntry), None), None);
    }

    #[test]
    fn wrappers_go_around_the_game_outermost_first() {
        let config = LaunchConfig {
            wrappers: vec!["gamescope".into(), "obs-gamecapture".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        };

        let Some(Route::HostSpawn(command)) =
            route(&host_game(LaunchKind::HostCommand), Some(&config))
        else {
            panic!("no host route");
        };

        assert_eq!(
            command.argv,
            [
                "gamescope",
                "obs-gamecapture",
                "gamemoderun",
                "mangohud",
                "/opt/Celeste Game/Celeste",
                "--fullscreen"
            ]
        );
    }

    #[test]
    fn flatpak_spawn_passes_the_folder_and_environment_as_options() {
        let command = HostCommand {
            argv: vec![
                "/usr/bin/flatpak".into(),
                "run".into(),
                "org.example.Game".into(),
            ],
            environment: [
                ("A".to_owned(), "1".to_owned()),
                ("DXVK_CONFIG".to_owned(), "dxgi.syncInterval = 1".to_owned()),
            ]
            .into(),
            working_dir: Some("/games".into()),
        };

        assert_eq!(
            command.flatpak_spawn_argv(),
            [
                "flatpak-spawn",
                "--host",
                "--directory=/games",
                "--env=A=1",
                "--env=DXVK_CONFIG=dxgi.syncInterval = 1",
                "--",
                "/usr/bin/flatpak",
                "run",
                "org.example.Game",
            ]
        );
        // The game must stay our child, so the spawn never watches the bus.
        assert!(
            !command
                .flatpak_spawn_argv()
                .iter()
                .any(|arg| arg == "--watch-bus")
        );
    }

    #[test]
    fn flatpak_spawn_without_a_folder_or_environment_only_wraps_the_argv() {
        let command = HostCommand {
            argv: vec!["neverball".into()],
            environment: Default::default(),
            working_dir: None,
        };

        assert_eq!(
            command.flatpak_spawn_argv(),
            ["flatpak-spawn", "--host", "--", "neverball"]
        );
    }

    fn store_game(source: Source, native_id: &str) -> Game {
        Game::new(
            GameId::new(source, native_id),
            "Game",
            LaunchKind::StoreUri,
            0,
        )
    }

    #[test]
    fn steam_games_start_by_app_id() {
        let game = store_game(Source::Steam, "570");

        assert_eq!(
            route(&game, None),
            Some(Route::StoreUri("steam://rungameid/570".into()))
        );
    }

    #[test]
    fn steam_games_can_ask_steam_for_launch_options() {
        let mut game = store_game(Source::Steam, "570");
        game.steam_launch_dialog = true;

        assert_eq!(
            route(&game, None),
            Some(Route::StoreUri("steam://launch/570/dialog".into()))
        );
    }

    #[test]
    fn heroic_games_name_their_runner() {
        let cases = [
            (Source::HeroicEpic, "Sugar", "legendary"),
            (Source::HeroicGog, "1207658924", "gog"),
            (Source::HeroicAmazon, "amzn1.adg.product.0001", "nile"),
            (Source::HeroicSideload, "aB3dE", "sideload"),
        ];
        for (source, app_name, runner) in cases {
            assert_eq!(
                route(&store_game(source, app_name), None),
                Some(Route::StoreUri(format!(
                    "heroic://launch?appName={app_name}&runner={runner}"
                ))),
                "{source}"
            );
        }
    }

    #[test]
    fn lutris_games_start_by_their_lutris_id() {
        let game = store_game(Source::Lutris, "42");

        assert_eq!(
            route(&game, None),
            Some(Route::StoreUri("lutris:rungameid/42".into()))
        );
        // Lutris has no URI that only opens it.
        assert_eq!(launcher_uri(&game), None);
    }

    #[test]
    fn bottles_programs_start_by_the_uri_their_source_gave() {
        let mut game = store_game(Source::Bottles, "7f1e2d3c");
        assert_eq!(route(&game, None), None);

        game.launch_uri = Some("bottles:run/Gaming/Celeste".into());

        assert_eq!(
            route(&game, None),
            Some(Route::StoreUri("bottles:run/Gaming/Celeste".into()))
        );
        assert_eq!(launcher_uri(&game), None);
    }

    #[test]
    fn app_names_are_percent_encoded() {
        let game = store_game(Source::HeroicSideload, "my game&more=1");

        assert_eq!(
            route(&game, None),
            Some(Route::StoreUri(
                "heroic://launch?appName=my%20game%26more%3D1&runner=sideload".into()
            ))
        );
    }

    #[test]
    fn games_without_a_store_launcher_have_no_store_route() {
        let manual = Game::new(
            GameId::new(Source::Manual, "x"),
            "X",
            LaunchKind::HostCommand,
            0,
        );

        assert_eq!(route(&manual, None), None);
    }

    #[test]
    fn open_in_launcher_shows_the_game_in_steam_or_starts_heroic() {
        assert_eq!(
            launcher_uri(&store_game(Source::Steam, "570")).as_deref(),
            Some("steam://nav/games/details/570")
        );
        assert_eq!(
            launcher_uri(&store_game(Source::HeroicGog, "1")).as_deref(),
            Some("heroic://ping")
        );
        assert_eq!(
            launcher_uri(&Game::new(
                GameId::new(Source::Manual, "x"),
                "X",
                LaunchKind::HostCommand,
                0
            )),
            None
        );
    }
}
