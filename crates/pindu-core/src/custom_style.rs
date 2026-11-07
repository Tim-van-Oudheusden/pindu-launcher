//! The user's own stylesheet for fullscreen mode: when it is in force, which file changes
//! concern it, and the problems GTK reports while loading it, worded for display.

use std::path::{Path, PathBuf};
use std::{fmt, fs, io};

/// Whether GTK dropped part of the stylesheet or only warned about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// GTK dropped part of the stylesheet.
    Error,
    /// GTK only warned.
    Warning,
}

/// One problem GTK reported while parsing the stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleProblem {
    /// The line the problem starts on, counted from zero as GTK reports it.
    pub line: usize,
    /// GTK's description of the problem.
    pub message: String,
    /// Whether it is an error or a warning.
    pub severity: Severity,
}

impl fmt::Display for StyleProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line + 1, self.message)
    }
}

/// How many problems are errors; warnings are not, as GTK says they are never fatal.
pub fn error_count(problems: &[StyleProblem]) -> usize {
    problems
        .iter()
        .filter(|problem| problem.severity == Severity::Error)
        .count()
}

/// How many errors a summary names (`ProblemSummary::first`); the log has them all.
pub const SUMMARY_LINES: usize = 3;

/// The stylesheet's problems in brief, as Preferences shows them: how many errors there
/// are and the first few, each worded `line N: message`. Warnings are left out, as they
/// are from `error_count`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProblemSummary {
    /// How many errors there are.
    pub count: usize,
    /// The first `SUMMARY_LINES` errors, worded for display.
    pub first: Vec<String>,
}

impl ProblemSummary {
    /// Sums up `problems` as GTK reported them.
    pub fn new(problems: &[StyleProblem]) -> Self {
        Self {
            count: error_count(problems),
            first: problems
                .iter()
                .filter(|problem| problem.severity == Severity::Error)
                .take(SUMMARY_LINES)
                .map(ToString::to_string)
                .collect(),
        }
    }
}

/// What decides whether the user's stylesheet and the chosen design are in force.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StyleState {
    /// The user turned the custom style on.
    pub enabled: bool,
    /// The window is in fullscreen mode.
    pub fullscreen: bool,
    /// The system asks for high contrast.
    pub high_contrast: bool,
}

impl StyleState {
    /// Whether the stylesheet should be in force: never under high contrast, so the user's
    /// colours cannot defeat it.
    pub fn applies(self) -> bool {
        self.enabled && self.fullscreen && !self.high_contrast
    }

    /// Whether the chosen design should be in force: in fullscreen mode without high
    /// contrast, whether or not the user's own stylesheet is on.
    pub fn design_applies(self) -> bool {
        self.fullscreen && !self.high_contrast
    }
}

/// Whether a change in the style's folder, to `file` and, for a rename, `other`, touches
/// the stylesheet at `style`.
pub fn concerns_style(style: &Path, file: &Path, other: Option<&Path>) -> bool {
    file == style || other == Some(style)
}

/// Where the stylesheet at `style` goes when it is replaced or reset: beside it, with
/// `.bak` appended (`fullscreen.css.bak`).
pub fn backup_path(style: &Path) -> PathBuf {
    let mut backup = style.as_os_str().to_owned();
    backup.push(".bak");
    PathBuf::from(backup)
}

/// Moves the stylesheet at `style` to its backup (`backup_path`), replacing an older
/// backup, so a reset never loses the user's work; without a stylesheet there is nothing
/// to do.
pub fn reset_style(style: &Path) -> io::Result<()> {
    match fs::rename(style, backup_path(style)) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}

/// The folder of the stylesheet at `style`, created if it is missing, for the user to
/// open and put the stylesheet and its assets in.
pub fn style_folder(style: &Path) -> io::Result<&Path> {
    let folder = style.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(folder)?;
    Ok(folder)
}

/// Copies the stylesheet the user picked at `picked` to `style`, creating its folder, so
/// `url()` and `@import` resolve there; a stylesheet already at `style` becomes its
/// backup (`reset_style`). The copy is made beside `style` first, so a failed copy
/// leaves the old stylesheet in place and a watcher sees the new one arrive whole.
pub fn import_style(picked: &Path, style: &Path) -> io::Result<()> {
    style_folder(style)?;
    let mut partial = style.as_os_str().to_owned();
    partial.push(".import");
    let partial = PathBuf::from(partial);
    if let Err(error) = fs::copy(picked, &partial) {
        let _ = fs::remove_file(&partial);
        return Err(error);
    }
    reset_style(style)?;
    fs::rename(&partial, style)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn problems_name_their_line_counting_from_one() {
        let problem = StyleProblem {
            line: 0,
            message: "Unknown property name".to_owned(),
            severity: Severity::Error,
        };

        assert_eq!(problem.to_string(), "line 1: Unknown property name");
    }

    #[test]
    fn only_errors_count_towards_the_problem_count() {
        let problem = |line, severity| StyleProblem {
            line,
            message: "Unknown property name".to_owned(),
            severity,
        };
        let problems = [
            problem(2, Severity::Error),
            problem(5, Severity::Warning),
            problem(9, Severity::Error),
        ];

        assert_eq!(error_count(&problems), 2);
        assert_eq!(error_count(&problems[1..2]), 0);
    }

    #[test]
    fn the_style_applies_only_in_fullscreen_mode_with_the_setting_on() {
        let state = |enabled, fullscreen| StyleState {
            enabled,
            fullscreen,
            high_contrast: false,
        };

        assert!(state(true, true).applies());
        assert!(!state(true, false).applies());
        assert!(!state(false, true).applies());
    }

    #[test]
    fn high_contrast_suspends_the_style() {
        let state = StyleState {
            enabled: true,
            fullscreen: true,
            high_contrast: true,
        };

        assert!(!state.applies());
    }

    #[test]
    fn a_design_applies_in_fullscreen_mode_without_the_custom_style_setting() {
        let state = |enabled, fullscreen, high_contrast| StyleState {
            enabled,
            fullscreen,
            high_contrast,
        };

        assert!(state(false, true, false).design_applies());
        assert!(!state(true, false, false).design_applies());
        assert!(!state(true, true, true).design_applies());
    }

    #[test]
    fn changes_concern_the_style_when_they_touch_its_file() {
        let style = Path::new("/config/pindu-launcher/fullscreen.css");
        let other = Path::new("/config/pindu-launcher/notes.txt");
        let temp = Path::new("/config/pindu-launcher/.fullscreen.css.swp");

        assert!(concerns_style(style, style, None));
        assert!(!concerns_style(style, other, None));
        // An editor that saves by writing a temporary file and renaming it over the style.
        assert!(concerns_style(style, temp, Some(style)));
        // Renaming the style away.
        assert!(concerns_style(style, style, Some(other)));
    }

    #[test]
    fn the_backup_sits_beside_the_style_with_bak_appended() {
        let style = Path::new("/config/pindu-launcher/fullscreen.css");

        assert_eq!(
            backup_path(style),
            Path::new("/config/pindu-launcher/fullscreen.css.bak")
        );
    }

    #[test]
    fn reset_moves_the_style_to_its_backup_replacing_an_older_one() {
        let dir = tempfile::tempdir().unwrap();
        let style = dir.path().join("fullscreen.css");
        std::fs::write(&style, "new").unwrap();
        std::fs::write(backup_path(&style), "old").unwrap();

        reset_style(&style).unwrap();

        assert!(!style.exists());
        assert_eq!(std::fs::read_to_string(backup_path(&style)).unwrap(), "new");
        // Nothing to reset is not a failure.
        reset_style(&style).unwrap();
        assert_eq!(std::fs::read_to_string(backup_path(&style)).unwrap(), "new");
    }

    #[test]
    fn import_copies_the_file_in_keeping_the_old_style_as_its_backup() {
        let dir = tempfile::tempdir().unwrap();
        let picked = dir.path().join("downloads/neon.css");
        let style = dir.path().join("config/pindu-launcher/fullscreen.css");
        std::fs::create_dir_all(picked.parent().unwrap()).unwrap();
        std::fs::write(&picked, "first").unwrap();

        // The style's folder need not exist yet.
        import_style(&picked, &style).unwrap();

        assert_eq!(std::fs::read_to_string(&style).unwrap(), "first");
        assert!(!backup_path(&style).exists());
        assert_eq!(std::fs::read_to_string(&picked).unwrap(), "first");

        std::fs::write(&picked, "second").unwrap();
        import_style(&picked, &style).unwrap();

        assert_eq!(std::fs::read_to_string(&style).unwrap(), "second");
        assert_eq!(
            std::fs::read_to_string(backup_path(&style)).unwrap(),
            "first"
        );
    }

    #[test]
    fn the_summary_counts_the_errors_and_names_the_first_few() {
        let problem = |line, severity| StyleProblem {
            line,
            message: "Unknown property name".to_owned(),
            severity,
        };
        let problems = [
            problem(1, Severity::Error),
            problem(2, Severity::Warning),
            problem(4, Severity::Error),
            problem(6, Severity::Error),
            problem(8, Severity::Error),
        ];

        let summary = ProblemSummary::new(&problems);

        assert_eq!(summary.count, 4);
        assert_eq!(
            summary.first,
            [
                "line 2: Unknown property name",
                "line 5: Unknown property name",
                "line 7: Unknown property name",
            ]
        );
        assert_eq!(ProblemSummary::new(&problems[1..2]).count, 0);
        assert!(ProblemSummary::new(&problems[1..2]).first.is_empty());
    }

    #[test]
    fn the_style_folder_is_created_when_it_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let style = dir.path().join("config/pindu-launcher/fullscreen.css");

        let folder = style_folder(&style).unwrap();

        assert_eq!(folder, dir.path().join("config/pindu-launcher"));
        assert!(folder.is_dir());
        // An existing folder is fine.
        assert_eq!(style_folder(&style).unwrap(), folder);
    }
}
