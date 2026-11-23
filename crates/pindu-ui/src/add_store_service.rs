//! Keeps the "add a store" flow (`pindu_core::add_store`) for the shells, over
//! [`FlatpakBackend`]: the state they show, and Choose, Cancel and Done for them to call.
//! An install is a task in Activity, which can cancel it; one that ends refreshes the
//! library and posts a notification. The native build installs with the host's `flatpak`;
//! the Flatpak build, which cannot and gets no permission to, opens the software centre.

use std::cell::Cell;
use std::rc::Rc;

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::add_store::{AddStore, AddStoreState, Method, Step, StoreApp};
use pindu_core::notifications::{Kind, Target};
use pindu_core::tasks::{Progress, TaskId, TaskKind};

use crate::add_store_backend::FlatpakBackend;
use crate::notification_service::NotificationService;
use crate::permission_service::in_flatpak;
use crate::sync_service::SyncService;
use crate::task_service::TaskService;

/// The flow, shared by both shells.
#[derive(Clone, Debug)]
pub struct AddStoreService {
    flow: AddStore,
}

impl AddStoreService {
    /// The flow, reporting installs to `tasks` and `notifications` and refreshing the
    /// library through `sync` once one has installed.
    pub fn new(
        sync: &SyncService,
        tasks: &TaskService,
        notifications: &NotificationService,
    ) -> Self {
        let method = if in_flatpak() {
            Method::SoftwareCentre
        } else {
            Method::Flatpak
        };
        let flow = AddStore::new(method, Box::new(FlatpakBackend::new()));
        let monitor = gio::NetworkMonitor::default();
        flow.set_online(monitor.is_network_available());
        let weak = flow.downgrade();
        monitor.connect_network_changed(move |_, online| {
            if let Some(flow) = weak.upgrade() {
                flow.set_online(online);
            }
        });
        flow.connect_installed(glib::clone!(
            #[weak]
            sync,
            move |_| sync.sync()
        ));
        let task: Rc<Cell<Option<TaskId>>> = Rc::default();
        let tasks = tasks.clone();
        let notifications = notifications.clone();
        let weak = flow.downgrade();
        flow.connect_changed(move |state| {
            follow_task(state, &task, &tasks, &weak);
            notify(state, &notifications);
        });
        Self { flow }
    }

    /// The flow as it is.
    pub fn state(&self) -> AddStoreState {
        self.flow.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&AddStoreState) + 'static) {
        self.flow.connect_changed(listener);
    }

    /// Finds out again which stores are installed: the flow shows.
    pub fn refresh(&self) {
        self.flow.refresh();
    }

    /// Installs `app`, or opens its page in the software centre.
    pub fn choose(&self, app: StoreApp) {
        self.flow.choose(app);
    }

    /// Stops the install that runs.
    pub fn cancel(&self) {
        self.flow.cancel();
    }

    /// Leaves an installed, opened or failed store for the stores again.
    pub fn dismiss(&self) {
        self.flow.dismiss();
    }
}

/// Lists the install that runs in Activity, cancellable there, with how far it is, and
/// takes it off once it ends.
fn follow_task(
    state: &AddStoreState,
    task: &Cell<Option<TaskId>>,
    tasks: &TaskService,
    flow: &pindu_core::add_store::WeakAddStore,
) {
    match (&state.step, task.get()) {
        (Step::Installing { app, .. }, None) => {
            let cancellable = gio::Cancellable::new();
            let flow = flow.clone();
            cancellable.connect_cancelled_local(move |_| {
                if let Some(flow) = flow.upgrade() {
                    flow.cancel();
                }
            });
            // Translators: {store} is a store's name, such as Steam.
            let label = gettext("Installing {store}").replace("{store}", store_name(*app));
            task.set(Some(tasks.start(
                TaskKind::StoreInstall,
                &label,
                Some(&cancellable),
            )));
        }
        (
            Step::Installing {
                progress,
                cancelling,
                ..
            },
            Some(id),
        ) => {
            // Cancelled on the level: Activity says so too. Its own cancel lands here
            // again, and changes nothing.
            if *cancelling {
                tasks.cancel(id);
            }
            tasks.set_progress(id, Progress::Fraction(*progress));
        }
        (_, Some(id)) => {
            task.set(None);
            tasks.finish(id);
        }
        (_, None) => {}
    }
}

/// Posts a notification once an install ends, so it is there after its level closed.
fn notify(state: &AddStoreState, notifications: &NotificationService) {
    match &state.step {
        Step::Installed(app) => notifications.post(
            Kind::StoreInstalled,
            // Translators: {store} is a store's name, such as Steam.
            &gettext("{store} Installed").replace("{store}", store_name(*app)),
            &next_step(*app),
            (*app == StoreApp::Steam).then_some(Target::SteamBigPicture),
        ),
        Step::Failed {
            app,
            error,
            network,
        } if state.method == Method::Flatpak => notifications.post(
            Kind::StoreFailed,
            // Translators: {store} is a store's name, such as Steam.
            &gettext("{store} Did Not Install").replace("{store}", store_name(*app)),
            &if *network {
                network_failure(*app)
            } else {
                error.clone()
            },
            None,
        ),
        _ => {}
    }
}

/// A store's name, which is not translated.
pub fn store_name(app: StoreApp) -> &'static str {
    match app {
        StoreApp::Steam => "Steam",
        StoreApp::Heroic => "Heroic",
        StoreApp::Lutris => "Lutris",
    }
}

/// What a store is for, in a line.
pub fn store_description(app: StoreApp) -> String {
    match app {
        StoreApp::Steam => gettext("Valve’s store, with Proton for Windows games"),
        StoreApp::Heroic => gettext("Your Epic Games, GOG and Amazon games"),
        StoreApp::Lutris => gettext("Games from many sources, set up by install scripts"),
    }
}

/// What to do once `app` is installed: sign in to it.
pub fn next_step(app: StoreApp) -> String {
    // Translators: {store} is a store's name, such as Steam.
    gettext("Open {store} and sign in to your account. Its games then show in your library.")
        .replace("{store}", store_name(app))
}

/// Why `app` did not install when the network failed it.
pub fn network_failure(app: StoreApp) -> String {
    // Translators: {store} is a store's name, such as Steam.
    gettext("{store} could not be downloaded, because the internet could not be reached. Check the network connection, then try again.")
        .replace("{store}", store_name(app))
}
