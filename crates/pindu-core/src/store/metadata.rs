use std::collections::HashMap;
use std::error::Error;

use rusqlite::{Row, params};

use super::{Result, Store, to_json};
use crate::metadata::{FieldValue, Metadata, MetadataValue};
use crate::model::{DeckCompatibility, GameId, MetadataField, MetadataOrigin, Timestamp};

impl Store {
    /// Replaces the metadata values one origin gives a game. A value that did not change
    /// keeps the time it was set; a changed one gets `now`; a field `metadata` leaves
    /// empty loses this origin's value.
    pub fn set_metadata(
        &self,
        id: &GameId,
        origin: MetadataOrigin,
        metadata: &Metadata,
        now: Timestamp,
    ) -> Result<()> {
        self.in_transaction(|store| {
            let values = metadata.values();
            for &field in MetadataField::ALL {
                if !values.iter().any(|value| value.field() == field) {
                    store.conn.execute(
                        "DELETE FROM game_metadata
                         WHERE game_id = ?1 AND field = ?2 AND origin = ?3",
                        params![id, field, origin],
                    )?;
                }
            }
            for value in &values {
                store.conn.execute(
                    "INSERT INTO game_metadata (game_id, field, origin, value, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (game_id, field, origin) DO UPDATE
                     SET value = excluded.value, updated_at = excluded.updated_at
                     WHERE value IS NOT excluded.value",
                    params![id, value.field(), origin, value_text(value), now],
                )?;
            }
            Ok(())
        })
    }

    /// Every metadata value stored for a game, ordered by field and origin name.
    pub fn metadata_values(&self, id: &GameId) -> Result<Vec<MetadataValue>> {
        let mut statement = self.conn.prepare(
            "SELECT field, origin, value, updated_at FROM game_metadata
             WHERE game_id = ?1 ORDER BY field, origin",
        )?;
        let values = statement
            .query_map([id], metadata_value_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(values)
    }

    /// Every stored metadata value, by game.
    pub(super) fn all_metadata_values(&self) -> Result<HashMap<GameId, Vec<MetadataValue>>> {
        let mut statement = self
            .conn
            .prepare("SELECT game_id, field, origin, value, updated_at FROM game_metadata")?;
        let mut rows = statement.query([])?;
        let mut values: HashMap<GameId, Vec<MetadataValue>> = HashMap::new();
        while let Some(row) = rows.next()? {
            values
                .entry(row.get("game_id")?)
                .or_default()
                .push(metadata_value_from_row(row)?);
        }
        Ok(values)
    }
}

/// The stored text of a value: genres as a JSON array, a Steam Deck rating as a JSON
/// object `{"rating": "verified", "tested_at": 1700000000}`, other fields as they are.
fn value_text(value: &FieldValue) -> String {
    match value {
        FieldValue::Developer(text) | FieldValue::Description(text) => text.clone(),
        FieldValue::Genres(genres) => to_json(genres),
        FieldValue::DeckCompatibility(deck) => serde_json::json!({
            "rating": deck.rating.as_str(),
            "tested_at": deck.tested_at,
        })
        .to_string(),
    }
}

fn metadata_value_from_row(row: &Row<'_>) -> rusqlite::Result<MetadataValue> {
    let text: String = row.get("value")?;
    let value = match row.get("field")? {
        MetadataField::Developer => FieldValue::Developer(text),
        MetadataField::Description => FieldValue::Description(text),
        MetadataField::Genres => {
            FieldValue::Genres(serde_json::from_str(&text).map_err(conversion_failure)?)
        }
        MetadataField::DeckCompatibility => {
            FieldValue::DeckCompatibility(deck_compatibility(&text).map_err(conversion_failure)?)
        }
    };
    Ok(MetadataValue {
        value,
        origin: row.get("origin")?,
        updated_at: row.get("updated_at")?,
    })
}

/// A Steam Deck rating from its stored JSON object.
fn deck_compatibility(text: &str) -> std::result::Result<DeckCompatibility, BoxedError> {
    let json: serde_json::Value = serde_json::from_str(text)?;
    Ok(DeckCompatibility {
        rating: json["rating"].as_str().unwrap_or_default().parse()?,
        tested_at: json["tested_at"].as_i64(),
    })
}

type BoxedError = Box<dyn Error + Send + Sync>;

fn conversion_failure(error: impl Into<BoxedError>) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
}
