use std::collections::HashMap;

use rusqlite::{OptionalExtension, params};

use super::{Result, Store, StoreError};
use crate::model::{GameGroup, GameId, SourceDetails};
use crate::sources::ServiceRef;

impl Store {
    /// Groups games the user says are copies of one game; returns the new group's ID.
    ///
    /// Needs two or more different stored games, none of them in a group yet. A remembered
    /// split between two of them does not stop the user.
    pub fn create_group(&self, games: &[GameId]) -> Result<i64> {
        self.insert_group(games, false)
    }

    /// Groups two copies an exact store cross-reference links, unless either is in a group
    /// already or the pair is remembered as not the same game. Returns the new group's ID,
    /// or `None` when no group was made.
    pub fn create_automatic_group(&self, first: &GameId, second: &GameId) -> Result<Option<i64>> {
        self.in_transaction(|store| {
            if store.group_id_of(first)?.is_some()
                || store.group_id_of(second)?.is_some()
                || store.is_split(first, second)?
            {
                return Ok(None);
            }
            store
                .insert_group(&[first.clone(), second.clone()], true)
                .map(Some)
        })
    }

    fn insert_group(&self, games: &[GameId], automatic: bool) -> Result<i64> {
        let mut games = games.to_vec();
        games.sort_unstable();
        games.dedup();
        if games.len() < 2 {
            return Err(StoreError::TooFewCopies);
        }
        self.in_transaction(|store| {
            for game in &games {
                store.check_ungrouped(game)?;
            }
            store.conn.execute(
                "INSERT INTO game_groups (automatic) VALUES (?1)",
                [automatic],
            )?;
            let id = store.conn.last_insert_rowid();
            for game in &games {
                store.insert_member(id, game)?;
            }
            Ok(id)
        })
    }

    /// Adds a stored game in no group to a group; the group becomes the user's own.
    pub fn add_to_group(&self, group_id: i64, game: &GameId) -> Result<()> {
        self.in_transaction(|store| {
            store.game_group(group_id)?.ok_or(StoreError::NotFound)?;
            store.check_ungrouped(game)?;
            store.insert_member(group_id, game)?;
            store.mark_by_hand(group_id)
        })
    }

    /// Takes a game out of its group; the group becomes the user's own. A group left with
    /// one game is removed, and a game in no group changes nothing.
    pub fn remove_from_group(&self, game: &GameId) -> Result<()> {
        self.in_transaction(|store| {
            let Some(group_id) = store.group_id_of(game)? else {
                return Ok(());
            };
            store
                .conn
                .execute("DELETE FROM game_group_members WHERE game_id = ?1", [game])?;
            store.conn.execute(
                "UPDATE game_groups SET preferred_game_id = NULL
                 WHERE id = ?1 AND preferred_game_id = ?2",
                params![group_id, game],
            )?;
            store.mark_by_hand(group_id)?;
            store.drop_small_groups()
        })
    }

    /// Removes a group; its games show on their own again exactly as before. An unknown
    /// group changes nothing.
    pub fn ungroup(&self, group_id: i64) -> Result<()> {
        self.conn
            .execute("DELETE FROM game_groups WHERE id = ?1", [group_id])?;
        Ok(())
    }

    /// Removes a group and remembers every pair of its games as not the same game, so
    /// neither the automatic grouping nor a suggestion brings it back. This is how a split
    /// by hand and the undo of an automatic group end.
    pub fn split_group(&self, group_id: i64) -> Result<()> {
        self.in_transaction(|store| {
            let group = store.game_group(group_id)?.ok_or(StoreError::NotFound)?;
            for (index, first) in group.members.iter().enumerate() {
                for second in &group.members[index + 1..] {
                    store.remember_split(first, second)?;
                }
            }
            store.ungroup(group_id)
        })
    }

    /// Stores the copy the user chose for a group, one of its games; `None` goes back to
    /// the default order.
    pub fn set_preferred_copy(&self, group_id: i64, game: Option<&GameId>) -> Result<()> {
        self.in_transaction(|store| {
            let group = store.game_group(group_id)?.ok_or(StoreError::NotFound)?;
            if let Some(game) = game
                && !group.members.contains(game)
            {
                return Err(StoreError::NotInGroup(game.clone()));
            }
            store.conn.execute(
                "UPDATE game_groups SET preferred_game_id = ?2 WHERE id = ?1",
                params![group_id, game],
            )?;
            Ok(())
        })
    }

    /// Every group, ordered by ID.
    pub fn game_groups(&self) -> Result<Vec<GameGroup>> {
        self.query_groups("", [])
    }

    /// The group with this ID, or `None` when there is none.
    pub fn game_group(&self, group_id: i64) -> Result<Option<GameGroup>> {
        Ok(self.query_groups("WHERE g.id = ?1", [group_id])?.pop())
    }

    /// The group holding a game, or `None` when the game is in no group.
    pub fn game_group_of(&self, game: &GameId) -> Result<Option<GameGroup>> {
        match self.group_id_of(game)? {
            Some(group_id) => self.game_group(group_id),
            None => Ok(None),
        }
    }

    fn query_groups(&self, filter: &str, params: impl rusqlite::Params) -> Result<Vec<GameGroup>> {
        let mut statement = self.conn.prepare(&format!(
            "SELECT g.id, g.preferred_game_id, g.automatic, m.game_id
             FROM game_groups g JOIN game_group_members m ON m.group_id = g.id
             {filter} ORDER BY g.id, m.game_id"
        ))?;
        let mut groups: Vec<GameGroup> = Vec::new();
        let mut rows = statement.query(params)?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let member: GameId = row.get(3)?;
            match groups.last_mut() {
                Some(group) if group.id == id => group.members.push(member),
                _ => groups.push(GameGroup {
                    id,
                    members: vec![member],
                    preferred: row.get(1)?,
                    automatic: row.get(2)?,
                }),
            }
        }
        Ok(groups)
    }

    fn group_id_of(&self, game: &GameId) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row(
                "SELECT group_id FROM game_group_members WHERE game_id = ?1",
                [game],
                |row| row.get(0),
            )
            .optional()?)
    }

    /// Fails unless the game is stored and in no group.
    fn check_ungrouped(&self, game: &GameId) -> Result<()> {
        if self.game(game)?.is_none() {
            return Err(StoreError::NotFound);
        }
        if self.group_id_of(game)?.is_some() {
            return Err(StoreError::AlreadyGrouped(game.clone()));
        }
        Ok(())
    }

    fn insert_member(&self, group_id: i64, game: &GameId) -> Result<()> {
        self.conn.execute(
            "INSERT INTO game_group_members (game_id, group_id) VALUES (?1, ?2)",
            params![game, group_id],
        )?;
        Ok(())
    }

    fn mark_by_hand(&self, group_id: i64) -> Result<()> {
        self.conn.execute(
            "UPDATE game_groups SET automatic = 0 WHERE id = ?1",
            [group_id],
        )?;
        Ok(())
    }

    /// Removes the groups that hold fewer than two games.
    pub(super) fn drop_small_groups(&self) -> Result<()> {
        self.conn.execute(
            "DELETE FROM game_groups WHERE id NOT IN (
                SELECT group_id FROM game_group_members GROUP BY group_id HAVING COUNT(*) >= 2)",
            [],
        )?;
        Ok(())
    }

    /// Remembers that two games are not the same game; remembering a pair again changes
    /// nothing. Groups holding both stay as they are.
    pub fn remember_split(&self, first: &GameId, second: &GameId) -> Result<()> {
        let (first, second) = ordered(first, second);
        self.conn.execute(
            "INSERT OR IGNORE INTO game_splits (first_game_id, second_game_id) VALUES (?1, ?2)",
            params![first, second],
        )?;
        Ok(())
    }

    /// Whether two games are remembered as not the same game, in either order.
    pub fn is_split(&self, first: &GameId, second: &GameId) -> Result<bool> {
        let (first, second) = ordered(first, second);
        Ok(self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM game_splits
                WHERE first_game_id = ?1 AND second_game_id = ?2)",
            params![first, second],
            |row| row.get(0),
        )?)
    }

    /// Every pair remembered as not the same game, the lower ID first, ordered.
    pub fn splits(&self) -> Result<Vec<(GameId, GameId)>> {
        let mut statement = self.conn.prepare(
            "SELECT first_game_id, second_game_id FROM game_splits
             ORDER BY first_game_id, second_game_id",
        )?;
        let splits = statement
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(splits)
    }

    /// Stores what a source says about a game beyond its own fields, replacing what was
    /// there; details with nothing in them remove the row.
    pub fn set_source_details(&self, game: &GameId, details: &SourceDetails) -> Result<()> {
        if *details == SourceDetails::default() {
            self.conn
                .execute("DELETE FROM source_details WHERE game_id = ?1", [game])?;
            return Ok(());
        }
        let service = details.service.as_ref();
        self.conn.execute(
            "INSERT OR REPLACE INTO source_details (game_id, runner, service, service_id, year)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                game,
                details.runner,
                service.map(|service| &service.service),
                service.map(|service| &service.service_id),
                details.year,
            ],
        )?;
        Ok(())
    }

    /// What a source says about a game beyond its own fields; empty when nothing is stored.
    pub fn source_details(&self, game: &GameId) -> Result<SourceDetails> {
        Ok(self
            .conn
            .query_row(
                "SELECT runner, service, service_id, year FROM source_details WHERE game_id = ?1",
                [game],
                details_from_row,
            )
            .optional()?
            .unwrap_or_default())
    }

    /// The stored source details of every game that has any.
    pub fn all_source_details(&self) -> Result<HashMap<GameId, SourceDetails>> {
        let mut statement = self
            .conn
            .prepare("SELECT game_id, runner, service, service_id, year FROM source_details")?;
        let details = statement
            .query_map([], |row| Ok((row.get("game_id")?, details_from_row(row)?)))?
            .collect::<rusqlite::Result<_>>()?;
        Ok(details)
    }
}

fn ordered<'a>(first: &'a GameId, second: &'a GameId) -> (&'a GameId, &'a GameId) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

fn details_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SourceDetails> {
    let service: Option<String> = row.get("service")?;
    let service_id: Option<String> = row.get("service_id")?;
    Ok(SourceDetails {
        runner: row.get("runner")?,
        service: service
            .zip(service_id)
            .map(|(service, service_id)| ServiceRef {
                service,
                service_id,
            }),
        year: row.get("year")?,
    })
}
