//! The debugging information About carries: a plain-text report of the version, how the
//! app was installed, the system, the session, each source's state and game count, the
//! permission tier, the tools found, the library's schema version and the last sync's
//! problems. The user copies or saves it and decides where it goes, so it holds no game
//! title and no home path: home folders are written as `~`, and titles in the problems
//! the sources report are replaced.

use std::fmt;
use std::path::PathBuf;

use crate::model::{Game, GameId, Source, SourceFound, SourceState};
use crate::permissions::host_spawn_allowed;
use crate::store::LATEST_SCHEMA_VERSION;
use crate::system::update::OsRelease;

/// Where the host's os-release is inside Flatpak, in the order to look: the copy Flatpak
/// provides, then the host's `/usr` (`host-os:ro`). The sandbox's own `/etc/os-release`
/// names the runtime.
pub const HOST_OS_RELEASE_FILES: [&str; 2] =
    ["/run/host/os-release", "/run/host/usr/lib/os-release"];

/// How the app was installed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Install {
    /// As a Flatpak, on the runtime and the Flatpak version its `/.flatpak-info` names.
    Flatpak {
        /// The runtime, such as `runtime/org.gnome.Platform/x86_64/51`.
        runtime: Option<String>,
        /// The version of Flatpak that started it.
        flatpak_version: Option<String>,
    },
    /// As part of Pindu OS.
    PinduOs,
    /// As a native package, or built from source.
    Native,
}

/// The session the app runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionFacts {
    /// `XDG_SESSION_TYPE`, such as `wayland`.
    pub session_type: Option<String>,
    /// `XDG_CURRENT_DESKTOP`, such as `GNOME`.
    pub desktop: Option<String>,
    /// The gamescope the app runs in, if any.
    pub gamescope: Option<Gamescope>,
}

/// The gamescope the app runs in.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gamescope {
    /// Its version, when it could be asked.
    pub version: Option<String>,
    /// Whether it runs with `--steam`, so the app speaks its focus protocol.
    pub steam_mode: bool,
    /// Whether the app runs as its own login session.
    pub own_session: bool,
}

/// What the sandbox lets the app start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissions {
    /// No sandbox: the app starts every game itself.
    Unsandboxed,
    /// Flatpak with host spawn granted: every game can start.
    HostSpawn,
    /// Flatpak without host spawn: store launches and games in the sandbox only.
    Sandboxed,
}

/// A tool the app looks for, and what was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tool {
    /// The tool's name, such as `umu-run`.
    pub name: String,
    /// What was found, such as its path or the builds; `None` when it was not.
    pub found: Option<String>,
}

/// One source's state and how many games it has in the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFacts {
    /// The stored state: on or off, its folder, the last sync's problems.
    pub state: SourceState,
    /// The games it has in the library, apart from removed and missing ones.
    pub games: usize,
    /// Its games the last sync found gone.
    pub missing: usize,
}

/// Everything the report says, gathered by the UI's services.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// The app's version.
    pub version: String,
    /// The app ID, which tells a development build from a release.
    pub app_id: String,
    /// How the app was installed.
    pub install: Install,
    /// The distribution's name, from the host's os-release.
    pub distribution: Option<String>,
    /// The session the app runs in.
    pub session: SessionFacts,
    /// What the sandbox lets the app start.
    pub permissions: Permissions,
    /// The library's schema version; `None` when the library could not be read.
    pub schema_version: Option<u32>,
    /// Each source's state and game count.
    pub sources: Vec<SourceFacts>,
    /// The tools the app looks for.
    pub tools: Vec<Tool>,
}

/// What the report must not show: the home folder, in every spelling the system uses
/// (such as `/home/ada` and `/var/home/ada`), and the library's titles.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Private {
    /// The home folder's spellings, written as `~`.
    pub homes: Vec<PathBuf>,
    /// The titles of the library's games.
    pub titles: Vec<String>,
}

impl Private {
    /// What to keep out of a report about a library holding `games`.
    pub fn new(homes: Vec<PathBuf>, games: &[Game]) -> Self {
        Self {
            homes,
            titles: games.iter().map(|game| game.title.clone()).collect(),
        }
    }
}

/// A game's newest log, which the user chose to add to the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChosenLog {
    /// The chosen game's title, which the log keeps because the user chose the game.
    pub title: String,
    /// The game's newest log; `None` when it has none yet.
    pub log: Option<LogFile>,
}

/// A log file: its name and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogFile {
    /// The file's name, such as `2026-11-05T14-03-22.log`.
    pub name: String,
    /// The file's text.
    pub text: String,
}

impl ChosenLog {
    /// The lines added to the end of the report: the log, with every home folder spelling
    /// written as `~` and every other title of the library replaced.
    pub fn section(&self, private: &Private) -> String {
        let mut section = format!("\nNewest log of “{}”", self.title);
        match &self.log {
            Some(log) => section.push_str(&format!(" ({}):\n{}", log.name, trimmed(&log.text))),
            None => section.push_str(": none, start the game with Play with Logs\n"),
        }
        without_homes(
            &without_titles_but(&section, &private.titles, &self.title),
            &private.homes,
        )
    }
}

/// A game whose newest log the user can add to the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogChoice {
    /// The game.
    pub id: GameId,
    /// The game's title.
    pub title: String,
    /// The name of the game's newest log, such as `2026-11-05T14-03-22.log`.
    pub newest: String,
}

/// The `games` that have a log, as `newest` finds each one's newest log, the game played
/// with logs last first, so the game that just went wrong leads.
pub fn log_choices(games: &[Game], newest: impl Fn(&GameId) -> Option<String>) -> Vec<LogChoice> {
    let mut choices: Vec<LogChoice> = games
        .iter()
        .filter_map(|game| {
            Some(LogChoice {
                newest: newest(&game.id)?,
                id: game.id.clone(),
                title: game.title.clone(),
            })
        })
        .collect();
    // A log's name starts with the time it started, so the latest name is the newest.
    choices.sort_by(|a, b| b.newest.cmp(&a.newest));
    choices
}

/// How many of a long log's first lines the report keeps: the header Play with Logs
/// writes, with the command, and the game's first words.
const LOG_HEAD: usize = 10;

/// How many of a long log's last lines the report keeps, where a crash usually shows.
const LOG_TAIL: usize = 100;

/// A log's text with its first [`LOG_HEAD`] and last [`LOG_TAIL`] lines, and a line
/// saying how many lines between them are left out, so a long Proton log stays readable.
fn trimmed(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= LOG_HEAD + LOG_TAIL {
        return text.to_owned();
    }
    let left_out = lines.len() - LOG_HEAD - LOG_TAIL;
    let mut kept = lines[..LOG_HEAD].join("\n");
    kept.push_str(&format!("\n[{left_out} lines left out]\n"));
    kept.push_str(&lines[lines.len() - LOG_TAIL..].join("\n"));
    kept.push('\n');
    kept
}

impl Facts {
    /// The report, as plain text in English for whoever reads the bug report. Titles are
    /// taken out of the problems, the only free text from the sources, and every home
    /// folder spelling is written as `~`.
    pub fn report(&self, private: &Private) -> String {
        let mut lines = vec![
            format!("Pindu launcher {}", self.version),
            format!("App ID: {}", self.app_id),
            format!("Installed as: {}", self.install),
            format!(
                "Distribution: {}",
                self.distribution.as_deref().unwrap_or(UNKNOWN)
            ),
            format!(
                "Session: {}, desktop {}",
                self.session.session_type.as_deref().unwrap_or(UNKNOWN),
                self.session.desktop.as_deref().unwrap_or(UNKNOWN)
            ),
            format!(
                "gamescope: {}",
                gamescope_line(self.session.gamescope.as_ref())
            ),
            format!("Permissions: {}", self.permissions),
            format!(
                "Library schema: {} (this version knows {LATEST_SCHEMA_VERSION})",
                self.schema_version
                    .map_or_else(|| "unreadable".to_owned(), |version| version.to_string())
            ),
            String::new(),
            "Sources:".to_owned(),
        ];
        lines.extend(self.sources.iter().map(source_line));
        lines.extend([String::new(), "Tools:".to_owned()]);
        lines.extend(self.tools.iter().map(|tool| {
            format!(
                "  {}: {}",
                tool.name,
                tool.found.as_deref().unwrap_or("not found")
            )
        }));
        lines.extend([String::new(), "Last sync problems:".to_owned()]);
        let problems: Vec<String> = self
            .sources
            .iter()
            .flat_map(|facts| {
                facts.state.errors.iter().map(|problem| {
                    format!(
                        "  {}: {}",
                        facts.state.source,
                        without_titles(problem, &private.titles)
                    )
                })
            })
            .collect();
        if problems.is_empty() {
            lines.push("  none".to_owned());
        }
        lines.extend(problems);
        let mut report = lines.join("\n");
        report.push('\n');
        without_homes(&report, &private.homes)
    }
}

/// What the report says for a fact that could not be found out.
const UNKNOWN: &str = "unknown";

/// What stands in for a title in the report.
const TITLE: &str = "[title]";

impl fmt::Display for Install {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Flatpak {
                runtime,
                flatpak_version,
            } => write!(
                f,
                "Flatpak (runtime {}, Flatpak {})",
                runtime.as_deref().unwrap_or(UNKNOWN),
                flatpak_version.as_deref().unwrap_or(UNKNOWN)
            ),
            Self::PinduOs => f.write_str("Pindu OS"),
            Self::Native => f.write_str("native"),
        }
    }
}

impl fmt::Display for Permissions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unsandboxed => "no sandbox",
            Self::HostSpawn => "Flatpak with host spawn",
            Self::Sandboxed => {
                "Flatpak without host spawn (store launches and games in the sandbox only)"
            }
        })
    }
}

/// What the report says about the gamescope the app runs in.
fn gamescope_line(gamescope: Option<&Gamescope>) -> String {
    let Some(gamescope) = gamescope else {
        return "not running in it".to_owned();
    };
    let mut line = gamescope
        .version
        .as_deref()
        .unwrap_or("version unknown")
        .to_owned();
    if gamescope.steam_mode {
        line.push_str(", --steam");
    }
    if gamescope.own_session {
        line.push_str(", own session");
    }
    line
}

/// One source's line: on or off, its folders, its games and when it last synced.
fn source_line(facts: &SourceFacts) -> String {
    let state = &facts.state;
    if !state.enabled {
        return format!("  {}: off", state.source);
    }
    let mut parts = vec!["on".to_owned()];
    parts.push(match state.found() {
        SourceFound::Folder(folder) => format!("found at {}", folder.display()),
        SourceFound::ApplicationFolders => "found in the application folders".to_owned(),
        SourceFound::NotFound => "not found".to_owned(),
    });
    if let Some(folder) = &state.user_override {
        parts.push(format!("folder picked {}", folder.display()));
    }
    parts.push(format!("{} games", facts.games));
    if facts.missing > 0 {
        parts.push(format!("{} missing", facts.missing));
    }
    if state.last_sync.is_none() {
        parts.push("never synced".to_owned());
    }
    if !state.errors.is_empty() {
        parts.push(format!("{} problems", state.errors.len()));
    }
    format!("  {}: {}", state.source, parts.join(", "))
}

/// `text` with every one of `titles` replaced, the longest first, so that a title inside
/// another is not left half shown.
fn without_titles(text: &str, titles: &[String]) -> String {
    let mut titles: Vec<&str> = titles
        .iter()
        .map(String::as_str)
        .filter(|title| !title.is_empty())
        .collect();
    titles.sort_by_key(|title| std::cmp::Reverse(title.len()));
    titles
        .into_iter()
        .fold(text.to_owned(), |text, title| text.replace(title, TITLE))
}

/// `text` with every one of `titles` replaced but `kept`: first the titles that hold
/// `kept`, then the others wherever `kept` is not.
fn without_titles_but(text: &str, titles: &[String], kept: &str) -> String {
    if kept.is_empty() {
        return without_titles(text, titles);
    }
    let (holding, others): (Vec<String>, Vec<String>) = titles
        .iter()
        .filter(|title| title.as_str() != kept)
        .cloned()
        .partition(|title| title.contains(kept));
    without_titles(text, &holding)
        .split(kept)
        .map(|part| without_titles(part, &others))
        .collect::<Vec<_>>()
        .join(kept)
}

/// `text` with every spelling of the home folder in `homes` written as `~`, the longest
/// first, so that `/var/home/ada` does not become `/var~`.
fn without_homes(text: &str, homes: &[PathBuf]) -> String {
    let mut homes: Vec<&str> = homes
        .iter()
        .filter_map(|home| home.to_str())
        .map(|home| home.trim_end_matches('/'))
        .filter(|home| !home.is_empty())
        .collect();
    homes.sort_by_key(|home| std::cmp::Reverse(home.len()));
    homes
        .into_iter()
        .fold(text.to_owned(), |text, home| without_home(&text, home))
}

/// `text` with `home` written as `~` wherever it is a whole folder: followed by `/` or
/// by a character a folder name does not go on with, so `/home/ada` in `/home/adam` stays.
fn without_home(text: &str, home: &str) -> String {
    let mut redacted = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(home) {
        let after = &rest[at + home.len()..];
        let whole = after
            .chars()
            .next()
            .is_none_or(|next| next == '/' || !(next.is_alphanumeric() || "-_.".contains(next)));
        redacted.push_str(&rest[..at]);
        redacted.push_str(if whole { "~" } else { home });
        rest = after;
    }
    redacted.push_str(rest);
    redacted
}

/// How the app was installed, from the sandbox's `/.flatpak-info` (`None` outside
/// Flatpak) and the host's os-release.
pub fn install(flatpak_info: Option<&str>, os_release: &OsRelease) -> Install {
    let Some(info) = flatpak_info else {
        return if os_release.is_pindu_os() {
            Install::PinduOs
        } else {
            Install::Native
        };
    };
    let (mut runtime, mut flatpak_version) = (None, None);
    let mut group = "";
    for line in info.lines().map(str::trim) {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            group = name;
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let value = Some(value.trim().to_owned());
        match (group, key.trim()) {
            ("Application", "runtime") => runtime = value,
            ("Instance", "flatpak-version") => flatpak_version = value,
            _ => {}
        }
    }
    Install::Flatpak {
        runtime,
        flatpak_version,
    }
}

/// The permission tier, from the sandbox's `/.flatpak-info` (`None` outside Flatpak).
pub fn permissions(flatpak_info: Option<&str>) -> Permissions {
    match flatpak_info {
        None => Permissions::Unsandboxed,
        Some(info) if host_spawn_allowed(Some(info)) => Permissions::HostSpawn,
        Some(_) => Permissions::Sandboxed,
    }
}

/// Every source, in their order, with its stored state among `states` (or the state of a
/// source never seen) and its games among `games`.
pub fn source_facts(states: &[SourceState], games: &[Game]) -> Vec<SourceFacts> {
    Source::ALL
        .iter()
        .map(|&source| {
            let state = states
                .iter()
                .find(|state| state.source == source)
                .cloned()
                .unwrap_or_else(|| SourceState::new(source));
            let (mut count, mut missing) = (0, 0);
            for game in games
                .iter()
                .filter(|game| game.id.source() == source && !game.user_removed)
            {
                if game.missing_since.is_some() {
                    missing += 1;
                } else {
                    count += 1;
                }
            }
            SourceFacts {
                state,
                games: count,
                missing,
            }
        })
        .collect()
}

/// The version `gamescope --version` prints, from its output.
pub fn gamescope_version(output: &str) -> Option<String> {
    let (_, after) = output.split_once("gamescope version ")?;
    after.split_whitespace().next().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LaunchKind;

    fn facts() -> Facts {
        let mut steam = SourceState::new(Source::Steam);
        steam.resolved_location = Some(PathBuf::from("/var/home/ada/.local/share/Steam"));
        steam.errors =
            vec!["/home/ada/.local/share/Steam/steamapps/appmanifest_7.acf: not VDF".to_owned()];
        let mut bottles = SourceState::new(Source::Bottles);
        bottles.errors = vec![
            "“Secret Quest” in bottle “games” cannot be started through Bottles: its name \
             has a “/”"
                .to_owned(),
        ];
        Facts {
            version: "0.6.0".to_owned(),
            app_id: "io.github.tim_van_oudheusden.pindu-launcher".to_owned(),
            install: Install::Native,
            distribution: Some("Fedora Linux 45 (Workstation Edition)".to_owned()),
            session: SessionFacts::default(),
            permissions: Permissions::Unsandboxed,
            schema_version: Some(7),
            sources: vec![
                SourceFacts {
                    state: steam,
                    games: 2,
                    missing: 0,
                },
                SourceFacts {
                    state: bottles,
                    games: 1,
                    missing: 0,
                },
            ],
            tools: vec![Tool {
                name: "umu-run".to_owned(),
                found: Some("/home/ada/.local/bin/umu-run".to_owned()),
            }],
        }
    }

    fn private() -> Private {
        Private {
            homes: vec![PathBuf::from("/home/ada"), PathBuf::from("/var/home/ada")],
            titles: vec!["Secret Quest".to_owned(), "Hidden Gem".to_owned()],
        }
    }

    #[test]
    fn the_report_holds_no_title_and_no_home_path() {
        let report = facts().report(&private());

        assert!(report.contains("~/.local/share/Steam/steamapps/appmanifest_7.acf"));
        assert!(report.contains("~/.local/bin/umu-run"));
        assert!(!report.contains("Secret Quest"), "{report}");
        assert!(!report.contains("/home/ada"), "{report}");
    }

    #[test]
    fn the_report_names_each_fact() {
        let mut facts = facts();
        facts.install = Install::Flatpak {
            runtime: Some("runtime/org.gnome.Platform/x86_64/51".to_owned()),
            flatpak_version: Some("1.16.1".to_owned()),
        };
        facts.permissions = Permissions::Sandboxed;
        facts.session = SessionFacts {
            session_type: Some("wayland".to_owned()),
            desktop: Some("gamescope".to_owned()),
            gamescope: Some(Gamescope {
                version: Some("3.16.4".to_owned()),
                steam_mode: true,
                own_session: false,
            }),
        };
        facts.sources[1].state.enabled = false;
        facts.tools.push(Tool {
            name: "Proton builds".to_owned(),
            found: None,
        });

        let report = facts.report(&private());

        for line in [
            "Pindu launcher 0.6.0",
            "App ID: io.github.tim_van_oudheusden.pindu-launcher",
            "Installed as: Flatpak (runtime runtime/org.gnome.Platform/x86_64/51, Flatpak 1.16.1)",
            "Distribution: Fedora Linux 45 (Workstation Edition)",
            "Session: wayland, desktop gamescope",
            "gamescope: 3.16.4, --steam",
            "Permissions: Flatpak without host spawn",
            "Library schema: 7",
            "  steam: on, found at ~/.local/share/Steam, 2 games, never synced, 1 problems",
            "  bottles: off",
            "  umu-run: ~/.local/bin/umu-run",
            "  Proton builds: not found",
            "  bottles: “[title]” in bottle “games”",
        ] {
            assert!(report.contains(line), "{line:?} not in\n{report}");
        }
    }

    #[test]
    fn the_desktop_source_is_found_in_the_application_folders() {
        let mut facts = facts();
        let mut desktop = SourceState::new(Source::Desktop);
        desktop.last_sync = Some(1_700_000_000);
        facts.sources.push(SourceFacts {
            state: desktop,
            games: 9,
            missing: 0,
        });

        let report = facts.report(&private());

        let line = "  desktop: on, found in the application folders, 9 games";
        assert!(report.contains(line), "{line:?} not in\n{report}");
    }

    #[test]
    fn a_folder_that_only_starts_like_home_stays() {
        let mut facts = facts();
        facts.tools[0].found = Some("/home/adam/bin/umu-run".to_owned());

        assert!(facts.report(&private()).contains("/home/adam/bin/umu-run"));
    }

    #[test]
    fn flatpak_info_names_the_runtime_and_flatpak_version() {
        let info = "[Application]\nname=io.github.tim_van_oudheusden.pindu-launcher\n\
                    runtime=runtime/org.gnome.Platform/x86_64/51\n\n\
                    [Instance]\nflatpak-version=1.16.1\n";
        let fedora = OsRelease::parse("ID=fedora\n");

        assert_eq!(
            install(Some(info), &fedora),
            Install::Flatpak {
                runtime: Some("runtime/org.gnome.Platform/x86_64/51".to_owned()),
                flatpak_version: Some("1.16.1".to_owned()),
            }
        );
    }

    #[test]
    fn outside_flatpak_pindu_os_is_told_from_a_native_install() {
        let pindu_os = OsRelease::parse("ID=pindu-os\n");
        let fedora = OsRelease::parse("ID=fedora\n");

        assert_eq!(install(None, &pindu_os), Install::PinduOs);
        assert_eq!(install(None, &fedora), Install::Native);
    }

    #[test]
    fn the_permission_tier_follows_the_sandbox() {
        let granted = "[Session Bus Policy]\norg.freedesktop.Flatpak=talk\n";

        assert_eq!(permissions(None), Permissions::Unsandboxed);
        assert_eq!(permissions(Some(granted)), Permissions::HostSpawn);
        assert_eq!(permissions(Some("[Context]\n")), Permissions::Sandboxed);
    }

    #[test]
    fn every_source_is_counted_without_removed_games() {
        let game =
            |source, id: &str| Game::new(GameId::new(source, id), id, LaunchKind::StoreUri, 0);
        let mut removed = game(Source::Steam, "3");
        removed.user_removed = true;
        let mut gone = game(Source::Steam, "4");
        gone.missing_since = Some(1);
        let games = [
            game(Source::Steam, "1"),
            game(Source::Steam, "2"),
            removed,
            gone,
            game(Source::Lutris, "5"),
        ];
        let mut lutris = SourceState::new(Source::Lutris);
        lutris.enabled = false;

        let sources = source_facts(&[lutris.clone()], &games);

        assert_eq!(sources.len(), Source::ALL.len());
        let steam = &sources[0];
        assert_eq!(steam.state, SourceState::new(Source::Steam));
        assert_eq!((steam.games, steam.missing), (2, 1));
        let lutris_facts = sources
            .iter()
            .find(|facts| facts.state.source == Source::Lutris)
            .unwrap();
        assert_eq!(lutris_facts.state, lutris);
        assert_eq!(lutris_facts.games, 1);
    }

    #[test]
    fn gamescopes_version_is_read_from_its_output() {
        assert_eq!(
            gamescope_version("gamescope version 3.16.4 (gcc 15.2.1)\n").as_deref(),
            Some("3.16.4")
        );
        assert_eq!(
            gamescope_version("[gamescope] [Info]  console: gamescope version 3.14.2\n").as_deref(),
            Some("3.14.2")
        );
        assert_eq!(gamescope_version("usage: gamescope [options]\n"), None);
    }

    fn chosen(text: &str) -> ChosenLog {
        ChosenLog {
            title: "Secret Quest".to_owned(),
            log: Some(LogFile {
                name: "2026-11-05T14-03-22.log".to_owned(),
                text: text.to_owned(),
            }),
        }
    }

    #[test]
    fn the_chosen_log_keeps_its_title_and_no_home_path() {
        let section =
            chosen("Game: Secret Quest\nCommand: /var/home/ada/Games/quest.exe\n\nstarted\n")
                .section(&private());

        assert!(section.contains("2026-11-05T14-03-22.log"), "{section}");
        assert!(section.contains("Game: Secret Quest\n"), "{section}");
        assert!(
            section.contains("Command: ~/Games/quest.exe\n"),
            "{section}"
        );
        assert!(!section.contains("/home/ada"), "{section}");
    }

    #[test]
    fn the_chosen_log_hides_every_other_title_even_inside_its_own() {
        let mut log = chosen(
            "Game: Hidden Gem 2\nwine: Hidden Gem and Secret Quest saves found\n\
             dlc: Hidden Gem 2 Deluxe\n",
        );
        log.title = "Hidden Gem 2".to_owned();
        let mut private = private();
        private
            .titles
            .extend(["Hidden Gem 2".to_owned(), "Hidden Gem 2 Deluxe".to_owned()]);

        let section = log.section(&private);

        assert!(section.contains("Game: Hidden Gem 2\n"), "{section}");
        assert!(
            section.contains("wine: [title] and [title] saves found\n"),
            "{section}"
        );
        assert!(section.contains("dlc: [title]\n"), "{section}");
    }

    #[test]
    fn a_game_without_a_log_says_how_to_get_one() {
        let log = ChosenLog {
            title: "Secret Quest".to_owned(),
            log: None,
        };

        assert_eq!(
            log.section(&private()),
            "\nNewest log of “Secret Quest”: none, start the game with Play with Logs\n"
        );
    }

    #[test]
    fn a_long_log_keeps_its_start_and_its_end() {
        let text: String = (1..=1000).map(|n| format!("line {n}\n")).collect();

        let section = chosen(&text).section(&private());

        assert!(section.contains("\nline 1\n"), "{section}");
        assert!(section.contains("\nline 10\n[890 lines left out]\nline 901\n"));
        assert!(section.ends_with("\nline 1000\n"));
        assert!(!section.contains("\nline 11\n"));
    }

    #[test]
    fn the_games_with_a_log_are_offered_newest_log_first() {
        let game = |id: &str, title: &str| {
            Game::new(
                GameId::new(Source::Manual, id),
                title,
                LaunchKind::HostCommand,
                0,
            )
        };
        let games = [game("1", "Alpha"), game("2", "Beta"), game("3", "Gamma")];
        let newest = |id: &GameId| match id.as_str() {
            "manual:1" => Some("2026-11-01T10-00-00.log".to_owned()),
            "manual:3" => Some("2026-11-05T14-03-22.log".to_owned()),
            _ => None,
        };

        let titles: Vec<String> = log_choices(&games, newest)
            .into_iter()
            .map(|choice| choice.title)
            .collect();

        assert_eq!(titles, ["Gamma", "Alpha"]);
    }
}
