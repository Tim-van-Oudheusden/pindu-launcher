use rusqlite::types::ToSql;
use rusqlite::{OptionalExtension, Row};

use super::{Result, Store, StoreError, path_column, path_to_sql};
use crate::metadata::{self, Metadata};
use crate::model::{Game, GameId, MetadataOrigin, Source};

const COLUMNS: &str = "id, title, sort_title, platform, installed, install_path, size_bytes, \
    added_at, last_played_at, favourite, hidden, pin_order, user_removed, missing_since, \
    launch_kind, launch_uri, steam_launch_dialog";

impl Store {
    /// Stores a new game; a game with the same ID already stored is an error. Its
    /// developer, genres and description become its source's metadata, set at `added_at`.
    pub fn insert_game(&self, game: &Game) -> Result<()> {
        self.in_transaction(|store| {
            store.write_game(
                game,
                "INSERT INTO games (id, source, native_id, title, sort_title, platform,
                    installed, install_path, size_bytes, added_at, last_played_at, favourite,
                    hidden, pin_order, user_removed, missing_since, launch_kind, launch_uri,
                    steam_launch_dialog)
                 VALUES (:id, :source, :native_id, :title, :sort_title, :platform,
                    :installed, :install_path, :size_bytes, :added_at, :last_played_at,
                    :favourite, :hidden, :pin_order, :user_removed, :missing_since,
                    :launch_kind, :launch_uri, :steam_launch_dialog)",
            )?;
            store.set_metadata(
                &game.id,
                MetadataOrigin::Source,
                &Metadata::of(game),
                game.added_at,
            )
        })
    }

    /// Replaces every stored field of an existing game except its metadata (developer,
    /// genres, description), which a game carries resolved from every origin: those
    /// change through [`Store::set_metadata`] only.
    pub fn update_game(&self, game: &Game) -> Result<()> {
        let changed = self.write_game(
            game,
            "UPDATE games SET title = :title, sort_title = :sort_title,
                platform = :platform, installed = :installed, install_path = :install_path,
                size_bytes = :size_bytes, added_at = :added_at, last_played_at = :last_played_at,
                favourite = :favourite, hidden = :hidden, pin_order = :pin_order,
                user_removed = :user_removed, missing_since = :missing_since,
                launch_kind = :launch_kind, launch_uri = :launch_uri,
                steam_launch_dialog = :steam_launch_dialog
             WHERE id = :id AND source = :source AND native_id = :native_id",
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    fn write_game(&self, game: &Game, sql: &str) -> Result<usize> {
        let install_path = game.install_path.as_deref().map(path_to_sql).transpose()?;
        let params: &[(&str, &dyn ToSql)] = &[
            (":id", &game.id),
            (":source", &game.source()),
            (":native_id", &game.id.native_id()),
            (":title", &game.title),
            (":sort_title", &game.sort_title),
            (":platform", &game.platform),
            (":installed", &game.installed),
            (":install_path", &install_path),
            (":size_bytes", &game.size_bytes),
            (":added_at", &game.added_at),
            (":last_played_at", &game.last_played_at),
            (":favourite", &game.favourite),
            (":hidden", &game.hidden),
            (":pin_order", &game.pin_order),
            (":user_removed", &game.user_removed),
            (":missing_since", &game.missing_since),
            (":launch_kind", &game.launch_kind),
            (":launch_uri", &game.launch_uri),
            (":steam_launch_dialog", &game.steam_launch_dialog),
        ];
        Ok(self.conn.execute(sql, params)?)
    }

    /// The stored game with this ID, with its metadata resolved, or `None` when there is
    /// none.
    pub fn game(&self, id: &GameId) -> Result<Option<Game>> {
        let Some(mut game) = self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM games WHERE id = ?1"),
                [id.as_str()],
                game_from_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        metadata::resolve(&self.metadata_values(id)?).apply_to(&mut game);
        Ok(Some(game))
    }

    /// Every stored game, including hidden, removed and missing ones, with its metadata
    /// resolved, ordered by ID.
    pub fn games(&self) -> Result<Vec<Game>> {
        self.games_where("", ())
    }

    /// Every stored game of one source, with its metadata resolved, ordered by ID.
    pub fn games_from(&self, source: Source) -> Result<Vec<Game>> {
        self.games_where("WHERE source = ?1", [source])
    }

    fn games_where<P: rusqlite::Params>(&self, filter: &str, params: P) -> Result<Vec<Game>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM games {filter} ORDER BY id"))?;
        let mut games: Vec<Game> = statement
            .query_map(params, game_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        let mut values = self.all_metadata_values()?;
        for game in &mut games {
            if let Some(values) = values.remove(&game.id) {
                metadata::resolve(&values).apply_to(game);
            }
        }
        Ok(games)
    }

    /// Deletes a game together with its launch config, artwork, sessions, metadata and
    /// memberships; a group left with one game is removed.
    pub fn delete_game(&self, id: &GameId) -> Result<()> {
        self.in_transaction(|store| {
            store
                .conn
                .execute("DELETE FROM games WHERE id = ?1", [id.as_str()])?;
            store.drop_small_groups()
        })
    }
}

/// A game from its row, without its metadata.
fn game_from_row(row: &Row<'_>) -> rusqlite::Result<Game> {
    Ok(Game {
        id: row.get("id")?,
        title: row.get("title")?,
        sort_title: row.get("sort_title")?,
        developer: None,
        platform: row.get("platform")?,
        installed: row.get("installed")?,
        install_path: path_column(row, "install_path")?,
        size_bytes: row.get("size_bytes")?,
        added_at: row.get("added_at")?,
        last_played_at: row.get("last_played_at")?,
        favourite: row.get("favourite")?,
        hidden: row.get("hidden")?,
        pin_order: row.get("pin_order")?,
        user_removed: row.get("user_removed")?,
        missing_since: row.get("missing_since")?,
        launch_kind: row.get("launch_kind")?,
        launch_uri: row.get("launch_uri")?,
        genres: Vec::new(),
        description: None,
        release_year: None,
        controller_support: None,
        steam_launch_dialog: row.get("steam_launch_dialog")?,
    })
}
