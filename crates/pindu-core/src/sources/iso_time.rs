//! ISO 8601 times as launchers write them.

use crate::model::Timestamp;

/// Parses an ISO 8601 date and time as Python's `isoformat()` and JavaScript's
/// `toISOString()` write it:
/// `YYYY-MM-DD[T ]HH:MM:SS[.ffffff][Z|±HH:MM]`. A time without an offset is local time,
/// `utc_offset` seconds east of UTC.
pub(crate) fn parse_iso_timestamp(text: &str, utc_offset: i64) -> Option<Timestamp> {
    let (date, time) = text.split_once(['T', ' '])?;
    let mut date_parts = date.splitn(3, '-').map(|part| part.parse::<i64>().ok());
    let (year, month, day) = (
        date_parts.next()??,
        date_parts.next()??,
        date_parts.next()??,
    );
    let (clock, offset) = if let Some(clock) = time.strip_suffix('Z') {
        (clock, 0)
    } else if let Some(at) = time.rfind(['+', '-']) {
        let (clock, zone) = time.split_at(at);
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let (hours, minutes) = zone[1..].split_once(':').unwrap_or((&zone[1..], "0"));
        (
            clock,
            sign * (hours.parse::<i64>().ok()? * 3600 + minutes.parse::<i64>().ok()? * 60),
        )
    } else {
        (time, utc_offset)
    };
    let clock = clock.split_once('.').map_or(clock, |(whole, _)| whole);
    let mut clock_parts = clock.splitn(3, ':').map(|part| part.parse::<i64>().ok());
    let (hour, minute, second) = (
        clock_parts.next()??,
        clock_parts.next()??,
        clock_parts.next().flatten().unwrap_or(0),
    );
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    Some(days * 86_400 + hour * 3600 + minute * 60 + second - offset)
}

/// Days since 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_isoformat_times_parse_in_every_form() {
        assert_eq!(parse_iso_timestamp("1970-01-01T00:00:00", 0), Some(0));
        assert_eq!(
            parse_iso_timestamp("2024-02-29T23:59:59", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29 23:59:59.999999", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29T23:59:59Z", 7200),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-03-01T01:59:59+02:00", 0),
            Some(1_709_251_199)
        );
        assert_eq!(
            parse_iso_timestamp("2024-02-29T18:29:59.5-05:30", 0),
            Some(1_709_251_199)
        );
    }

    #[test]
    fn a_time_without_an_offset_is_local_time() {
        // 01:00 in UTC+1 is midnight UTC.
        assert_eq!(parse_iso_timestamp("1970-01-01T01:00:00", 3600), Some(0));
        assert_eq!(parse_iso_timestamp("1969-12-31T19:00:00", -18_000), Some(0));
    }

    #[test]
    fn malformed_times_are_ignored() {
        for text in [
            "",
            "yesterday",
            "2024-02-29",
            "2024-13-01T00:00:00",
            "2024-02-29T25:00:00",
        ] {
            assert_eq!(parse_iso_timestamp(text, 0), None, "{text}");
        }
    }
}
