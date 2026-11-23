//! Library Health: one report of whether the library can find, start and dress its games,
//! in four sections (Sources, Launching, Artwork, Library data), computed from facts the
//! services gather, so the desktop page and fullscreen mode's Quick Access level show the
//! same thing.
//!
//! The report only reports and points: each [`Problem`] names a [`NextStep`] that already
//! exists in the app (a Preferences page, the `flatpak override` command, How to Install).
//! Nothing here repairs anything; the app cannot grant itself host access. A row the
//! session cannot have is left out (`None`), as the system panel's availability rules do
//! (`docs/system-panel.md`): it is never shown as a problem.

use std::path::{Component, Path, PathBuf};

use crate::model::{Game, Source, SourceState, Timestamp};
use crate::permissions;
use crate::setup::SetupState;
use crate::sources::sync::IMPORTED_SOURCES;

/// What the services found out about one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFacts {
    /// The source's stored state: on or off, the folder found, the user's folder, the last
    /// sync and its problems.
    pub state: SourceState,
    /// How many of the library's games come from the source.
    pub games: usize,
}

/// What the services found out about starting games.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LaunchFacts {
    /// Whether the app runs in the Flatpak sandbox.
    pub in_flatpak: bool,
    /// The app's ID, which the `flatpak override` command names.
    pub app_id: String,
    /// Whether host spawn is allowed (always natively).
    pub host_spawn: bool,
    /// How many games in the library start on the host, and so need host spawn.
    pub games_needing_host: usize,
    /// Whether `umu-run` was found.
    pub umu: bool,
    /// The display names of the Proton builds installed, in the order they were found.
    pub proton_builds: Vec<String>,
    /// Whether MangoHud can run: in the Flatpak its Vulkan-layer extension is installed,
    /// natively the system's `mangohud` is.
    pub mangohud: bool,
    /// Whether GameMode can be asked to favour a game.
    pub gamemode: bool,
    /// Whether this is a gamescope session.
    pub gamescope_session: bool,
}

/// What the services found out about artwork.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ArtworkFacts {
    /// Whether a SteamGridDB key is stored in the keyring.
    pub key_stored: bool,
    /// How many bytes the artwork cache takes.
    pub cache_bytes: u64,
}

/// What the services found out about the library file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LibraryFacts {
    /// Where the library file is.
    pub file: PathBuf,
    /// The file's schema version.
    pub schema_version: u32,
    /// When the library was last backed up, if it is known.
    pub last_backup: Option<Timestamp>,
}

/// Everything the report is computed from.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HealthFacts {
    /// Each source, in the order the report lists them.
    pub sources: Vec<SourceFacts>,
    /// Starting games.
    pub launch: LaunchFacts,
    /// Artwork.
    pub artwork: ArtworkFacts,
    /// The library file.
    pub library: LibraryFacts,
}

/// How a launcher is installed, judged from the folder its data was found in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Install {
    /// A package of the system, or the launcher's own installer.
    Native,
    /// A Flatpak, whose data lives under `~/.var/app`.
    Flatpak,
}

impl Install {
    /// How the launcher whose data is in `folder` is installed: a Flatpak keeps it under
    /// `.var/app`, anything else is native.
    pub fn of(folder: &Path) -> Self {
        let components: Vec<_> = folder.components().collect();
        let flatpak = components.windows(2).any(|pair| {
            pair[0] == Component::Normal(".var".as_ref())
                && pair[1] == Component::Normal("app".as_ref())
        });
        if flatpak { Self::Flatpak } else { Self::Native }
    }
}

/// A Preferences page a next step opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreferencesPage {
    /// Sources, with each source's folder, its last sync and that sync's problems.
    Sources,
}

/// A tool whose install instructions a next step opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    /// umu-launcher, which runs Windows games with Proton.
    Umu,
}

/// What the user can do about a problem, with a part of the app that already exists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NextStep {
    /// Open this Preferences page.
    Preferences(PreferencesPage),
    /// Run this `flatpak override` command, then restart the app.
    HostOverride {
        /// The command, as Preferences shows it.
        command: String,
    },
    /// Read how to install this tool.
    HowToInstall(Tool),
}

/// Where fullscreen mode's Quick Access shows the report, which decides whether a next step
/// can be taken from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FullscreenSession {
    /// Quick Access is open over a running game, where nothing opens over the launcher.
    pub over_game: bool,
    /// This is a gamescope session, which has no desktop for a web browser to open on.
    pub gamescope_session: bool,
}

/// Whether fullscreen mode can take a next step, or where else it is taken. As in the
/// system panel (`docs/system-panel.md`), a step the session cannot take stays in the
/// report and says why, never as an error.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepReach {
    /// From the row itself.
    Here,
    /// Once the game has ended, from the library: Preferences and web pages open over the
    /// launcher, not over a game.
    AfterTheGame,
    /// On another device: a gamescope session has no browser to open the page in, so the
    /// row shows its address.
    OnAnotherDevice,
}

impl NextStep {
    /// Whether fullscreen mode can take the step in `session`.
    pub fn reach_in_fullscreen(&self, session: FullscreenSession) -> StepReach {
        match self {
            Self::HowToInstall(_) if session.gamescope_session => StepReach::OnAnotherDevice,
            _ if session.over_game => StepReach::AfterTheGame,
            _ => StepReach::Here,
        }
    }
}

/// Something that keeps the library from finding or starting games.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    /// The folder the user chose for the source lacks what the launcher keeps there.
    FolderIncomplete {
        /// The source.
        source: Source,
        /// The folder the user chose.
        folder: PathBuf,
    },
    /// The source's last sync ran into problems.
    SyncProblems {
        /// The source.
        source: Source,
        /// How many.
        count: usize,
    },
    /// Games in the library start on the host, which the sandbox does not allow.
    NoHostAccess {
        /// How many games need it.
        games: usize,
    },
    /// umu-launcher, which runs Windows games, is not installed.
    UmuMissing,
}

impl Problem {
    /// The step that already exists for the problem.
    pub fn next_step(&self, app_id: &str) -> NextStep {
        match self {
            Self::FolderIncomplete { .. } | Self::SyncProblems { .. } => {
                NextStep::Preferences(PreferencesPage::Sources)
            }
            Self::NoHostAccess { .. } => NextStep::HostOverride {
                command: permissions::host_override_command(app_id),
            },
            Self::UmuMissing => NextStep::HowToInstall(Tool::Umu),
        }
    }
}

/// One source in the Sources section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceHealth {
    /// The source.
    pub source: Source,
    /// Whether the user has it turned on.
    pub enabled: bool,
    /// The folder its data was found in; `None` when it was not found on this computer.
    pub folder: Option<PathBuf>,
    /// How the launcher is installed; `None` when it was not found.
    pub install: Option<Install>,
    /// How many of the library's games come from it.
    pub games: usize,
    /// When it was last synced, if ever.
    pub last_sync: Option<Timestamp>,
    /// What its last sync could not read.
    pub sync_problems: Vec<String>,
    /// What keeps it from being read.
    pub problems: Vec<Problem>,
}

/// Whether games may start on the host, a row only the Flatpak has.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostAccess {
    /// Whether host spawn is allowed.
    pub allowed: bool,
    /// How many games in the library need it.
    pub games_needing_it: usize,
    /// The problem when games need it and it is not allowed.
    pub problem: Option<Problem>,
}

/// The Launching section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchingHealth {
    /// Whether games may start on the host; `None` natively, where they always may.
    pub host_access: Option<HostAccess>,
    /// Whether umu-launcher is installed.
    pub umu: bool,
    /// The problem when it is not.
    pub umu_problem: Option<Problem>,
    /// The display names of the Proton builds installed. None is not a problem: umu
    /// downloads one the first time a game needs it.
    pub proton_builds: Vec<String>,
    /// Whether MangoHud can run. Without it the per-game switch says so; it is not a
    /// problem.
    pub mangohud: bool,
    /// Whether GameMode can be asked to favour a game.
    pub gamemode: bool,
    /// Whether this is a gamescope session.
    pub gamescope_session: bool,
}

/// The Artwork section.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtworkHealth {
    /// Whether a SteamGridDB key is stored. Without one the launchers' own art shows.
    pub key_stored: bool,
    /// How many bytes the artwork cache takes.
    pub cache_bytes: u64,
}

/// The Library data section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryHealth {
    /// Where the library file is.
    pub file: PathBuf,
    /// Its schema version.
    pub schema_version: u32,
    /// When it was last backed up, if that is known.
    pub last_backup: Option<Timestamp>,
}

/// The Library Health report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthReport {
    /// Sources: each source, found or not, where, how it is installed, its games, its last
    /// sync and its problems.
    pub sources: Vec<SourceHealth>,
    /// Launching: host access, umu, Proton builds, MangoHud, GameMode, gamescope.
    pub launching: LaunchingHealth,
    /// Artwork: the stored key and the cache.
    pub artwork: ArtworkHealth,
    /// Library data: the file, its schema version and the last backup.
    pub library: LibraryHealth,
}

impl HealthReport {
    /// Every problem in the report, section by section.
    pub fn problems(&self) -> impl Iterator<Item = &Problem> {
        let sources = self.sources.iter().flat_map(|source| &source.problems);
        let launching = self
            .launching
            .host_access
            .iter()
            .flat_map(|row| &row.problem);
        sources.chain(launching).chain(&self.launching.umu_problem)
    }
}

/// Each source that has an importer, in sync order, with its stored state (a new one when
/// none is stored yet) and how many of `games` come from it. Games the user removed are
/// not counted.
pub fn source_facts(states: &[SourceState], games: &[Game]) -> Vec<SourceFacts> {
    IMPORTED_SOURCES
        .iter()
        .map(|&source| SourceFacts {
            state: states
                .iter()
                .find(|state| state.source == source)
                .cloned()
                .unwrap_or_else(|| SourceState::new(source)),
            games: games
                .iter()
                .filter(|game| !game.user_removed && game.source() == source)
                .count(),
        })
        .collect()
}

/// How many of `games` start on the host, and so need host spawn in the Flatpak. Games
/// the user removed are not counted.
pub fn games_needing_host(games: &[Game]) -> usize {
    games
        .iter()
        .filter(|game| !game.user_removed && permissions::needs_host_spawn(game.launch_kind))
        .count()
}

/// How many bytes the files below `folder` take, such as the artwork cache's. Links are
/// not followed, and what cannot be read counts as nothing, as does a missing folder.
pub fn folder_bytes(folder: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => folder_bytes(&entry.path()),
            Ok(kind) if kind.is_file() => entry.metadata().map_or(0, |metadata| metadata.len()),
            _ => 0,
        })
        .sum()
}

/// The report the facts make.
pub fn report(facts: &HealthFacts) -> HealthReport {
    let launch = &facts.launch;
    let host_access = launch.in_flatpak.then(|| HostAccess {
        allowed: launch.host_spawn,
        games_needing_it: launch.games_needing_host,
        problem: (!launch.host_spawn && launch.games_needing_host > 0).then_some(
            Problem::NoHostAccess {
                games: launch.games_needing_host,
            },
        ),
    });
    HealthReport {
        sources: facts.sources.iter().map(source_health).collect(),
        launching: LaunchingHealth {
            host_access,
            umu: launch.umu,
            umu_problem: (!launch.umu).then_some(Problem::UmuMissing),
            proton_builds: launch.proton_builds.clone(),
            mangohud: launch.mangohud,
            gamemode: launch.gamemode,
            gamescope_session: launch.gamescope_session,
        },
        artwork: ArtworkHealth {
            key_stored: facts.artwork.key_stored,
            cache_bytes: facts.artwork.cache_bytes,
        },
        library: LibraryHealth {
            file: facts.library.file.clone(),
            schema_version: facts.library.schema_version,
            last_backup: facts.library.last_backup,
        },
    }
}

/// One source's row. A source turned off is not read, so it has no problems; one that is
/// not installed is not a problem either.
fn source_health(facts: &SourceFacts) -> SourceHealth {
    let state = &facts.state;
    let mut problems = Vec::new();
    if state.enabled {
        if let (Some(folder), None) = (&state.user_override, &state.resolved_location) {
            problems.push(Problem::FolderIncomplete {
                source: state.source,
                folder: folder.clone(),
            });
        }
        if !state.errors.is_empty() {
            problems.push(Problem::SyncProblems {
                source: state.source,
                count: state.errors.len(),
            });
        }
    }
    SourceHealth {
        source: state.source,
        enabled: state.enabled,
        folder: state.resolved_location.clone(),
        install: state.resolved_location.as_deref().map(Install::of),
        games: facts.games,
        last_sync: state.last_sync,
        sync_problems: state.errors.clone(),
        problems,
    }
}

/// Whether the sync about to run is the library's first: no source has synced before.
pub fn is_first_sync(states: &[SourceState]) -> bool {
    states.iter().all(|state| state.last_sync.is_none())
}

/// What the library's first sync found, summarised once in both shells, which point at
/// Library Health from it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FirstSyncSummary {
    /// The launchers whose data was found, in sync order.
    pub launchers: Vec<Source>,
    /// How many games the library has from them.
    pub games: usize,
    /// What needs attention, as the report lists it.
    pub problems: Vec<Problem>,
}

/// Whether a sync still owes the summary: it is the library's `first_sync` (from
/// [`is_first_sync`] before it ran), or the library had no game before it
/// (`games_before`), so a new console whose first syncs find nothing gets the summary
/// once its first games come in. A library that had games, as after an upgrade, owes none.
pub fn summary_owed(first_sync: bool, games_before: u32) -> bool {
    first_sync || games_before == 0
}

/// The summary to show after a sync with `report`: only when the sync owes it
/// ([`summary_owed`]) and it was not `shown` before, and only once there is something to
/// summarise: a launcher found or a game imported. While the set-up checklist shows in
/// fullscreen mode (`setup`), it waits for the first games, which end the checklist, so
/// a store that has run but holds nothing yet never covers it.
pub fn first_sync_summary(
    report: &HealthReport,
    owed: bool,
    shown: bool,
    setup: SetupState,
) -> Option<FirstSyncSummary> {
    if !owed || shown {
        return None;
    }
    let launchers: Vec<Source> = report
        .sources
        .iter()
        .filter(|source| source.folder.is_some())
        .map(|source| source.source)
        .collect();
    let games = report.sources.iter().map(|source| source.games).sum();
    if games == 0 && (launchers.is_empty() || setup.shows()) {
        return None;
    }
    Some(FirstSyncSummary {
        launchers,
        games,
        problems: report.problems().cloned().collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GameId, LaunchKind};

    const APP_ID: &str = "io.example.App";

    fn found(source: Source, folder: &str, games: usize) -> SourceFacts {
        SourceFacts {
            state: SourceState {
                resolved_location: Some(PathBuf::from(folder)),
                last_sync: Some(1_700_000_000),
                ..SourceState::new(source)
            },
            games,
        }
    }

    fn missing(source: Source) -> SourceFacts {
        SourceFacts {
            state: SourceState::new(source),
            games: 0,
        }
    }

    fn library() -> LibraryFacts {
        LibraryFacts {
            file: PathBuf::from("/home/u/.local/share/pindu-launcher/library.sqlite3"),
            schema_version: 6,
            last_backup: None,
        }
    }

    /// The Flatpak as installed: no host spawn, umu bundled, no MangoHud layer.
    fn flatpak_without_host_access() -> HealthFacts {
        HealthFacts {
            sources: vec![
                found(
                    Source::Steam,
                    "/home/u/.var/app/com.valvesoftware.Steam/data/Steam",
                    40,
                ),
                missing(Source::Lutris),
            ],
            launch: LaunchFacts {
                in_flatpak: true,
                app_id: APP_ID.to_owned(),
                host_spawn: false,
                games_needing_host: 3,
                umu: true,
                proton_builds: vec!["GE-Proton10-1".to_owned()],
                mangohud: false,
                gamemode: true,
                gamescope_session: false,
            },
            artwork: ArtworkFacts {
                key_stored: false,
                cache_bytes: 0,
            },
            library: library(),
        }
    }

    /// A native install on a desktop, without umu.
    fn native_install() -> HealthFacts {
        HealthFacts {
            sources: vec![
                found(Source::Steam, "/home/u/.local/share/Steam", 40),
                found(Source::Lutris, "/home/u/.local/share/lutris", 5),
            ],
            launch: LaunchFacts {
                in_flatpak: false,
                app_id: APP_ID.to_owned(),
                host_spawn: true,
                games_needing_host: 3,
                umu: false,
                proton_builds: Vec::new(),
                mangohud: true,
                gamemode: false,
                gamescope_session: false,
            },
            artwork: ArtworkFacts {
                key_stored: true,
                cache_bytes: 12_345,
            },
            library: library(),
        }
    }

    #[test]
    fn a_flatpak_without_host_access_points_to_the_override_command() {
        let report = report(&flatpak_without_host_access());

        let host = report
            .launching
            .host_access
            .as_ref()
            .expect("a Flatpak row");
        assert!(!host.allowed);
        assert_eq!(host.games_needing_it, 3);
        let problem = host.problem.as_ref().expect("games need host access");
        assert_eq!(problem, &Problem::NoHostAccess { games: 3 });
        assert_eq!(
            problem.next_step(APP_ID),
            NextStep::HostOverride {
                command: permissions::host_override_command(APP_ID)
            }
        );
        assert_eq!(report.problems().count(), 1);
    }

    #[test]
    fn a_flatpak_reports_its_sources_and_tools_without_calling_them_problems() {
        let report = report(&flatpak_without_host_access());

        let steam = &report.sources[0];
        assert_eq!(steam.source, Source::Steam);
        assert_eq!(steam.install, Some(Install::Flatpak));
        assert_eq!(steam.games, 40);
        assert_eq!(steam.last_sync, Some(1_700_000_000));
        let lutris = &report.sources[1];
        assert_eq!(lutris.folder, None);
        assert_eq!(lutris.install, None);
        assert!(lutris.problems.is_empty(), "not installed is not a problem");
        assert!(report.launching.umu);
        assert!(!report.launching.mangohud);
        assert_eq!(report.launching.proton_builds, ["GE-Proton10-1"]);
        assert!(!report.artwork.key_stored);
    }

    #[test]
    fn a_flatpak_whose_games_need_no_host_access_has_no_problem_there() {
        let mut facts = flatpak_without_host_access();
        facts.launch.games_needing_host = 0;

        let report = report(&facts);

        let host = report.launching.host_access.expect("a Flatpak row");
        assert!(!host.allowed);
        assert_eq!(host.problem, None);
    }

    #[test]
    fn a_flatpak_with_host_access_has_no_problem_there() {
        let mut facts = flatpak_without_host_access();
        facts.launch.host_spawn = true;

        let report = report(&facts);

        let host = report.launching.host_access.expect("a Flatpak row");
        assert!(host.allowed);
        assert_eq!(host.problem, None);
    }

    #[test]
    fn a_native_install_has_no_host_access_row() {
        let report = report(&native_install());

        assert_eq!(report.launching.host_access, None);
    }

    #[test]
    fn a_native_install_without_umu_points_to_how_to_install() {
        let report = report(&native_install());

        assert!(!report.launching.umu);
        let problem = report
            .launching
            .umu_problem
            .as_ref()
            .expect("umu is missing");
        assert_eq!(problem, &Problem::UmuMissing);
        assert_eq!(problem.next_step(APP_ID), NextStep::HowToInstall(Tool::Umu));
        assert_eq!(
            report.problems().collect::<Vec<_>>(),
            [&Problem::UmuMissing]
        );
    }

    #[test]
    fn a_native_install_reports_native_sources_artwork_and_library_data() {
        let report = report(&native_install());

        assert!(
            report
                .sources
                .iter()
                .all(|source| source.install == Some(Install::Native))
        );
        assert_eq!(report.sources[1].games, 5);
        assert!(report.launching.proton_builds.is_empty());
        assert!(report.launching.mangohud);
        assert!(!report.launching.gamemode);
        assert_eq!(
            report.artwork,
            ArtworkHealth {
                key_stored: true,
                cache_bytes: 12_345
            }
        );
        assert_eq!(report.library.file, library().file);
        assert_eq!(report.library.schema_version, 6);
        assert_eq!(report.library.last_backup, None);
    }

    #[test]
    fn a_sync_s_problems_point_to_the_sources_page() {
        let mut facts = native_install();
        facts.sources[0].state.errors =
            vec!["a.acf: not VDF".to_owned(), "b.acf: empty".to_owned()];

        let report = report(&facts);

        let steam = &report.sources[0];
        assert_eq!(steam.sync_problems.len(), 2);
        let problem = &steam.problems[0];
        assert_eq!(
            problem,
            &Problem::SyncProblems {
                source: Source::Steam,
                count: 2
            }
        );
        assert_eq!(
            problem.next_step(APP_ID),
            NextStep::Preferences(PreferencesPage::Sources)
        );
    }

    #[test]
    fn a_chosen_folder_that_was_not_read_points_to_the_sources_page() {
        let mut facts = native_install();
        facts.sources[1] = SourceFacts {
            state: SourceState {
                user_override: Some(PathBuf::from("/mnt/games/lutris")),
                ..SourceState::new(Source::Lutris)
            },
            games: 0,
        };

        let report = report(&facts);

        let problem = &report.sources[1].problems[0];
        assert_eq!(
            problem,
            &Problem::FolderIncomplete {
                source: Source::Lutris,
                folder: PathBuf::from("/mnt/games/lutris")
            }
        );
        assert_eq!(
            problem.next_step(APP_ID),
            NextStep::Preferences(PreferencesPage::Sources)
        );
    }

    #[test]
    fn a_source_turned_off_has_no_problems() {
        let mut facts = native_install();
        let state = &mut facts.sources[0].state;
        state.enabled = false;
        state.user_override = Some(PathBuf::from("/nowhere"));
        state.resolved_location = None;
        state.errors = vec!["old".to_owned()];

        let report = report(&facts);

        assert!(!report.sources[0].enabled);
        assert!(report.sources[0].problems.is_empty());
    }

    #[test]
    fn flatpak_data_lives_under_var_app() {
        assert_eq!(
            Install::of(Path::new("/home/u/.var/app/net.lutris.Lutris/data/lutris")),
            Install::Flatpak
        );
        assert_eq!(
            Install::of(Path::new("/home/u/.local/share/lutris")),
            Install::Native
        );
        assert_eq!(Install::of(Path::new("/home/u/app/.var")), Install::Native);
    }

    fn game(source: Source, id: &str, kind: LaunchKind) -> Game {
        Game::new(GameId::new(source, id), id, kind, 0)
    }

    #[test]
    fn every_imported_source_is_listed_with_its_games_in_sync_order() {
        let states = vec![SourceState {
            resolved_location: Some(PathBuf::from("/home/u/.local/share/lutris")),
            ..SourceState::new(Source::Lutris)
        }];
        let mut removed = game(Source::Lutris, "c", LaunchKind::HostCommand);
        removed.user_removed = true;
        let games = [
            game(Source::Lutris, "a", LaunchKind::HostCommand),
            game(Source::Lutris, "b", LaunchKind::HostCommand),
            removed,
            game(Source::Steam, "1", LaunchKind::StoreUri),
        ];

        let facts = source_facts(&states, &games);

        let order: Vec<_> = facts.iter().map(|facts| facts.state.source).collect();
        assert_eq!(order, IMPORTED_SOURCES);
        let lutris = facts
            .iter()
            .find(|facts| facts.state.source == Source::Lutris)
            .unwrap();
        assert_eq!(lutris.state, states[0]);
        assert_eq!(lutris.games, 2);
        assert_eq!(facts[0].state, SourceState::new(Source::Steam));
        assert_eq!(facts[0].games, 1);
    }

    #[test]
    fn only_games_started_on_the_host_need_host_access() {
        let mut removed = game(Source::Desktop, "gone", LaunchKind::DesktopEntry);
        removed.user_removed = true;
        let games = [
            game(Source::Lutris, "a", LaunchKind::HostCommand),
            game(Source::Desktop, "b", LaunchKind::DesktopEntry),
            game(Source::Steam, "1", LaunchKind::StoreUri),
            game(Source::Manual, "w", LaunchKind::Proton),
            removed,
        ];

        assert_eq!(games_needing_host(&games), 2);
    }

    #[test]
    fn a_folder_takes_the_bytes_of_every_file_below_it() {
        let folder = tempfile::tempdir().unwrap();
        std::fs::write(folder.path().join("a"), [0; 10]).unwrap();
        std::fs::create_dir_all(folder.path().join("sub/deeper")).unwrap();
        std::fs::write(folder.path().join("sub/b"), [0; 20]).unwrap();
        std::fs::write(folder.path().join("sub/deeper/c"), [0; 30]).unwrap();

        assert_eq!(folder_bytes(folder.path()), 60);
        assert_eq!(folder_bytes(&folder.path().join("missing")), 0);
    }

    #[test]
    fn fullscreen_takes_every_next_step_in_the_library() {
        let session = FullscreenSession::default();
        for step in [
            NextStep::Preferences(PreferencesPage::Sources),
            NextStep::HostOverride {
                command: permissions::host_override_command(APP_ID),
            },
            NextStep::HowToInstall(Tool::Umu),
        ] {
            assert_eq!(
                step.reach_in_fullscreen(session),
                StepReach::Here,
                "{step:?}"
            );
        }
    }

    #[test]
    fn over_a_game_fullscreen_leaves_the_next_steps_for_after_it() {
        let session = FullscreenSession {
            over_game: true,
            gamescope_session: false,
        };
        for step in [
            NextStep::Preferences(PreferencesPage::Sources),
            NextStep::HostOverride {
                command: permissions::host_override_command(APP_ID),
            },
            NextStep::HowToInstall(Tool::Umu),
        ] {
            assert_eq!(
                step.reach_in_fullscreen(session),
                StepReach::AfterTheGame,
                "{step:?}"
            );
        }
    }

    #[test]
    fn a_gamescope_session_reads_install_instructions_on_another_device() {
        for over_game in [false, true] {
            let session = FullscreenSession {
                over_game,
                gamescope_session: true,
            };
            assert_eq!(
                NextStep::HowToInstall(Tool::Umu).reach_in_fullscreen(session),
                StepReach::OnAnotherDevice
            );
        }
        let session = FullscreenSession {
            over_game: false,
            gamescope_session: true,
        };
        assert_eq!(
            NextStep::Preferences(PreferencesPage::Sources).reach_in_fullscreen(session),
            StepReach::Here
        );
    }

    #[test]
    fn a_sync_is_the_first_when_no_source_has_synced_before() {
        let never = [
            SourceState::new(Source::Steam),
            SourceState::new(Source::Lutris),
        ];
        assert!(is_first_sync(&never));
        assert!(is_first_sync(&[]));

        let synced = [
            SourceState::new(Source::Steam),
            SourceState {
                last_sync: Some(1_700_000_000),
                ..SourceState::new(Source::Lutris)
            },
        ];
        assert!(!is_first_sync(&synced));
    }

    #[test]
    fn the_first_sync_s_summary_names_launchers_found_games_and_problems() {
        let report = report(&flatpak_without_host_access());

        let summary = first_sync_summary(&report, true, false, SetupState::Unseen)
            .expect("the first sync, not shown yet");

        assert_eq!(summary.launchers, [Source::Steam], "Lutris was not found");
        assert_eq!(summary.games, 40);
        assert_eq!(summary.problems, [Problem::NoHostAccess { games: 3 }]);
    }

    #[test]
    fn the_summary_shows_only_after_the_first_sync_and_only_once() {
        let report = report(&native_install());

        assert_eq!(
            first_sync_summary(&report, false, false, SetupState::Unseen),
            None,
            "a later sync"
        );
        assert_eq!(
            first_sync_summary(&report, true, true, SetupState::Unseen),
            None,
            "dismissed before"
        );
        assert_eq!(
            first_sync_summary(&report, false, true, SetupState::Unseen),
            None
        );
    }

    /// A new console: no launcher found, no game imported, umu-launcher missing.
    fn new_console() -> HealthFacts {
        HealthFacts {
            sources: vec![missing(Source::Steam), missing(Source::Lutris)],
            ..native_install()
        }
    }

    #[test]
    fn a_new_console_whose_sync_found_nothing_gets_no_summary_over_its_checklist() {
        let report = report(&new_console());
        for setup in [
            SetupState::Unseen,
            SetupState::Active,
            SetupState::Dismissed,
        ] {
            assert_eq!(
                first_sync_summary(&report, true, false, setup),
                None,
                "{setup:?}: nothing to summarise, whatever needs attention"
            );
        }
    }

    #[test]
    fn a_store_found_without_games_waits_while_the_set_up_checklist_shows() {
        let facts = HealthFacts {
            sources: vec![
                found(Source::Steam, "/home/u/.local/share/Steam", 0),
                missing(Source::Lutris),
            ],
            ..native_install()
        };
        let no_games = report(&facts);
        assert_eq!(
            first_sync_summary(&no_games, true, false, SetupState::Active),
            None,
            "Steam has run, but nothing is signed in yet"
        );
        assert!(
            first_sync_summary(&no_games, true, false, SetupState::Unseen).is_some(),
            "a desktop install that found a launcher still gets it"
        );
        let signed_in = report(&HealthFacts {
            sources: vec![found(Source::Steam, "/home/u/.local/share/Steam", 12)],
            ..native_install()
        });
        assert_eq!(
            first_sync_summary(&signed_in, true, false, SetupState::Active)
                .map(|summary| summary.games),
            Some(12),
            "the first games end the checklist, and the library is ready"
        );
    }

    #[test]
    fn the_summary_is_owed_until_a_sync_brings_games_in() {
        assert!(summary_owed(true, 0), "the library's first sync");
        assert!(
            summary_owed(false, 0),
            "a later sync of a library that has no games yet"
        );
        assert!(
            !summary_owed(false, 3),
            "a library that had games before, as after an upgrade"
        );
        assert!(summary_owed(true, 3));
    }
}
