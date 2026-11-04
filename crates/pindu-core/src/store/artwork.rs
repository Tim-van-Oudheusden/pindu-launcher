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
            .query_map([game_id], |row| {
                Ok(Artwork {
                    game_id: row.get("game_id")?,
                    layout: row.get("layout")?,
                    origin: row.get("origin")?,
                    path: row.get::<_, String>("path")?.into(),
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(artwork)
    }

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
}
