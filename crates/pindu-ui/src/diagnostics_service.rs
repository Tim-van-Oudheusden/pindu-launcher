//! Gathers the debugging information About carries ([`pindu_core::diagnostics`]): reads
//! the sandbox's and the system's files, the library, the tools and, when the user chose
//! a game, its newest log on a worker thread, and asks the gamescope the app runs in for
//! its version. Fullscreen mode's report also saves a copy to the reports folder
//! ([`pindu_core::report_screen`]).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;
use std::{fs, io};

use gtk::{gio, glib};
use pindu_core::diagnostics::{
    self, ChosenLog, Facts, Gamescope, HOST_OS_RELEASE_FILES, LogChoice, LogFile, Private,
    SessionFacts, Tool,
};
use pindu_core::launch::Command;
use pindu_core::model::GameId;
use pindu_core::paths::default_library_db_path;
use pindu_core::report_screen::{self, KEEP};
use pindu_core::store::Store;
use pindu_core::system::update::{OS_RELEASE_FILES, OsRelease};
use pindu_core::umu;

use crate::config::{APP_ID, VERSION};
use crate::log_service::{self, data_home, file_names};
use crate::{permission_service, proton_launch, session_service};

/// How long `gamescope --version` may take before the report goes without it.
const GAMESCOPE_TIMEOUT: Duration = Duration::from_secs(2);

/// The report for About's Debugging Information. `steam_gamescope` says whether the app
/// runs in a `--steam` gamescope, `own_session` whether it runs as its own login session.
/// With `game`, the game's newest log from Play with Logs follows the report.
pub async fn debug_info(steam_gamescope: bool, own_session: bool, game: Option<GameId>) -> String {
    let var = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    let in_gamescope = steam_gamescope || var("GAMESCOPE_WAYLAND_DISPLAY").is_some();
    let gamescope = if in_gamescope {
        Some(Gamescope {
            version: gamescope_version().await,
            steam_mode: steam_gamescope,
            own_session,
        })
    } else {
        None
    };
    let session = SessionFacts {
        session_type: var("XDG_SESSION_TYPE"),
        desktop: var("XDG_CURRENT_DESKTOP"),
        gamescope,
    };
    gio::spawn_blocking(move || report(session, game.as_ref()))
        .await
        .unwrap_or_else(|_| "The debugging information could not be gathered.".to_owned())
}

/// Saves `report` to the reports folder, named after the local time, and drops the saved
/// reports beyond the newest [`KEEP`]; gives the file's path, or why it could not be saved.
pub async fn save_report(report: String) -> Result<PathBuf, String> {
    gio::spawn_blocking(move || write_report(&report).map_err(|error| error.to_string()))
        .await
        .unwrap_or_else(|_| Err("the report could not be saved".to_owned()))
}

fn write_report(report: &str) -> io::Result<PathBuf> {
    let dir = report_screen::reports_dir(&data_home());
    fs::create_dir_all(&dir)?;
    let now = glib::DateTime::now_local().map_err(io::Error::other)?;
    let stamp = now.format("%Y-%m-%dT%H-%M-%S").map_err(io::Error::other)?;
    let mut names = file_names(&dir)?;
    let name = report_screen::new_report_name(&stamp, &names);
    let path = dir.join(&name);
    fs::write(&path, report)?;
    names.push(name);
    for stale in report_screen::stale_reports(&names, KEEP) {
        if let Err(error) = fs::remove_file(dir.join(&stale)) {
            glib::g_warning!("pindu", "Could not remove the old report {stale}: {error}");
        }
    }
    Ok(path)
}

/// The games whose newest log the user can add to the report, the game played with logs
/// last first.
pub async fn log_choices() -> Vec<LogChoice> {
    gio::spawn_blocking(|| {
        let games = Store::open(&default_library_db_path())
            .and_then(|store| store.games())
            .unwrap_or_default();
        diagnostics::log_choices(&games, |id| {
            log_service::latest(id)?
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
    })
    .await
    .unwrap_or_default()
}

/// Asks the host's gamescope for its version; `None` when it cannot be run, as inside a
/// Flatpak without host spawn.
async fn gamescope_version() -> Option<String> {
    if !permission_service::host_spawn_allowed() {
        return None;
    }
    let command = Command {
        argv: vec!["gamescope".to_owned(), "--version".to_owned()],
        environment: BTreeMap::new(),
        working_dir: None,
    };
    let child = session_service::spawn_captured(&command).ok()?;
    let output =
        glib::future_with_timeout(GAMESCOPE_TIMEOUT, child.communicate_utf8_future(None)).await;
    let Ok(Ok((stdout, stderr))) = output else {
        child.force_exit();
        return None;
    };
    [stdout, stderr]
        .into_iter()
        .flatten()
        .find_map(|output| diagnostics::gamescope_version(&output))
}

/// Reads everything else the report says, and writes it, followed by `game`'s newest log.
fn report(session: SessionFacts, game: Option<&GameId>) -> String {
    let flatpak_info = std::fs::read_to_string("/.flatpak-info").ok();
    let os_release_files = if flatpak_info.is_some() {
        HOST_OS_RELEASE_FILES
    } else {
        OS_RELEASE_FILES
    };
    let os_release = os_release_files
        .iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .map(|text| OsRelease::parse(&text))
        .unwrap_or_default();
    let store = Store::open(&default_library_db_path()).ok();
    let (states, games) = store.as_ref().map_or_else(Default::default, |store| {
        (
            store.source_states().unwrap_or_default(),
            store.games().unwrap_or_default(),
        )
    });
    let facts = Facts {
        version: VERSION.to_owned(),
        app_id: APP_ID.to_owned(),
        install: diagnostics::install(flatpak_info.as_deref(), &os_release),
        distribution: os_release.pretty_name,
        session,
        permissions: diagnostics::permissions(flatpak_info.as_deref()),
        schema_version: store.and_then(|store| store.schema_version().ok()),
        sources: diagnostics::source_facts(&states, &games),
        tools: tools(flatpak_info.is_some()),
    };
    let private = Private::new(homes(), &games);
    let mut report = facts.report(&private);
    let chosen = game.and_then(|id| games.iter().find(|game| &game.id == id));
    if let Some(game) = chosen {
        let log = ChosenLog {
            title: game.title.clone(),
            log: log_service::latest(&game.id).and_then(|path| {
                Some(LogFile {
                    name: path.file_name()?.to_string_lossy().into_owned(),
                    text: String::from_utf8_lossy(&std::fs::read(&path).ok()?).into_owned(),
                })
            }),
        };
        report.push_str(&log.section(&private));
    }
    report
}

/// The tools the app looks for: umu-run, the Proton builds and, in Flatpak, the MangoHud
/// Vulkan layer.
fn tools(in_flatpak: bool) -> Vec<Tool> {
    let umu_run = std::env::var_os("PATH")
        .and_then(|path| umu::find_run(&path, umu::is_executable))
        .map(|path| path.display().to_string());
    let builds: Vec<String> = proton_launch::builds()
        .into_iter()
        .map(|build| build.name)
        .collect();
    let mut tools = vec![
        Tool {
            name: umu::RUN.to_owned(),
            found: umu_run,
        },
        Tool {
            name: "Proton builds".to_owned(),
            found: (!builds.is_empty()).then(|| builds.join(", ")),
        },
    ];
    if in_flatpak {
        tools.push(Tool {
            name: "MangoHud Vulkan layer".to_owned(),
            found: permission_service::mangohud_layer_installed().then(|| "installed".to_owned()),
        });
    }
    tools
}

/// The spellings of the home folder: as `HOME` gives it, and with its links resolved
/// (`/var/home/ada` for `/home/ada` on Fedora Atomic).
fn homes() -> Vec<PathBuf> {
    let Some(home) = std::env::home_dir() else {
        return Vec::new();
    };
    let resolved = std::fs::canonicalize(&home).ok();
    std::iter::once(home).chain(resolved).collect()
}
