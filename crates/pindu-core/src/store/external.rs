use rusqlite::{OptionalExtension, Row, named_params};

use super::{Result, Store, json_column, path_column, path_to_sql, to_json};
use crate::model::ExternalImporterState;

const COLUMNS: &str = "helper_id, enabled, root, watch, last_sync, errors";

impl Store {
    /// Stores what we know about an external importer, replacing the earlier state.
    pub fn set_external_importer(&self, state: &ExternalImporterState) -> Result<()> {
        let watch = state
            .watch
            .iter()
            .map(|path| path_to_sql(path))
            .collect::<Result<Vec<_>>>()?;
        self.conn.execute(
            "INSERT OR REPLACE INTO external_importers (helper_id, enabled, root, watch,
                last_sync, errors)
             VALUES (:helper_id, :enabled, :root, :watch, :last_sync, :errors)",
            named_params! {
                ":helper_id": state.helper_id,
                ":enabled": state.enabled,
                ":root": state.root.as_deref().map(path_to_sql).transpose()?,
                ":watch": to_json(&watch),
                ":last_sync": state.last_sync,
                ":errors": to_json(&state.errors),
            },
        )?;
        Ok(())
    }

    /// What we know about the external importer `helper_id`, or `None` when nothing is
    /// stored for it yet.
    pub fn external_importer(&self, helper_id: &str) -> Result<Option<ExternalImporterState>> {
        Ok(self
            .conn
            .query_row(
                &format!("SELECT {COLUMNS} FROM external_importers WHERE helper_id = ?1"),
                [helper_id],
                external_importer_from_row,
            )
            .optional()?)
    }

    /// Every stored external importer state, ordered by helper ID.
    pub fn external_importers(&self) -> Result<Vec<ExternalImporterState>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT {COLUMNS} FROM external_importers ORDER BY helper_id"
        ))?;
        let states = statement
            .query_map([], external_importer_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(states)
    }

    /// Forgets the external importer `helper_id`: its state, not its games.
    pub fn delete_external_importer(&self, helper_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM external_importers WHERE helper_id = ?1",
            [helper_id],
        )?;
        Ok(())
    }
}

fn external_importer_from_row(row: &Row<'_>) -> rusqlite::Result<ExternalImporterState> {
    let watch: Vec<String> = json_column(row, "watch")?;
    Ok(ExternalImporterState {
        helper_id: row.get("helper_id")?,
        enabled: row.get("enabled")?,
        root: path_column(row, "root")?,
        watch: watch.into_iter().map(Into::into).collect(),
        last_sync: row.get("last_sync")?,
        errors: json_column(row, "errors")?,
    })
}
