//! Why a game's last launch failed, in plain words, and the next steps that exist for it:
//! How to Enable a missing permission, How to Install umu-launcher, another Proton build,
//! Start Safely and Open Log. The game page and fullscreen mode's options sheet show it.

use crate::launch;
use crate::launch_outcome::{Ending, LaunchOutcome, SIGTERM};
use crate::model::{Game, LaunchRoute};

/// A game that quits within this many seconds of its start quit straight away.
pub const AT_ONCE_SECS: i64 = 10;

/// The signals a program gets when it goes wrong: `SIGILL`, `SIGABRT`, `SIGBUS`, `SIGFPE`
/// and `SIGSEGV`.
const CRASH_SIGNALS: [i32; 5] = [4, 6, 7, 8, 11];

/// What Pindu knows now, beside the last launch, about what a game needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Facts<'a> {
    /// Starting the game needs a permission the sandbox lacks.
    pub needs_permission: bool,
    /// umu-launcher, which runs Windows games and the Steam Linux Runtime, is installed.
    pub umu_installed: bool,
    /// The names of the Proton builds on this machine, as a launch config stores them.
    pub proton_builds: &'a [String],
    /// The last launch's log is still in the game's logs folder.
    pub log_kept: bool,
}

/// Why the last launch failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reason {
    /// The game did not start, and Pindu cannot tell why.
    NotStarted,
    /// The game quit with this error status.
    QuitWithError(i32),
    /// The game quit with this error status straight after it started, within
    /// [`AT_ONCE_SECS`].
    QuitAtOnce(i32),
    /// The game crashed: it was ended by this signal, which a program gets when it goes
    /// wrong (such as a segmentation fault or an abort).
    Crashed(i32),
    /// Something other than Stop ended the game with this signal.
    Ended(i32),
    /// umu-launcher, which the game starts through, is not installed.
    UmuMissing,
    /// The Proton build of this name, which the game was to run with, is not on this
    /// machine any more.
    ProtonMissing(String),
    /// No Proton build is on this machine to run the Windows game with.
    NoProton,
    /// The game's store launcher could not be opened.
    LauncherNotOpened,
    /// The game's store launcher did not answer.
    LauncherNoAnswer,
}

/// A next step after a failed launch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// How to give the sandbox the permission the game needs: Preferences at its
    /// Permissions group.
    HowToEnable,
    /// How to install umu-launcher, which the game starts through.
    HowToInstall,
    /// Choose another Proton build for the Windows game.
    AnotherProton,
    /// Start the game once without its wrappers, MangoHud, GameMode or its own
    /// environment (see [`crate::launch::Start::Safely`]).
    StartSafely,
    /// Open the launch's log.
    OpenLog,
}

/// The last launch failed: why, and what to do next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Explanation {
    /// Why it failed.
    pub reason: Reason,
    /// The next steps that exist for the game, in the order they are offered.
    pub steps: Vec<Step>,
    /// The name of the launch's log in the game's logs folder, for [`Step::OpenLog`].
    pub log: Option<String>,
}

/// Explains the `last` launch of `game`, when it failed, with what is known now (`facts`):
/// why, in [`Reason`], and the [`Step`]s that exist for the game. A step that cannot help
/// while a permission or umu-launcher is missing, or the Proton build is, is left out.
pub fn explain(game: &Game, last: &LaunchOutcome, facts: &Facts<'_>) -> Option<Explanation> {
    let at_once = last
        .ended_at
        .is_some_and(|ended_at| ended_at - last.started_at <= AT_ONCE_SECS);
    let reason = match last.ending {
        Ending::Exited(status) if status != 0 && at_once => Reason::QuitAtOnce(status),
        Ending::Exited(status) if status != 0 => Reason::QuitWithError(status),
        Ending::Signalled(signal) if CRASH_SIGNALS.contains(&signal) => Reason::Crashed(signal),
        Ending::Signalled(signal) if signal != SIGTERM => Reason::Ended(signal),
        Ending::NotStarted => not_started(last, facts),
        Ending::HandOffFailed => Reason::LauncherNotOpened,
        Ending::NoAnswer => Reason::LauncherNoAnswer,
        Ending::Pending
        | Ending::Exited(_)
        | Ending::Signalled(_)
        | Ending::Lost
        | Ending::HandedOff => return None,
    };
    let through_umu = matches!(last.route, LaunchRoute::Proton | LaunchRoute::Runtime);
    let enable = last.route == LaunchRoute::Host && facts.needs_permission;
    let install = through_umu && !facts.umu_installed;
    let another_proton = last.route == LaunchRoute::Proton
        && launch::offers_proton_choice(game)
        && !install
        && facts
            .proton_builds
            .iter()
            .any(|build| Some(build) != last.proton.as_ref());
    let blocked =
        enable || install || matches!(reason, Reason::ProtonMissing(_) | Reason::NoProton);
    let safely =
        last.route != LaunchRoute::HandOff && launch::launch_settings(game).is_some() && !blocked;
    let log = last.log.clone().filter(|_| facts.log_kept);
    let steps = [
        enable.then_some(Step::HowToEnable),
        install.then_some(Step::HowToInstall),
        another_proton.then_some(Step::AnotherProton),
        safely.then_some(Step::StartSafely),
        log.is_some().then_some(Step::OpenLog),
    ];
    Some(Explanation {
        reason,
        steps: steps.into_iter().flatten().collect(),
        log,
    })
}

/// Why a launch by `last`'s route never started, as far as `facts` tell.
fn not_started(last: &LaunchOutcome, facts: &Facts<'_>) -> Reason {
    let through_umu = matches!(last.route, LaunchRoute::Proton | LaunchRoute::Runtime);
    if through_umu && !facts.umu_installed {
        return Reason::UmuMissing;
    }
    if last.route == LaunchRoute::Proton {
        match &last.proton {
            Some(build) if !facts.proton_builds.contains(build) => {
                return Reason::ProtonMissing(build.clone());
            }
            None if facts.proton_builds.is_empty() => return Reason::NoProton,
            _ => {}
        }
    }
    Reason::NotStarted
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GameId, LaunchKind, Source};

    const STARTED: i64 = 1_000;

    fn game(kind: LaunchKind) -> Game {
        let source = if kind == LaunchKind::StoreUri {
            Source::Steam
        } else {
            Source::Manual
        };
        Game::new(GameId::new(source, "1"), "Celeste", kind, 0)
    }

    fn outcome(route: LaunchRoute, ending: Ending, seconds: i64) -> LaunchOutcome {
        LaunchOutcome {
            id: 1,
            game_id: GameId::new(Source::Manual, "1"),
            route,
            proton: None,
            started_at: STARTED,
            ended_at: Some(STARTED + seconds),
            ending,
            window_seen: None,
            log: None,
        }
    }

    fn facts() -> Facts<'static> {
        Facts {
            needs_permission: false,
            umu_installed: true,
            proton_builds: &[],
            log_kept: false,
        }
    }

    #[test]
    fn a_game_that_quit_with_an_error_says_so() {
        let last = outcome(LaunchRoute::Host, Ending::Exited(1), 120);
        let explanation = explain(&game(LaunchKind::HostCommand), &last, &facts());
        assert_eq!(
            explanation.map(|explanation| explanation.reason),
            Some(Reason::QuitWithError(1))
        );
    }

    #[test]
    fn a_game_that_quit_with_an_error_at_once_says_it_quit_straight_away() {
        let last = outcome(LaunchRoute::Host, Ending::Exited(3), AT_ONCE_SECS);
        let explanation = explain(&game(LaunchKind::HostCommand), &last, &facts());
        assert_eq!(
            explanation.map(|explanation| explanation.reason),
            Some(Reason::QuitAtOnce(3))
        );
    }

    #[test]
    fn a_game_ended_by_a_crash_signal_crashed() {
        let last = outcome(LaunchRoute::Host, Ending::Signalled(11), 120);
        let explanation = explain(&game(LaunchKind::HostCommand), &last, &facts());
        assert_eq!(
            explanation.map(|explanation| explanation.reason),
            Some(Reason::Crashed(11))
        );
    }

    fn reason(game: &Game, last: &LaunchOutcome, facts: &Facts<'_>) -> Option<Reason> {
        explain(game, last, facts).map(|explanation| explanation.reason)
    }

    #[test]
    fn a_game_ended_by_another_signal_than_stop_s_was_ended() {
        let last = outcome(LaunchRoute::Host, Ending::Signalled(9), 120);
        let reason = reason(&game(LaunchKind::HostCommand), &last, &facts());
        assert_eq!(reason, Some(Reason::Ended(9)));
    }

    #[test]
    fn a_launch_that_ended_well_or_has_not_ended_is_not_explained() {
        let host = game(LaunchKind::HostCommand);
        for ending in [
            Ending::Pending,
            Ending::Exited(0),
            Ending::Signalled(crate::launch_outcome::SIGTERM),
            Ending::Lost,
        ] {
            let last = outcome(LaunchRoute::Host, ending, 120);
            assert_eq!(explain(&host, &last, &facts()), None, "{ending}");
        }
        let store = game(LaunchKind::StoreUri);
        let last = outcome(LaunchRoute::HandOff, Ending::HandedOff, 1);
        assert_eq!(explain(&store, &last, &facts()), None);
    }

    #[test]
    fn a_game_that_did_not_start_without_umu_says_umu_is_missing() {
        let facts = Facts {
            umu_installed: false,
            ..facts()
        };
        for (kind, route) in [
            (LaunchKind::Proton, LaunchRoute::Proton),
            (LaunchKind::RuntimeNative, LaunchRoute::Runtime),
        ] {
            let last = outcome(route, Ending::NotStarted, 0);
            assert_eq!(reason(&game(kind), &last, &facts), Some(Reason::UmuMissing));
        }
    }

    #[test]
    fn a_windows_game_that_did_not_start_names_its_missing_proton_build() {
        let builds = ["GE-Proton10-1".to_owned()];
        let facts = Facts {
            proton_builds: &builds,
            ..facts()
        };
        let last = LaunchOutcome {
            proton: Some("proton_9".to_owned()),
            ..outcome(LaunchRoute::Proton, Ending::NotStarted, 0)
        };
        assert_eq!(
            reason(&game(LaunchKind::Proton), &last, &facts),
            Some(Reason::ProtonMissing("proton_9".to_owned()))
        );
    }

    #[test]
    fn a_windows_game_that_did_not_start_without_any_proton_build_says_so() {
        let last = outcome(LaunchRoute::Proton, Ending::NotStarted, 0);
        assert_eq!(
            reason(&game(LaunchKind::Proton), &last, &facts()),
            Some(Reason::NoProton)
        );
    }

    #[test]
    fn a_game_that_did_not_start_for_no_known_cause_did_not_start() {
        let last = outcome(LaunchRoute::Host, Ending::NotStarted, 0);
        assert_eq!(
            reason(&game(LaunchKind::HostCommand), &last, &facts()),
            Some(Reason::NotStarted)
        );
    }

    #[test]
    fn a_hand_off_whose_launcher_could_not_be_opened_says_so() {
        let last = outcome(LaunchRoute::HandOff, Ending::HandOffFailed, 0);
        assert_eq!(
            reason(&game(LaunchKind::StoreUri), &last, &facts()),
            Some(Reason::LauncherNotOpened)
        );
    }

    #[test]
    fn a_hand_off_whose_launcher_never_answered_says_so() {
        let last = outcome(LaunchRoute::HandOff, Ending::NoAnswer, 10);
        assert_eq!(
            reason(&game(LaunchKind::StoreUri), &last, &facts()),
            Some(Reason::LauncherNoAnswer)
        );
    }

    fn steps(game: &Game, last: &LaunchOutcome, facts: &Facts<'_>) -> Vec<Step> {
        explain(game, last, facts).map_or_else(Vec::new, |explanation| explanation.steps)
    }

    #[test]
    fn a_game_that_crashed_offers_start_safely_and_its_log() {
        let last = LaunchOutcome {
            log: Some("2026-11-19T20-00-00.short.log".to_owned()),
            ..outcome(LaunchRoute::Host, Ending::Signalled(11), 120)
        };
        let facts = Facts {
            log_kept: true,
            ..facts()
        };
        let explanation = explain(&game(LaunchKind::HostCommand), &last, &facts);
        assert_eq!(
            explanation,
            Some(Explanation {
                reason: Reason::Crashed(11),
                steps: vec![Step::StartSafely, Step::OpenLog],
                log: Some("2026-11-19T20-00-00.short.log".to_owned()),
            })
        );
    }

    #[test]
    fn a_log_that_is_gone_is_not_offered() {
        let last = LaunchOutcome {
            log: Some("2026-11-19T20-00-00.short.log".to_owned()),
            ..outcome(LaunchRoute::Host, Ending::Exited(1), 120)
        };
        let explanation = explain(&game(LaunchKind::HostCommand), &last, &facts());
        assert_eq!(
            explanation.map(|explanation| (explanation.steps, explanation.log)),
            Some((vec![Step::StartSafely], None))
        );
    }

    #[test]
    fn a_game_without_umu_offers_how_to_install_it_alone() {
        let facts = Facts {
            umu_installed: false,
            ..facts()
        };
        let last = outcome(LaunchRoute::Runtime, Ending::NotStarted, 0);
        assert_eq!(
            steps(&game(LaunchKind::RuntimeNative), &last, &facts),
            [Step::HowToInstall]
        );
    }

    #[test]
    fn a_windows_game_offers_another_proton_build_only_when_there_is_one() {
        let windows = game(LaunchKind::Proton);
        let last = LaunchOutcome {
            proton: Some("proton_9".to_owned()),
            ..outcome(LaunchRoute::Proton, Ending::Exited(1), 120)
        };
        let one = ["proton_9".to_owned()];
        let facts_one = Facts {
            proton_builds: &one,
            ..facts()
        };
        assert_eq!(steps(&windows, &last, &facts_one), [Step::StartSafely]);
        let two = ["proton_9".to_owned(), "GE-Proton10-1".to_owned()];
        let facts_two = Facts {
            proton_builds: &two,
            ..facts()
        };
        assert_eq!(
            steps(&windows, &last, &facts_two),
            [Step::AnotherProton, Step::StartSafely]
        );
    }

    #[test]
    fn a_missing_proton_build_offers_another_but_not_start_safely() {
        let builds = ["GE-Proton10-1".to_owned()];
        let facts = Facts {
            proton_builds: &builds,
            ..facts()
        };
        let last = LaunchOutcome {
            proton: Some("proton_9".to_owned()),
            ..outcome(LaunchRoute::Proton, Ending::NotStarted, 0)
        };
        assert_eq!(
            steps(&game(LaunchKind::Proton), &last, &facts),
            [Step::AnotherProton]
        );
    }

    #[test]
    fn a_game_that_needs_a_permission_offers_how_to_enable_it_alone() {
        let facts = Facts {
            needs_permission: true,
            ..facts()
        };
        let last = outcome(LaunchRoute::Host, Ending::Exited(1), 120);
        assert_eq!(
            steps(&game(LaunchKind::HostCommand), &last, &facts),
            [Step::HowToEnable]
        );
    }

    #[test]
    fn a_failed_hand_off_offers_no_step() {
        let facts = Facts {
            log_kept: true,
            ..facts()
        };
        let last = outcome(LaunchRoute::HandOff, Ending::NoAnswer, 10);
        assert_eq!(steps(&game(LaunchKind::StoreUri), &last, &facts), []);
    }
}
