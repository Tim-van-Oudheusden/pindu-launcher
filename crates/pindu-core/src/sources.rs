//! Game sources: where each launcher keeps its data and how we read it.
//!
//! An importer reads one source's files, read-only, into a [`Snapshot`]; syncing that
//! snapshot with the library is a separate step.

pub mod heroic;
pub mod location;
mod sha256;
pub mod steam;
pub mod sync;
pub mod vdf;

use std::fmt;
use std::path::PathBuf;

use crate::model::{ArtLayout, LaunchKind, Platform, Source, Timestamp};

/// Everything one import run found in a source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub source: Source,
    pub games: Vec<ImportedGame>,
    /// Entries that could not be read; the rest of the source still imports.
    pub errors: Vec<String>,
}

impl Snapshot {
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
    pub native_id: String,
    pub title: String,
    pub developer: Option<String>,
    pub platform: Option<Platform>,
    pub installed: bool,
    pub install_path: Option<PathBuf>,
    pub size_bytes: Option<i64>,
    pub last_played_at: Option<Timestamp>,
    pub launch_kind: LaunchKind,
    pub genres: Vec<String>,
    pub description: Option<String>,
    /// The source's own art, at most one per layout.
    pub artwork: Vec<(ArtLayout, PathBuf)>,
    /// Flags the source keeps itself; applied once, when the game first enters the library.
    pub source_flags: SourceFlags,
}

/// Hidden, favourite and category flags a launcher keeps for a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceFlags {
    pub hidden: bool,
    pub favourite: bool,
    /// Names of the launcher's categories that hold the game; imported as collections.
    pub categories: Vec<String>,
}

impl ImportedGame {
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
            launch_kind,
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
    pub path: PathBuf,
    pub message: String,
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ImportError {}
