//! Game sources: where each launcher keeps its data and how we read it.
//!
//! An importer reads one source's files, read-only, into a [`Snapshot`]; syncing that
//! snapshot with the library is a separate step.

pub mod bottles;
pub mod desktop;
pub mod faugus;
pub mod heroic;
mod iso_time;
pub mod location;
pub mod lutris;
pub(crate) mod sha256;
pub mod steam;
pub mod sync;
pub mod vdf;
pub mod watch;

use std::fmt;
use std::path::PathBuf;

use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source, Timestamp};

/// Everything one import run found in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// The source that was read.
    pub source: Source,
    /// The games the source lists.
    pub games: Vec<ImportedGame>,
    /// Entries that could not be read; the rest of the source still imports.
    pub errors: Vec<String>,
}

impl Snapshot {
    /// An empty snapshot of `source`, with no games and no errors.
    pub fn new(source: Source) -> Self {
        Self {
            source,
            games: Vec::new(),
            errors: Vec::new(),
        }
    }
}

/// One game as its source describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedGame {
    /// The source's own ID for the game; the part of the game ID after the colon.
    pub native_id: String,
    /// The game's title.
    pub title: String,
    /// The game's developer, when the source knows it.
    pub developer: Option<String>,
    /// The operating system the installed build targets, when the source says.
    pub platform: Option<Platform>,
    /// Whether the game is installed.
    pub installed: bool,
    /// Where the game is installed, when the source says.
    pub install_path: Option<PathBuf>,
    /// Size on disk, in bytes.
    pub size_bytes: Option<i64>,
    /// When the game was last played, as the source records it.
    pub last_played_at: Option<Timestamp>,
    /// Total playtime the source tracks itself, in seconds.
    pub playtime_secs: Option<i64>,
    /// How Play starts the game.
    pub launch_kind: LaunchKind,
    /// A launch URI the source gives, when the URI cannot be built from the native ID.
    pub launch_uri: Option<String>,
    /// The command the source gives for starting the game ourselves.
    pub launch_config: Option<LaunchConfig>,
    /// The game's genres, as the source names them.
    pub genres: Vec<String>,
    /// The game's description, when the source has one.
    pub description: Option<String>,
    /// The source's own art, at most one per layout.
    pub artwork: Vec<(ArtLayout, PathBuf)>,
    /// Flags the source keeps itself; applied once, when the game first enters the library.
    pub source_flags: SourceFlags,
}

/// Hidden, favourite and category flags a launcher keeps for a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceFlags {
    /// Whether the launcher hides the game.
    pub hidden: bool,
    /// Whether the launcher marks the game as a favourite.
    pub favourite: bool,
    /// Names of the launcher's categories that hold the game; imported as collections.
    pub categories: Vec<String>,
}

impl ImportedGame {
    /// A game with only its native ID, title and launch kind set; it starts uninstalled.
    pub fn new(
        native_id: impl Into<String>,
        title: impl Into<String>,
        launch_kind: LaunchKind,
    ) -> Self {
        Self {
            native_id: native_id.into(),
            title: title.into(),
            developer: None,
            platform: None,
            installed: false,
            install_path: None,
            size_bytes: None,
            last_played_at: None,
            playtime_secs: None,
            launch_kind,
            launch_uri: None,
            launch_config: None,
            genres: Vec::new(),
            description: None,
            artwork: Vec::new(),
            source_flags: SourceFlags::default(),
        }
    }
}

/// A source whose data could not be read at all.
#[derive(Debug)]
pub struct ImportError {
    /// The file or directory that could not be read.
    pub path: PathBuf,
    /// What went wrong.
    pub message: String,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ImportError {}
