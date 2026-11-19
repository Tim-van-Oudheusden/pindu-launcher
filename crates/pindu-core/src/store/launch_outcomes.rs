use rusqlite::{Row, params};

use super::{Result, Store, StoreError};
use crate::launch_outcome::{Ending, KEEP, LaunchOutcome, NewLaunch};
use crate::model::{GameId, Timestamp};

impl Store {
    /// Records that a game's launch began, as [`Ending::Pending`], and drops the game's
    /// outcomes beyond the newest [`KEEP`]. Returns the record's ID, which
    /// [`Self::end_launch`] takes.
    pub fn begin_launch(&self, game_id: &GameId, launch: &NewLaunch<'_>) -> Result<i64> {
        self.in_transaction(|store| {
            store.conn.execute(
                "INSERT INTO launch_outcomes (game_id, route, proton, started_at, ending, log)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    game_id,
                    launch.route,
                    launch.proton,
                    launch.started_at,
                    Ending::Pending.as_str(),
                    launch.log,
                ],
            )?;
            let id = store.conn.last_insert_rowid();
            store.conn.execute(
                "DELETE FROM launch_outcomes WHERE game_id = ?1 AND id NOT IN (
                     SELECT id FROM launch_outcomes WHERE game_id = ?1
                     ORDER BY started_at DESC, id DESC LIMIT ?2)",
                params![game_id, KEEP as i64],
            )?;
            Ok(id)
        })
    }

    /// Records how the launch with `id` ended, at `ended_at`, and whether a window of the
    /// game appeared where the session can tell. Only a pending launch ends, or one whose
    /// launcher never answered and then does after all; any other is left as it is and
    /// fails with [`StoreError::NotFound`], as does a launch no longer kept.
    pub fn end_launch(
        &self,
        id: i64,
        ending: Ending,
        ended_at: Timestamp,
        window_seen: Option<bool>,
    ) -> Result<()> {
        let changed = self.conn.execute(
            "UPDATE launch_outcomes SET ending = ?2, code = ?3, ended_at = ?4, window_seen = ?5
             WHERE id = ?1 AND ending IN (?6, ?7)",
            params![
                id,
                ending.as_str(),
                ending.code(),
                ended_at,
                window_seen,
                Ending::Pending.as_str(),
                Ending::NoAnswer.as_str(),
            ],
        )?;
        if changed == 0 {
            return Err(StoreError::NotFound);
        }
        Ok(())
    }

    /// A game's kept outcomes, newest first.
    pub fn launch_outcomes(&self, game_id: &GameId) -> Result<Vec<LaunchOutcome>> {
        let mut statement = self.conn.prepare(
            "SELECT id, game_id, route, proton, started_at, ended_at, ending, code,
                    window_seen, log
             FROM launch_outcomes WHERE game_id = ?1 ORDER BY started_at DESC, id DESC",
        )?;
        let outcomes = statement
            .query_map([game_id], outcome_from_row)?
            .collect::<rusqlite::Result<_>>()?;
        Ok(outcomes)
    }
}

fn outcome_from_row(row: &Row<'_>) -> rusqlite::Result<LaunchOutcome> {
    let kind: String = row.get("ending")?;
    let ending = Ending::parse(&kind, row.get("code")?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(LaunchOutcome {
        id: row.get("id")?,
        game_id: row.get("game_id")?,
        route: row.get("route")?,
        proton: row.get("proton")?,
        started_at: row.get("started_at")?,
        ended_at: row.get("ended_at")?,
        ending,
        window_seen: row.get("window_seen")?,
        log: row.get("log")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, LaunchKind, LaunchRoute, Source};

    fn store_with_game() -> (tempfile::TempDir, Store, GameId) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let game = Game::new(
            GameId::new(Source::Manual, "celeste"),
            "Celeste",
            LaunchKind::Proton,
            0,
        );
        store.insert_game(&game).unwrap();
        (dir, store, game.id)
    }

    fn launch(route: LaunchRoute, started_at: Timestamp) -> NewLaunch<'static> {
        NewLaunch {
            route,
            proton: None,
            log: None,
            started_at,
        }
    }

    #[test]
    fn a_clean_exit_is_kept_with_its_route_build_times_status_and_log() {
        let (_dir, store, id) = store_with_game();
        let row = store
            .begin_launch(
                &id,
                &NewLaunch {
                    route: LaunchRoute::Proton,
                    proton: Some("GE-Proton10-25"),
                    log: Some("2026-11-19T20-00-00.short.log"),
                    started_at: 1_000,
                },
            )
            .unwrap();
        store
            .end_launch(row, Ending::Exited(0), 4_600, Some(true))
            .unwrap();

        let outcomes = store.launch_outcomes(&id).unwrap();

        assert_eq!(
            outcomes,
            [LaunchOutcome {
                id: row,
                game_id: id,
                route: LaunchRoute::Proton,
                proton: Some("GE-Proton10-25".to_owned()),
                started_at: 1_000,
                ended_at: Some(4_600),
                ending: Ending::Exited(0),
                window_seen: Some(true),
                log: Some("2026-11-19T20-00-00.short.log".to_owned()),
            }]
        );
        assert!(!outcomes[0].ending.failed());
    }

    #[test]
    fn a_failed_start_is_kept_as_a_failure_where_no_window_could_be_seen() {
        let (_dir, store, id) = store_with_game();
        let row = store
            .begin_launch(&id, &launch(LaunchRoute::Host, 1_000))
            .unwrap();
        store
            .end_launch(row, Ending::NotStarted, 1_001, None)
            .unwrap();

        let outcomes = store.launch_outcomes(&id).unwrap();

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].ending, Ending::NotStarted);
        assert_eq!(outcomes[0].ended_at, Some(1_001));
        assert_eq!(outcomes[0].window_seen, None);
        assert!(outcomes[0].ending.failed());
    }

    #[test]
    fn a_hand_off_the_launcher_never_answers_stays_pending_until_it_times_out() {
        let (_dir, store, id) = store_with_game();
        let row = store
            .begin_launch(&id, &launch(LaunchRoute::HandOff, 1_000))
            .unwrap();

        let waiting = store.launch_outcomes(&id).unwrap();
        assert_eq!(waiting[0].ending, Ending::Pending);
        assert_eq!(waiting[0].ended_at, None);

        store
            .end_launch(row, Ending::NoAnswer, 1_010, None)
            .unwrap();

        let outcomes = store.launch_outcomes(&id).unwrap();
        assert_eq!(outcomes[0].ending, Ending::NoAnswer);
        assert_eq!(outcomes[0].ended_at, Some(1_010));
        assert!(outcomes[0].ending.failed());
    }

    #[test]
    fn a_launcher_that_answers_after_the_timeout_still_counts_as_answering() {
        let (_dir, store, id) = store_with_game();
        let row = store
            .begin_launch(&id, &launch(LaunchRoute::HandOff, 1_000))
            .unwrap();
        store
            .end_launch(row, Ending::NoAnswer, 1_010, None)
            .unwrap();

        store
            .end_launch(row, Ending::HandedOff, 1_030, None)
            .unwrap();

        let outcome = &store.launch_outcomes(&id).unwrap()[0];
        assert_eq!(outcome.ending, Ending::HandedOff);
        assert_eq!(outcome.ended_at, Some(1_030));
    }

    #[test]
    fn a_launch_that_ended_is_not_ended_again() {
        let (_dir, store, id) = store_with_game();
        let row = store
            .begin_launch(&id, &launch(LaunchRoute::Host, 1_000))
            .unwrap();
        store
            .end_launch(row, Ending::Exited(0), 2_000, None)
            .unwrap();

        let again = store.end_launch(row, Ending::Lost, 3_000, None);

        assert!(matches!(again, Err(StoreError::NotFound)));
        assert_eq!(
            store.launch_outcomes(&id).unwrap()[0].ending,
            Ending::Exited(0)
        );
    }

    #[test]
    fn only_the_newest_outcomes_of_a_game_are_kept_newest_first() {
        let (_dir, store, id) = store_with_game();
        let other = Game::new(
            GameId::new(Source::Manual, "hades"),
            "Hades",
            LaunchKind::HostCommand,
            0,
        );
        store.insert_game(&other).unwrap();
        store
            .begin_launch(&other.id, &launch(LaunchRoute::Host, 1))
            .unwrap();

        let rows: Vec<i64> = (0..KEEP as i64 + 2)
            .map(|n| {
                store
                    .begin_launch(&id, &launch(LaunchRoute::Host, 100 + n))
                    .unwrap()
            })
            .collect();

        let kept: Vec<i64> = store
            .launch_outcomes(&id)
            .unwrap()
            .iter()
            .map(|outcome| outcome.id)
            .collect();
        let newest: Vec<i64> = rows.iter().rev().take(KEEP).copied().collect();
        assert_eq!(kept, newest);
        assert_eq!(store.launch_outcomes(&other.id).unwrap().len(), 1);
    }

    #[test]
    fn a_games_outcomes_go_with_it() {
        let (_dir, store, id) = store_with_game();
        store
            .begin_launch(&id, &launch(LaunchRoute::Host, 1))
            .unwrap();

        store.delete_game(&id).unwrap();

        assert!(store.launch_outcomes(&id).unwrap().is_empty());
    }
}
