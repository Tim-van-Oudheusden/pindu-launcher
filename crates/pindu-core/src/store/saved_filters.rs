use std::collections::HashMap;

use rusqlite::params;

use super::{Result, Store};
use crate::library::Filters;
use crate::model::Collection;
use crate::saved_filters::{self, SavedFilter};

impl Store {
    /// Creates a saved filter after every existing collection: a collection whose games
    /// `filter` picks.
    pub fn create_saved_filter(&self, name: &str, filter: &SavedFilter) -> Result<Collection> {
        self.in_transaction(|store| {
            let collection = store.create_collection(name)?;
            let toggles = &filter.filters;
            store.conn.execute(
                "INSERT INTO saved_filters
                     (collection_id, view, installed, not_installed, favourites, windows,
                      native, never_played, genre, search)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    collection.id,
                    saved_filters::view_text(filter.view),
                    toggles.installed,
                    toggles.not_installed,
                    toggles.favourites,
                    toggles.windows,
                    toggles.native,
                    toggles.never_played,
                    toggles.genre,
                    filter.search,
                ],
            )?;
            Ok(collection)
        })
    }

    /// The filter of every saved filter, by collection ID. One with a view this version
    /// does not know is left out, and so shows as a plain collection.
    pub fn saved_filters(&self) -> Result<HashMap<i64, SavedFilter>> {
        let mut statement = self.conn.prepare(
            "SELECT collection_id, view, installed, not_installed, favourites, windows, native,
                    never_played, genre, search
             FROM saved_filters",
        )?;
        let rows = statement.query_map([], |row| {
            let view: String = row.get("view")?;
            let filters = Filters {
                installed: row.get("installed")?,
                not_installed: row.get("not_installed")?,
                favourites: row.get("favourites")?,
                windows: row.get("windows")?,
                native: row.get("native")?,
                never_played: row.get("never_played")?,
                genre: row.get("genre")?,
                hidden: false,
            };
            Ok((
                row.get::<_, i64>("collection_id")?,
                view,
                filters,
                row.get::<_, String>("search")?,
            ))
        })?;
        let mut saved = HashMap::new();
        for row in rows {
            let (id, view, filters, search) = row?;
            if let Some(view) = saved_filters::view_from_text(&view) {
                saved.insert(
                    id,
                    SavedFilter {
                        view,
                        filters,
                        search,
                    },
                );
            }
        }
        Ok(saved)
    }
}
