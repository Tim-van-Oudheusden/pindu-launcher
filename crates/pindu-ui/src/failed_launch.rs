//! What both shells say about a game's last failed launch (`pindu_core::failed_launch`),
//! and the actions its next steps take. The desktop game page shows the reason with a
//! button per step; fullscreen mode's game page shows the reason, and its options sheet
//! the reason and the steps.

use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::failed_launch::{Reason, Step};
use pindu_core::model::{Game, GameId};

use crate::launch_service::launcher_name;

/// Why the last launch of `game` failed, in a sentence.
pub fn reason_text(reason: &Reason, game: &Game) -> String {
    let launcher =
        || launcher_name(game.source()).map_or_else(|| gettext("The launcher"), str::to_owned);
    match reason {
        Reason::NotStarted => gettext("The game did not start"),
        // Translators: {code} is the number the game exited with.
        Reason::QuitWithError(code) => {
            gettext("The game quit with error code {code}").replace("{code}", &code.to_string())
        }
        // Translators: {code} is the number the game exited with.
        Reason::QuitAtOnce(code) => {
            gettext("The game quit straight after it started, with error code {code}")
                .replace("{code}", &code.to_string())
        }
        // Translators: {signal} is the number of the signal that ended the game.
        Reason::Crashed(signal) => {
            gettext("The game crashed (signal {signal})").replace("{signal}", &signal.to_string())
        }
        // Translators: {signal} is the number of the signal that ended the game.
        Reason::Ended(signal) => gettext("Something else ended the game (signal {signal})")
            .replace("{signal}", &signal.to_string()),
        Reason::UmuMissing => gettext("umu-launcher, which runs the game, is not installed"),
        // Translators: {proton} is the name of a Proton build.
        Reason::ProtonMissing(build) => {
            gettext("The game’s Proton build, {proton}, is not on this computer any more")
                .replace("{proton}", build)
        }
        Reason::NoProton => gettext("No Proton build is installed to run the game"),
        // Translators: {launcher} is a store launcher's name, such as Steam.
        Reason::LauncherNotOpened => {
            gettext("{launcher} could not be opened").replace("{launcher}", &launcher())
        }
        // Translators: {launcher} is a store launcher's name, such as Steam.
        Reason::LauncherNoAnswer => {
            gettext("{launcher} did not answer").replace("{launcher}", &launcher())
        }
    }
}

/// The label of the button or entry that takes `step`.
pub fn step_label(step: Step) -> String {
    match step {
        Step::HowToEnable => gettext("How to Enable"),
        Step::HowToInstall => gettext("How to Install umu-launcher"),
        Step::AnotherProton => gettext("Choose Another Proton Build"),
        Step::StartSafely => gettext("Start Safely"),
        Step::OpenLog => gettext("Open Log"),
    }
}

/// The icon of fullscreen mode's entry for `step`.
pub fn step_icon(step: Step) -> &'static str {
    match step {
        Step::HowToEnable | Step::AnotherProton => "emblem-system-symbolic",
        Step::HowToInstall => "adw-external-link-symbolic",
        Step::StartSafely => "security-high-symbolic",
        Step::OpenLog => "text-x-generic-symbolic",
    }
}

/// The detailed action that takes `step` for the game with `id`, whose launch kept the
/// log named `log`: Preferences at its Permissions group, umu-launcher's install
/// instructions, Start Safely or the log. `None` for [`Step::AnotherProton`], which each
/// shell shows its own way, and for Open Log without a log.
pub fn step_action(
    step: Step,
    id: &GameId,
    log: Option<&str>,
) -> Option<(&'static str, Option<glib::Variant>)> {
    match step {
        Step::HowToEnable => Some(("app.permissions", None)),
        Step::HowToInstall => Some(("app.install-umu", None)),
        Step::StartSafely => Some(("app.start-safely", Some(id.as_str().to_variant()))),
        Step::OpenLog => {
            log.map(|log| ("win.open-launch-log", Some((id.as_str(), log).to_variant())))
        }
        Step::AnotherProton => None,
    }
}
