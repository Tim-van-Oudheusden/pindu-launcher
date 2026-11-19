//! Private copies of launchers' SQLite databases, so a running launcher never sees our
//! reads or locks.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags};

/// A copy of a database in the temporary folder, with its write-ahead log when it has
/// one, removed when dropped.
pub(crate) struct SqliteCopy(PathBuf);

impl SqliteCopy {
    /// Copies the database at `live`, and its `-wal` file if there is one, so rows a
    /// running launcher has not checkpointed yet are in the copy. `label` names the copy.
    pub(crate) fn of(live: &Path, label: &str) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "pindu-{label}-{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let copy = Self(std::env::temp_dir().join(name));
        std::fs::copy(live, &copy.0)?;
        match std::fs::copy(with_suffix(live, "-wal"), with_suffix(&copy.0, "-wal")) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        Ok(copy)
    }

    /// Opens the copy. It is ours alone, so it opens for writing, which lets SQLite
    /// replay the copied log into it.
    pub(crate) fn open(&self) -> rusqlite::Result<Connection> {
        Connection::open_with_flags(&self.0, OpenFlags::SQLITE_OPEN_READ_WRITE)
    }
}

impl Drop for SqliteCopy {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(with_suffix(&self.0, suffix));
        }
    }
}

/// `path` with `suffix` appended to its file name, as SQLite names a database's log.
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(suffix);
    name.into()
}
