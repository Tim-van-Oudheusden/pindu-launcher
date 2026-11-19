//! The diagnostics report ([`crate::diagnostics`]) in fullscreen mode, where nothing can be
//! copied or pasted: it shows in pages a phone can photograph one at a time, and each
//! time it opens a copy is saved to `$XDG_DATA_HOME/pindu-launcher/reports/`, named after
//! the local time, of which the newest few are kept as "Play with Logs" keeps its logs
//! ([`crate::logs`]).

use std::path::{Path, PathBuf};

use crate::logs;
use crate::paths::APP_DIR;

/// How many characters wide a page's lines are; longer lines go on in the next line.
pub const PAGE_COLUMNS: usize = 60;

/// How many lines a page holds, so that it fits a 1080p screen in large text.
pub const PAGE_LINES: usize = 24;

/// How many saved reports are kept; saving another drops the oldest.
pub const KEEP: usize = 10;

/// The folder the saved reports are in.
pub fn reports_dir(data_home: &Path) -> PathBuf {
    data_home.join(APP_DIR).join("reports")
}

/// The name of a report saved at `stamp` (local time, e.g. `2026-11-05T14-03-22`), unique
/// among the folder's `existing` names.
pub fn new_report_name(stamp: &str, existing: &[String]) -> String {
    logs::new_stamped_name(stamp, REPORT, existing)
}

/// The saved reports among the folder's `names` beyond the newest `keep`, oldest first; a
/// file the launcher did not name is left alone.
pub fn stale_reports(names: &[String], keep: usize) -> Vec<String> {
    logs::stale_stamped(names, REPORT, keep)
}

/// The extension of a saved report's name.
const REPORT: &str = ".txt";

/// The `report` split into pages of at most `lines` lines each, with every line longer
/// than `columns` characters wrapped at a space where it can be (the lines it goes on in
/// are indented by two spaces). A report always has at least one page.
pub fn pages(report: &str, columns: usize, lines: usize) -> Vec<String> {
    let wrapped: Vec<String> = report
        .lines()
        .flat_map(|line| wrap(line, columns))
        .collect();
    if wrapped.is_empty() {
        return vec![String::new()];
    }
    wrapped
        .chunks(lines.max(1))
        .map(|page| page.join("\n"))
        .collect()
}

/// `line` as lines of at most `columns` characters, broken at the last space that fits
/// or else mid-word; the lines it goes on in keep its indent and add two spaces.
fn wrap(line: &str, columns: usize) -> Vec<String> {
    let indent = line.chars().take_while(|&c| c == ' ').count() + 2;
    // Room for at least one character after the indent.
    let columns = columns.max(indent + 1);
    let mut wrapped = Vec::new();
    let mut rest: Vec<char> = line.chars().collect();
    let mut lead = indent - 2;
    while rest.len() > columns {
        let (end, skip) = match rest[lead..=columns].iter().rposition(|&c| c == ' ') {
            Some(space) if space > 0 => (lead + space, 1),
            _ => (columns, 0),
        };
        wrapped.push(rest[..end].iter().collect::<String>().trim_end().to_owned());
        let after = rest[end + skip..].iter().skip_while(|&&c| c == ' ');
        rest = std::iter::repeat_n(' ', indent)
            .chain(after.copied())
            .collect();
        lead = indent;
    }
    wrapped.push(rest.into_iter().collect());
    wrapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_is_split_into_pages_of_whole_lines() {
        let report = "one\ntwo\nthree\nfour\nfive";

        assert_eq!(
            pages(report, 60, 2),
            vec!["one\ntwo", "three\nfour", "five"]
        );
    }

    #[test]
    fn a_long_line_goes_on_at_a_space_indented_by_two_more() {
        let report = "  Proton builds: GE-Proton10-4, GE-Proton9-27, Proton Experimental";

        assert_eq!(
            pages(report, 31, 24),
            vec!["  Proton builds: GE-Proton10-4,\n    GE-Proton9-27, Proton\n    Experimental"]
        );
    }

    #[test]
    fn a_word_longer_than_a_line_is_broken_mid_word() {
        let report = "umu-run: ~/.local/share/umu/bin/umu-run";

        assert_eq!(
            pages(report, 20, 24),
            vec!["umu-run:\n  ~/.local/share/umu\n  /bin/umu-run"]
        );
    }

    #[test]
    fn wrapped_lines_count_towards_a_pages_lines() {
        let report = "aaa bbb ccc\nddd";

        assert_eq!(pages(report, 8, 2), vec!["aaa bbb\n  ccc", "ddd"]);
    }

    #[test]
    fn an_empty_report_has_one_empty_page() {
        assert_eq!(pages("", 60, 24), vec![String::new()]);
    }

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|&name| name.to_owned()).collect()
    }

    #[test]
    fn reports_are_saved_in_the_apps_data_folder() {
        assert_eq!(
            reports_dir(Path::new("/data")),
            Path::new("/data/pindu-launcher/reports")
        );
    }

    #[test]
    fn a_report_is_named_after_when_it_was_saved_and_never_overwrites_one() {
        let stamp = "2026-11-19T18-05-09";

        assert_eq!(new_report_name(stamp, &[]), "2026-11-19T18-05-09.txt");
        assert_eq!(
            new_report_name(stamp, &names(&["2026-11-19T18-05-09.txt"])),
            "2026-11-19T18-05-09_2.txt"
        );
    }

    #[test]
    fn only_the_newest_reports_are_kept() {
        let folder = names(&[
            "2026-11-19T18-05-09.txt",
            "notes.txt",
            "2026-11-18T09-00-00.txt",
            "2026-11-19T18-05-09_2.txt",
        ]);

        assert_eq!(
            stale_reports(&folder, 2),
            names(&["2026-11-18T09-00-00.txt"])
        );
    }
}
