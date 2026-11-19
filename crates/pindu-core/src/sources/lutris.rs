//! Lutris: games from its SQLite library, `pga.db`.
//!
//! The database is copied to a temporary file and the copy is queried, so a running
//! Lutris never sees our reads or locks. Covers come from `coverart/<slug>.jpg` and icons
//! from the `lutris_<slug>.png` Lutris installs next to its data folder. Each game also
//! carries the store record Lutris links it to (`service`, `service_id`), its runner and
//! its year, for matching copies of one game across sources. Nothing is written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::sqlite_copy::SqliteCopy;
use super::{ImportError, ImportedGame, ServiceRef, Snapshot};
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
    let copy = SqliteCopy::of(&live, "lutris").map_err(|error| fail(error.to_string()))?;
    let db = copy.open().map_err(|error| fail(error.to_string()))?;
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
        "SELECT id, name, slug, platform, directory, lastplayed, installed, playtime,
         service, service_id, runner, year
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
        game.service = match (text(row, "service")?, text(row, "service_id")?) {
            (Some(service), Some(service_id)) => Some(ServiceRef {
                service,
                service_id,
            }),
            _ => None,
        };
        game.runner = text(row, "runner")?;
        // Lutris writes 0 or nothing when it does not know the year.
        game.year = row.get::<_, Option<i32>>("year")?.filter(|year| *year > 0);
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

/// A text column, treating an empty string, which Lutris writes for "none", as missing.
fn text(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<Option<String>> {
    Ok(row
        .get::<_, Option<String>>(column)?
        .filter(|value| !value.is_empty()))
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
