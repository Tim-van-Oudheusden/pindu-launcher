//! The SQLite library store.
//!
//! One database at `$XDG_DATA_HOME/pindu-launcher/library.db`, in WAL mode, with ordered
//! migrations tracked in `PRAGMA user_version`. Migrations are append-only: a released
//! migration is never edited, a schema change is a new entry in `MIGRATIONS`.

mod artwork;
mod collections;
mod games;
mod groups;
mod launch_configs;
mod launch_outcomes;
mod metadata;
mod playtime;
mod saved_filters;
mod sources;
mod wikidata;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, Row, TransactionBehavior};

use crate::model::{
    ArtLayout, ArtOrigin, GameId, LaunchKind, LaunchRoute, MetadataField, MetadataOrigin, Platform,
    Source,
};

/// Schema migrations in order; entry `n` takes the database from version `n` to `n + 1`.
const MIGRATIONS: &[&str] = &[
    include_str!("store/migrations/0001_initial.sql"),
    include_str!("store/migrations/0002_steam_launch_dialog.sql"),
    include_str!("store/migrations/0003_launch_uri.sql"),
    include_str!("store/migrations/0004_square_art.sql"),
    include_str!("store/migrations/0005_steamgriddb_matches.sql"),
    include_str!("store/migrations/0006_game_groups.sql"),
    include_str!("store/migrations/0007_saved_filters.sql"),
    include_str!("store/migrations/0008_game_metadata.sql"),
    include_str!("store/migrations/0009_launch_outcomes.sql"),
    include_str!("store/migrations/0010_saved_filter_genre.sql"),
    include_str!("store/migrations/0011_wikidata_items.sql"),
    include_str!("store/migrations/0012_genre_categories.sql"),
];

/// The schema version a freshly opened store is at.
pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

/// How long a connection waits for another one's lock before giving up.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    /// An error from SQLite.
    Sqlite(rusqlite::Error),
    /// A file or folder beside the database could not be written.
    Io(std::io::Error),
    /// The database was written by a newer version of the app.
    TooNew {
        /// The database's schema version.
        version: u32,
    },
    /// The row to update does not exist.
    NotFound,
    /// A path that is not valid UTF-8; the store keeps paths as text.
    NonUtf8Path(PathBuf),
    /// The copy of the database kept before a migration could not be written, so the
    /// database was left as it was.
    Backup {
        /// Where the copy was to go.
        path: PathBuf,
        /// Why it failed.
        error: Box<StoreError>,
    },
    /// A group needs two or more different games.
    TooFewCopies,
    /// The game is in a group already; a game is in at most one.
    AlreadyGrouped(GameId),
    /// The game is not one of the group's games.
    NotInGroup(GameId),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "database error: {error}"),
            Self::Io(error) => write!(f, "file error: {error}"),
            Self::TooNew { version } => write!(
                f,
                "the library was saved by a newer version (schema {version}, this version knows {LATEST_SCHEMA_VERSION})"
            ),
            Self::NotFound => f.write_str("no such entry in the library"),
            Self::NonUtf8Path(path) => write!(f, "path is not valid UTF-8: {}", path.display()),
            Self::Backup { path, error } => write!(
                f,
                "could not keep a copy of the library at {} before upgrading it: {error}",
                path.display()
            ),
            Self::TooFewCopies => f.write_str("a group needs two or more different games"),
            Self::AlreadyGrouped(game) => write!(f, "{game} is in a group already"),
            Self::NotInGroup(game) => write!(f, "{game} is not in the group"),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::Backup { error, .. } => Some(error.as_ref()),
            Self::TooNew { .. }
            | Self::NotFound
            | Self::NonUtf8Path(_)
            | Self::TooFewCopies
            | Self::AlreadyGrouped(_)
            | Self::NotInGroup(_) => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

/// A store result, failing with [`StoreError`] by default.
pub type Result<T, E = StoreError> = std::result::Result<T, E>;

/// The library database: one SQLite connection.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens or creates the database at `path`, creating its folder, and migrates it.
    /// Before migrating an existing database it keeps a copy beside it, named for its
    /// schema version (`library.db.v5.bak`), and only the newest two such copies.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let _setting_up = SettingUp::begin(path)?;
        let conn = Connection::open(path)?;
        // Another thread or process may hold the database for a moment (start-up loads
        // the library while the sync writes): wait for it instead of failing.
        conn.busy_timeout(BUSY_TIMEOUT)?;
        use_wal(&conn)?;
        Self::init(conn, path)
    }

    fn init(mut conn: Connection, path: &Path) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn, path)?;
        Ok(Self { conn })
    }

    /// The database's journal mode, as SQLite names it, such as `wal`.
    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?)
    }

    /// The database's schema version, from `PRAGMA user_version`.
    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }

    /// Runs `work` in one transaction: every write lands, or none does. Inside another
    /// transaction, `work` becomes part of it.
    pub fn in_transaction<T>(&self, work: impl FnOnce(&Self) -> Result<T>) -> Result<T> {
        // Inside another transaction, the work joins it: SQLite does not nest them.
        if !self.conn.is_autocommit() {
            return work(self);
        }
        let transaction = self.conn.unchecked_transaction()?;
        let value = work(self)?;
        transaction.commit()?;
        Ok(value)
    }
}

/// Claims setting up one database (switching it to WAL, migrating it) for this thread.
///
/// Setting up commits several times, and under heavy disk load one commit can hold the
/// write lock for longer than [`BUSY_TIMEOUT`]: threads of this process opening the
/// same database would give up with "database is locked" while the first one is still
/// making progress. So they set it up one at a time, waiting here for as long as it
/// takes; the next one then finds it set up and needs no write lock. Only waits on
/// other processes stay bounded by the timeout.
struct SettingUp(PathBuf);

/// The databases a thread of this process is setting up, by [`setup_key`].
static SETTING_UP: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Signalled whenever a database leaves [`SETTING_UP`].
static SET_UP: std::sync::Condvar = std::sync::Condvar::new();

impl SettingUp {
    /// Waits until no other thread is setting up the database at `path`, whose folder
    /// exists, then claims it until the guard drops.
    fn begin(path: &Path) -> Result<Self> {
        let key = setup_key(path)?;
        let mut claimed = SETTING_UP
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while claimed.contains(&key) {
            claimed = SET_UP
                .wait(claimed)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        claimed.push(key.clone());
        Ok(Self(key))
    }
}

impl Drop for SettingUp {
    fn drop(&mut self) {
        SETTING_UP
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .retain(|key| *key != self.0);
        SET_UP.notify_all();
    }
}

/// One name for the database at `path` however it is spelt: its file name in the
/// canonical form of its folder, which exists while the file itself may not yet.
fn setup_key(path: &Path) -> Result<PathBuf> {
    let folder = path
        .parent()
        .filter(|folder| !folder.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let folder = std::fs::canonicalize(folder).map_err(StoreError::Io)?;
    Ok(match path.file_name() {
        Some(name) => folder.join(name),
        None => path.to_owned(),
    })
}

/// Switches to write-ahead logging. The switch needs the database to itself, and while
/// another connection is setting up a new database SQLite fails it at once instead of
/// waiting, so it is retried until `BUSY_TIMEOUT`.
fn use_wal(conn: &Connection) -> Result<()> {
    let started = std::time::Instant::now();
    loop {
        match conn.pragma_update(None, "journal_mode", "wal") {
            Err(rusqlite::Error::SqliteFailure(error, _))
                if error.code == rusqlite::ErrorCode::DatabaseBusy
                    && started.elapsed() < BUSY_TIMEOUT =>
            {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            result => return Ok(result?),
        }
    }
}

/// Brings the schema up to date, one migration per transaction. Each step takes the
/// write lock before reading the version, so connections opening a new database at the
/// same time apply every migration once between them. An existing database is copied
/// beside `path` first; without the copy nothing is migrated.
fn migrate(conn: &mut Connection, path: &Path) -> Result<()> {
    let version = schema_version(conn)?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(StoreError::TooNew { version });
    }
    if version > 0 && version < LATEST_SCHEMA_VERSION {
        keep_copy(conn, path, version)?;
    }
    while schema_version(conn)? < LATEST_SCHEMA_VERSION {
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = schema_version(&tx)?;
        if let Some(migration) = MIGRATIONS.get(version as usize) {
            tx.execute_batch(migration)?;
            tx.pragma_update(None, "user_version", version + 1)?;
        }
        tx.commit()?;
    }
    Ok(())
}

fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |row| row.get(0))?)
}

/// How many copies of the database [`Store::open`] keeps from before its migrations.
const KEPT_COPIES: usize = 2;

/// Copies the database at `path`, at schema `version`, to `<file>.v<version>.bak` beside
/// it, then removes all but the newest [`KEPT_COPIES`] copies.
fn keep_copy(conn: &Connection, path: &Path, version: u32) -> Result<()> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| StoreError::NonUtf8Path(path.to_owned()))?;
    let copy = path.with_file_name(format!("{name}.v{version}.bak"));
    write_copy(conn, &copy, version)
        .and_then(|()| remove_old_copies(path, name))
        .map_err(|error| StoreError::Backup {
            path: copy,
            error: Box::new(error),
        })
}

/// Writes the copy with `VACUUM INTO`, which reads one consistent snapshot including
/// writes still in the write-ahead log. It goes to a name of its own first and becomes
/// `copy` only if it holds `version`: when another connection migrated in the meantime,
/// that connection kept the copy.
fn write_copy(conn: &Connection, copy: &Path, version: u32) -> Result<()> {
    static PARTIALS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let mut partial = copy.as_os_str().to_owned();
    partial.push(format!(
        ".{}-{}.partial",
        std::process::id(),
        PARTIALS.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let partial = PathBuf::from(partial);
    let result = (|| {
        conn.execute("VACUUM INTO ?1", [path_to_sql(&partial)?])?;
        if schema_version(&Connection::open(&partial)?)? == version {
            std::fs::rename(&partial, copy).map_err(StoreError::Io)?;
        }
        Ok(())
    })();
    if partial.exists() {
        // Best effort: a leftover partial copy only takes space.
        let _ = std::fs::remove_file(&partial);
    }
    result
}

/// Removes the copies of the database `name` beside `path` except the newest
/// [`KEPT_COPIES`], newest meaning the highest schema version.
fn remove_old_copies(path: &Path, name: &str) -> Result<()> {
    let folder = path.parent().unwrap_or(Path::new("."));
    let mut copies = Vec::new();
    for entry in std::fs::read_dir(folder).map_err(StoreError::Io)? {
        let entry = entry.map_err(StoreError::Io)?;
        let file = entry.file_name();
        let version = file.to_str().and_then(|file| {
            file.strip_prefix(name)?
                .strip_prefix(".v")?
                .strip_suffix(".bak")?
                .parse::<u32>()
                .ok()
        });
        if let Some(version) = version {
            copies.push((version, entry.path()));
        }
    }
    copies.sort_unstable_by_key(|&(version, _)| std::cmp::Reverse(version));
    for (_, old) in copies.into_iter().skip(KEPT_COPIES) {
        std::fs::remove_file(old).map_err(StoreError::Io)?;
    }
    Ok(())
}

/// Stores the text form of model values and parses it back on read.
macro_rules! sql_text {
    ($($ty:ty),+) => {$(
        impl ToSql for $ty {
            fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
                Ok(ToSqlOutput::from(self.as_str()))
            }
        }

        impl FromSql for $ty {
            fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
                value.as_str()?.parse().map_err(|error| FromSqlError::Other(Box::new(error)))
            }
        }
    )+};
}

sql_text!(
    GameId,
    Source,
    Platform,
    LaunchKind,
    ArtLayout,
    ArtOrigin,
    MetadataField,
    MetadataOrigin,
    LaunchRoute
);

fn path_to_sql(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| StoreError::NonUtf8Path(path.to_owned()))
}

fn path_column(row: &Row<'_>, column: &str) -> rusqlite::Result<Option<PathBuf>> {
    Ok(row.get::<_, Option<String>>(column)?.map(PathBuf::from))
}

fn to_json<T: serde::Serialize + ?Sized>(value: &T) -> String {
    serde_json::to_string(value).expect("plain strings and maps always serialize")
}

fn json_column<T: serde::de::DeserializeOwned>(row: &Row<'_>, column: &str) -> rusqlite::Result<T> {
    let text: String = row.get(column)?;
    serde_json::from_str(&text).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod group_tests;
