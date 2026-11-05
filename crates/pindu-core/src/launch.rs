//! How Play starts a game and how "Open in launcher" reaches its launcher.

use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::model::{Game, LaunchConfig, LaunchKind, Source};
use crate::sources::sha256::sha256_hex;

/// The way to start a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Hand the game to its launcher through a URI (the OpenURI portal in the sandbox).
    /// Fire and forget: there is no process to follow.
    StoreUri(String),
    /// Run a command on the host as our child (through `flatpak-spawn --host` in the
    /// sandbox), which needs the host-spawn permission.
    HostSpawn(Command),
    /// Run a Windows game through umu and Proton inside our sandbox; the build and
    /// prefix are picked by [`crate::proton::command`].
    Proton(LaunchConfig),
}

/// A command to run: an argument list, never a shell string, with the environment and
/// folder it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub argv: Vec<String>,
    pub environment: BTreeMap<String, String>,
    pub working_dir: Option<PathBuf>,
}

impl Command {
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
        LaunchKind::HostCommand | LaunchKind::DesktopEntry => config.map(|config| {
            let mut command = host_command(config);
            if tracks_sessions(game) {
                // Ours first, so the user's own variables win.
                let mut environment = session_environment(game);
                environment.append(&mut command.environment);
                command.environment = environment;
            }
            Route::HostSpawn(command)
        }),
        LaunchKind::Proton => config.cloned().map(Route::Proton),
        LaunchKind::RuntimeNative => None,
    }
}

/// Whether we record play sessions for a game: every game we start ourselves, except
/// Faugus's, whose launcher tracks playtime and which we import instead.
pub fn tracks_sessions(game: &Game) -> bool {
    matches!(
        game.launch_kind,
        LaunchKind::HostCommand | LaunchKind::DesktopEntry
    ) && game.source() != Source::Faugus
}

/// The launch settings a game's page offers. Every game we run has an environment,
/// MangoHud, GameMode and wrappers on top of its command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LaunchSettings {
    /// Its Proton build and prefix: a Windows game.
    pub proton: bool,
    /// Its arguments: a game added by hand. An imported game's arguments are its
    /// source's and come back at each import.
    pub arguments: bool,
}

/// The launch settings a game's page offers; `None` for store games, whose launcher
/// keeps their settings, and games this version cannot start.
pub fn launch_settings(game: &Game) -> Option<LaunchSettings> {
    match game.launch_kind {
        LaunchKind::StoreUri | LaunchKind::RuntimeNative => None,
        LaunchKind::HostCommand | LaunchKind::DesktopEntry | LaunchKind::Proton => {
            Some(LaunchSettings {
                proton: game.launch_kind == LaunchKind::Proton,
                arguments: game.source() == Source::Manual,
            })
        }
    }
}

/// A game ID and window class of the game's own, so umu, the compositor and the shell
/// tell our games apart: `pindu-<hash>` from the game's ID.
pub(crate) fn session_environment(game: &Game) -> BTreeMap<String, String> {
    let hash = sha256_hex(game.id.as_str().as_bytes());
    let class = format!("pindu-{}", &hash[..12]);
    BTreeMap::from([
        ("GAMEID".to_owned(), format!("umu-{class}")),
        ("SDL_VIDEO_WAYLAND_WMCLASS".to_owned(), class.clone()),
        ("SDL_VIDEO_X11_WMCLASS".to_owned(), class),
    ])
}

/// The command for a launch config: the wrappers (outermost first), then `gamemoderun`
/// and `mangohud` when switched on, then the game. Each wrapper is one argument.
fn host_command(config: &LaunchConfig) -> Command {
    let mut argv: Vec<String> = config.wrappers.clone();
    if config.gamemode {
        argv.push("gamemoderun".to_owned());
    }
    if config.mangohud {
        argv.push("mangohud".to_owned());
    }
    argv.push(config.executable.to_string_lossy().into_owned());
    argv.extend(config.arguments.iter().cloned());
    Command {
        argv,
        environment: config.environment.clone(),
        working_dir: config.working_dir.clone(),
    }
}

/// Reads typed environment variables, `NAME=value` words split like
/// [`split_arguments`](crate::manual::split_arguments), so values with spaces are quoted.
/// Fails on a word without a valid name and `=`.
pub fn parse_environment(text: &str) -> Result<BTreeMap<String, String>, String> {
    crate::manual::split_arguments(text)?
        .into_iter()
        .map(|word| {
            let (name, value) = word
                .split_once('=')
                .ok_or_else(|| format!("{word} has no “=”"))?;
            let valid = name.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
                && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
            if !valid {
                return Err(format!("{name} is not a variable name"));
            }
            Ok((name.to_owned(), value.to_owned()))
        })
        .collect()
}

/// Joins environment variables into text that [`parse_environment`] reads back the same.
pub fn join_environment(environment: &BTreeMap<String, String>) -> String {
    let words: Vec<String> = environment
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    crate::manual::join_arguments(&words)
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
pub(crate) fn percent_encode(text: &str) -> String {
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
    use std::path::Path;

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
    fn games_we_run_offer_launch_settings_and_store_games_none() {
        let game = |source, kind| Game::new(GameId::new(source, "x"), "X", kind, 0);

        assert_eq!(
            launch_settings(&game(Source::Steam, LaunchKind::StoreUri)),
            None
        );
        assert_eq!(
            launch_settings(&game(Source::Manual, LaunchKind::Proton)),
            Some(LaunchSettings {
                proton: true,
                arguments: true
            })
        );
        assert_eq!(
            launch_settings(&game(Source::Manual, LaunchKind::HostCommand)),
            Some(LaunchSettings {
                proton: false,
                arguments: true
            })
        );
        // An imported game's command is its source's; options on top are ours.
        assert_eq!(
            launch_settings(&game(Source::Lutris, LaunchKind::HostCommand)),
            Some(LaunchSettings {
                proton: false,
                arguments: false
            })
        );
    }

    #[test]
    fn typed_environment_reads_as_variables_and_joins_back_the_same() {
        let text = r#"DXVK_HUD=fps PROTON_LOG=1 WINEDLLOVERRIDES="dxgi=n,b" EMPTY="#;

        let environment = parse_environment(text).unwrap();

        assert_eq!(
            environment,
            BTreeMap::from(
                [
                    ("DXVK_HUD", "fps"),
                    ("EMPTY", ""),
                    ("PROTON_LOG", "1"),
                    ("WINEDLLOVERRIDES", "dxgi=n,b"),
                ]
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
            )
        );
        assert_eq!(
            parse_environment(&join_environment(&environment)).unwrap(),
            environment
        );
        assert_eq!(parse_environment("  ").unwrap(), BTreeMap::new());
    }

    #[test]
    fn typed_environment_needs_a_name_and_an_equals_sign() {
        assert!(parse_environment("PROTON_LOG").is_err());
        assert!(parse_environment("=1").is_err());
        assert!(parse_environment("1ABC=x").is_err());
        assert!(parse_environment("A-B=x").is_err());
        assert!(parse_environment("UNCLOSED=\"x").is_err());
    }

    #[test]
    fn desktop_entries_and_host_commands_spawn_their_argv_on_the_host() {
        for kind in [LaunchKind::DesktopEntry, LaunchKind::HostCommand] {
            let Some(Route::HostSpawn(command)) = route(&host_game(kind), Some(&config())) else {
                panic!("no host route for {kind}");
            };

            assert_eq!(command.argv, ["/opt/Celeste Game/Celeste", "--fullscreen"]);
            assert_eq!(
                command.working_dir.as_deref(),
                Some(Path::new("/opt/Celeste Game"))
            );
            assert_eq!(command.environment["SDL_VIDEODRIVER"], "wayland");
        }
    }

    #[test]
    fn tracked_games_get_their_own_game_id_and_window_class() {
        let environment = |native_id: &str| {
            let game = Game::new(
                GameId::new(Source::Desktop, native_id),
                "Game",
                LaunchKind::DesktopEntry,
                0,
            );
            match route(&game, Some(&config())) {
                Some(Route::HostSpawn(command)) => command.environment,
                other => panic!("{other:?}"),
            }
        };

        let celeste = environment("celeste");
        let other = environment("neverball");

        let class = celeste["SDL_VIDEO_WAYLAND_WMCLASS"].clone();
        assert!(class.starts_with("pindu-"), "{class}");
        assert_eq!(celeste["SDL_VIDEO_X11_WMCLASS"], class);
        assert_eq!(celeste["GAMEID"], format!("umu-{class}"));
        assert_ne!(other["SDL_VIDEO_WAYLAND_WMCLASS"], class);
        assert_eq!(environment("celeste"), celeste);
    }

    #[test]
    fn the_users_environment_wins_over_ours() {
        let mut config = config();
        config
            .environment
            .insert("SDL_VIDEO_WAYLAND_WMCLASS".into(), "mine".into());

        let Some(Route::HostSpawn(command)) =
            route(&host_game(LaunchKind::DesktopEntry), Some(&config))
        else {
            panic!("no host route");
        };

        assert_eq!(command.environment["SDL_VIDEO_WAYLAND_WMCLASS"], "mine");
    }

    #[test]
    fn faugus_games_keep_their_own_tracking() {
        let faugus = Game::new(
            GameId::new(Source::Faugus, "hades"),
            "Hades",
            LaunchKind::HostCommand,
            0,
        );

        assert!(!tracks_sessions(&faugus));
        assert!(tracks_sessions(&host_game(LaunchKind::DesktopEntry)));
        assert!(!tracks_sessions(&store_game(Source::Steam, "570")));
        let Some(Route::HostSpawn(command)) = route(&faugus, Some(&config())) else {
            panic!("no host route");
        };
        assert!(!command.environment.contains_key("GAMEID"));
    }

    #[test]
    fn a_host_game_without_a_command_has_no_route() {
        assert_eq!(route(&host_game(LaunchKind::DesktopEntry), None), None);
    }

    #[test]
    fn proton_games_run_through_umu_with_their_launch_config() {
        let game = host_game(LaunchKind::Proton);

        assert_eq!(route(&game, Some(&config())), Some(Route::Proton(config())));
        assert_eq!(route(&game, None), None);
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
        let command = Command {
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
        let command = Command {
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
