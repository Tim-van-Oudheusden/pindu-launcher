//! How Play starts a game and how "Open in launcher" reaches its launcher.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Source};
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
    /// Run a native game in the Steam Linux Runtime through umu inside our sandbox, as
    /// [`crate::proton::runtime_command`] has it.
    Runtime(Command),
}

/// A command to run: an argument list, never a shell string, with the environment and
/// folder it runs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    /// The program and its arguments.
    pub argv: Vec<String>,
    /// Environment variables set for the command.
    pub environment: BTreeMap<String, String>,
    /// The folder it runs in, when it needs one.
    pub working_dir: Option<PathBuf>,
}

/// The display the launcher shows on, which games it starts on the host should open on
/// too. Inside Flatpak, `flatpak-spawn --host` hands a game the host session's display
/// rather than ours, which differs when the launcher runs in a nested gamescope.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LauncherDisplay {
    /// Our X11 display (`DISPLAY`), if we have one.
    pub x11: Option<String>,
    /// Our Wayland display (`WAYLAND_DISPLAY`), if we have one.
    pub wayland: Option<String>,
}

impl LauncherDisplay {
    /// Our display from our environment, which `var` reads by name. Flatpak before 1.14
    /// renames the X11 display to `:99.0` in the sandbox, a name the host does not know,
    /// so that one is not ours to pass on.
    pub fn from_vars(var: impl Fn(&str) -> Option<String>) -> Self {
        Self {
            x11: var("DISPLAY").filter(|display| display != ":99.0"),
            wayland: var("WAYLAND_DISPLAY"),
        }
    }
}

impl Command {
    /// The argument list that runs this command on the host from inside Flatpak. The
    /// environment and folder go as options, since `flatpak-spawn` does not forward ours;
    /// there is no `--watch-bus`, so the game stays our child until it exits.
    pub fn flatpak_spawn_argv(&self) -> Vec<String> {
        self.spawn_argv(None)
    }

    /// [`Command::flatpak_spawn_argv`] for a game that opens on our display rather than
    /// the host session's.
    pub fn flatpak_spawn_argv_on(&self, display: &LauncherDisplay) -> Vec<String> {
        self.spawn_argv(Some(display))
    }

    fn spawn_argv(&self, display: Option<&LauncherDisplay>) -> Vec<String> {
        let mut argv = vec!["flatpak-spawn".to_owned(), "--host".to_owned()];
        if let Some(dir) = &self.working_dir {
            argv.push(format!("--directory={}", dir.display()));
        }
        let mut environment: BTreeMap<&str, &str> = BTreeMap::new();
        if let Some(display) = display {
            if let Some(x11) = &display.x11 {
                environment.insert("DISPLAY", x11);
            }
            // Without a Wayland display of ours, an empty name rather than none: unset,
            // libwayland falls back to `wayland-0`, the desktop's, and the game opens
            // there; an empty name connects nowhere, so the game uses our X11 display.
            environment.insert("WAYLAND_DISPLAY", display.wayland.as_deref().unwrap_or(""));
        }
        environment.extend(
            self.environment
                .iter()
                .map(|(key, value)| (key.as_str(), value.as_str())),
        );
        argv.extend(
            environment
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
        LaunchKind::RuntimeNative => {
            config.map(|config| Route::Runtime(crate::proton::runtime_command(game, config)))
        }
    }
}

/// Whether we record play sessions for a game: every game we start ourselves, Windows
/// games and games in the Steam Linux Runtime included, except Faugus's, whose launcher
/// tracks playtime and which we import instead.
pub fn tracks_sessions(game: &Game) -> bool {
    matches!(
        game.launch_kind,
        LaunchKind::HostCommand
            | LaunchKind::DesktopEntry
            | LaunchKind::Proton
            | LaunchKind::RuntimeNative
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
/// keeps their settings.
pub fn launch_settings(game: &Game) -> Option<LaunchSettings> {
    match game.launch_kind {
        LaunchKind::StoreUri => None,
        LaunchKind::HostCommand
        | LaunchKind::DesktopEntry
        | LaunchKind::Proton
        | LaunchKind::RuntimeNative => Some(LaunchSettings {
            proton: game.launch_kind == LaunchKind::Proton,
            arguments: game.source() == Source::Manual,
        }),
    }
}

/// Whether fullscreen mode offers a game's Performance Overlay (MangoHud) and GameMode
/// switches, in Quick Access and its options sheet: every game we run ourselves, whose
/// launch settings are ours. A store game's launcher keeps its settings, so it has none.
pub fn offers_performance(game: &Game) -> bool {
    launch_settings(game).is_some()
}

/// How a game we run starts this once: as its launch settings say, with a log of it, or
/// safely, to tell a broken overlay or wrapper from a broken game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Start {
    /// Play: as its launch settings say.
    Play,
    /// "Play with Logs": as its launch settings say, keeping its output in a new log.
    WithLogs,
    /// "Start Safely": without its wrappers, MangoHud, GameMode or its own environment
    /// variables, keeping its output in a new log. Its saved settings stay as they are.
    Safely,
}

impl Start {
    /// Whether this start keeps the game's output in a new log.
    pub fn keeps_log(self) -> bool {
        self != Self::Play
    }

    /// The launch config the game starts with this once: `config` itself, or for a safe
    /// start only its program, arguments, folder, Proton build and prefix.
    pub fn config(self, config: LaunchConfig) -> LaunchConfig {
        match self {
            Self::Play | Self::WithLogs => config,
            Self::Safely => LaunchConfig {
                environment: BTreeMap::new(),
                mangohud: false,
                gamemode: false,
                wrappers: Vec::new(),
                ..config
            },
        }
    }
}

/// A game ID and window class of the game's own, so umu, the compositor and the shell
/// tell our games apart: `pindu-<hash>` from the game's ID.
pub(crate) fn session_environment(game: &Game) -> BTreeMap<String, String> {
    let class = window_class(&game.id);
    BTreeMap::from([
        ("GAMEID".to_owned(), format!("umu-{class}")),
        ("SDL_VIDEO_WAYLAND_WMCLASS".to_owned(), class.clone()),
        ("SDL_VIDEO_X11_WMCLASS".to_owned(), class),
    ])
}

/// The window class of the game's own the game runs with (see [`session_environment`]):
/// `pindu-<hash>` from its ID.
pub(crate) fn window_class(game: &GameId) -> String {
    let hash = sha256_hex(game.as_str().as_bytes());
    format!("pindu-{}", &hash[..12])
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

/// What decides how Play hands a Steam game to Steam: a URI, or the `steam` command with
/// `-silent` (see [`silent_steam`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamSession<'a> {
    /// The launcher runs in a `gamescope --steam` session.
    pub steam_gamescope: bool,
    /// The launcher may run commands on the host.
    pub host_spawn: bool,
    /// The Steam folder the library was imported from, if one was found.
    pub steam_root: Option<&'a Path>,
}

/// The Steam Flatpak's app ID, whose data lives under `~/.var/app/<id>`.
const STEAM_FLATPAK: &str = "com.valvesoftware.Steam";

/// The command that hands a Steam game's `uri` to Steam with `-silent`, so a Steam client
/// that is not running yet starts without its library window, or `None` when the URI
/// stays the hand-off. A library under `~/.var/app/com.valvesoftware.Steam` belongs to
/// the Steam Flatpak, which `flatpak run` starts; any other belongs to the host's `steam`.
pub fn silent_steam(game: &Game, uri: &str, session: &SteamSession<'_>) -> Option<Command> {
    if game.source() != Source::Steam {
        return None;
    }
    steam_command(uri, session)
}

/// Steam Big Picture's URI, which opens the Steam client in its controller mode.
pub const BIG_PICTURE_URI: &str = "steam://open/bigpicture";

/// How Home opens Steam Big Picture, the way Play hands a Steam game to Steam (see
/// [`silent_steam`]): the `steam` command with `-silent` where that is how a game goes,
/// its URI otherwise. `None` where no Steam folder was found, so Home does not offer it.
pub fn big_picture(session: &SteamSession<'_>) -> Option<Route> {
    session.steam_root?;
    Some(match steam_command(BIG_PICTURE_URI, session) {
        Some(command) => Route::HostSpawn(command),
        None => Route::StoreUri(BIG_PICTURE_URI.to_owned()),
    })
}

/// The command that hands `uri` to Steam with `-silent` in a `gamescope --steam` session
/// where the launcher may run commands on the host and knows which Steam has the library.
fn steam_command(uri: &str, session: &SteamSession<'_>) -> Option<Command> {
    if !(session.steam_gamescope && session.host_spawn) {
        return None;
    }
    let root = session.steam_root?;
    let mut argv = if in_steam_flatpak(root) {
        vec![
            "flatpak".to_owned(),
            "run".to_owned(),
            STEAM_FLATPAK.to_owned(),
        ]
    } else {
        vec!["steam".to_owned()]
    };
    argv.extend(["-silent".to_owned(), uri.to_owned()]);
    Some(Command {
        argv,
        environment: BTreeMap::new(),
        working_dir: None,
    })
}

/// Whether a Steam folder lies in the Steam Flatpak's data, `.var/app/<id>`.
fn in_steam_flatpak(root: &Path) -> bool {
    let parts: Vec<_> = root.components().map(|part| part.as_os_str()).collect();
    parts
        .windows(3)
        .any(|window| window == [".var", "app", STEAM_FLATPAK])
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

/// A page about a game that its players keep on the web, which the browser opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebPage {
    /// ProtonDB's reports of how the game runs on Linux.
    ProtonDb,
    /// PCGamingWiki's fixes and settings for the game.
    PcGamingWiki,
}

/// The address of `page` for `game`, by its Steam app ID; `None` for a game that is not
/// on Steam. Pindu launcher only hands the address to the browser and fetches nothing.
pub fn web_page_uri(game: &Game, page: WebPage) -> Option<String> {
    let app_id = game.id.native_id();
    if game.source() != Source::Steam
        || app_id.is_empty()
        || !app_id.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    Some(match page {
        WebPage::ProtonDb => format!("https://www.protondb.com/app/{app_id}"),
        WebPage::PcGamingWiki => {
            format!("https://www.pcgamingwiki.com/api/appid.php?appid={app_id}")
        }
    })
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

    use crate::model::{Game, GameId, LaunchConfig, LaunchKind, PerformanceTool, Source};

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
        assert_eq!(
            launch_settings(&game(Source::Manual, LaunchKind::RuntimeNative)),
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
    fn games_we_run_offer_the_performance_switches_and_store_games_do_not() {
        let game = |source, kind| Game::new(GameId::new(source, "x"), "X", kind, 0);

        for (source, kind) in [
            (Source::Manual, LaunchKind::Proton),
            (Source::Manual, LaunchKind::HostCommand),
            (Source::Manual, LaunchKind::RuntimeNative),
            (Source::Lutris, LaunchKind::HostCommand),
            (Source::Desktop, LaunchKind::DesktopEntry),
        ] {
            assert!(offers_performance(&game(source, kind)), "{source} {kind}");
        }
        for source in [Source::Steam, Source::HeroicEpic, Source::Lutris] {
            let store_game = game(source, LaunchKind::StoreUri);
            assert!(
                !offers_performance(&store_game),
                "{source}: its launcher runs it"
            );
        }
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
    fn proton_games_are_tracked_like_the_games_we_start_on_the_host() {
        assert!(tracks_sessions(&host_game(LaunchKind::Proton)));
    }

    #[test]
    fn runtime_native_games_run_in_the_steam_linux_runtime_through_umu() {
        let game = host_game(LaunchKind::RuntimeNative);

        let Some(Route::Runtime(command)) = route(&game, Some(&config())) else {
            panic!("no runtime route");
        };

        assert_eq!(
            command.argv,
            ["umu-run", "/opt/Celeste Game/Celeste", "--fullscreen"]
        );
        // umu runs the program as it is, in its default runtime, without Proton.
        assert_eq!(command.environment["UMU_NO_PROTON"], "1");
        assert!(!command.environment.contains_key("PROTONPATH"));
        assert!(command.environment["GAMEID"].starts_with("umu-pindu-"));
        assert_eq!(command.environment["SDL_VIDEODRIVER"], "wayland");
        assert_eq!(
            command.working_dir.as_deref(),
            Some(Path::new("/opt/Celeste Game"))
        );
        assert!(tracks_sessions(&game));
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
    fn a_safe_start_runs_the_bare_game_with_only_our_variables() {
        let game = host_game(LaunchKind::HostCommand);
        let config = Start::Safely.config(LaunchConfig {
            wrappers: vec!["gamescope".into(), "obs-gamecapture".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        });

        let Some(Route::HostSpawn(command)) = route(&game, Some(&config)) else {
            panic!("no host route");
        };

        assert_eq!(command.argv, ["/opt/Celeste Game/Celeste", "--fullscreen"]);
        assert_eq!(command.environment, session_environment(&game));
        assert_eq!(command.working_dir, Some("/opt/Celeste Game".into()));
    }

    #[test]
    fn a_safe_start_keeps_the_game_s_program_build_and_prefix() {
        let config = LaunchConfig {
            proton_version: Some("GE-Proton10-34".into()),
            prefix_path: Some("/prefixes/celeste".into()),
            wrappers: vec!["gamescope".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        };

        assert_eq!(
            Start::Safely.config(config.clone()),
            LaunchConfig {
                environment: BTreeMap::new(),
                mangohud: false,
                gamemode: false,
                wrappers: Vec::new(),
                ..config
            }
        );
    }

    #[test]
    fn play_keeps_the_settings_and_only_play_leaves_no_log() {
        let config = LaunchConfig {
            wrappers: vec!["gamescope".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        };

        assert_eq!(Start::Play.config(config.clone()), config);
        assert_eq!(Start::WithLogs.config(config.clone()), config);
        assert!(!Start::Play.keeps_log());
        assert!(Start::WithLogs.keeps_log());
        assert!(Start::Safely.keeps_log());
    }

    #[test]
    fn a_safe_start_leaves_the_saved_settings_as_they_are() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(&dir.path().join("library.db")).unwrap();
        let game = host_game(LaunchKind::HostCommand);
        let saved = LaunchConfig {
            wrappers: vec!["gamescope".into()],
            gamemode: true,
            mangohud: true,
            ..config()
        };
        store.insert_game(&game).unwrap();
        store.set_launch_config(&game.id, &saved).unwrap();

        let loaded = store.launch_config(&game.id).unwrap().unwrap();
        route(&game, Some(&Start::Safely.config(loaded)));

        assert_eq!(store.launch_config(&game.id).unwrap(), Some(saved));
    }

    #[test]
    fn switching_a_host_game_s_tools_on_wraps_its_next_launch_in_them() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(&dir.path().join("library.db")).unwrap();
        let game = host_game(LaunchKind::HostCommand);
        store.insert_game(&game).unwrap();
        store.set_launch_config(&game.id, &config()).unwrap();

        store
            .set_performance_tool(&game.id, PerformanceTool::Overlay, true)
            .unwrap();
        store
            .set_performance_tool(&game.id, PerformanceTool::GameMode, true)
            .unwrap();

        let config = store.launch_config(&game.id).unwrap();
        let Some(Route::HostSpawn(command)) = route(&game, config.as_ref()) else {
            panic!("no host route");
        };
        assert_eq!(
            command.argv,
            [
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

    #[test]
    fn flatpak_spawn_hands_the_game_our_display() {
        let command = Command {
            argv: vec!["neverball".into()],
            environment: Default::default(),
            working_dir: None,
        };
        let display = LauncherDisplay {
            x11: Some(":1".into()),
            wayland: Some("gamescope-0".into()),
        };

        assert_eq!(
            command.flatpak_spawn_argv_on(&display),
            [
                "flatpak-spawn",
                "--host",
                "--env=DISPLAY=:1",
                "--env=WAYLAND_DISPLAY=gamescope-0",
                "--",
                "neverball",
            ]
        );
    }

    #[test]
    fn flatpak_spawn_without_a_wayland_display_of_ours_drops_the_host_s() {
        // `--nosocket=wayland` in a nested gamescope: a Wayland-capable game must not
        // find the desktop compositor, or it opens there rather than on our X display.
        // Unset is not enough: libwayland then tries `wayland-0`, the desktop's. An empty
        // name connects nowhere, and GTK and SDL fall back to X11.
        let command = Command {
            argv: vec!["neverball".into()],
            environment: Default::default(),
            working_dir: None,
        };
        let display = LauncherDisplay {
            x11: Some(":1".into()),
            wayland: None,
        };

        assert_eq!(
            command.flatpak_spawn_argv_on(&display),
            [
                "flatpak-spawn",
                "--host",
                "--env=DISPLAY=:1",
                "--env=WAYLAND_DISPLAY=",
                "--",
                "neverball",
            ]
        );
    }

    #[test]
    fn flatpak_spawn_lets_the_user_s_own_display_win() {
        let command = Command {
            argv: vec!["neverball".into()],
            environment: [
                ("DISPLAY".to_owned(), ":0".to_owned()),
                ("WAYLAND_DISPLAY".to_owned(), "wayland-1".to_owned()),
            ]
            .into(),
            working_dir: None,
        };
        let display = LauncherDisplay {
            x11: Some(":1".into()),
            wayland: None,
        };

        assert_eq!(
            command.flatpak_spawn_argv_on(&display),
            [
                "flatpak-spawn",
                "--host",
                "--env=DISPLAY=:0",
                "--env=WAYLAND_DISPLAY=wayland-1",
                "--",
                "neverball",
            ]
        );
    }

    #[test]
    fn our_display_comes_from_display_and_wayland_display() {
        let vars: BTreeMap<&str, &str> = [("DISPLAY", ":1"), ("HOME", "/home/a")].into();

        assert_eq!(
            LauncherDisplay::from_vars(|name| vars.get(name).map(|value| (*value).to_owned())),
            LauncherDisplay {
                x11: Some(":1".into()),
                wayland: None,
            }
        );
    }

    #[test]
    fn the_sandbox_s_own_x11_display_name_is_not_ours_to_pass_on() {
        // Flatpak before 1.14 hides the host's display number behind `:99.0`, a name
        // that means nothing on the host; the host session's display is right there.
        let display =
            LauncherDisplay::from_vars(|name| (name == "DISPLAY").then(|| ":99.0".to_owned()));

        assert_eq!(display.x11, None);
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

    #[test]
    fn a_steam_game_links_to_its_protondb_and_pcgamingwiki_pages_by_app_id() {
        let steam = store_game(Source::Steam, "570");
        assert_eq!(
            web_page_uri(&steam, WebPage::ProtonDb).as_deref(),
            Some("https://www.protondb.com/app/570")
        );
        assert_eq!(
            web_page_uri(&steam, WebPage::PcGamingWiki).as_deref(),
            Some("https://www.pcgamingwiki.com/api/appid.php?appid=570")
        );
    }

    #[test]
    fn only_steam_games_with_a_numeric_app_id_have_web_pages() {
        for game in [
            store_game(Source::HeroicGog, "570"),
            store_game(Source::Steam, "570/../x"),
            store_game(Source::Steam, ""),
        ] {
            assert_eq!(
                web_page_uri(&game, WebPage::ProtonDb),
                None,
                "{:?}",
                game.id
            );
            assert_eq!(web_page_uri(&game, WebPage::PcGamingWiki), None);
        }
    }

    fn steam_session(root: &Path) -> SteamSession<'_> {
        SteamSession {
            steam_gamescope: true,
            host_spawn: true,
            steam_root: Some(root),
        }
    }

    #[test]
    fn in_a_steam_gamescope_native_steam_starts_the_game_silently() {
        let game = store_game(Source::Steam, "2379780");
        let session = steam_session(Path::new("/home/me/.local/share/Steam"));

        assert_eq!(
            silent_steam(&game, "steam://rungameid/2379780", &session).map(|command| command.argv),
            Some(vec![
                "steam".to_owned(),
                "-silent".to_owned(),
                "steam://rungameid/2379780".to_owned(),
            ])
        );
    }

    #[test]
    fn a_library_from_the_steam_flatpak_starts_through_flatpak_run() {
        let game = store_game(Source::Steam, "2379780");
        let session = steam_session(Path::new(
            "/home/me/.var/app/com.valvesoftware.Steam/data/Steam",
        ));

        assert_eq!(
            silent_steam(&game, "steam://rungameid/2379780", &session).map(|command| command.argv),
            Some(vec![
                "flatpak".to_owned(),
                "run".to_owned(),
                "com.valvesoftware.Steam".to_owned(),
                "-silent".to_owned(),
                "steam://rungameid/2379780".to_owned(),
            ])
        );
    }

    #[test]
    fn the_launch_options_dialog_goes_to_steam_silently_too() {
        let mut game = store_game(Source::Steam, "570");
        game.steam_launch_dialog = true;
        let Some(Route::StoreUri(uri)) = route(&game, None) else {
            panic!("a Steam game has a store URI");
        };
        let session = steam_session(Path::new("/home/me/.steam/steam"));

        assert_eq!(
            silent_steam(&game, &uri, &session).map(|command| command.argv),
            Some(vec![
                "steam".to_owned(),
                "-silent".to_owned(),
                "steam://launch/570/dialog".to_owned(),
            ])
        );
    }

    #[test]
    fn outside_a_steam_gamescope_the_uri_stays() {
        let game = store_game(Source::Steam, "570");
        let session = SteamSession {
            steam_gamescope: false,
            ..steam_session(Path::new("/home/me/.steam/steam"))
        };

        assert_eq!(silent_steam(&game, "steam://rungameid/570", &session), None);
    }

    #[test]
    fn without_host_spawn_the_uri_stays() {
        let game = store_game(Source::Steam, "570");
        let session = SteamSession {
            host_spawn: false,
            ..steam_session(Path::new("/home/me/.steam/steam"))
        };

        assert_eq!(silent_steam(&game, "steam://rungameid/570", &session), None);
    }

    #[test]
    fn other_store_games_keep_their_uri() {
        let session = steam_session(Path::new("/home/me/.steam/steam"));
        for (source, uri) in [
            (
                Source::HeroicEpic,
                "heroic://launch?appName=Sugar&runner=legendary",
            ),
            (Source::Lutris, "lutris:rungameid/42"),
            (Source::Bottles, "bottles:run/Gaming/Celeste"),
        ] {
            assert_eq!(
                silent_steam(&store_game(source, "1"), uri, &session),
                None,
                "{source}"
            );
        }
    }

    #[test]
    fn without_a_known_steam_folder_the_uri_stays() {
        // Which Steam has the game is unknown, so the portal picks the one that
        // handles `steam://`.
        let game = store_game(Source::Steam, "570");
        let session = SteamSession {
            steam_root: None,
            ..steam_session(Path::new("/home/me/.steam/steam"))
        };

        assert_eq!(silent_steam(&game, "steam://rungameid/570", &session), None);
    }

    #[test]
    fn in_a_steam_gamescope_big_picture_opens_through_steam_silently() {
        let native = steam_session(Path::new("/home/me/.local/share/Steam"));
        let flatpak = steam_session(Path::new(
            "/home/me/.var/app/com.valvesoftware.Steam/data/Steam",
        ));

        assert_eq!(
            big_picture(&native),
            Some(Route::HostSpawn(Command {
                argv: vec![
                    "steam".to_owned(),
                    "-silent".to_owned(),
                    "steam://open/bigpicture".to_owned(),
                ],
                environment: BTreeMap::new(),
                working_dir: None,
            }))
        );
        let Some(Route::HostSpawn(command)) = big_picture(&flatpak) else {
            panic!("the Steam Flatpak opens Big Picture through its command");
        };
        assert_eq!(
            command.argv,
            [
                "flatpak",
                "run",
                "com.valvesoftware.Steam",
                "-silent",
                "steam://open/bigpicture"
            ]
        );
    }

    #[test]
    fn big_picture_is_offered_only_where_a_steam_folder_was_found() {
        let found = SteamSession {
            steam_gamescope: false,
            ..steam_session(Path::new("/home/me/.steam/steam"))
        };
        assert_eq!(
            big_picture(&found),
            Some(Route::StoreUri("steam://open/bigpicture".to_owned()))
        );
        let without_host_spawn = SteamSession {
            host_spawn: false,
            ..steam_session(Path::new("/home/me/.steam/steam"))
        };
        assert_eq!(
            big_picture(&without_host_spawn),
            Some(Route::StoreUri("steam://open/bigpicture".to_owned()))
        );
        for steam_gamescope in [false, true] {
            let none = SteamSession {
                steam_gamescope,
                host_spawn: true,
                steam_root: None,
            };
            assert_eq!(big_picture(&none), None);
        }
    }
}
