//! The installed apps (`pindu_core::apps`, #830) for both shells' Apps: read off the main
//! thread at start, and again a second after the application folders last changed, so an
//! app installed or removed while the launcher runs comes or goes without a restart.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::apps::{self, App, Session};
use pindu_core::sources::desktop;
use pindu_core::sources::location::HostDirs;

/// How long the application folders must settle before the apps are read again.
const DEBOUNCE: Duration = Duration::from_secs(1);

/// Something a shell does when the apps change.
type Listener = Box<dyn Fn()>;

#[derive(Default)]
struct State {
    apps: Vec<App>,
    listeners: Vec<Listener>,
    monitors: Vec<gio::FileMonitor>,
    pending: Option<glib::SourceId>,
    /// Counts the reads, so only the newest one's apps are kept.
    generation: u64,
}

/// The installed apps, kept current.
#[derive(Clone)]
pub struct AppsService {
    state: Rc<RefCell<State>>,
}

impl std::fmt::Debug for AppsService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppsService")
            .field("apps", &self.state.borrow().apps.len())
            .finish_non_exhaustive()
    }
}

impl AppsService {
    /// Reads the apps, and watches the application folders for changes.
    pub fn new() -> Self {
        let service = Self {
            state: Rc::default(),
        };
        service.watch();
        service.reload();
        service
    }

    /// The installed apps, by name.
    pub fn apps(&self) -> Vec<App> {
        self.state.borrow().apps.clone()
    }

    /// The installed app with desktop ID `id`, if there is one.
    pub fn app(&self, id: &str) -> Option<App> {
        self.state
            .borrow()
            .apps
            .iter()
            .find(|app| app.id == id)
            .cloned()
    }

    /// Calls `listener` whenever the apps change.
    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.state.borrow_mut().listeners.push(Box::new(listener));
    }

    /// Watches each application folder, there yet or not.
    fn watch(&self) {
        let mut monitors = Vec::new();
        for folder in desktop::application_dirs(&HostDirs::from_env()) {
            let file = gio::File::for_path(&folder);
            match file.monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
                Ok(monitor) => {
                    let service = self.clone();
                    monitor.connect_changed(move |_, _, _, _| service.changed());
                    monitors.push(monitor);
                }
                Err(error) => glib::g_debug!("pindu", "Cannot watch {folder:?}: {error}"),
            }
        }
        self.state.borrow_mut().monitors = monitors;
    }

    /// An application folder changed: read the apps again once changes have settled.
    fn changed(&self) {
        if let Some(pending) = self.state.borrow_mut().pending.take() {
            pending.remove();
        }
        let service = self.clone();
        let id = glib::timeout_add_local_once(DEBOUNCE, move || {
            service.state.borrow_mut().pending = None;
            service.reload();
        });
        self.state.borrow_mut().pending = Some(id);
    }

    /// Reads the apps off the main thread, and tells the listeners when they changed.
    fn reload(&self) {
        let generation = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.generation
        };
        let service = self.clone();
        glib::spawn_future_local(async move {
            let Ok(apps) = gio::spawn_blocking(|| {
                let session = Session::from_vars(|name| std::env::var(name).ok());
                apps::installed(&HostDirs::from_env(), &session)
            })
            .await
            else {
                return;
            };
            {
                let mut state = service.state.borrow_mut();
                if state.generation != generation || state.apps == apps {
                    return;
                }
                glib::g_debug!("pindu", "{} apps installed", apps.len());
                state.apps = apps;
            }
            for listener in &service.state.borrow().listeners {
                listener();
            }
        });
    }
}

impl Default for AppsService {
    fn default() -> Self {
        Self::new()
    }
}
