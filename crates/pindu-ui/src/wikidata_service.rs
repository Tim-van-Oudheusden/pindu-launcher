//! Wikidata metadata: a refresh the user asks for, which looks the library's games up by
//! Steam app ID in batches, one request at a time, backing off on HTTP 429, and stores
//! what Wikidata has as provider values. It is a task in Activity the user can cancel;
//! nothing here runs during an import.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::GameId;
use pindu_core::paths::default_library_db_path;
use pindu_core::steamgriddb::retry_delay;
use pindu_core::store::Store;
use pindu_core::tasks::{Progress, TaskId, TaskKind};
use pindu_core::wikidata::{self, RefreshRun, RunEnd};
use soup::prelude::*;

use crate::session_service;
use crate::task_service::TaskService;

/// The GSettings key that turns Wikidata metadata on; off by default.
pub const SETTINGS_KEY: &str = "wikidata-metadata";

/// Refreshes the library's metadata from Wikidata and says what it did; `None` when the
/// library could not be read. Each batch's details are stored as it comes, so a cancelled
/// or failed run keeps what it got. A run with games to look up is a task in `tasks`,
/// which the user can cancel; `on_start` hears of it before the first request.
pub async fn refresh_metadata(
    tasks: &TaskService,
    on_start: impl FnOnce(TaskId),
) -> Option<RefreshRun> {
    let app_ids = gio::spawn_blocking(|| {
        let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
        wikidata::games_to_look_up(&store).map_err(|error| error.to_string())
    })
    .await
    .unwrap_or_else(|_| Err("the library read stopped unexpectedly".into()));
    let app_ids = match app_ids {
        Ok(app_ids) => app_ids,
        Err(error) => {
            glib::g_warning!("pindu", "Could not read the library for Wikidata: {error}");
            return None;
        }
    };
    let mut run = RefreshRun {
        looked_up: app_ids.len(),
        updated: 0,
        end: RunEnd::Done,
    };
    let batches = wikidata::batches(&app_ids);
    if batches.is_empty() {
        return Some(run);
    }
    let languages = wikidata::label_languages(&glib::language_names());
    let session = soup::Session::new();
    session.set_user_agent(wikidata::USER_AGENT);
    let cancellable = gio::Cancellable::new();
    let task = tasks.start(
        TaskKind::MetadataFetch,
        &gettext("Metadata From Wikidata"),
        Some(&cancellable),
    );
    on_start(task);
    let total = batches.len();
    let app_ids = Arc::new(app_ids);
    for (done, batch) in batches.iter().enumerate() {
        let url = wikidata::query_url(batch, &languages);
        let answer = gio::CancellableFuture::new(get(&session, &url), cancellable.clone()).await;
        let answer = match answer {
            Ok(Ok(answer)) => answer,
            Ok(Err(error)) => {
                glib::g_debug!("pindu", "Wikidata: {error}");
                run.end = RunEnd::Unreachable;
                break;
            }
            Err(gio::Cancelled) => {
                run.end = RunEnd::Cancelled;
                break;
            }
        };
        match store_answer(app_ids.clone(), answer).await {
            Ok(updated) => run.updated += updated,
            Err(error) => {
                glib::g_warning!("pindu", "Could not store Wikidata metadata: {error}");
                run.end = RunEnd::Unreachable;
                break;
            }
        }
        tasks.set_progress(task, Progress::of(done + 1, total));
    }
    tasks.finish(task);
    Some(run)
}

/// Reads one batch's answer and stores its details; says how many games got some.
async fn store_answer(
    app_ids: Arc<BTreeMap<GameId, u32>>,
    answer: Vec<u8>,
) -> Result<usize, String> {
    let now = session_service::now();
    gio::spawn_blocking(move || {
        let items = wikidata::parse(&String::from_utf8_lossy(&answer))?;
        let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
        wikidata::store_items(&store, &app_ids, &items, now).map_err(|error| error.to_string())
    })
    .await
    .unwrap_or_else(|_| Err("storing stopped unexpectedly".into()))
}

/// GETs `url`, retrying rate-limited answers after the server's `Retry-After` or the
/// policy's delay ([`retry_delay`]).
async fn get(session: &soup::Session, url: &str) -> Result<Vec<u8>, String> {
    let mut attempt = 0;
    loop {
        let message = soup::Message::new("GET", url).map_err(|error| error.to_string())?;
        if let Some(headers) = message.request_headers() {
            headers.append("Accept", wikidata::ACCEPT);
        }
        let bytes = session
            .send_and_read_future(&message, glib::Priority::DEFAULT)
            .await
            .map_err(|error| error.to_string())?;
        // libsoup's status enum has no name for 429.
        match message.status_code() {
            200 => return Ok(bytes.to_vec()),
            429 => {
                let retry_after = message
                    .response_headers()
                    .and_then(|headers| headers.one("Retry-After"))
                    .and_then(|value| value.trim().parse().ok());
                let Some(delay) = retry_delay(attempt, retry_after) else {
                    return Err("Wikidata is busy".into());
                };
                glib::timeout_future(delay.max(Duration::from_secs(1))).await;
                attempt += 1;
            }
            status => return Err(format!("Wikidata answered {status}")),
        }
    }
}
