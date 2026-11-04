//! Runs the games we start ourselves and records their play sessions.
//!
//! A session is written when the game starts, its end moved forward every 30 s, and
//! finished when the game exits, so a crash loses at most 30 s. While a game runs the
//! application holds itself and asks the Background portal, so closing the window never
//! ends the game or its session.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsStr;
use std::rc::Rc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::launch::Command;
use pindu_core::model::{GameId, Timestamp};
use pindu_core::paths::default_library_db_path;
use pindu_core::store::Store;

use crate::library_service::LibraryService;
use crate::permission_service;

/// How often a running session's end is written.
const FLUSH_INTERVAL: Duration = Duration::from_secs(30);

/// The signal Stop sends, which games handle as "quit".
const SIGTERM: i32 = 15;

#[derive(Debug)]
struct Running {
    child: gio::Subprocess,
    _hold: gio::ApplicationHoldGuard,
}

#[derive(Debug, Clone)]
pub struct SessionService {
    library: LibraryService,
    running: Rc<RefCell<HashMap<GameId, Running>>>,
}

impl SessionService {
    pub fn new(library: &LibraryService) -> Self {
        Self {
            library: library.clone(),
            running: Rc::default(),
        }
    }

    /// Whether any game we started is running.
    pub fn any_playing(&self) -> bool {
        !self.running.borrow().is_empty()
    }

    /// Whether a game we started is still running.
    pub fn is_playing(&self, id: &GameId) -> bool {
        self.running.borrow().contains_key(id)
    }

    /// Starts a game on the host as our child and tracks its session until it exits.
    pub fn start(
        &self,
        app: &gio::Application,
        id: &GameId,
        command: &Command,
    ) -> Result<(), glib::Error> {
        let child = spawn(command)?;
        let first = self.running.borrow().is_empty();
        self.running.borrow_mut().insert(
            id.clone(),
            Running {
                child: child.clone(),
                _hold: app.hold(),
            },
        );
        if first {
            request_background();
        }
        self.set_playing(id, true);
        let service = self.clone();
        let id = id.clone();
        glib::spawn_future_local(async move {
            service.track(&id, &child).await;
            service.running.borrow_mut().remove(&id);
            service.set_playing(&id, false);
            service.library.reload();
        });
        Ok(())
    }

    /// Asks a running game to quit. Inside Flatpak, `flatpak-spawn` passes the signal on
    /// to the game on the host.
    pub fn stop(&self, id: &GameId) {
        if let Some(running) = self.running.borrow().get(id) {
            running.child.send_signal(SIGTERM);
        }
    }

    /// Writes the session as it goes and once more when the child exits.
    async fn track(&self, id: &GameId, child: &gio::Subprocess) {
        let started_at = now();
        let session = write(
            move |store, id| {
                store
                    .add_play_session(id, started_at, started_at)
                    .map(|session| session.id)
            },
            id,
        )
        .await;
        let mut exit = std::pin::pin!(child.wait_future());
        loop {
            match glib::future_with_timeout(FLUSH_INTERVAL, exit.as_mut()).await {
                Ok(result) => {
                    log_exit(id, child, result);
                    break;
                }
                Err(_) => update_end(session, id).await,
            }
        }
        update_end(session, id).await;
    }

    fn set_playing(&self, id: &GameId, playing: bool) {
        if let Some(game) = self.library.model().game_object(id) {
            game.set_playing(playing);
        }
    }
}

async fn update_end(session: Option<i64>, id: &GameId) {
    let Some(session) = session else { return };
    let ended_at = now();
    write(
        move |store, _| store.update_play_session_end(session, ended_at),
        id,
    )
    .await;
}

/// Runs a store write on a worker thread; failures are logged, since a lost update only
/// costs playtime.
async fn write<T: Send + 'static>(
    work: impl FnOnce(&Store, &GameId) -> pindu_core::store::Result<T> + Send + 'static,
    id: &GameId,
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
            glib::g_warning!("pindu", "Could not record a play session: {error}");
            None
        }
        Err(_) => None,
    }
}

/// Spawns the command on the host: through `flatpak-spawn --host` inside Flatpak (never
/// `--watch-bus`), directly outside it.
pub fn spawn(command: &Command) -> Result<gio::Subprocess, glib::Error> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
    let argv = if permission_service::in_flatpak() {
        command.flatpak_spawn_argv()
    } else {
        for (key, value) in &command.environment {
            launcher.setenv(key, value, true);
        }
        if let Some(dir) = &command.working_dir {
            launcher.set_cwd(dir);
        }
        command.argv.clone()
    };
    let argv: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    launcher.spawn(&argv)
}

fn log_exit(id: &GameId, child: &gio::Subprocess, result: Result<(), glib::Error>) {
    match result {
        Ok(()) if child.is_successful() => glib::g_info!("pindu", "{id} exited"),
        Ok(()) if child.has_signaled() => {
            glib::g_info!("pindu", "{id} ended by signal {}", child.term_sig());
        }
        Ok(()) => glib::g_warning!("pindu", "{id} exited with status {}", child.exit_status()),
        Err(error) => glib::g_warning!("pindu", "Lost track of {id}: {error}"),
    }
}

/// Asks the Background portal to let us keep running without a window. The answer only
/// matters to the user's system settings, so it is logged, not waited for.
fn request_background() {
    gio::bus_get(gio::BusType::Session, gio::Cancellable::NONE, |bus| {
        let Ok(bus) = bus else { return };
        let options = glib::VariantDict::new(None);
        options.insert(
            "reason",
            gettextrs::gettext("A game you started is running"),
        );
        options.insert("autostart", false);
        bus.call(
            Some("org.freedesktop.portal.Desktop"),
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Background",
            "RequestBackground",
            Some(&("", options.end()).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
            |reply| {
                if let Err(error) = reply {
                    glib::g_info!("pindu", "No Background portal: {error}");
                }
            },
        );
    });
}

fn now() -> Timestamp {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs() as i64)
}
