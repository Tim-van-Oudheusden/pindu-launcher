//! Records how each launch of a game ended (see `pindu_core::launch_outcome`): a launch is
//! recorded as it begins and again when it ends. A game Pindu runs ends when its process
//! exits, and in a `--steam` gamescope its outcome says whether a window of it appeared. A
//! store game handed to its launcher ends when the launcher answers, or as having no
//! answer after the launch overlay's hand-off timeout ([`HAND_OFF_TIMEOUT`]); a launcher
//! that answers later still counts as answering. [`last_failure`] reads the last outcome
//! back to explain a failed launch, and the application's `launch-recorded` signal says
//! when there is a new one.

use std::cell::Cell;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::failed_launch::{self, Explanation, Facts};
use pindu_core::launch_outcome::{Ending, NewLaunch};
use pindu_core::launch_overlay::HAND_OFF_TIMEOUT;
use pindu_core::model::{Game, GameId, LaunchRoute};
use pindu_core::paths::default_library_db_path;
use pindu_core::store::Store;

use crate::application::PinduApplication;
use crate::gamescope_service::GamescopeService;
use crate::log_service::{self, LaunchLog};
use crate::permission_service;
use crate::proton_launch;
use crate::session_service;

/// One launch as it is recorded.
#[derive(Clone)]
pub struct Recording {
    inner: Rc<Inner>,
}

struct Inner {
    game: GameId,
    route: LaunchRoute,
    /// The launch's record; `None` when it could not be written.
    row: Option<i64>,
    /// Where a window of the game can be seen, in a `--steam` gamescope.
    gamescope: Option<GamescopeService>,
    /// How it ended, once it has.
    ending: Cell<Option<Ending>>,
}

impl std::fmt::Debug for Recording {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Recording")
            .field("game", &self.inner.game)
            .field("row", &self.inner.row)
            .finish_non_exhaustive()
    }
}

/// Records that the game with `id` is launched by `route`, with the Proton build named
/// `proton` and the log named `log`. A launch Pindu runs starts noting whether a window of
/// it appears; a hand-off ends as having no answer after [`HAND_OFF_TIMEOUT`] unless the
/// launcher answers first.
pub async fn begin(
    app: &gtk::Application,
    id: &GameId,
    route: LaunchRoute,
    proton: Option<String>,
    log: Option<String>,
) -> Recording {
    let gamescope = app
        .downcast_ref::<PinduApplication>()
        .map(|application| application.gamescope_service().clone());
    if route != LaunchRoute::HandOff
        && let Some(gamescope) = &gamescope
    {
        gamescope.watch_windows(id);
    }
    let started_at = session_service::now();
    let row = write(id, move |store, id| {
        store.begin_launch(
            id,
            &NewLaunch {
                route,
                proton: proton.as_deref(),
                log: log.as_deref(),
                started_at,
            },
        )
    })
    .await;
    recorded(id);
    let recording = Recording {
        inner: Rc::new(Inner {
            game: id.clone(),
            route,
            row,
            gamescope,
            ending: Cell::new(None),
        }),
    };
    if route == LaunchRoute::HandOff {
        let waiting = recording.clone();
        glib::timeout_add_local_once(HAND_OFF_TIMEOUT, move || {
            if waiting.inner.ending.get().is_none() {
                glib::spawn_future_local(async move { waiting.end(Ending::NoAnswer).await });
            }
        });
    }
    recording
}

/// Records a launch of the game with `id` by `route`, with the Proton build named `proton`,
/// that never started, keeping the app up until it is written.
pub fn not_started(
    app: &gtk::Application,
    id: &GameId,
    route: LaunchRoute,
    proton: Option<String>,
) {
    let (app, id, hold) = (app.clone(), id.clone(), app.hold());
    glib::spawn_future_local(async move {
        begin(&app, &id, route, proton, None)
            .await
            .end(Ending::NotStarted)
            .await;
        drop(hold);
    });
}

impl Recording {
    /// Records how the launch ended. Only the first ending counts, except that a launcher
    /// that answers after it was given up on still counts as answering.
    pub async fn end(&self, ending: Ending) {
        let inner = &self.inner;
        let ends = match inner.ending.get() {
            None => true,
            Some(Ending::NoAnswer) => matches!(ending, Ending::HandedOff | Ending::HandOffFailed),
            Some(_) => false,
        };
        if !ends {
            return;
        }
        inner.ending.set(Some(ending));
        let window_seen = match (&inner.gamescope, inner.route) {
            (_, LaunchRoute::HandOff) | (None, _) => None,
            (Some(gamescope), _) => gamescope.window_seen(&inner.game),
        };
        let Some(row) = inner.row else { return };
        let ended_at = session_service::now();
        write(&inner.game, move |store, _| {
            store.end_launch(row, ending, ended_at, window_seen)
        })
        .await;
        recorded(&inner.game);
    }

    /// Follows a game Pindu runs as `child` until it exits, then records how it ended and
    /// ends its `log`, keeping `hold` on the app until both are written. A short log reads
    /// the game's output from `child`'s pipe as it comes, passing it on to ours.
    pub fn follow(
        self,
        child: gio::Subprocess,
        mut log: Option<LaunchLog>,
        hold: Option<gio::ApplicationHoldGuard>,
    ) {
        glib::spawn_future_local(async move {
            if let Some(log) = log.as_mut().filter(|log| log.reads_pipe())
                && let Some(pipe) = child.stdout_pipe()
            {
                let output = gio::DataInputStream::new(&pipe);
                loop {
                    match output.read_line_future(glib::Priority::DEFAULT).await {
                        Ok(Some(line)) => {
                            let line = String::from_utf8_lossy(&line);
                            // The game's output goes on to ours, as it did without a log.
                            eprintln!("{line}");
                            log.line(&line);
                        }
                        Ok(None) => break,
                        Err(error) => {
                            let game = &self.inner.game;
                            glib::g_warning!("pindu", "Lost the output of {game}: {error}");
                            break;
                        }
                    }
                }
            }
            let waited = child.wait_future().await;
            self.end(ending(&child, waited)).await;
            if let Some(log) = log {
                finish(log, &self.inner.game).await;
            }
            drop(hold);
        });
    }
}

/// How a child Pindu waited for ended.
pub fn ending(child: &gio::Subprocess, waited: Result<(), glib::Error>) -> Ending {
    match waited {
        Err(_) => Ending::Lost,
        Ok(()) if child.has_signaled() => Ending::Signalled(child.term_sig()),
        Ok(()) => Ending::Exited(child.exit_status()),
    }
}

/// Ends the log of a launch of the game with `id` on a worker thread.
pub async fn finish(log: LaunchLog, id: &GameId) {
    match gio::spawn_blocking(move || log.finish()).await {
        Ok(Ok(())) => {}
        Ok(Err(error)) => glib::g_warning!("pindu", "Could not end the log of {id}: {error}"),
        Err(_) => glib::g_warning!("pindu", "Could not end the log of {id}"),
    }
}

/// Tells the shells that a launch of the game with `id` was recorded, as it began or
/// ended, so a page explaining its last failure reads it again.
fn recorded(id: &GameId) {
    if let Some(application) = gio::Application::default().and_downcast::<PinduApplication>() {
        application.emit_launch_recorded(id);
    }
}

/// Why the last launch of `game` failed and the next steps that exist for it (see
/// `pindu_core::failed_launch`), read on a worker thread with what is known now; `None`
/// when it did not fail, has no outcome, or the outcome could not be read.
pub async fn last_failure(game: &Game) -> Option<Explanation> {
    let game = game.clone();
    let read = gio::spawn_blocking(move || -> pindu_core::store::Result<Option<Explanation>> {
        let store = Store::open(&default_library_db_path())?;
        let Some(last) = store.launch_outcomes(&game.id)?.into_iter().next() else {
            return Ok(None);
        };
        let builds: Vec<String> = if last.route == LaunchRoute::Proton {
            proton_launch::builds()
                .into_iter()
                .map(|build| build.name)
                .collect()
        } else {
            Vec::new()
        };
        let log_kept = last
            .log
            .as_deref()
            .is_some_and(|name| log_service::launch_log(&game.id, name).is_some());
        let facts = Facts {
            needs_permission: permission_service::needs_permission(&game),
            umu_installed: proton_launch::umu_installed(),
            proton_builds: &builds,
            log_kept,
        };
        Ok(failed_launch::explain(&game, &last, &facts))
    })
    .await;
    match read {
        Ok(Ok(explanation)) => explanation,
        Ok(Err(error)) => {
            glib::g_warning!("pindu", "Could not read how a launch ended: {error}");
            None
        }
        Err(_) => None,
    }
}

/// Runs a store write about the game with `id` on a worker thread; failures are logged,
/// since a lost outcome only costs the explanation of a failure.
async fn write<T: Send + 'static>(
    id: &GameId,
    work: impl FnOnce(&Store, &GameId) -> pindu_core::store::Result<T> + Send + 'static,
) -> Option<T> {
    let id = id.clone();
    let written = gio::spawn_blocking(move || {
        let store = Store::open(&default_library_db_path())?;
        work(&store, &id)
    })
    .await;
    match written {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            glib::g_warning!("pindu", "Could not record how a launch ended: {error}");
            None
        }
        Err(_) => None,
    }
}
