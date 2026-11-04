use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use super::{Result, Store, StoreError};
use crate::model::{GameId, ImportedPlaytime, PlaySession, Timestamp};

impl Store {
    /// Records a session of a game we started; its duration is `ended_at - started_at`.
    pub fn add_play_session(
        &self,
        game_id: &GameId,
        started_at: Timestamp,
        ended_at: Timestamp,
    ) -> Result<PlaySession> {
        let duration_secs = ended_at - started_at;
        self.conn.execute(
            "INSERT INTO play_sessions (game_id, started_at, ended_at, duration_secs)
             VALUES (?1, ?2, ?3, ?4)",
            params![game_id, started_at, ended_at, duration_secs],
        )?;
        Ok(PlaySession {
            id: self.conn.last_insert_rowid(),
            game_id: game_id.clone(),
            started_at,
            ended_at,
            duration_secs,
        })
    }

    /// Moves the end of a running session forward, as the tracker does every 30 s.
    pub fn update_play_session_end(&self, id: i64, ended_at: Timestamp) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE play_sessions SET ended_at = ?2, duration_secs = ?2 - started_at WHERE id = ?1",
            params![id, ended_at],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// A game's sessions, oldest first.
    pub fn play_sessions(&self, game_id: &GameId) -> Result<Vec<PlaySession>> {
        let mut statement = self.conn.prepare(
            "SELECT id, game_id, started_at, ended_at, duration_secs FROM play_sessions
             WHERE game_id = ?1 ORDER BY started_at, id",
        )?;
        let sessions = statement
            .query_map([game_id], |row| {
                Ok(PlaySession {
                    id: row.get("id")?,
                    game_id: row.get("game_id")?,
                    started_at: row.get("started_at")?,
                    ended_at: row.get("ended_at")?,
                    duration_secs: row.get("duration_secs")?,
                })
            })?
            .collect::<rusqlite::Result<_>>()?;
        Ok(sessions)
    }

    /// Stores a source's playtime totals for a game, replacing earlier ones.
    pub fn set_imported_playtime(&self, playtime: &ImportedPlaytime) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO imported_playtime (game_id, source, total_secs, last_played_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                playtime.game_id,
                playtime.source,
                playtime.total_secs,
                playtime.last_played_at,
            ],
        )?;
        Ok(())
    }

    pub fn imported_playtime(&self, game_id: &GameId) -> Result<Option<ImportedPlaytime>> {
        Ok(self
            .conn
            .query_row(
                "SELECT game_id, source, total_secs, last_played_at FROM imported_playtime
                 WHERE game_id = ?1",
                [game_id],
                |row| {
                    Ok(ImportedPlaytime {
                        game_id: row.get("game_id")?,
                        source: row.get("source")?,
                        total_secs: row.get("total_secs")?,
                        last_played_at: row.get("last_played_at")?,
                    })
                },
            )
            .optional()?)
    }
}

impl Store {
    /// Total seconds of our own sessions per game.
    pub fn session_totals(&self) -> Result<HashMap<GameId, i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT game_id, SUM(duration_secs) FROM play_sessions GROUP BY game_id")?;
        let totals = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(totals)
    }

    /// Imported playtime totals per game.
    pub fn imported_totals(&self) -> Result<HashMap<GameId, i64>> {
        let mut statement = self
            .conn
            .prepare("SELECT game_id, total_secs FROM imported_playtime")?;
        let totals = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(totals)
    }
}
