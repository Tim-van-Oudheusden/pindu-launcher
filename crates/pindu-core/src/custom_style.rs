//! The user's own stylesheet for fullscreen mode: when it is in force, which file changes
//! concern it, and the problems GTK reports while loading it, worded for display.

use std::fmt;
use std::path::Path;

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
}
