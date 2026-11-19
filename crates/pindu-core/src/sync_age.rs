//! How long ago a source last synced, as Preferences, Sources says it: "Never", "Just
//! now", or in whole minutes, hours or days. The shell words it.

use crate::model::Timestamp;

/// How long ago a source last synced, in the largest whole unit that fits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncAge {
    /// It has never synced.
    Never,
    /// Less than a minute ago, or at a time the clock has not reached.
    JustNow,
    /// This many whole minutes ago, from 1 to 59.
    Minutes(u32),
    /// This many whole hours ago, from 1 to 23.
    Hours(u32),
    /// This many whole days ago, at least 1.
    Days(u32),
}

/// How long before `now` a source that last synced at `last_sync` did so.
pub fn sync_age(last_sync: Option<Timestamp>, now: Timestamp) -> SyncAge {
    let Some(last_sync) = last_sync else {
        return SyncAge::Never;
    };
    let seconds = now.saturating_sub(last_sync).max(0);
    let whole = |unit: i64| u32::try_from(seconds / unit).unwrap_or(u32::MAX);
    match seconds {
        ..60 => SyncAge::JustNow,
        60..3_600 => SyncAge::Minutes(whole(60)),
        3_600..86_400 => SyncAge::Hours(whole(3_600)),
        _ => SyncAge::Days(whole(86_400)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: Timestamp = 1_700_000_000;

    #[test]
    fn a_source_never_synced_says_never() {
        assert_eq!(sync_age(None, NOW), SyncAge::Never);
    }

    #[test]
    fn under_a_minute_ago_is_just_now() {
        assert_eq!(sync_age(Some(NOW), NOW), SyncAge::JustNow);
        assert_eq!(sync_age(Some(NOW - 59), NOW), SyncAge::JustNow);
    }

    #[test]
    fn a_sync_the_clock_has_not_reached_is_just_now() {
        assert_eq!(sync_age(Some(NOW + 600), NOW), SyncAge::JustNow);
    }

    #[test]
    fn under_an_hour_ago_counts_whole_minutes() {
        assert_eq!(sync_age(Some(NOW - 60), NOW), SyncAge::Minutes(1));
        assert_eq!(sync_age(Some(NOW - 3_599), NOW), SyncAge::Minutes(59));
    }

    #[test]
    fn under_a_day_ago_counts_whole_hours() {
        assert_eq!(sync_age(Some(NOW - 3_600), NOW), SyncAge::Hours(1));
        assert_eq!(sync_age(Some(NOW - 86_399), NOW), SyncAge::Hours(23));
    }

    #[test]
    fn a_day_or_more_ago_counts_whole_days() {
        assert_eq!(sync_age(Some(NOW - 86_400), NOW), SyncAge::Days(1));
        assert_eq!(
            sync_age(Some(NOW - 40 * 86_400 - 5), NOW),
            SyncAge::Days(40)
        );
    }

    #[test]
    fn a_sync_at_the_dawn_of_time_does_not_overflow() {
        assert_eq!(
            sync_age(Some(Timestamp::MIN), Timestamp::MAX),
            SyncAge::Days(u32::MAX)
        );
    }
}
