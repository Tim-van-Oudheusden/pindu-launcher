use std::collections::HashMap;

use rusqlite::params;

use super::{Result, Store, StoreError};
use crate::model::{Collection, GameId};

impl Store {
    /// Creates an empty collection after every existing one.
    pub fn create_collection(&self, name: &str) -> Result<Collection> {
        let position: i64 = self.conn.query_row(
            "SELECT COALESCE(MAX(position) + 1, 0) FROM collections",
            [],
            |row| row.get(0),
        )?;
        self.conn.execute(
            "INSERT INTO collections (name, position) VALUES (?1, ?2)",
            params![name, position],
        )?;
        Ok(Collection {
            id: self.conn.last_insert_rowid(),
            name: name.to_owned(),
            position,
        })
    }

    /// Every collection, in the user's order.
    pub fn collections(&self) -> Result<Vec<Collection>> {
        let mut statement = self
            .conn
            .prepare("SELECT id, name, position FROM collections ORDER BY position, id")?;
        let collections = statement
            .query_map([], |row| {
                Ok(Collection {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    position: row.get("position")?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(collections)
    }

    /// Stores a collection's name and position.
    pub fn update_collection(&self, collection: &Collection) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE collections SET name = ?2, position = ?3 WHERE id = ?1",
            params![collection.id, collection.name, collection.position],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// Puts the collections in the given order; collections not listed keep their place
    /// after them.
    pub fn set_collection_order(&self, ids: &[i64]) -> Result<()> {
        self.in_transaction(|store| {
            for (position, id) in ids.iter().enumerate() {
                store.conn.execute(
                    "UPDATE collections SET position = ?2 WHERE id = ?1",
                    params![id, position as i64],
                )?;
            }
            Ok(())
        })
    }

    /// For every game in a collection, the IDs of its collections, ascending.
    pub fn collection_memberships(&self) -> Result<HashMap<GameId, Vec<i64>>> {
        let mut statement = self.conn.prepare(
            "SELECT game_id, collection_id FROM collection_members ORDER BY collection_id",
        )?;
        let mut memberships: HashMap<GameId, Vec<i64>> = HashMap::new();
        let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        for row in rows {
            let (game_id, collection_id) = row?;
            memberships.entry(game_id).or_default().push(collection_id);
        }
        Ok(memberships)
    }

    /// Deletes a collection; its games stay in the library.
    pub fn delete_collection(&self, id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM collections WHERE id = ?1", [id])?;
        Ok(())
    }

    /// Adds a game to a collection; adding a member again changes nothing.
    pub fn add_to_collection(&self, collection_id: i64, game_id: &GameId) -> Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO collection_members (collection_id, game_id) VALUES (?1, ?2)",
            params![collection_id, game_id],
        )?;
        Ok(())
    }

    pub fn remove_from_collection(&self, collection_id: i64, game_id: &GameId) -> Result<()> {
        self.conn.execute(
            "DELETE FROM collection_members WHERE collection_id = ?1 AND game_id = ?2",
            params![collection_id, game_id],
        )?;
        Ok(())
    }

    /// The games in a collection, ordered by ID.
    pub fn collection_members(&self, collection_id: i64) -> Result<Vec<GameId>> {
        let mut statement = self.conn.prepare(
            "SELECT game_id FROM collection_members WHERE collection_id = ?1 ORDER BY game_id",
        )?;
        let members = statement
            .query_map([collection_id], |row| row.get(0))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(members)
    }
}
