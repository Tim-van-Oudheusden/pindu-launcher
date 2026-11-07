//! Runs the importers off the main thread and reports progress, problems and the games
//! each sync added or found missing to the shells.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use gettextrs::gettext;
use gtk::glib;
use gtk::glib::subclass::Signal;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::model::{Source, SourceState};
use pindu_core::paths::default_library_db_path;
use pindu_core::sources::location::{self, HostDirs};
use pindu_core::sources::sync::{SyncSummary, Tally, sync_all};
use pindu_core::store::Store;
use pindu_core::tasks::TaskKind;

use crate::sources::source_name;
use crate::task_service::TaskService;

mod imp {
    use std::sync::LazyLock;

    use super::*;

    #[derive(Debug, Default, glib::Properties)]
    #[properties(wrapper_type = super::SyncService)]
    pub struct SyncService {
        /// Whether a sync is running.
        #[property(get)]
        pub syncing: Cell<bool>,
        /// Problems of the last sync, one line per problem.
        pub problems: RefCell<Vec<String>>,
        /// The games the last sync added, and the games it found missing.
        pub added: RefCell<Tally>,
        pub missing: RefCell<Tally>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SyncService {
        const NAME: &'static str = "PinduSyncService";
        type Type = super::SyncService;
    }

    #[glib::derived_properties]
    impl ObjectImpl for SyncService {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("finished").build()]);
            SIGNALS.as_ref()
        }
    }
}

glib::wrapper! {
    /// Runs the importers and keeps what the last sync found.
    pub struct SyncService(ObjectSubclass<imp::SyncService>);
}

impl SyncService {
    /// A service that has not synced yet.
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Problems of the last sync, one line per problem.
    pub fn problems(&self) -> Vec<String> {
        self.imp().problems.borrow().clone()
    }

    /// The games the last sync added to the library, and their sources.
    pub fn added(&self) -> Tally {
        self.imp().added.borrow().clone()
    }

    /// The games the last sync found gone from their sources, and those sources.
    pub fn missing(&self) -> Tally {
        self.imp().missing.borrow().clone()
    }

    /// Each source's stored state: enabled, override, where it was last found.
    pub async fn source_states(&self) -> Vec<SourceState> {
        gtk::gio::spawn_blocking(|| {
            Store::open(&default_library_db_path())
                .and_then(|store| store.source_states())
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    }

    /// Turns a source's import on or off, then syncs.
    pub fn set_source_enabled(&self, source: Source, enabled: bool) {
        self.change_source(move |store| {
            let mut state = store
                .source_state(source)?
                .unwrap_or_else(|| SourceState::new(source));
            state.enabled = enabled;
            store.set_source_state(&state)
        });
    }

    /// Sets (or with `None` clears) the folder a source is read from, then syncs.
    pub fn set_source_override(&self, source: Source, folder: Option<PathBuf>) {
        self.change_source(move |store| {
            location::set_user_override(store, source, folder.as_deref())
        });
    }

    fn change_source(
        &self,
        change: impl FnOnce(&Store) -> pindu_core::store::Result<()> + Send + 'static,
    ) {
        let service = self.clone();
        glib::spawn_future_local(async move {
            let changed =
                gtk::gio::spawn_blocking(move || change(&Store::open(&default_library_db_path())?))
                    .await;
            match changed {
                Ok(Ok(())) => service.sync(),
                Ok(Err(error)) => glib::g_warning!("pindu", "Could not change a source: {error}"),
                Err(_) => {}
            }
        });
    }

    /// Syncs every source on a worker thread; a call while syncing is ignored. The sync is
    /// a task in the application's Activity while it runs.
    pub fn sync(&self) {
        if self.syncing() {
            return;
        }
        let tasks = TaskService::of_application();
        let task = tasks
            .as_ref()
            .map(|tasks| tasks.start(TaskKind::LibrarySync, &gettext("Refreshing Library"), None));
        self.set_syncing(true);
        let service = self.clone();
        glib::spawn_future_local(async move {
            let outcome = gtk::gio::spawn_blocking(run_sync)
                .await
                .unwrap_or_else(|_| Err(gettext("The import stopped unexpectedly")));
            let (problems, added, missing) = match outcome {
                Ok(summary) => (
                    summary
                        .problems
                        .iter()
                        .map(|(source, problem)| format!("{}: {problem}", source_name(*source)))
                        .collect(),
                    summary.added(),
                    summary.missing(),
                ),
                Err(message) => (vec![message], Tally::default(), Tally::default()),
            };
            let imp = service.imp();
            imp.problems.replace(problems);
            imp.added.replace(added);
            imp.missing.replace(missing);
            service.set_syncing(false);
            if let (Some(tasks), Some(task)) = (tasks, task) {
                tasks.finish(task);
            }
            service.emit_by_name::<()>("finished", &[]);
        });
    }

    /// Calls `callback` when a sync finishes.
    pub fn connect_finished<F: Fn(&Self) + 'static>(&self, callback: F) -> glib::SignalHandlerId {
        self.connect_local("finished", false, move |values| {
            let service = values[0]
                .get::<Self>()
                .expect("finished is emitted by the service");
            callback(&service);
            None
        })
    }

    fn set_syncing(&self, syncing: bool) {
        self.imp().syncing.set(syncing);
        self.notify_syncing();
    }
}

impl Default for SyncService {
    fn default() -> Self {
        Self::new()
    }
}

/// Opens its own connection to the library and syncs every source.
fn run_sync() -> Result<SyncSummary, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    // GLib reads the local time zone; the core has none of its own.
    let utc_offset = glib::DateTime::now_local().map_or(0, |local| local.utc_offset().as_seconds());
    sync_all(&store, &HostDirs::from_env(), now, utc_offset).map_err(|error| error.to_string())
}
