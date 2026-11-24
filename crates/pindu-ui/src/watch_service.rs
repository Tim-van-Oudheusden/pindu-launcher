//! Watches each source's key files, and where each source not found yet would appear, and
//! resyncs the library when a launcher changes or first writes them, two seconds after the
//! last change. The sync itself runs off the main thread.

use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::paths::default_library_db_path;
use pindu_core::sources::location::HostDirs;
use pindu_core::sources::watch;
use pindu_core::store::Store;

use crate::sync_service::SyncService;

/// How long changes must settle before a resync.
const DEBOUNCE: Duration = Duration::from_secs(2);

#[derive(Debug, Default)]
struct State {
    monitors: Vec<gio::FileMonitor>,
    pending: Option<glib::SourceId>,
}

/// Resyncs the library when a source's key files change.
#[derive(Debug, Clone)]
pub struct WatchService {
    sync: SyncService,
    state: Rc<RefCell<State>>,
}

impl WatchService {
    /// Watches the sources' key files, refreshing what is watched after every sync, as a
    /// sync can find a source in a new place.
    pub fn new(sync: &SyncService) -> Self {
        let service = Self {
            sync: sync.clone(),
            state: Rc::default(),
        };
        let watcher = service.clone();
        sync.connect_finished(move |_| watcher.rewatch());
        service
    }

    /// Replaces the monitors with ones on the key files of every source found.
    fn rewatch(&self) {
        let service = self.clone();
        glib::spawn_future_local(async move {
            let paths = gio::spawn_blocking(watched_paths).await.unwrap_or_default();
            let mut monitors = Vec::new();
            for path in paths {
                let file = gio::File::for_path(&path);
                let monitor = if path.is_dir() {
                    file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                } else {
                    file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
                };
                match monitor {
                    Ok(monitor) => {
                        let watcher = service.clone();
                        monitor.connect_changed(move |_, _, _, _| watcher.changed());
                        monitors.push(monitor);
                    }
                    Err(error) => glib::g_debug!("pindu", "Cannot watch {path:?}: {error}"),
                }
            }
            glib::g_debug!("pindu", "Watching {} source paths", monitors.len());
            let old = std::mem::replace(&mut service.state.borrow_mut().monitors, monitors);
            for monitor in old {
                monitor.cancel();
            }
        });
    }

    /// A key file changed: resync once changes have settled for two seconds.
    fn changed(&self) {
        if let Some(pending) = self.state.borrow_mut().pending.take() {
            pending.remove();
        }
        let service = self.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            service.state.borrow_mut().pending = None;
            if service.sync.syncing() {
                // The running sync may have read the files before this change.
                service.changed();
            } else {
                service.sync.sync();
            }
        });
        self.state.borrow_mut().pending = Some(id);
    }
}

/// The key files of each source where the last sync found it, where each source it did
/// not find would appear (`watch::appear_paths`: a store installed but never run, such
/// as Steam before its first sign-in), what each external importer that is on named in its
/// last probe, and the desktop folders.
fn watched_paths() -> Vec<PathBuf> {
    let dirs = HostDirs::from_env();
    let mut paths = watch::desktop_paths(&dirs);
    if let Ok(store) = Store::open(&default_library_db_path()) {
        if let Ok(states) = store.source_states() {
            for state in states.into_iter().filter(|state| state.enabled) {
                match &state.resolved_location {
                    Some(root) => paths.extend(watch::key_paths(state.source, root)),
                    None => paths.extend(watch::appear_paths(state.source, &dirs)),
                }
            }
        }
        if let Ok(helpers) = store.external_importers() {
            paths.extend(
                helpers
                    .into_iter()
                    .filter(|helper| helper.enabled)
                    .flat_map(|helper| helper.watch),
            );
        }
    }
    paths
}
