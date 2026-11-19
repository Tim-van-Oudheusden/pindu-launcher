use rusqlite::params;

use super::{Result, Store, path_to_sql};
use crate::model::{ArtLayout, ArtOrigin, Artwork, GameId};

impl Store {
    /// Stores one image, replacing the earlier one of the same game, layout and origin.
    pub fn set_artwork(&self, artwork: &Artwork) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO artwork (game_id, layout, origin, path) VALUES (?1, ?2, ?3, ?4)",
            params![
                artwork.game_id,
                artwork.layout,
                artwork.origin,
                path_to_sql(&artwork.path)?,
            ],
        )?;
        Ok(())
    }

    /// Every stored image of a game, ordered by layout and origin.
    pub fn artwork(&self, game_id: &GameId) -> Result<Vec<Artwork>> {
        let mut statement = self.conn.prepare(
            "SELECT game_id, layout, origin, path FROM artwork WHERE game_id = ?1
             ORDER BY layout, origin",
        )?;
        let artwork = statement
            .query_map([game_id], artwork_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(artwork)
    }

    /// Every stored image of every game, ordered by game, layout and origin.
    pub fn all_artwork(&self) -> Result<Vec<Artwork>> {
        let mut statement = self.conn.prepare(
            "SELECT game_id, layout, origin, path FROM artwork ORDER BY game_id, layout, origin",
        )?;
        let artwork = statement
            .query_map([], artwork_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(artwork)
    }

    /// Deletes the stored image of a game for one layout and origin, if there is one.
    pub fn delete_artwork(
        &self,
        game_id: &GameId,
        layout: ArtLayout,
        origin: ArtOrigin,
    ) -> Result<()> {
        self.conn.execute(
            "DELETE FROM artwork WHERE game_id = ?1 AND layout = ?2 AND origin = ?3",
            params![game_id, layout, origin],
        )?;
        Ok(())
    }

    /// Remembers the SteamGridDB game the user chose for a game, replacing an earlier one.
    pub fn set_steamgriddb_match(&self, game_id: &GameId, grid_id: u64) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO steamgriddb_matches (game_id, grid_id) VALUES (?1, ?2)",
            params![game_id, grid_id as i64],
        )?;
        Ok(())
    }

    /// The SteamGridDB game the user chose for a game, if they chose one.
    pub fn steamgriddb_match(&self, game_id: &GameId) -> Result<Option<u64>> {
        let mut statement = self
            .conn
            .prepare("SELECT grid_id FROM steamgriddb_matches WHERE game_id = ?1")?;
        let mut rows = statement.query_map([game_id], |row| row.get::<_, i64>(0))?;
        Ok(rows.next().transpose()?.map(|id| id as u64))
    }
}

fn artwork_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Artwork> {
    Ok(Artwork {
        game_id: row.get("game_id")?,
        layout: row.get("layout")?,
        origin: row.get("origin")?,
        path: row.get::<_, String>("path")?.into(),
    })
}
