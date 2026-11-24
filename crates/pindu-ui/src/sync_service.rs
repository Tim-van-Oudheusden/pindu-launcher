//! Runs the importers off the main thread and reports progress, problems and the games
//! each sync added or found missing to the shells.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use gettextrs::gettext;
use gtk::glib;
use gtk::glib::subclass::Signal;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::health::is_first_sync;
use pindu_core::model::{ExternalImporterState, Source, SourceState};
use pindu_core::paths::{default_importers_dir, default_library_db_path};
use pindu_core::sources::external::listing::{Listed, listing};
use pindu_core::sources::external::{self, runner};
use pindu_core::sources::location::{self, HostDirs};
use pindu_core::sources::sync::{SyncSummary, Tally, sync_all};
use pindu_core::store::Store;
use pindu_core::tasks::TaskKind;

use crate::config::PKGLIBEXECDIR;
use crate::permission_service;
use crate::session_service;
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
        /// Whether the last sync was the library's first.
        pub first: Cell<bool>,
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

    /// Whether the last sync was the library's first: no source had synced before it.
    pub fn was_first(&self) -> bool {
        self.imp().first.get()
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

    /// Each source's stored state (enabled, override, where it was last found), and each
    /// external importer as the Sources lists show it, in the order Pindu finds them: both
    /// at once, so a Sources list fills in one step.
    pub async fn sources(&self) -> (Vec<SourceState>, Vec<Listed>) {
        gtk::gio::spawn_blocking(|| {
            let dirs = HostDirs::from_env();
            let helpers = helper_environment(&dirs);
            let store = Store::open(&default_library_db_path()).ok();
            let states = store
                .as_ref()
                .and_then(|store| store.source_states().ok())
                .unwrap_or_default();
            let helper_states = store
                .as_ref()
                .and_then(|store| store.external_importers().ok())
                .unwrap_or_default();
            let listed = listing(
                &external::discover(&helpers.installed, helpers.bundled.as_deref()),
                &helper_states,
                helpers.place,
                &helpers.home,
            );
            (states, listed)
        })
        .await
        .unwrap_or_default()
    }

    /// Turns an external importer on or off, then syncs.
    pub fn set_helper_enabled(&self, helper_id: String, enabled: bool) {
        self.change_source(move |store| {
            let mut state = store
                .external_importer(&helper_id)?
                .unwrap_or_else(|| ExternalImporterState::new(&helper_id));
            state.enabled = enabled;
            store.set_external_importer(&state)
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
            // The toast is the only other place these show up; keep them in the journal too.
            match &outcome {
                Ok((summary, _)) => {
                    for (source, problem) in &summary.problems {
                        glib::g_warning!("pindu", "Could not import from {source}: {problem}");
                    }
                }
                Err(message) => {
                    glib::g_warning!("pindu", "Could not refresh the library: {message}")
                }
            }
            let (problems, added, missing, first) = match outcome {
                Ok((summary, first)) => (
                    summary
                        .problems
                        .iter()
                        .map(|(source, problem)| format!("{}: {problem}", source_name(*source)))
                        .collect(),
                    summary.added(),
                    summary.missing(),
                    first,
                ),
                Err(message) => (vec![message], Tally::default(), Tally::default(), false),
            };
            let imp = service.imp();
            imp.problems.replace(problems);
            imp.added.replace(added);
            imp.missing.replace(missing);
            imp.first.set(first);
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

/// Opens its own connection to the library and syncs every source. Also says whether
/// this was the library's first sync.
fn run_sync() -> Result<(SyncSummary, bool), String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let first = store
        .source_states()
        .map(|states| is_first_sync(&states))
        .map_err(|error| error.to_string())?;
    let now = session_service::now();
    // GLib reads the local time zone; the core has none of its own.
    let utc_offset = glib::DateTime::now_local().map_or(0, |local| local.utc_offset().as_seconds());
    let dirs = HostDirs::from_env();
    let helpers = helper_environment(&dirs);
    sync_all(&store, &dirs, &helpers, now, utc_offset)
        .map(|summary| (summary, first))
        .map_err(|error| error.to_string())
}

/// Where the external importers live, and what this instance can offer them.
fn helper_environment(dirs: &HostDirs) -> external::Environment {
    external::Environment {
        installed: default_importers_dir(),
        bundled: Some(PathBuf::from(PKGLIBEXECDIR).join("importers")),
        place: runner::Place {
            in_flatpak: permission_service::in_flatpak(),
            host_spawn: permission_service::host_spawn_allowed(),
        },
        home: dirs.home.clone(),
        limits: runner::Limits::PROTOCOL,
    }
}
