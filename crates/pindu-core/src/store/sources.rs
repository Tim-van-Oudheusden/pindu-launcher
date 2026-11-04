use rusqlite::{OptionalExtension, Row, named_params};

use super::{Result, Store, json_column, path_column, path_to_sql, to_json};
use crate::model::{Source, SourceState};

const COLUMNS: &str = "source, enabled, resolved_location, user_override, last_sync, errors";

impl Store {
    /// Stores what we know about a source, replacing the earlier state.
    pub fn set_source_state(&self, state: &SourceState) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO source_states (source, enabled, resolved_location,
                user_override, last_sync, errors)
             VALUES (:source, :enabled, :resolved_location, :user_override, :last_sync, :errors)",
            named_params! {
                ":source": state.source,
                ":enabled": state.enabled,
                ":resolved_location": state.resolved_location.as_deref().map(path_to_sql).transpose()?,
                ":user_override": state.user_override.as_deref().map(path_to_sql).transpose()?,
                ":last_sync": state.last_sync,
                ":errors": to_json(&state.errors),
            },
        )?;
        Ok(())
    }

    pub fn source_state(&self, source: Source) -> Result<Option<SourceState>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM source_states WHERE source = ?1"),
                [source],
                source_state_from_row,
            )
            .optional()?)
    }

    /// Every stored source state, ordered by source.
    pub fn source_states(&self) -> Result<Vec<SourceState>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM source_states ORDER BY source"
        ))?;
        let states = statement
            .query_map([], source_state_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(states)
    }
}

fn source_state_from_row(row: &Row<'_>) -> rusqlite::Result<SourceState> {
    Ok(SourceState {
        source: row.get("source")?,
        enabled: row.get("enabled")?,
        resolved_location: path_column(row, "resolved_location")?,
        user_override: path_column(row, "user_override")?,
        last_sync: row.get("last_sync")?,
        errors: json_column(row, "errors")?,
    })
}
