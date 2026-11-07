//! The clock in fullscreen mode's top bar: the time as the desktop's `clock-format` setting
//! shows it, and when the next minute starts.

use std::time::Duration;

/// How the desktop shows the time (`org.gnome.desktop.interface` `clock-format`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClockFormat {
    /// `"24h"`: 0 to 23 hours, no period.
    #[default]
    TwentyFourHour,
    /// `"12h"`: 1 to 12 hours with the locale's AM or PM.
    TwelveHour,
}

impl ClockFormat {
    /// The format a `clock-format` value names: `"12h"`, or else `"24h"`.
    pub fn from_setting(value: &str) -> Self {
        if value == "12h" {
            Self::TwelveHour
        } else {
            Self::TwentyFourHour
        }
    }
}

/// The time `hour:minute` (`hour` from 0 to 23) as the clock shows it in `format`, with the
/// locale's names for before and after noon in `periods` (`["AM", "PM"]` in English).
pub fn clock_text(hour: u32, minute: u32, format: ClockFormat, periods: [&str; 2]) -> String {
    match format {
        ClockFormat::TwentyFourHour => format!("{hour:02}:{minute:02}"),
        ClockFormat::TwelveHour => {
            let period = periods[usize::from(hour >= 12)];
            let hour = match hour % 12 {
                0 => 12,
                hour => hour,
            };
            if period.is_empty() {
                format!("{hour}:{minute:02}")
            } else {
                format!("{hour}:{minute:02} {period}")
            }
        }
    }
}

/// How long until the next minute starts, `second` and `microsecond` into this one.
pub fn until_next_minute(second: u32, microsecond: u32) -> Duration {
    Duration::from_secs(60)
        - Duration::from_secs(second.min(59).into())
        - Duration::from_micros(microsecond.min(999_999).into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_24_hour_clock_shows_hours_and_minutes() {
        assert_eq!(
            clock_text(21, 5, ClockFormat::TwentyFourHour, ["AM", "PM"]),
            "21:05"
        );
        assert_eq!(
            clock_text(7, 30, ClockFormat::TwentyFourHour, ["AM", "PM"]),
            "07:30"
        );
        assert_eq!(
            clock_text(0, 0, ClockFormat::TwentyFourHour, ["AM", "PM"]),
            "00:00"
        );
    }

    #[test]
    fn the_12_hour_clock_counts_from_12_with_the_period() {
        let text = |hour, minute| clock_text(hour, minute, ClockFormat::TwelveHour, ["AM", "PM"]);
        assert_eq!(text(0, 5), "12:05 AM");
        assert_eq!(text(7, 30), "7:30 AM");
        assert_eq!(text(12, 0), "12:00 PM");
        assert_eq!(text(21, 59), "9:59 PM");
    }

    #[test]
    fn the_12_hour_clock_leaves_out_a_period_the_locale_has_no_name_for() {
        assert_eq!(
            clock_text(21, 59, ClockFormat::TwelveHour, ["", ""]),
            "9:59"
        );
    }

    #[test]
    fn the_setting_picks_the_format_and_defaults_to_24_hours() {
        assert_eq!(ClockFormat::from_setting("12h"), ClockFormat::TwelveHour);
        assert_eq!(
            ClockFormat::from_setting("24h"),
            ClockFormat::TwentyFourHour
        );
        assert_eq!(ClockFormat::from_setting(""), ClockFormat::TwentyFourHour);
    }

    #[test]
    fn the_next_minute_starts_after_the_rest_of_this_one() {
        assert_eq!(until_next_minute(0, 0), Duration::from_secs(60));
        assert_eq!(until_next_minute(30, 0), Duration::from_secs(30));
        assert_eq!(until_next_minute(59, 500_000), Duration::from_millis(500));
    }
}
