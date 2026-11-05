//! Per-game logs from "Play with Logs": one file per launch in
//! `$XDG_DATA_HOME/pindu-launcher/logs/<game>/`, named after the local time it started,
//! and only the newest few kept.

use std::path::{Path, PathBuf};

use crate::launch::Command;
use crate::manual::join_arguments;
use crate::model::GameId;
use crate::paths::APP_DIR;

/// How many logs a game keeps; starting another drops the oldest.
pub const KEEP: usize = 10;

/// The folder that holds a game's logs, named after its ID with anything but letters,
/// digits, `-` and `.` turned into `_`.
pub fn game_logs_dir(data_home: &Path, id: &GameId) -> PathBuf {
    let name: String = id
        .as_str()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    data_home.join(APP_DIR).join("logs").join(name)
}

/// The name of a new log started at `stamp` (local time, e.g. `2026-11-05T14-03-22`),
/// unique among the folder's `existing` names.
pub fn new_log_name(stamp: &str, existing: &[String]) -> String {
    let taken = |name: &String| existing.contains(name);
    std::iter::once(format!("{stamp}.log"))
        .chain((2..).map(|n| format!("{stamp}_{n}.log")))
        .find(|name| !taken(name))
        .expect("some name is free")
}

/// The logs among a folder's `names` beyond the newest `keep`, oldest first; what is not
/// a log is left alone.
pub fn stale_logs(names: &[String], keep: usize) -> Vec<String> {
    let logs = oldest_first(names);
    let stale = logs.len().saturating_sub(keep);
    logs.into_iter().take(stale).cloned().collect()
}

/// The newest log among a folder's `names`.
pub fn latest_log(names: &[String]) -> Option<&String> {
    oldest_first(names).pop()
}

/// Has Proton write its log, and Wine's output, to a file in `dir`. "Play with Logs" wins
/// over the game's own `PROTON_LOG`.
pub fn log_proton(command: &mut Command, dir: &Path) {
    let environment = &mut command.environment;
    environment.insert("PROTON_LOG".to_owned(), "1".to_owned());
    environment.insert(
        "PROTON_LOG_DIR".to_owned(),
        dir.to_string_lossy().into_owned(),
    );
}

/// The lines a log starts with: which game, when, and the command it ran.
pub fn header(title: &str, started: &str, argv: &[String]) -> String {
    let command = join_arguments(argv);
    format!("Game: {title}\nStarted: {started}\nCommand: {command}\n\n")
}

/// The logs among `names`, oldest first: by start, then by the number that keeps logs
/// started in the same second apart.
fn oldest_first(names: &[String]) -> Vec<&String> {
    let order = |name: &String| {
        let stem = name.strip_suffix(".log")?;
        Some(match stem.rsplit_once('_') {
            Some((stamp, n)) if let Ok(n) = n.parse::<u32>() => (stamp.to_owned(), n),
            _ => (stem.to_owned(), 1),
        })
    };
    let mut logs: Vec<_> = names
        .iter()
        .filter_map(|name| Some((order(name)?, name)))
        .collect();
    logs.sort();
    logs.into_iter().map(|(_, name)| name).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Source;

    #[test]
    fn a_games_logs_live_in_a_folder_named_after_its_id() {
        let id = GameId::new(Source::Manual, "Note pad/2");

        let dir = game_logs_dir(Path::new("/data"), &id);

        assert_eq!(
            dir,
            Path::new("/data/pindu-launcher/logs/manual_Note_pad_2")
        );
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    #[test]
    fn a_new_log_is_named_after_its_start_and_never_replaces_another() {
        let stamp = "2026-11-05T14-03-22";

        assert_eq!(new_log_name(stamp, &[]), "2026-11-05T14-03-22.log");
        assert_eq!(
            new_log_name(stamp, &names(&["2026-11-05T14-03-22.log"])),
            "2026-11-05T14-03-22_2.log"
        );
        assert_eq!(
            new_log_name(
                stamp,
                &names(&["2026-11-05T14-03-22.log", "2026-11-05T14-03-22_2.log"])
            ),
            "2026-11-05T14-03-22_3.log"
        );
    }

    #[test]
    fn only_the_newest_logs_are_kept() {
        let names = names(&[
            "2026-11-05T14-03-22_10.log",
            "2026-11-04T09-00-00.log",
            "notes.txt",
            "2026-11-05T14-03-22_2.log",
            "2026-11-05T14-03-22.log",
            "2026-11-06T08-00-00.log",
        ]);

        assert_eq!(
            stale_logs(&names, 3),
            ["2026-11-04T09-00-00.log", "2026-11-05T14-03-22.log"]
        );
        assert!(stale_logs(&names, 5).is_empty());
    }

    #[test]
    fn the_latest_log_is_the_newest_one() {
        let names = names(&[
            "2026-11-05T14-03-22_2.log",
            "notes.txt",
            "2026-11-05T14-03-22_10.log",
            "2026-11-04T09-00-00.log",
        ]);

        assert_eq!(
            latest_log(&names).map(String::as_str),
            Some("2026-11-05T14-03-22_10.log")
        );
        assert_eq!(latest_log(&["notes.txt".to_owned()]), None);
    }

    #[test]
    fn proton_writes_its_log_to_the_given_folder() {
        let mut command = Command {
            argv: vec!["umu-run".to_owned(), "/games/notepad.exe".to_owned()],
            environment: [("PROTON_LOG", "0"), ("WINEPREFIX", "/prefixes/notepad")]
                .map(|(key, value)| (key.to_owned(), value.to_owned()))
                .into(),
            working_dir: None,
        };

        log_proton(&mut command, Path::new("/tmp/proton-log"));

        assert_eq!(command.environment["PROTON_LOG"], "1");
        assert_eq!(command.environment["PROTON_LOG_DIR"], "/tmp/proton-log");
        assert_eq!(command.environment["WINEPREFIX"], "/prefixes/notepad");
        assert_eq!(command.argv, ["umu-run", "/games/notepad.exe"]);
    }

    #[test]
    fn a_log_starts_with_the_game_its_start_and_its_command() {
        let argv = ["umu-run", "/games/Note pad.exe", "-x"].map(str::to_owned);

        let header = header("Notepad", "2026-11-05 14:03:22", &argv);

        assert_eq!(
            header,
            "Game: Notepad\n\
             Started: 2026-11-05 14:03:22\n\
             Command: umu-run '/games/Note pad.exe' -x\n\n"
        );
    }
}
