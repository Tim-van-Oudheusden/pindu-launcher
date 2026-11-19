//! How each launch of a game ended. Every launch of a game Pindu runs, and every hand-off of
//! a store game to its launcher, is recorded when it begins and again when it ends: the
//! route, the Proton build, start and end, the exit status, whether a window appeared
//! where the session can tell, and for a hand-off whether the launcher answered. The store
//! keeps the newest [`KEEP`] per game (see `Store::begin_launch`).

use std::fmt;

use crate::model::{GameId, LaunchRoute, Timestamp, UnknownValue};

/// How many outcomes, and short logs, a game keeps; a new launch drops the oldest.
pub const KEEP: usize = 5;

/// The signal Stop sends, which games handle as "quit": a game it ends did not fail.
pub const SIGTERM: i32 = 15;

/// How a launch ended, as far as Pindu could follow it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ending {
    /// It has not ended yet: the game runs, or the launcher has not answered the hand-off.
    /// A launch Pindu stopped following, because it quit first, stays so.
    Pending,
    /// The game never started.
    NotStarted,
    /// The game exited by itself with this status.
    Exited(i32),
    /// The game was ended by this signal.
    Signalled(i32),
    /// Pindu lost track of the game, so how it ended is unknown.
    Lost,
    /// The launcher answered the hand-off: it has the game.
    HandedOff,
    /// The launcher could not be opened.
    HandOffFailed,
    /// The launcher never answered the hand-off.
    NoAnswer,
}

impl Ending {
    /// Whether the launch failed: the game never started, exited with an error or by a
    /// signal other than Stop's, or its launcher could not be reached.
    pub fn failed(self) -> bool {
        match self {
            Self::NotStarted | Self::HandOffFailed | Self::NoAnswer => true,
            Self::Exited(status) => status != 0,
            Self::Signalled(signal) => signal != SIGTERM,
            Self::Pending | Self::Lost | Self::HandedOff => false,
        }
    }

    /// The fixed string the ending is stored as.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::NotStarted => "not-started",
            Self::Exited(_) => "exited",
            Self::Signalled(_) => "signalled",
            Self::Lost => "lost",
            Self::HandedOff => "handed-off",
            Self::HandOffFailed => "hand-off-failed",
            Self::NoAnswer => "no-answer",
        }
    }

    /// The exit status or the signal it carries, as stored beside [`Self::as_str`].
    pub fn code(self) -> Option<i32> {
        match self {
            Self::Exited(code) | Self::Signalled(code) => Some(code),
            Self::Pending
            | Self::NotStarted
            | Self::Lost
            | Self::HandedOff
            | Self::HandOffFailed
            | Self::NoAnswer => None,
        }
    }

    /// The ending stored as `kind`, with `code` for an exit status or a signal.
    pub fn parse(kind: &str, code: Option<i32>) -> Result<Self, UnknownValue> {
        let unknown = || UnknownValue {
            kind: "launch ending",
            value: kind.to_owned(),
        };
        Ok(match kind {
            "pending" => Self::Pending,
            "not-started" => Self::NotStarted,
            "exited" => Self::Exited(code.ok_or_else(unknown)?),
            "signalled" => Self::Signalled(code.ok_or_else(unknown)?),
            "lost" => Self::Lost,
            "handed-off" => Self::HandedOff,
            "hand-off-failed" => Self::HandOffFailed,
            "no-answer" => Self::NoAnswer,
            _ => return Err(unknown()),
        })
    }
}

impl fmt::Display for Ending {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.code() {
            Some(code) => write!(f, "{} {code}", self.as_str()),
            None => f.write_str(self.as_str()),
        }
    }
}

/// One recorded launch of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchOutcome {
    /// The record's ID, which ends it (see `Store::end_launch`).
    pub id: i64,
    /// The game launched.
    pub game_id: GameId,
    /// How it was launched.
    pub route: LaunchRoute,
    /// The Proton build a Windows game ran with, by the name a launch config stores.
    pub proton: Option<String>,
    /// When it was launched.
    pub started_at: Timestamp,
    /// When it ended; `None` while it is [`Ending::Pending`].
    pub ended_at: Option<Timestamp>,
    /// How it ended.
    pub ending: Ending,
    /// Whether a window of the game appeared; `None` where the session cannot tell.
    pub window_seen: Option<bool>,
    /// The name of its log in the game's logs folder (see [`crate::logs`]), if it kept
    /// one.
    pub log: Option<String>,
}

/// A launch as it begins, for `Store::begin_launch`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NewLaunch<'a> {
    /// How it is launched.
    pub route: LaunchRoute,
    /// The Proton build a Windows game runs with.
    pub proton: Option<&'a str>,
    /// The name of its log in the game's logs folder, if it keeps one.
    pub log: Option<&'a str>,
    /// When it was launched.
    pub started_at: Timestamp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_launch_failed_unless_it_ended_cleanly_or_was_stopped() {
        for ending in [
            Ending::NotStarted,
            Ending::Exited(1),
            Ending::Signalled(11),
            Ending::HandOffFailed,
            Ending::NoAnswer,
        ] {
            assert!(ending.failed(), "{ending}");
        }
        for ending in [
            Ending::Pending,
            Ending::Exited(0),
            Ending::Signalled(SIGTERM),
            Ending::Lost,
            Ending::HandedOff,
        ] {
            assert!(!ending.failed(), "{ending}");
        }
    }

    #[test]
    fn an_ending_reads_back_as_it_was_stored() {
        for ending in [
            Ending::Pending,
            Ending::NotStarted,
            Ending::Exited(3),
            Ending::Signalled(9),
            Ending::Lost,
            Ending::HandedOff,
            Ending::HandOffFailed,
            Ending::NoAnswer,
        ] {
            assert_eq!(Ending::parse(ending.as_str(), ending.code()), Ok(ending));
        }
        assert!(Ending::parse("exited", None).is_err());
        assert!(Ending::parse("crashed", None).is_err());
    }

    #[test]
    fn a_route_reads_back_as_it_was_stored() {
        for &route in LaunchRoute::ALL {
            assert_eq!(route.as_str().parse(), Ok(route));
        }
        assert!("teleport".parse::<LaunchRoute>().is_err());
    }
}
