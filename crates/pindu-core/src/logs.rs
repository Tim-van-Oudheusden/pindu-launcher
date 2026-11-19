//! Per-game logs: one file per launch in `$XDG_DATA_HOME/pindu-launcher/logs/<game>/`,
//! named after the local time it started. "Play with Logs" and "Start Safely" keep the
//! launch's whole output in a full log, of which the newest [`KEEP`] stay. Every other
//! launch of a game Pindu runs keeps a short log: the last [`SHORT_LOG_BYTES`] of its
//! output, of which the newest [`crate::launch_outcome::KEEP`] stay.

use std::collections::VecDeque;
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
    new_stamped_name(stamp, LOG, existing)
}

/// The logs among a folder's `names` beyond the newest `keep`, oldest first; a file the
/// launcher did not name, such as a `backup.log` kept by hand, is left alone.
pub fn stale_logs(names: &[String], keep: usize) -> Vec<String> {
    stale_stamped(names, LOG, keep)
}

/// The newest log among a folder's `names`, ignoring files the launcher did not name.
pub fn latest_log(names: &[String]) -> Option<&String> {
    oldest_first(names, LOG).pop()
}

/// The most a short log keeps of a launch's output: its last lines up to this many bytes.
pub const SHORT_LOG_BYTES: usize = 64 * 1024;

/// The name of a new short log started at `stamp`, unique among the folder's `existing`
/// names, as [`new_log_name`] names a full one.
pub fn new_short_log_name(stamp: &str, existing: &[String]) -> String {
    new_stamped_name(stamp, SHORT_LOG, existing)
}

/// The short logs among a folder's `names` beyond the newest `keep`, oldest first.
pub fn stale_short_logs(names: &[String], keep: usize) -> Vec<String> {
    stale_stamped(names, SHORT_LOG, keep)
}

/// The extension of a short log's name. A full log's [`LOG`] alone never matches it: its
/// stem does not end in a start time.
const SHORT_LOG: &str = ".short.log";

/// The end of a launch's output, for its short log: the last lines that fit in a number of
/// bytes, newlines included, and how many earlier lines were left out.
#[derive(Debug, Clone)]
pub struct Tail {
    lines: VecDeque<String>,
    bytes: usize,
    limit: usize,
    left_out: usize,
}

impl Tail {
    /// An empty tail that keeps at most `limit` bytes.
    pub fn new(limit: usize) -> Self {
        Self {
            lines: VecDeque::new(),
            bytes: 0,
            limit,
            left_out: 0,
        }
    }

    /// Adds a line of output, without its newline, leaving out the oldest lines that no
    /// longer fit. A line longer than the whole tail keeps its end.
    pub fn push(&mut self, line: &str) {
        let room = self.limit.saturating_sub(1);
        let mut start = line.len().saturating_sub(room);
        while !line.is_char_boundary(start) {
            start += 1;
        }
        let line = &line[start..];
        let size = line.len() + 1;
        while self.bytes + size > self.limit {
            let Some(old) = self.lines.pop_front() else {
                // Not even an empty line fits.
                self.left_out += 1;
                return;
            };
            self.bytes -= old.len() + 1;
            self.left_out += 1;
        }
        self.lines.push_back(line.to_owned());
        self.bytes += size;
    }

    /// The kept lines, each ending in a newline, after a line saying how many earlier
    /// lines were left out, if any were.
    pub fn text(&self) -> String {
        let mut text = match self.left_out {
            0 => String::new(),
            1 => "[1 earlier line left out]\n".to_owned(),
            n => format!("[{n} earlier lines left out]\n"),
        };
        text.reserve(self.bytes);
        for line in &self.lines {
            text.push_str(line);
            text.push('\n');
        }
        text
    }
}

/// The extension of a log's name.
const LOG: &str = ".log";

/// The name of a new file made at `stamp` and ending in `extension`, unique among the
/// folder's `existing` names: a second one in the same second gets `_2`, and so on.
pub(crate) fn new_stamped_name(stamp: &str, extension: &str, existing: &[String]) -> String {
    let taken = |name: &String| existing.contains(name);
    std::iter::once(format!("{stamp}{extension}"))
        .chain((2..).map(|n| format!("{stamp}_{n}{extension}")))
        .find(|name| !taken(name))
        .expect("some name is free")
}

/// The files [`new_stamped_name`] named with `extension` among `names` beyond the newest
/// `keep`, oldest first; any other file is left alone.
pub(crate) fn stale_stamped(names: &[String], extension: &str, keep: usize) -> Vec<String> {
    let files = oldest_first(names, extension);
    let stale = files.len().saturating_sub(keep);
    files.into_iter().take(stale).cloned().collect()
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

/// The lines the log of a safe start ("Start Safely") begins with: [`header`]'s, and what
/// the start left out.
pub fn safe_header(title: &str, started: &str, argv: &[String]) -> String {
    let command = join_arguments(argv);
    format!(
        "Game: {title}\nStarted: {started}\nStarted safely: without wrappers, MangoHud, \
         GameMode or the game's own environment variables\nCommand: {command}\n\n"
    )
}

/// The files [`new_stamped_name`] named with `extension` among `names`, oldest first: by
/// stamp, then by the number that keeps files made in the same second apart.
fn oldest_first<'a>(names: &'a [String], extension: &str) -> Vec<&'a String> {
    let mut files: Vec<_> = names
        .iter()
        .filter_map(|name| Some((stamped_order(name, extension)?, name)))
        .collect();
    files.sort();
    files.into_iter().map(|(_, name)| name).collect()
}

/// Where a file named `name` sorts: its stamp and its number in that second. `None` for a
/// name [`new_stamped_name`] never gives with `extension`.
fn stamped_order<'a>(name: &'a str, extension: &str) -> Option<(&'a str, u32)> {
    let stem = name.strip_suffix(extension)?;
    let (stamp, n) = match stem.split_once('_') {
        Some((stamp, n)) => (stamp, launch_number(n)?),
        None => (stem, 1),
    };
    is_stamp(stamp).then_some((stamp, n))
}

/// Whether `stem` is a start time as `new_log_name` gets it, e.g. `2026-11-05T14-03-22`.
fn is_stamp(stem: &str) -> bool {
    const SHAPE: &[u8] = b"0000-00-00T00-00-00";
    stem.len() == SHAPE.len()
        && stem.bytes().zip(SHAPE).all(|(byte, &shape)| match shape {
            b'0' => byte.is_ascii_digit(),
            _ => byte == shape,
        })
}

/// The number `new_log_name` adds for another launch in the same second: 2 or more,
/// written without a sign or leading zeros.
fn launch_number(digits: &str) -> Option<u32> {
    if digits.starts_with('0') || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|&n| n >= 2)
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
    fn log_files_the_launcher_did_not_name_are_neither_dropped_nor_the_latest() {
        let names = names(&[
            "backup.log",
            "zzz.log",
            "backup_3.log",
            "2026-11-05T14-03-22_backup.log",
            "2026-11-05T14-03-22_+4.log",
            "2026-11-05T14-03-22_05.log",
            "2026-11-05T14-03-22_1.log",
            "2026-11-05 14-03-22.log",
            "2026-11-05T14-03-2.log",
            "2026-11-04T09-00-00.log",
            "2026-11-05T14-03-22.log",
            "2026-11-05T14-03-22_2.log",
        ]);

        assert_eq!(stale_logs(&names, 2), ["2026-11-04T09-00-00.log"]);
        assert_eq!(
            latest_log(&names).map(String::as_str),
            Some("2026-11-05T14-03-22_2.log")
        );
        assert_eq!(latest_log(&["backup.log".to_owned()]), None);
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

    #[test]
    fn a_safe_start_s_log_says_what_it_left_out() {
        let argv = ["umu-run", "/games/notepad.exe"].map(str::to_owned);

        let header = safe_header("Notepad", "2026-11-05 14:03:22", &argv);

        assert_eq!(
            header,
            "Game: Notepad\n\
             Started: 2026-11-05 14:03:22\n\
             Started safely: without wrappers, MangoHud, GameMode or the game's own environment variables\n\
             Command: umu-run /games/notepad.exe\n\n"
        );
    }

    #[test]
    fn short_logs_are_named_kept_and_dropped_apart_from_full_logs() {
        let stamp = "2026-11-05T14-03-22";
        let names = names(&[
            "2026-11-05T14-03-22.log",
            "2026-11-05T14-03-22.short.log",
            "2026-11-04T09-00-00.short.log",
            "2026-11-06T08-00-00.short.log",
        ]);

        assert_eq!(
            new_short_log_name(stamp, &names),
            "2026-11-05T14-03-22_2.short.log"
        );
        assert_eq!(
            stale_short_logs(&names, 2),
            ["2026-11-04T09-00-00.short.log"]
        );
        assert!(stale_logs(&names, 1).is_empty());
        assert_eq!(
            latest_log(&names).map(String::as_str),
            Some("2026-11-05T14-03-22.log")
        );
    }

    #[test]
    fn a_tail_keeps_the_last_lines_that_fit_and_says_how_many_it_left_out() {
        let mut tail = Tail::new(12);

        for line in ["one", "two", "three", "four"] {
            tail.push(line);
        }

        // "three\n" and "four\n" make 11 bytes; "two\n" would make 15.
        assert_eq!(tail.text(), "[2 earlier lines left out]\nthree\nfour\n");
    }

    #[test]
    fn a_tail_that_left_nothing_out_is_the_output_itself() {
        let mut tail = Tail::new(SHORT_LOG_BYTES);

        tail.push("wine: starting");
        tail.push("");

        assert_eq!(tail.text(), "wine: starting\n\n");
        assert_eq!(Tail::new(8).text(), "");
    }

    #[test]
    fn a_line_longer_than_the_tail_keeps_its_end() {
        let mut tail = Tail::new(7);

        tail.push("first");
        tail.push("abcdéfghij");

        // Six bytes and the newline would cut `é` in two, so five and the newline.
        assert_eq!(tail.text(), "[1 earlier line left out]\nfghij\n");
    }
}
