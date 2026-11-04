//! Runs the games we start ourselves and records their play sessions.
//!
//! A session is written when the game starts, its end moved forward every 30 s, and
//! finished when the game exits, so a crash loses at most 30 s. While a game runs the
//! application holds itself and asks the Background portal, so closing the window never
//! ends the game or its session. Host games are tracked from their spawn; Windows games
//! from the moment umu starts them (see `proton_launch`).

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
pub const SIGTERM: i32 = 15;

struct Running {
    /// What Stop does for this game.
    stop: Rc<dyn Fn()>,
    _hold: gio::ApplicationHoldGuard,
}

impl std::fmt::Debug for Running {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Running").finish_non_exhaustive()
    }
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
        let signalled = child.clone();
        self.track(app, id, &child, move || signalled.send_signal(SIGTERM));
        Ok(())
    }

    /// Tracks the session of a game that runs as our child, from now until the child
    /// exits. `stop` is what Stop does to end the game.
    pub fn track(
        &self,
        app: &gio::Application,
        id: &GameId,
        child: &gio::Subprocess,
        stop: impl Fn() + 'static,
    ) {
        let first = self.running.borrow().is_empty();
        self.running.borrow_mut().insert(
            id.clone(),
            Running {
                stop: Rc::new(stop),
                _hold: app.hold(),
            },
        );
        if first {
            request_background();
        }
        self.set_playing(id, true);
        let (service, id, child) = (self.clone(), id.clone(), child.clone());
        glib::spawn_future_local(async move {
            service.record(&id, &child).await;
            service.running.borrow_mut().remove(&id);
            service.set_playing(&id, false);
            service.library.reload();
        });
    }

    /// Ends a running game: a host game is asked to quit (inside Flatpak, `flatpak-spawn`
    /// passes the signal on to the game on the host); a Windows game's prefix is ended.
    pub fn stop(&self, id: &GameId) {
        let stop = self
            .running
            .borrow()
            .get(id)
            .map(|running| running.stop.clone());
        if let Some(stop) = stop {
            stop();
        }
    }

    /// Writes the session as it goes and once more when the child exits.
    async fn record(&self, id: &GameId, child: &gio::Subprocess) {
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
            // `(sa{sv})`: a plain tuple would wrap the options in a variant, `(sv)`.
            Some(&glib::Variant::tuple_from_iter([
                "".to_variant(),
                options.end(),
            ])),
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
