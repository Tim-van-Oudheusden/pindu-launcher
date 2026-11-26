//! Get Games for the shells (#885, #886; `docs/get-games.md`): the catalogue, read off the
//! main thread from the installation's AppStream file and filtered by the lists file, and
//! the jobs (`pindu_core::get_games`) that install, remove and refresh it through the
//! host's `flatpak`. Each job is a task in Activity, which can cancel an install; one that
//! ends posts a notification, and an install or removal refreshes the library. The native
//! build only: in the Flatpak build, which cannot run the host's `flatpak`, the service
//! does nothing.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::SystemTime;

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::add_store::{Installation, REMOTES_COMMAND, StoreApp};
use pindu_core::catalogue::{self, Lists, Offer, appstream, index, lists};
use pindu_core::get_games::{
    CatalogueStatus, GetGames, Job, Outcome, Refusal, State, catalogue_status, installed_apps,
    needs_refresh,
};
use pindu_core::model::{GameId, Source};
use pindu_core::notifications::{Kind, Target};
use pindu_core::sources::location::HostDirs;
use pindu_core::tasks::{Progress, TaskId, TaskKind};

use crate::get_games_backend::{FlatpakJobs, query};
use crate::notification_service::NotificationService;
use crate::permission_service::in_flatpak;
use crate::sync_service::SyncService;
use crate::task_service::TaskService;

/// What the screens show of the catalogue.
#[derive(Debug, Clone, Default)]
pub struct Catalogue {
    /// The installation it comes from and installs go to, once known.
    pub installation: Option<Installation>,
    /// Its folder, which the apps' icon paths are under.
    pub folder: Option<PathBuf>,
    /// What it offers.
    pub offer: Offer,
    /// Whether the installation has a catalogue at all.
    pub present: bool,
    /// Whether it is being read.
    pub loading: bool,
    /// Why it could not be read, when it is there but broken.
    pub broken: Option<String>,
}

/// Something the shells do when the catalogue or the jobs change.
type Listener = Box<dyn Fn()>;

/// Get Games, shared by both shells.
#[derive(Clone, Default)]
pub struct GetGamesService {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    jobs: RefCell<Option<GetGames>>,
    catalogue: RefCell<Catalogue>,
    /// The apps Flathub has ended, left out of the offer.
    end_of_life: RefCell<HashSet<String>>,
    /// The catalogue's entries as last read, before the offer filters them.
    apps: RefCell<Vec<catalogue::App>>,
    lists: RefCell<Lists>,
    /// The task each queued or running job shows as.
    tasks: RefCell<HashMap<Option<String>, TaskId>>,
    /// The screens asked for the catalogue, so it is read once the installation is known.
    wanted: Cell<bool>,
    /// The installation's folder, `/var/lib/flatpak` or the user's, once known.
    flatpak_dir: RefCell<Option<PathBuf>>,
    /// The apps installed in it, as last read.
    installed: RefCell<HashSet<String>>,
    online: Cell<bool>,
    /// When a refresh last ended. Flatpak leaves the catalogue as it was when Flathub's
    /// has not changed, so its own time alone would ask for a refresh on every read.
    refreshed_at: Cell<Option<SystemTime>>,
    listeners: RefCell<Vec<Listener>>,
    services: RefCell<Option<Services>>,
}

/// The services Get Games reports to.
#[derive(Clone)]
struct Services {
    sync: SyncService,
    tasks: TaskService,
    notifications: NotificationService,
}

impl std::fmt::Debug for GetGamesService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GetGamesService")
            .field("catalogue", &self.inner.catalogue.borrow())
            .finish_non_exhaustive()
    }
}

impl GetGamesService {
    /// Get Games, reporting jobs to `tasks` and `notifications` and refreshing the library
    /// through `sync` once an app is installed or removed. In the native build it finds the
    /// installation installs go to; in the Flatpak build it stays idle.
    pub fn new(
        sync: &SyncService,
        tasks: &TaskService,
        notifications: &NotificationService,
    ) -> Self {
        let service = Self::default();
        if !Self::available() {
            return service;
        }
        service.inner.services.replace(Some(Services {
            sync: sync.clone(),
            tasks: tasks.clone(),
            notifications: notifications.clone(),
        }));
        let monitor = gio::NetworkMonitor::default();
        service.inner.online.set(monitor.is_network_available());
        let weak = Rc::downgrade(&service.inner);
        monitor.connect_network_changed(move |_, online| {
            if let Some(inner) = weak.upgrade() {
                let service = Self { inner };
                service.inner.online.set(online);
                if let Some(jobs) = service.jobs() {
                    jobs.set_online(online);
                }
                service.changed();
            }
        });
        let weak = Rc::downgrade(&service.inner);
        glib::spawn_future_local(async move {
            let remotes = query(&REMOTES_COMMAND.map(str::to_owned)).await;
            let installation = match remotes {
                Ok(remotes) => Installation::from_system_remotes(&remotes),
                Err(_) => Installation::User,
            };
            if let Some(inner) = weak.upgrade() {
                Self { inner }.start(installation);
            }
        });
        service
    }

    /// Whether this build has Get Games: the native one, which runs the host's `flatpak`.
    pub fn available() -> bool {
        !in_flatpak()
    }

    /// The catalogue as it is.
    pub fn catalogue(&self) -> Catalogue {
        self.inner.catalogue.borrow().clone()
    }

    /// The apps installed in the installation, as last read.
    pub fn installed(&self) -> HashSet<String> {
        self.inner.installed.borrow().clone()
    }

    /// The jobs as they are; `None` until the installation is known.
    pub fn jobs_state(&self) -> Option<State> {
        self.jobs().map(|jobs| jobs.state())
    }

    /// What the catalogue screen shows.
    pub fn status(&self) -> CatalogueStatus {
        let catalogue = self.inner.catalogue.borrow();
        let refreshing =
            self.jobs().is_none_or(|jobs| jobs.state().refreshing()) || catalogue.loading;
        catalogue_status(catalogue.present, refreshing, self.inner.online.get())
    }

    /// Reads the catalogue for the screens, when it has not been, and refreshes it when it
    /// is missing or a day old.
    pub fn open(&self) {
        if !Self::available() {
            return;
        }
        self.inner.wanted.set(true);
        let loaded = {
            let catalogue = self.inner.catalogue.borrow();
            catalogue.loading
                || catalogue.installation.is_some() && !self.inner.apps.borrow().is_empty()
        };
        if !loaded {
            self.load();
        }
    }

    /// Installs the app `id` the catalogue offers. A store answers
    /// [`Refusal::Store`], for the shell to open Add a Store's flow.
    pub fn install(&self, id: &str) -> Result<(), Refusal> {
        let jobs = self.jobs().ok_or(Refusal::NotOffered)?;
        let offer = self.inner.catalogue.borrow().offer.clone();
        jobs.install(&offer, id)
    }

    /// Removes the app `id`, called `name`, with its data when `delete_data`.
    pub fn remove(&self, id: &str, name: &str, delete_data: bool) -> Result<(), Refusal> {
        let jobs = self.jobs().ok_or(Refusal::InvalidId)?;
        jobs.remove(id, name, delete_data)
    }

    /// Cancels the job for the app `id`.
    pub fn cancel(&self, id: &str) {
        if let Some(jobs) = self.jobs() {
            jobs.cancel(id);
        }
    }

    /// Asks Flathub for the sizes of the app `id`, for its page.
    pub fn fetch_sizes(&self, id: &str) {
        if let Some(jobs) = self.jobs() {
            jobs.fetch_sizes(id);
        }
    }

    /// Downloads the catalogue again, as the screen's Try Again does.
    pub fn refresh(&self) {
        if let Some(jobs) = self.jobs() {
            jobs.refresh();
        }
    }

    /// Calls `listener` whenever the catalogue or the jobs change.
    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    fn jobs(&self) -> Option<GetGames> {
        self.inner.jobs.borrow().clone()
    }

    /// The installation is known: the jobs can start, and the catalogue be read if the
    /// screens asked for it.
    fn start(&self, installation: Installation) {
        let dirs = HostDirs::from_env();
        let flatpak_dir = match installation {
            Installation::System => dirs.system_flatpak.clone(),
            Installation::User => dirs.data_home.join("flatpak"),
        };
        let jobs = GetGames::new(
            installation,
            Box::new(FlatpakJobs::new(flatpak_dir.clone())),
        );
        self.inner.flatpak_dir.replace(Some(flatpak_dir));
        jobs.set_online(self.inner.online.get());
        let weak = Rc::downgrade(&self.inner);
        jobs.connect_changed(move |state| {
            if let Some(inner) = weak.upgrade() {
                let service = Self { inner };
                service.follow_tasks(state);
                service.changed();
            }
        });
        let weak = Rc::downgrade(&self.inner);
        jobs.connect_finished(move |job, outcome| {
            if let Some(inner) = weak.upgrade() {
                Self { inner }.finished(job, outcome);
            }
        });
        self.inner.jobs.replace(Some(jobs));
        {
            let mut catalogue = self.inner.catalogue.borrow_mut();
            catalogue.installation = Some(installation);
            catalogue.folder = Some(catalogue::active_dir(installation, &dirs));
        }
        self.changed();
        self.read_installed();
        if self.inner.wanted.get() {
            self.load();
        }
    }

    /// Reads which apps the installation has, off the main thread.
    fn read_installed(&self) {
        let Some(dir) = self.inner.flatpak_dir.borrow().clone() else {
            return;
        };
        let weak = Rc::downgrade(&self.inner);
        glib::spawn_future_local(async move {
            let installed = gio::spawn_blocking(move || installed_apps(&dir))
                .await
                .unwrap_or_default();
            if let Some(inner) = weak.upgrade()
                && *inner.installed.borrow() != installed
            {
                inner.installed.replace(installed);
                Self { inner }.changed();
            }
        });
    }

    /// Reads the catalogue off the main thread, through its index, then which apps
    /// Flathub has ended; refreshes it when it is missing or a day old.
    fn load(&self) {
        let Some(installation) = self.inner.catalogue.borrow().installation else {
            return;
        };
        self.inner.catalogue.borrow_mut().loading = true;
        self.changed();
        let locale = glib::language_names()
            .first()
            .map(ToString::to_string)
            .unwrap_or_default();
        let cache = glib::user_cache_dir();
        let weak = Rc::downgrade(&self.inner);
        glib::spawn_future_local(async move {
            let read = gio::spawn_blocking(move || {
                let dirs = HostDirs::from_env();
                let folder = catalogue::active_dir(installation, &dirs);
                let distributor =
                    std::fs::read_to_string(lists::distributor_path(&dirs.host_usr)).ok();
                let (lists, lists_error) = lists::read(distributor.as_deref());
                let index = index::index_path(Some(&cache), &dirs.home, installation);
                let languages = appstream::languages(&locale);
                let loaded = index::load(&folder, &index, &languages, &lists);
                let modified = std::fs::metadata(folder.join("appstream.xml"))
                    .and_then(|metadata| metadata.modified())
                    .ok();
                (lists, lists_error, loaded, modified)
            })
            .await;
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let service = Self { inner };
            let Ok((lists, lists_error, loaded, modified)) = read else {
                service.inner.catalogue.borrow_mut().loading = false;
                service.changed();
                return;
            };
            if let Some(error) = lists_error {
                glib::g_warning!("pindu", "Get Games' lists file was not read: {}", error.0);
            }
            // The refresh is queued first, so a missing catalogue shows as on its way
            // rather than as one that could not be downloaded.
            let checked = modified.max(service.inner.refreshed_at.get());
            if needs_refresh(checked, SystemTime::now())
                && let Some(jobs) = service.jobs()
            {
                jobs.refresh();
            }
            service.loaded(lists, loaded);
            service.read_end_of_life(installation);
        });
    }

    /// The catalogue was read as `loaded`, under `lists`.
    fn loaded(
        &self,
        lists: Lists,
        loaded: Result<(Vec<catalogue::App>, index::Origin), index::CatalogueError>,
    ) {
        {
            let mut catalogue = self.inner.catalogue.borrow_mut();
            catalogue.loading = false;
            match loaded {
                Ok((apps, _)) => {
                    catalogue.present = true;
                    catalogue.broken = None;
                    self.inner.apps.replace(apps);
                }
                Err(index::CatalogueError::Missing) => {
                    catalogue.present = false;
                    catalogue.broken = None;
                    self.inner.apps.replace(Vec::new());
                }
                Err(index::CatalogueError::Broken(error)) => {
                    glib::g_warning!("pindu", "Flathub's catalogue could not be read: {error}");
                    catalogue.present = false;
                    catalogue.broken = Some(error);
                    self.inner.apps.replace(Vec::new());
                }
            }
        }
        self.inner.lists.replace(lists);
        self.offer();
    }

    /// Asks `flatpak` which apps Flathub has ended, and leaves them out of the offer.
    /// When it cannot say, offline for one, nothing is left out.
    fn read_end_of_life(&self, installation: Installation) {
        let weak = Rc::downgrade(&self.inner);
        glib::spawn_future_local(async move {
            let output = query(&catalogue::end_of_life_command(installation)).await;
            if let (Some(inner), Ok(output)) = (weak.upgrade(), output) {
                let service = Self { inner };
                service
                    .inner
                    .end_of_life
                    .replace(catalogue::end_of_life(&output));
                service.offer();
            }
        });
    }

    /// Filters the catalogue's entries into the offer again.
    fn offer(&self) {
        let offer = Offer::new(
            self.inner.apps.borrow().clone(),
            self.inner.lists.borrow().clone(),
            &self.inner.end_of_life.borrow(),
        );
        self.inner.catalogue.borrow_mut().offer = offer;
        self.changed();
    }

    /// Keeps a task in Activity for each queued or running job, with how far the running
    /// one is.
    fn follow_tasks(&self, state: &State) {
        let Some(services) = self.inner.services.borrow().clone() else {
            return;
        };
        let tasks = &services.tasks;
        let mut shown = self.inner.tasks.borrow_mut();
        let jobs: Vec<(&Job, bool)> = state
            .running
            .iter()
            .map(|running| (&running.job, true))
            .chain(state.queue.iter().map(|job| (job, false)))
            .collect();
        shown.retain(|key, id| {
            let listed = jobs
                .iter()
                .any(|(job, _)| job.app_id().map(str::to_owned) == *key);
            if !listed {
                tasks.finish(*id);
            }
            listed
        });
        for (job, running) in jobs {
            let key = job.app_id().map(str::to_owned);
            let id = *shown.entry(key).or_insert_with(|| self.start_task(job));
            if !running {
                tasks.set_detail(id, Some(&gettext("Waiting")));
                continue;
            }
            let Some(running) = &state.running else {
                continue;
            };
            if running.cancelling {
                tasks.cancel(id);
            }
            tasks.set_progress(
                id,
                running
                    .progress
                    .map_or(Progress::Working, Progress::Fraction),
            );
            let detail = match (&running.job, running.step) {
                (Job::Install { .. }, Some((operation, operations))) => Some(
                    // Translators: which of an install's downloads runs, such as "Installing
                    // 2 of 3": the app, its runtime and their extensions.
                    gettext("Installing {operation} of {operations}")
                        .replace("{operation}", &operation.to_string())
                        .replace("{operations}", &operations.to_string()),
                ),
                (Job::Install { .. }, None) if running.progress.is_none() => {
                    Some(gettext("Checking the space"))
                }
                _ => None,
            };
            tasks.set_detail(id, detail.as_deref());
        }
    }

    /// Lists `job` in Activity; an install can be cancelled there.
    fn start_task(&self, job: &Job) -> TaskId {
        let Some(services) = self.inner.services.borrow().clone() else {
            return TaskId(u64::MAX);
        };
        match job {
            Job::Install { id, name } => {
                let cancellable = gio::Cancellable::new();
                let weak = Rc::downgrade(&self.inner);
                let id = id.clone();
                cancellable.connect_cancelled_local(move |_| {
                    if let Some(inner) = weak.upgrade() {
                        Self { inner }.cancel(&id);
                    }
                });
                // Translators: {name} is an app's name, such as SuperTuxKart.
                let label = gettext("Installing {name}").replace("{name}", name);
                services
                    .tasks
                    .start(TaskKind::GameInstall, &label, Some(&cancellable))
            }
            Job::Remove { name, .. } => {
                // Translators: {name} is an app's name, such as SuperTuxKart.
                let label = gettext("Removing {name}").replace("{name}", name);
                services.tasks.start(TaskKind::GameInstall, &label, None)
            }
            Job::Refresh => services.tasks.start(
                TaskKind::CatalogueRefresh,
                &gettext("Updating the Game Catalogue"),
                None,
            ),
        }
    }

    /// `job` ended as `outcome`: a notification says so, an install or removal refreshes
    /// the library, and a refresh reads the catalogue again.
    fn finished(&self, job: &Job, outcome: &Outcome) {
        let Some(services) = self.inner.services.borrow().clone() else {
            return;
        };
        let offer = self.inner.catalogue.borrow().offer.clone();
        if *job == Job::Refresh {
            self.inner.refreshed_at.set(Some(SystemTime::now()));
        }
        if job.app_id().is_some() {
            self.read_installed();
        }
        match (job, outcome) {
            (Job::Refresh, Outcome::Done) => self.load(),
            (Job::Refresh, Outcome::Failed { error, .. }) => {
                glib::g_warning!("pindu", "Flathub's catalogue was not refreshed: {error}");
            }
            (Job::Install { id, name }, Outcome::Done) => {
                services.sync.sync();
                let extra = offer.is_extra(id);
                let target = offer
                    .get(id)
                    .and_then(|app| app.desktop_id.clone())
                    .filter(|_| !extra)
                    .map(|desktop_id| Target::Game(GameId::new(Source::Desktop, &desktop_id)));
                services.notifications.post(
                    Kind::AppInstalled,
                    // Translators: {name} is an app's name, such as SuperTuxKart.
                    &gettext("{name} Installed").replace("{name}", name),
                    &if extra {
                        gettext("Open it from Apps.")
                    } else {
                        gettext("It is in your library.")
                    },
                    target,
                );
            }
            (Job::Remove { name, .. }, Outcome::Done) => {
                services.sync.sync();
                services.notifications.post(
                    Kind::AppRemoved,
                    // Translators: {name} is an app's name, such as SuperTuxKart.
                    &gettext("{name} Removed").replace("{name}", name),
                    "",
                    None,
                );
            }
            (Job::Install { name, .. }, Outcome::Failed { error, network }) => {
                services.notifications.post(
                    Kind::AppFailed,
                    // Translators: {name} is an app's name, such as SuperTuxKart.
                    &gettext("{name} Did Not Install").replace("{name}", name),
                    &if *network {
                        network_failure()
                    } else {
                        error.clone()
                    },
                    None,
                );
            }
            (Job::Remove { name, .. }, Outcome::Failed { error, .. }) => {
                services.notifications.post(
                    Kind::AppFailed,
                    // Translators: {name} is an app's name, such as SuperTuxKart.
                    &gettext("{name} Was Not Removed").replace("{name}", name),
                    error,
                    None,
                );
            }
            (Job::Install { name, .. }, Outcome::NoSpace { needed, free }) => {
                services.notifications.post(
                    Kind::AppFailed,
                    // Translators: {name} is an app's name, such as SuperTuxKart.
                    &gettext("Not Enough Space for {name}").replace("{name}", name),
                    &no_space(*needed, *free),
                    None,
                );
            }
            _ => {}
        }
    }

    fn changed(&self) {
        for listener in self.inner.listeners.borrow().iter() {
            listener();
        }
    }
}

/// Why an app did not install when the network failed it.
pub fn network_failure() -> String {
    gettext("Flathub could not be reached. Check the network, then try again.")
}

/// What an app that would not fit needs: `needed` bytes free, where `free` are.
pub fn no_space(needed: u64, free: u64) -> String {
    // Translators: {needed} and {free} are sizes, such as "1.2 GB".
    gettext("It needs {needed} free, and {free} is. Remove a game to make room.")
        .replace("{needed}", &glib::format_size(needed))
        .replace("{free}", &glib::format_size(free))
}

/// Whether the app `id` is a store, which installs through Add a Store's flow.
pub fn store(id: &str) -> Option<StoreApp> {
    StoreApp::ALL.into_iter().find(|store| store.app_id() == id)
}
