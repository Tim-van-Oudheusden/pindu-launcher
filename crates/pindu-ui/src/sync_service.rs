//! Runs the importers off the main thread and reports progress and problems to the shells.

use std::cell::{Cell, RefCell};
use std::time::{SystemTime, UNIX_EPOCH};

use gettextrs::gettext;
use gtk::glib;
use gtk::glib::subclass::Signal;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::paths::default_library_db_path;
use pindu_core::sources::location::HostDirs;
use pindu_core::sources::sync::sync_all;
use pindu_core::store::Store;

use crate::sources::source_name;

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
    pub struct SyncService(ObjectSubclass<imp::SyncService>);
}

impl SyncService {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Problems of the last sync, one line per problem.
    pub fn problems(&self) -> Vec<String> {
        self.imp().problems.borrow().clone()
    }

    /// Syncs every source on a worker thread; a call while syncing is ignored.
    pub fn sync(&self) {
        if self.syncing() {
            return;
        }
        self.set_syncing(true);
        let service = self.clone();
        glib::spawn_future_local(async move {
            let outcome = gtk::gio::spawn_blocking(run_sync)
                .await
                .unwrap_or_else(|_| Err(gettext("The import stopped unexpectedly")));
            let problems = outcome.unwrap_or_else(|message| vec![message]);
            service.imp().problems.replace(problems);
            service.set_syncing(false);
            service.emit_by_name::<()>("finished", &[]);
        });
    }

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
fn run_sync() -> Result<Vec<String>, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64);
    let summary =
        sync_all(&store, &HostDirs::from_env(), now).map_err(|error| error.to_string())?;
    Ok(summary
        .problems
        .into_iter()
        .map(|(source, problem)| format!("{}: {problem}", source_name(source)))
        .collect())
}
