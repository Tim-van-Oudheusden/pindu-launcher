//! The user's own stylesheet for fullscreen mode: the problems GTK reports while loading it,
//! worded for display.

use std::fmt;

/// Whether GTK dropped part of the stylesheet or only warned about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

/// One problem GTK reported while parsing the stylesheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StyleProblem {
    /// The line the problem starts on, counted from zero as GTK reports it.
    pub line: usize,
    pub message: String,
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
}
