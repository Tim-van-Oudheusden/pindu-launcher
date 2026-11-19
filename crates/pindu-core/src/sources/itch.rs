//! itch: games installed through the itch app, from butler's database, `db/butler.db`.
//!
//! butler, the itch app's daemon, records each install as a cave linked to its itch.io
//! game; the folder is in `install_locations`, or the cave's own custom folder. The
//! database is copied, with its write-ahead log, to a temporary file and only the copy's
//! `caves`, `games` and `install_locations` tables are queried: never `profiles`, which
//! holds the user's itch.io key. Games start through the itch app as
//! `itch://caves/<cave>/launch`, and the app tracks their playtime, which we import. A
//! game installed more than once starts the install the itch app would pick, the one run
//! or touched last. Pages that are not played (soundtracks, books, comics, assets,
//! physical games) are left out. Nothing is written.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::iso_time::parse_iso_timestamp;
use super::sqlite_copy::SqliteCopy;
use super::{ImportError, ImportedGame, Snapshot};
use crate::model::{LaunchKind, Platform, Source, Timestamp};

/// itch.io classifications of pages that are not something to play.
const NOT_PLAYED: &[&str] = &["assets", "soundtrack", "comic", "book", "physical_game"];

/// Imports the itch folder at `root` (a resolved itch location).
pub fn import(root: &Path) -> Result<Snapshot, ImportError> {
    let live = root.join("db/butler.db");
    let fail = |message: String| ImportError {
        path: live.clone(),
        message,
    };
    let copy = SqliteCopy::of(&live, "itch").map_err(|error| fail(error.to_string()))?;
    let db = copy.open().map_err(|error| fail(error.to_string()))?;
    read(&db).map_err(|error| fail(error.to_string()))
}

fn read(db: &Connection) -> rusqlite::Result<Snapshot> {
    let mut snapshot = Snapshot::new(Source::Itch);
    // Each game's freshest install so far, with its freshness, by index in the snapshot.
    let mut kept: HashMap<i64, (usize, Option<Timestamp>)> = HashMap::new();
    let mut statement = db.prepare(
        "SELECT caves.id AS cave, games.id AS game_id, games.title,
         games.classification, games.windows, games.linux, caves.installed_at,
         caves.last_touched_at, caves.local_last_run_at, caves.seconds_run,
         caves.local_seconds_run, caves.installed_size, caves.custom_install_folder,
         caves.install_folder_name, install_locations.path AS location
         FROM caves
         LEFT JOIN games ON games.id = caves.game_id
         LEFT JOIN install_locations ON install_locations.id = caves.install_location_id
         ORDER BY caves.id",
    )?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let cave: String = row.get("cave")?;
        let Some(game_id) = row.get::<_, Option<i64>>("game_id")? else {
            snapshot
                .errors
                .push(format!("itch install {cave} has no game record"));
            continue;
        };
        if text(row, "classification")?.is_some_and(|kind| NOT_PLAYED.contains(&kind.as_str())) {
            continue;
        }
        let Some(title) = text(row, "title")? else {
            snapshot
                .errors
                .push(format!("itch game {game_id} has no title"));
            continue;
        };
        let time = |column: &str| -> rusqlite::Result<Option<Timestamp>> {
            Ok(text(row, column)?.and_then(|time| parse_iso_timestamp(&time, 0)))
        };
        let (installed_at, touched_at, run_at) = (
            time("installed_at")?,
            time("last_touched_at")?,
            time("local_last_run_at")?,
        );
        // The order the itch app picks one of a game's installs by.
        let freshness = run_at.or(touched_at).or(installed_at);

        let mut game = ImportedGame::new(game_id.to_string(), title, LaunchKind::StoreUri);
        game.installed = true;
        game.launch_uri = Some(format!("itch://caves/{cave}/launch"));
        game.platform = if text(row, "linux")?.is_some() {
            Some(Platform::Linux)
        } else if text(row, "windows")?.is_some() {
            Some(Platform::Windows)
        } else {
            None
        };
        game.install_path = match text(row, "custom_install_folder")? {
            Some(folder) => Some(PathBuf::from(folder)),
            None => text(row, "location")?
                .zip(text(row, "install_folder_name")?)
                .map(|(location, folder)| Path::new(&location).join(folder)),
        };
        game.size_bytes = row
            .get::<_, Option<i64>>("installed_size")?
            .filter(|size| *size > 0);
        // The itch.io total and this machine's count overlap, so they are never summed.
        let seconds = |column: &str| {
            row.get::<_, Option<i64>>(column)
                .map(Option::unwrap_or_default)
        };
        game.playtime_secs =
            Some(seconds("seconds_run")?.max(seconds("local_seconds_run")?)).filter(|s| *s > 0);
        game.last_played_at = touched_at.max(run_at);

        match kept.get(&game_id) {
            Some(&(_, kept_freshness)) if kept_freshness >= freshness => {}
            Some(&(index, _)) => {
                snapshot.games[index] = game;
                kept.insert(game_id, (index, freshness));
            }
            None => {
                kept.insert(game_id, (snapshot.games.len(), freshness));
                snapshot.games.push(game);
            }
        }
    }
    Ok(snapshot)
}

/// A text column, treating an empty string, which butler writes for "none", as missing.
fn text(row: &rusqlite::Row<'_>, column: &str) -> rusqlite::Result<Option<String>> {
    Ok(row
        .get::<_, Option<String>>(column)?
        .filter(|value| !value.is_empty()))
}
