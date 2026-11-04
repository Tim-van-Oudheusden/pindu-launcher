//! Lutris: games from its SQLite library, `pga.db`.
//!
//! The database is copied to a temporary file and the copy is queried, so a running
//! Lutris never sees our reads or locks. Covers come from `coverart/<slug>.jpg` and icons
//! from the `lutris_<slug>.png` Lutris installs next to its data folder. Nothing is
//! written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rusqlite::{Connection, OpenFlags};

use super::{ImportError, ImportedGame, Snapshot};
use crate::model::{ArtLayout, LaunchKind, Platform, Source};

/// Lutris's category for hidden games.
const HIDDEN: &str = ".hidden";
/// Lutris's category for favourites.
const FAVOURITE: &str = "favorite";

/// Imports the Lutris folder at `root` (a resolved Lutris location).
pub fn import(root: &Path) -> Result<Snapshot, ImportError> {
    let live = root.join("pga.db");
    let fail = |message: String| ImportError {
        path: live.clone(),
        message,
    };
    let copy = TempCopy::of(&live).map_err(|error| fail(error.to_string()))?;
    let db = Connection::open_with_flags(&copy.0, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|error| fail(error.to_string()))?;
    read(&db, root).map_err(|error| fail(error.to_string()))
}

fn read(db: &Connection, root: &Path) -> rusqlite::Result<Snapshot> {
    let mut categories: HashMap<i64, Vec<String>> = HashMap::new();
    let mut statement = db.prepare(
        "SELECT games_categories.game_id, categories.name FROM games_categories
         JOIN categories ON categories.id = games_categories.category_id",
    )?;
    let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
    for row in rows {
        let (game_id, name): (i64, String) = row?;
        categories.entry(game_id).or_default().push(name);
    }

    let mut snapshot = Snapshot::new(Source::Lutris);
    let mut statement = db.prepare(
        "SELECT id, name, slug, platform, directory, lastplayed, installed, playtime
         FROM games ORDER BY id",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get("id")?;
        let Some(name) = row
            .get::<_, Option<String>>("name")?
            .filter(|n| !n.is_empty())
        else {
            snapshot
                .errors
                .push(format!("Lutris game {id} has no name"));
            continue;
        };
        let mut game = ImportedGame::new(id.to_string(), name, LaunchKind::StoreUri);
        game.platform = match row.get::<_, Option<String>>("platform")?.as_deref() {
            Some("Linux") => Some(Platform::Linux),
            Some("Windows") => Some(Platform::Windows),
            _ => None,
        };
        game.installed = row.get::<_, Option<i64>>("installed")? == Some(1);
        game.install_path = row
            .get::<_, Option<String>>("directory")?
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from);
        game.last_played_at = row
            .get::<_, Option<i64>>("lastplayed")?
            .filter(|time| *time > 0);
        // Lutris keeps playtime in hours.
        game.playtime_secs = row
            .get::<_, Option<f64>>("playtime")?
            .map(|hours| (hours * 3600.0).round() as i64)
            .filter(|secs| *secs > 0);
        if let Some(slug) = row.get::<_, Option<String>>("slug")? {
            game.artwork = artwork(root, &slug);
        }
        for category in categories.remove(&id).unwrap_or_default() {
            match category.as_str() {
                HIDDEN => game.source_flags.hidden = true,
                FAVOURITE => game.source_flags.favourite = true,
                // Other dot-names are Lutris's own bookkeeping, not user categories.
                name if name.starts_with('.') => {}
                _ => game.source_flags.categories.push(category),
            }
        }
        snapshot.games.push(game);
    }
    Ok(snapshot)
}

fn artwork(root: &Path, slug: &str) -> Vec<(ArtLayout, PathBuf)> {
    [
        (ArtLayout::Cover, root.join(format!("coverart/{slug}.jpg"))),
        (
            ArtLayout::Icon,
            root.join(format!("../icons/hicolor/128x128/apps/lutris_{slug}.png")),
        ),
    ]
    .into_iter()
    .filter(|(_, path)| path.is_file())
    .collect()
}

/// A private copy of a file in the temporary folder, removed when dropped.
struct TempCopy(PathBuf);

impl TempCopy {
    fn of(path: &Path) -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            "pindu-lutris-{}-{}.db",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let copy = Self(std::env::temp_dir().join(name));
        std::fs::copy(path, &copy.0)?;
        Ok(copy)
    }
}

impl Drop for TempCopy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
