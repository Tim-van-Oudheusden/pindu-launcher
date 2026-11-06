//! The session's notifications, which Quick Access lists: what happened in the background
//! while the user may not have been looking (a sync that found games, artwork fetched or
//! not, a Proton build installed or not, a game that needs a permission). Kept in memory
//! for the running session only, newest first, at most [`CAPACITY`] of them.

use std::collections::VecDeque;

use crate::model::{GameId, Timestamp};

/// The most notifications the log keeps; adding one more drops the oldest.
pub const CAPACITY: usize = 50;

/// What a notification is about, which picks its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A sync added games or found some missing.
    Library,
    /// Artwork was fetched.
    Artwork,
    /// Fetching artwork failed.
    ArtworkFailed,
    /// A Proton build was installed.
    Download,
    /// A Proton build was not installed.
    DownloadFailed,
    /// A game could not start for a permission the sandbox lacks.
    Permission,
}

/// What a notification opens when the user picks it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// The game's page.
    Game(GameId),
    /// Preferences, at its Permissions group.
    Permissions,
}

/// One entry of the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub kind: Kind,
    pub title: String,
    /// More about it; may be empty.
    pub body: String,
    /// When it was added.
    pub time: Timestamp,
    /// Whether the user has seen it.
    pub read: bool,
    pub target: Option<Target>,
}

impl Notification {
    /// A new, unread notification at `time`, without a target.
    pub fn new(kind: Kind, title: &str, body: &str, time: Timestamp) -> Self {
        Self {
            kind,
            title: title.to_owned(),
            body: body.to_owned(),
            time,
            read: false,
            target: None,
        }
    }

    /// The same notification, opening `target` when picked.
    pub fn with_target(self, target: Target) -> Self {
        Self {
            target: Some(target),
            ..self
        }
    }
}

/// The session's notifications, newest first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    entries: VecDeque<Notification>,
}

impl Log {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `notification` as the newest, dropping the oldest beyond [`CAPACITY`].
    pub fn add(&mut self, notification: Notification) {
        self.entries.push_front(notification);
        self.entries.truncate(CAPACITY);
    }

    /// How many notifications the user has not seen.
    pub fn unread(&self) -> usize {
        self.entries.iter().filter(|entry| !entry.read).count()
    }

    /// Marks every notification read, as when the user opens the list; says whether any
    /// was unread.
    pub fn mark_all_read(&mut self) -> bool {
        let mut changed = false;
        for entry in self.entries.iter_mut().filter(|entry| !entry.read) {
            entry.read = true;
            changed = true;
        }
        changed
    }

    /// The notifications, newest first.
    pub fn entries(&self) -> impl Iterator<Item = &Notification> {
        self.entries.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn titles(log: &Log) -> Vec<&str> {
        log.entries().map(|entry| entry.title.as_str()).collect()
    }

    #[test]
    fn added_notifications_list_newest_first_and_unread() {
        let mut log = Log::new();

        log.add(Notification::new(
            Kind::Library,
            "Added 3 games",
            "From Steam",
            100,
        ));
        log.add(Notification::new(
            Kind::Download,
            "GE-Proton10-1 installed",
            "",
            200,
        ));

        assert_eq!(titles(&log), ["GE-Proton10-1 installed", "Added 3 games"]);
        let first = log.entries().last().unwrap();
        assert_eq!(first.kind, Kind::Library);
        assert_eq!(first.body, "From Steam");
        assert_eq!(first.time, 100);
        assert!(log.entries().all(|entry| !entry.read));
    }

    #[test]
    fn the_log_keeps_the_newest_fifty() {
        let mut log = Log::new();

        for time in 0..60 {
            log.add(Notification::new(
                Kind::Artwork,
                &time.to_string(),
                "",
                time,
            ));
        }

        assert_eq!(log.entries().count(), CAPACITY);
        assert_eq!(log.entries().next().unwrap().time, 59);
        assert_eq!(log.entries().last().unwrap().time, 10);
    }

    #[test]
    fn the_unread_count_counts_what_was_added_since_the_log_was_read() {
        let mut log = Log::new();
        assert_eq!(log.unread(), 0);

        log.add(Notification::new(Kind::Library, "Added 3 games", "", 100));
        log.add(Notification::new(
            Kind::ArtworkFailed,
            "Could not reach SteamGridDB",
            "",
            200,
        ));

        assert_eq!(log.unread(), 2);
    }

    #[test]
    fn marking_the_log_read_clears_the_unread_count_and_says_whether_it_changed() {
        let mut log = Log::new();
        log.add(Notification::new(Kind::Library, "Added 3 games", "", 100));
        log.add(Notification::new(
            Kind::Permission,
            "Permission Needed",
            "",
            200,
        ));

        assert!(log.mark_all_read());

        assert_eq!(log.unread(), 0);
        assert!(log.entries().all(|entry| entry.read));
        assert!(!log.mark_all_read());
        log.add(Notification::new(
            Kind::Download,
            "GE-Proton10-1 installed",
            "",
            300,
        ));
        assert_eq!(log.unread(), 1);
    }
}
