//! The SQLite library store.
//!
//! One database at `$XDG_DATA_HOME/pindu-launcher/library.db`, in WAL mode, with ordered
//! migrations tracked in `PRAGMA user_version`. Migrations are append-only: a released
//! migration is never edited, a schema change is a new entry in [`MIGRATIONS`].

mod artwork;
mod collections;
mod games;
mod launch_configs;
mod playtime;
mod sources;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::types::{FromSql, FromSqlError, FromSqlResult, ToSql, ToSqlOutput, ValueRef};
use rusqlite::{Connection, Row};

use crate::model::{ArtLayout, ArtOrigin, GameId, LaunchKind, Platform, Source};

/// Schema migrations in order; entry `n` takes the database from version `n` to `n + 1`.
const MIGRATIONS: &[&str] = &[include_str!("store/migrations/0001_initial.sql")];

/// The schema version a freshly opened store is at.
pub const LATEST_SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;

#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    /// The database was written by a newer version of the app.
    TooNew {
        version: u32,
    },
    /// The row to update does not exist.
    NotFound,
    /// A path that is not valid UTF-8; the store keeps paths as text.
    NonUtf8Path(PathBuf),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(error) => write!(f, "database error: {error}"),
            Self::Io(error) => write!(f, "could not create the database folder: {error}"),
            Self::TooNew { version } => write!(
                f,
                "the library was saved by a newer version (schema {version}, this version knows {LATEST_SCHEMA_VERSION})"
            ),
            Self::NotFound => f.write_str("no such entry in the library"),
            Self::NonUtf8Path(path) => write!(f, "path is not valid UTF-8: {}", path.display()),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Sqlite(error) => Some(error),
            Self::Io(error) => Some(error),
            Self::TooNew { .. } | Self::NotFound | Self::NonUtf8Path(_) => None,
        }
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

pub type Result<T, E = StoreError> = std::result::Result<T, E>;

pub struct Store {
    conn: Connection,
}

impl Store {
    /// Opens or creates the database at `path`, creating its folder, and migrates it.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "wal")?;
        Self::init(conn)
    }

    fn init(mut conn: Connection) -> Result<Self> {
        conn.pragma_update(None, "foreign_keys", true)?;
        migrate(&mut conn)?;
        Ok(Self { conn })
    }

    pub fn journal_mode(&self) -> Result<String> {
        Ok(self
            .conn
            .pragma_query_value(None, "journal_mode", |row| row.get(0))?)
    }

    pub fn schema_version(&self) -> Result<u32> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |row| row.get(0))?)
    }
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let version: u32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version > LATEST_SCHEMA_VERSION {
        return Err(StoreError::TooNew { version });
    }
    for (index, migration) in MIGRATIONS.iter().enumerate().skip(version as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(migration)?;
        tx.pragma_update(None, "user_version", index as u32 + 1)?;
        tx.commit()?;
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

sql_text!(GameId, Source, Platform, LaunchKind, ArtLayout, ArtOrigin);

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
