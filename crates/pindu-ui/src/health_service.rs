//! Gathers the facts Library Health is computed from (each source's state and games,
//! the permission tier, umu, the Proton builds, MangoHud, GameMode, gamescope, the stored
//! SteamGridDB key, the artwork cache and the library file) through the services, and
//! keeps the report the core makes of them for both shells. It gathers again after every
//! sync.

use std::cell::{Cell, OnceCell, RefCell};

use gettextrs::{gettext, ngettext};
use gtk::glib;
use gtk::glib::subclass::Signal;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use pindu_core::art_cache::default_cache_dir;
use pindu_core::health::{
    self, ArtworkFacts, HealthFacts, HealthReport, LaunchFacts, LibraryFacts, NextStep,
    PreferencesPage, Problem, SourceFacts, Tool,
};
use pindu_core::paths::default_library_db_path;
use pindu_core::store::Store;

use crate::config::APP_ID;
use crate::library_service::LibraryService;
use crate::permission_service;
use crate::proton_launch;
use crate::sources::source_name;
use crate::steamgriddb_service;
use crate::sync_service::SyncService;

mod imp {
    use std::sync::LazyLock;

    use super::*;

    #[derive(Debug, Default)]
    pub struct HealthService {
        /// The report of the last gathering, once one finished.
        pub report: RefCell<Option<HealthReport>>,
        /// Counts gatherings, so only the newest one's report is kept.
        pub generation: Cell<u64>,
        /// Whether this is a gamescope session, as gamescope's service found at start-up.
        pub gamescope_session: Cell<bool>,
        /// The library, which knows when it was last backed up.
        pub library: OnceCell<LibraryService>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for HealthService {
        const NAME: &'static str = "PinduHealthService";
        type Type = super::HealthService;
    }

    impl ObjectImpl for HealthService {
        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> =
                LazyLock::new(|| vec![Signal::builder("changed").build()]);
            SIGNALS.as_ref()
        }
    }
}

glib::wrapper! {
    /// Keeps the Library Health report up to date.
    pub struct HealthService(ObjectSubclass<imp::HealthService>);
}

impl HealthService {
    /// A service that gathers after every sync of `sync`, with the last backup `library`
    /// recorded; `gamescope_session` says whether this is a gamescope session. It starts
    /// gathering at once.
    pub fn new(sync: &SyncService, library: &LibraryService, gamescope_session: bool) -> Self {
        let service: Self = glib::Object::new();
        let imp = service.imp();
        imp.gamescope_session.set(gamescope_session);
        imp.library
            .set(library.clone())
            .expect("a new service has no library");
        sync.connect_finished(glib::clone!(
            #[weak]
            service,
            move |_| service.refresh()
        ));
        service.refresh();
        service
    }

    /// The report of the last gathering; `None` before the first one finished.
    pub fn report(&self) -> Option<HealthReport> {
        self.imp().report.borrow().clone()
    }

    /// Gathers the facts again in the background, then emits `changed`. A library that
    /// cannot be read keeps the last report.
    pub fn refresh(&self) {
        let imp = self.imp();
        let generation = imp.generation.get() + 1;
        imp.generation.set(generation);
        let gamescope_session = imp.gamescope_session.get();
        let last_backup = imp.library.get().and_then(LibraryService::last_backup);
        let service = self.clone();
        glib::spawn_future_local(async move {
            let gathered = gtk::gio::spawn_blocking(gather_stored).await;
            let stored = match gathered {
                Ok(Ok(stored)) => stored,
                Ok(Err(error)) => {
                    glib::g_warning!("pindu", "Could not read the library's health: {error}");
                    return;
                }
                Err(_) => return,
            };
            let key_stored = matches!(steamgriddb_service::api_key().await, Ok(Some(_)));
            let facts = HealthFacts {
                sources: stored.sources,
                launch: LaunchFacts {
                    in_flatpak: permission_service::in_flatpak(),
                    app_id: APP_ID.to_owned(),
                    host_spawn: permission_service::host_spawn_allowed(),
                    games_needing_host: stored.games_needing_host,
                    umu: stored.umu,
                    proton_builds: stored.proton_builds,
                    mangohud: stored.mangohud,
                    gamemode: stored.gamemode,
                    gamescope_session,
                },
                artwork: ArtworkFacts {
                    key_stored,
                    cache_bytes: stored.cache_bytes,
                },
                library: LibraryFacts {
                    file: default_library_db_path(),
                    schema_version: stored.schema_version,
                    last_backup,
                },
            };
            if service.imp().generation.get() != generation {
                return;
            }
            service.imp().report.replace(Some(health::report(&facts)));
            service.emit_by_name::<()>("changed", &[]);
        });
    }

    /// Calls `callback` when a gathering finished with a new report.
    pub fn connect_changed<F: Fn(&Self) + 'static>(&self, callback: F) -> glib::SignalHandlerId {
        self.connect_local("changed", false, move |values| {
            let service = values[0]
                .get::<Self>()
                .expect("changed is emitted by the service");
            callback(&service);
            None
        })
    }
}

/// What a worker thread gathers: the store, the files and the programs on `PATH`.
struct Stored {
    sources: Vec<SourceFacts>,
    games_needing_host: usize,
    schema_version: u32,
    umu: bool,
    proton_builds: Vec<String>,
    mangohud: bool,
    gamemode: bool,
    cache_bytes: u64,
}

fn gather_stored() -> pindu_core::store::Result<Stored> {
    let store = Store::open(&default_library_db_path())?;
    let games = store.games()?;
    Ok(Stored {
        sources: health::source_facts(&store.source_states()?, &games),
        games_needing_host: health::games_needing_host(&games),
        schema_version: store.schema_version()?,
        umu: proton_launch::umu_installed(),
        proton_builds: proton_launch::builds()
            .into_iter()
            .map(|build| build.display_name)
            .collect(),
        mangohud: permission_service::mangohud_available(),
        gamemode: permission_service::gamemode_available(),
        cache_bytes: health::folder_bytes(&default_cache_dir()),
    })
}

/// What a problem is, in a sentence both shells show.
pub fn problem_title(problem: &Problem) -> String {
    match problem {
        Problem::FolderIncomplete { source, .. } => {
            gettext("{source} was not found in the folder you chose")
                .replace("{source}", &source_name(*source))
        }
        Problem::SyncProblems { source, count } => {
            let count = u32::try_from(*count).unwrap_or(u32::MAX);
            ngettext(
                "The last sync of {source} ran into {count} problem",
                "The last sync of {source} ran into {count} problems",
                count,
            )
            .replace("{source}", &source_name(*source))
            .replace("{count}", &count.to_string())
        }
        Problem::NoHostAccess { games } => {
            let count = u32::try_from(*games).unwrap_or(u32::MAX);
            ngettext(
                "{count} game needs permission to start on this computer",
                "{count} games need permission to start on this computer",
                count,
            )
            .replace("{count}", &count.to_string())
        }
        Problem::UmuMissing => gettext("umu-launcher, which runs Windows games, is not installed"),
    }
}

/// The label of the button that takes a next step.
pub fn next_step_label(step: &NextStep) -> String {
    match step {
        NextStep::Preferences(PreferencesPage::Sources) => gettext("Open Sources"),
        NextStep::HostOverride { .. } => gettext("Copy Command"),
        NextStep::HowToInstall(Tool::Umu) => gettext("How to Install"),
    }
}

/// The app action that takes a next step: Preferences at its Sources page, at its
/// Permissions group with the override command, or umu-launcher's install instructions.
pub fn next_step_action(step: &NextStep) -> &'static str {
    match step {
        NextStep::Preferences(PreferencesPage::Sources) => "app.source-preferences",
        NextStep::HostOverride { .. } => "app.permissions",
        NextStep::HowToInstall(Tool::Umu) => "app.install-umu",
    }
}
