use rusqlite::types::ToSql;
use rusqlite::{OptionalExtension, Row};

use super::{Result, Store, StoreError, json_column, path_column, path_to_sql, to_json};
use crate::model::{Game, GameId};

const COLUMNS: &str = "id, title, sort_title, developer, platform, installed, install_path, \
    size_bytes, added_at, last_played_at, favourite, hidden, pin_order, user_removed, \
    missing_since, launch_kind, genres, description";

impl Store {
    pub fn insert_game(&self, game: &Game) -> Result<()> {
        self.write_game(
            game,
            "INSERT INTO games (id, source, native_id, title, sort_title, developer, platform,
                installed, install_path, size_bytes, added_at, last_played_at, favourite, hidden,
                pin_order, user_removed, missing_since, launch_kind, genres, description)
             VALUES (:id, :source, :native_id, :title, :sort_title, :developer, :platform,
                :installed, :install_path, :size_bytes, :added_at, :last_played_at, :favourite,
                :hidden, :pin_order, :user_removed, :missing_since, :launch_kind, :genres,
                :description)",
        )?;
        Ok(())
    }

    /// Replaces every stored field of an existing game.
    pub fn update_game(&self, game: &Game) -> Result<()> {
        let changed = self.write_game(
            game,
            "UPDATE games SET title = :title, sort_title = :sort_title, developer = :developer,
                platform = :platform, installed = :installed, install_path = :install_path,
                size_bytes = :size_bytes, added_at = :added_at, last_played_at = :last_played_at,
                favourite = :favourite, hidden = :hidden, pin_order = :pin_order,
                user_removed = :user_removed, missing_since = :missing_since,
                launch_kind = :launch_kind, genres = :genres, description = :description
             WHERE id = :id AND source = :source AND native_id = :native_id",
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    fn write_game(&self, game: &Game, sql: &str) -> Result<usize> {
        let install_path = game.install_path.as_deref().map(path_to_sql).transpose()?;
        let genres = to_json(&game.genres);
        let params: &[(&str, &dyn ToSql)] = &[
            (":id", &game.id),
            (":source", &game.source()),
            (":native_id", &game.id.native_id()),
            (":title", &game.title),
            (":sort_title", &game.sort_title),
            (":developer", &game.developer),
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
            (":genres", &genres),
            (":description", &game.description),
        ];
        Ok(self.conn.execute(sql, params)?)
    }

    pub fn game(&self, id: &GameId) -> Result<Option<Game>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM games WHERE id = ?1"),
                [id.as_str()],
                game_from_row,
            )
            .optional()?)
    }

    /// Every stored game, including hidden, removed and missing ones, ordered by ID.
    pub fn games(&self) -> Result<Vec<Game>> {
        let mut statement = self
            .conn
            .prepare(&format!("SELECT {COLUMNS} FROM games ORDER BY id"))?;
        let games = statement
            .query_map([], game_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(games)
    }

    /// Deletes a game together with its launch config, artwork, sessions and memberships.
    pub fn delete_game(&self, id: &GameId) -> Result<()> {
        self.conn
            .execute("DELETE FROM games WHERE id = ?1", [id.as_str()])?;
        Ok(())
    }
}

fn game_from_row(row: &Row<'_>) -> rusqlite::Result<Game> {
    Ok(Game {
        id: row.get("id")?,
        title: row.get("title")?,
        sort_title: row.get("sort_title")?,
        developer: row.get("developer")?,
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
        genres: json_column(row, "genres")?,
        description: row.get("description")?,
    })
}
