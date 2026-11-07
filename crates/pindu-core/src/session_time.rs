//! How long a game we started has run, as its Playing Now card shows it: "Under a minute",
//! "12 min", "1 h 05 min", in whole minutes, and when that next changes.

use std::time::Duration;

use crate::model::Timestamp;

/// The words a session's elapsed time is said in, translated by the shell. `minutes` holds
/// `{minutes}`; `hours` holds `{hours}` and `{minutes}`, which it gets as two digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionTimeWords<'a> {
    /// "Under a minute".
    pub under_a_minute: &'a str,
    /// "{minutes} min".
    pub minutes: &'a str,
    /// "{hours} h {minutes} min".
    pub hours: &'a str,
}

/// The time a session that began at `started_at` has run at `now`, in `words`: whole
/// minutes, and hours once it has run one. A clock set back counts as no time.
pub fn session_time_text(
    started_at: Timestamp,
    now: Timestamp,
    words: SessionTimeWords<'_>,
) -> String {
    let minutes = elapsed(started_at, now) / 60;
    match (minutes / 60, minutes % 60) {
        (0, 0) => words.under_a_minute.to_owned(),
        (0, minutes) => words.minutes.replace("{minutes}", &minutes.to_string()),
        (hours, minutes) => words
            .hours
            .replace("{hours}", &hours.to_string())
            .replace("{minutes}", &format!("{minutes:02}")),
    }
}

/// How long after `now` the text of a session that began at `started_at` next changes:
/// when its next whole minute is up.
pub fn until_next_minute(started_at: Timestamp, now: Timestamp) -> Duration {
    let into_minute = elapsed(started_at, now) % 60;
    Duration::from_secs((60 - into_minute).unsigned_abs())
}

/// The seconds from `started_at` to `now`, none when the clock reads earlier.
fn elapsed(started_at: Timestamp, now: Timestamp) -> i64 {
    now.saturating_sub(started_at).max(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: SessionTimeWords<'static> = SessionTimeWords {
        under_a_minute: "Under a minute",
        minutes: "{minutes} min",
        hours: "{hours} h {minutes} min",
    };

    const START: Timestamp = 1_700_000_000;

    fn text(elapsed: i64) -> String {
        session_time_text(START, START + elapsed, WORDS)
    }

    #[test]
    fn the_first_minute_is_under_a_minute() {
        assert_eq!(text(0), "Under a minute");
        assert_eq!(text(59), "Under a minute");
    }

    #[test]
    fn whole_minutes_count_from_the_end_of_the_first() {
        assert_eq!(text(60), "1 min");
        assert_eq!(text(119), "1 min");
        assert_eq!(text(12 * 60 + 30), "12 min");
        assert_eq!(text(59 * 60 + 59), "59 min");
    }

    #[test]
    fn an_hour_on_shows_hours_and_two_digit_minutes() {
        assert_eq!(text(3600), "1 h 00 min");
        assert_eq!(text(3600 + 5 * 60), "1 h 05 min");
        assert_eq!(text(2 * 3600 - 1), "1 h 59 min");
        assert_eq!(text(2 * 3600), "2 h 00 min");
        assert_eq!(text(25 * 3600 + 42 * 60), "25 h 42 min");
    }

    #[test]
    fn the_words_come_from_the_translation() {
        let words = SessionTimeWords {
            under_a_minute: "Moins d’une minute",
            minutes: "{minutes} min",
            hours: "{hours} h {minutes}",
        };
        assert_eq!(
            session_time_text(START, START + 30, words),
            "Moins d’une minute"
        );
        assert_eq!(session_time_text(START, START + 3900, words), "1 h 05");
    }

    #[test]
    fn a_clock_set_back_counts_as_no_time() {
        assert_eq!(text(-90), "Under a minute");
        assert_eq!(
            until_next_minute(START, START - 90),
            Duration::from_secs(60)
        );
    }

    #[test]
    fn the_text_next_changes_when_the_next_minute_is_up() {
        assert_eq!(until_next_minute(START, START), Duration::from_secs(60));
        assert_eq!(until_next_minute(START, START + 59), Duration::from_secs(1));
        assert_eq!(
            until_next_minute(START, START + 60),
            Duration::from_secs(60)
        );
        assert_eq!(
            until_next_minute(START, START + 3600 + 15),
            Duration::from_secs(45)
        );
    }
}
