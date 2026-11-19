use std::collections::BTreeMap;

use rusqlite::{OptionalExtension, params};

use super::{Result, Store};
use crate::model::{GameId, Timestamp};
use crate::wikidata::{CrossId, Item, StoredItem};

impl Store {
    /// Keeps `item` as the Wikidata item of a game, fetched at `now`, replacing the one it
    /// had and its cross-IDs.
    pub fn set_wikidata_item(&self, id: &GameId, item: &Item, now: Timestamp) -> Result<()> {
        self.in_transaction(|store| {
            store.conn.execute(
                "INSERT INTO wikidata_items (game_id, qid, fetched_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT (game_id) DO UPDATE
                 SET qid = excluded.qid, fetched_at = excluded.fetched_at",
                params![id, item.qid, now],
            )?;
            store
                .conn
                .execute("DELETE FROM wikidata_cross_ids WHERE game_id = ?1", [id])?;
            for (cross_id, values) in &item.cross_ids {
                for value in values {
                    store.conn.execute(
                        "INSERT INTO wikidata_cross_ids (game_id, property, value)
                         VALUES (?1, ?2, ?3)",
                        params![id, cross_id.property(), value],
                    )?;
                }
            }
            Ok(())
        })
    }

    /// The Wikidata item a refresh kept for a game, if it found one.
    pub fn wikidata_item(&self, id: &GameId) -> Result<Option<StoredItem>> {
        let Some((qid, fetched_at)) = self
            .conn
            .query_row(
                "SELECT qid, fetched_at FROM wikidata_items WHERE game_id = ?1",
                [id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, Timestamp>(1)?)),
            )
            .optional()?
        else {
            return Ok(None);
        };
        let mut statement = self.conn.prepare(
            "SELECT property, value FROM wikidata_cross_ids
             WHERE game_id = ?1 ORDER BY property, value",
        )?;
        let mut cross_ids: BTreeMap<CrossId, Vec<String>> = BTreeMap::new();
        let rows = statement.query_map([id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (property, value) = row?;
            // A property this version does not know is left out.
            if let Some(cross_id) = CrossId::from_property(&property) {
                cross_ids.entry(cross_id).or_default().push(value);
            }
        }
        Ok(Some(StoredItem {
            qid,
            cross_ids,
            fetched_at,
        }))
    }
}
