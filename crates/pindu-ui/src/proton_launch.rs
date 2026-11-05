//! Runs Windows games through umu and Proton inside the sandbox, as our child. umu's log is
//! read as it comes, so a first start, which downloads the Steam Linux Runtime, shows its
//! progress, and a missing internet connection is said plainly instead of the game
//! silently not starting.
//!
//! A game's play session runs from umu reporting the game starting until umu exits: umu
//! waits for its container, whose `pv-adverb` reaps every process of the prefix, so a
//! game that starts another program and exits keeps its session until that one ends
//! too. Stop ends every process in the game's prefix with its build's `wineserver -k`.
//!
//! "Play with Logs" keeps umu's output and Proton's log in the game's log (see
//! `log_service`).

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::fs::File;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::{self, Command};
use pindu_core::model::{Game, LaunchConfig};
use pindu_core::paths::{default_prefixes_dir, default_tools_dir};
use pindu_core::proton::{self, ProtonError};
use pindu_core::sources::location::HostDirs;
use pindu_core::umu::{self, RuntimeSetup};

use crate::config::APP_ID;
use crate::launch_service::now;
use crate::log_service::{self, GameLog};
use crate::session_service::{SIGTERM, SessionService};

/// How often the setup dialog's progress moves.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Starts a Windows game with the Proton build and prefix its launch config names, or the
/// defaults from Preferences, and tracks its session from the moment umu starts it. The
/// app stays up while the game runs, since the game writes its log to us. With `logs`,
/// umu's output and Proton's log are kept in a new log of the game.
pub fn play(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: Game,
    config: LaunchConfig,
    sessions: SessionService,
    logs: bool,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let (app, window) = (app.clone(), window.cloned());
    let hold = app.hold();
    let defaults = defaults();
    glib::spawn_future_local(async move {
        let found = {
            let game = game.clone();
            gio::spawn_blocking(move || {
                let builds = builds();
                proton::command(&game, &config, &builds, &defaults).map(|command| {
                    let end_prefix = proton::stop_command(&command, &builds);
                    (command, end_prefix)
                })
            })
            .await
        };
        let title = game.title.clone();
        let (mut command, end_prefix) = match found {
            Ok(Ok(found)) => found,
            Ok(Err(error)) => {
                toast(adw::Toast::new(&cannot_start_message(&title, &error)));
                return;
            }
            Err(_) => {
                toast(adw::Toast::new(&could_not_start(&title)));
                return;
            }
        };
        let mut log = None;
        if logs {
            let (id, log_title) = (game.id.clone(), title.clone());
            let created = gio::spawn_blocking(move || {
                let log = GameLog::create(&id, &log_title, &mut command, true);
                (log, command)
            })
            .await;
            let Ok((created, logged)) = created else {
                toast(adw::Toast::new(&could_not_start(&title)));
                return;
            };
            command = logged;
            match created {
                Ok(created) => log = Some(created),
                Err(error) => {
                    glib::g_warning!("pindu", "Could not start a log of “{title}”: {error}");
                    toast(adw::Toast::new(&log_service::could_not_keep(&title)));
                }
            }
        }
        let toast = Rc::new(toast);
        let started = {
            let (app, toast, title) = (app.clone(), toast.clone(), title.clone());
            move |umu: &gio::Subprocess| {
                if launch::tracks_sessions(&game) {
                    let stop = stopper(umu.clone(), end_prefix);
                    sessions.track(app.upcast_ref(), &game.id, umu, stop);
                }
                let (library, id) = (sessions.library().clone(), game.id.clone());
                glib::spawn_future_local(async move { library.mark_played(&id, now()).await });
                toast(adw::Toast::new(
                    &gettext("Starting “{title}”").replace("{title}", &title),
                ));
            }
        };
        run(
            &app,
            window,
            &title,
            &command,
            log.as_mut(),
            &*toast,
            started,
        )
        .await;
        if let Some(log) = log {
            match gio::spawn_blocking(move || log.finish()).await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    glib::g_warning!("pindu", "Could not add Proton's log of “{title}”: {error}");
                }
                Err(_) => glib::g_warning!("pindu", "Could not add Proton's log of “{title}”"),
            }
        }
        drop(hold);
    });
}

/// What Stop does for a Windows game: end every process in its prefix, then umu, if it is
/// still there. Without the build's wineserver, umu alone is asked to quit; it passes the
/// signal on to the processes it started.
fn stopper(umu: gio::Subprocess, end_prefix: Option<Command>) -> impl Fn() {
    move || {
        let Some(end_prefix) = &end_prefix else {
            umu.send_signal(SIGTERM);
            return;
        };
        let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::NONE);
        for (key, value) in &end_prefix.environment {
            launcher.setenv(key, value, true);
        }
        let argv: Vec<&OsStr> = end_prefix.argv.iter().map(OsStr::new).collect();
        let umu = umu.clone();
        match launcher.spawn(&argv) {
            Ok(wineserver) => {
                glib::spawn_future_local(async move {
                    if let Err(error) = wineserver.wait_future().await {
                        glib::g_warning!("pindu", "Lost track of wineserver: {error}");
                    }
                    umu.send_signal(SIGTERM);
                });
            }
            Err(error) => {
                glib::g_warning!("pindu", "Could not run {:?}: {error}", end_prefix.argv);
                umu.send_signal(SIGTERM);
            }
        }
    }
}

/// Runs a game's installer through umu in the prefix the game will use, with the named
/// Proton build or the default one, and waits until it exits. Returns whether it ran.
pub async fn run_installer(
    app: &gtk::Application,
    window: Option<gtk::Window>,
    installer: PathBuf,
    proton_version: Option<String>,
    prefix: PathBuf,
    toast: impl Fn(adw::Toast),
) -> bool {
    let _hold = app.hold();
    let title = installer
        .file_name()
        .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
    let defaults = defaults();
    let found = gio::spawn_blocking(move || {
        proton::installer_command(
            &installer,
            proton_version.as_deref(),
            &prefix,
            &builds(),
            &defaults,
        )
    })
    .await;
    match found {
        Ok(Ok(command)) => run(app, window, &title, &command, None, &toast, |_| {}).await,
        Ok(Err(error)) => {
            toast(adw::Toast::new(&cannot_start_message(&title, &error)));
            false
        }
        Err(_) => {
            toast(adw::Toast::new(&could_not_start(&title)));
            false
        }
    }
}

/// The Proton builds on this machine, newest first.
pub fn builds() -> Vec<proton::ProtonBuild> {
    proton::discover(&proton::tool_dirs(
        &HostDirs::from_env(),
        &default_tools_dir(),
    ))
}

/// The Windows games defaults from Preferences. An empty build name stands for the
/// newest build, an empty folder for `~/Games/pindu-launcher/prefixes`.
pub fn defaults() -> proton::Defaults {
    let settings = gio::Settings::new(APP_ID);
    let proton = settings.string("default-proton");
    let folder = settings.string("prefix-folder");
    proton::Defaults {
        proton_version: (!proton.is_empty()).then(|| proton.to_string()),
        prefixes: if folder.is_empty() {
            default_prefixes_dir()
        } else {
            PathBuf::from(folder.as_str())
        },
        mangohud: settings.boolean("default-mangohud"),
        gamemode: settings.boolean("default-gamemode"),
    }
}

/// Runs a program through umu as our child and follows its log until umu exits, keeping
/// umu's output in `log` when there is one. The Steam Linux Runtime setup shows in a
/// dialog; `started` gets umu once it reports the program starting. Returns whether it
/// started; when it did not, the dialog or a toast says so.
async fn run(
    app: &gtk::Application,
    window: Option<gtk::Window>,
    title: &str,
    command: &Command,
    mut game_log: Option<&mut GameLog>,
    toast: &dyn Fn(adw::Toast),
    started: impl FnOnce(&gio::Subprocess),
) -> bool {
    let output = match game_log.as_deref().map(GameLog::output).transpose() {
        Ok(output) => output,
        Err(error) => {
            glib::g_warning!("pindu", "Could not open the log of “{title}”: {error}");
            None
        }
    };
    let child = match spawn(command, output) {
        Ok(child) => child,
        Err(error) => {
            glib::g_warning!("pindu", "Could not start {:?}: {error}", command.argv);
            toast(adw::Toast::new(&could_not_start(title)));
            return false;
        }
    };
    let setup = Setup::new(app.clone(), window, title.to_owned(), child.clone());
    let mut started = Some(started);
    let log = gio::DataInputStream::new(&child.stderr_pipe().expect("stderr is piped"));
    loop {
        let line = match log.read_line_future(glib::Priority::DEFAULT).await {
            Ok(Some(line)) => String::from_utf8_lossy(&line).into_owned(),
            Ok(None) => break,
            Err(error) => {
                glib::g_warning!("pindu", "Lost the log of “{title}”: {error}");
                break;
            }
        };
        // The program's log goes on to ours, as a host game's does.
        eprintln!("{line}");
        if let Some(game_log) = game_log.as_deref_mut() {
            game_log.line(&line);
        }
        match umu::setup_event(&line) {
            Some(RuntimeSetup::Ready) => {
                if let Some(started) = started.take() {
                    setup.close();
                    started(&child);
                }
            }
            Some(RuntimeSetup::Offline) => setup.show_offline(),
            Some(step) => setup.show(step),
            None => {}
        }
    }
    if let Err(error) = child.wait_future().await {
        glib::g_warning!("pindu", "Lost track of “{title}”: {error}");
    }
    let ran = started.is_none();
    if !ran && !setup.failed_offline() {
        if setup.is_shown() {
            setup.show_failed();
        } else {
            toast(adw::Toast::new(&could_not_start(title)));
        }
    }
    ran
}

/// Runs the command inside the sandbox, with its log piped to us, and its output written
/// to `output` when given.
fn spawn(command: &Command, output: Option<File>) -> Result<gio::Subprocess, glib::Error> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDERR_PIPE);
    if let Some(output) = output {
        launcher.take_stdout_fd(Some(output.into()));
    }
    for (key, value) in &command.environment {
        launcher.setenv(key, value, true);
    }
    if let Some(dir) = &command.working_dir {
        launcher.set_cwd(dir);
        // pressure-vessel takes its folder from `PWD` when that names the same folder.
        // Folders granted through the document portal must keep their
        // `/run/user/<uid>/doc/…` path: the container has no `/run/flatpak/doc`, which
        // is where that path leads in our sandbox.
        launcher.setenv("PWD", dir, true);
    }
    let argv: Vec<&OsStr> = command.argv.iter().map(OsStr::new).collect();
    launcher.spawn(&argv)
}

fn cannot_start_message(title: &str, error: &ProtonError) -> String {
    match error {
        ProtonError::NoBuilds => gettext("Could not start “{title}”: no Proton found"),
        ProtonError::Missing(build) => {
            gettext("Could not start “{title}”: {proton} is not installed")
                .replace("{proton}", build)
        }
    }
    .replace("{title}", title)
}

fn could_not_start(title: &str) -> String {
    gettext("Could not start “{title}”").replace("{title}", title)
}

/// The dialog that shows umu setting up the Steam Linux Runtime. It only appears once umu
/// reports a setup step, so starts with the runtime in place never show it.
struct Setup {
    app: gtk::Application,
    window: Option<gtk::Window>,
    title: String,
    child: gio::Subprocess,
    shown: RefCell<Option<Shown>>,
    offline: Cell<bool>,
    /// Whether umu is still setting up, so Cancel stops it. Cleared once the game starts
    /// or the setup ends, so closing the dialog never ends a running game.
    setting_up: Rc<Cell<bool>>,
}

struct Shown {
    dialog: adw::AlertDialog,
    progress: gtk::ProgressBar,
    step: Rc<Cell<RuntimeSetup>>,
    ticker: Option<glib::SourceId>,
}

impl Setup {
    fn new(
        app: gtk::Application,
        window: Option<gtk::Window>,
        title: String,
        child: gio::Subprocess,
    ) -> Self {
        Self {
            app,
            window,
            title,
            child,
            shown: RefCell::new(None),
            offline: Cell::new(false),
            setting_up: Rc::new(Cell::new(true)),
        }
    }

    fn is_shown(&self) -> bool {
        self.shown.borrow().is_some()
    }

    fn failed_offline(&self) -> bool {
        self.offline.get()
    }

    /// Shows the dialog at this step, opening it when needed.
    fn show(&self, step: RuntimeSetup) {
        self.ensure_shown().step.set(step);
    }

    /// The game cannot start: there is no runtime and no connection to download one.
    fn show_offline(&self) {
        self.offline.set(true);
        self.finish(
            &gettext("No Internet Connection"),
            &gettext(
                "Windows games need the Steam Linux Runtime, a one-time download. Connect to \
                 the internet, then try again.",
            ),
        );
    }

    /// umu ended before the game started.
    fn show_failed(&self) {
        self.finish(
            &gettext("Could Not Set Up Windows Games"),
            &gettext("The Steam Linux Runtime could not be installed. Try again later."),
        );
    }

    /// Closes the dialog once the game starts.
    fn close(&self) {
        self.setting_up.set(false);
        if let Some(mut shown) = self.shown.take() {
            if let Some(ticker) = shown.ticker.take() {
                ticker.remove();
            }
            shown.dialog.force_close();
        }
    }

    /// Turns the dialog into a message the user closes.
    fn finish(&self, heading: &str, body: &str) {
        self.setting_up.set(false);
        self.ensure_shown();
        let mut shown = self.shown.borrow_mut();
        let shown = shown.as_mut().expect("the dialog is shown");
        if let Some(ticker) = shown.ticker.take() {
            ticker.remove();
        }
        shown.dialog.set_heading(Some(heading));
        shown.dialog.set_body(body);
        shown
            .dialog
            .set_response_label("cancel", &gettext("_Close"));
        shown.progress.set_visible(false);
    }

    fn ensure_shown(&self) -> std::cell::RefMut<'_, Shown> {
        if self.shown.borrow().is_none() {
            *self.shown.borrow_mut() = Some(self.present());
        }
        std::cell::RefMut::map(self.shown.borrow_mut(), |shown| {
            shown.as_mut().expect("the dialog is shown")
        })
    }

    fn present(&self) -> Shown {
        let dialog = adw::AlertDialog::new(
            Some(&gettext("Setting Up Windows Games")),
            Some(
                &gettext(
                    "Windows games need the Steam Linux Runtime, a one-time download. \
                     “{title}” starts when it is ready.",
                )
                .replace("{title}", &self.title),
            ),
        );
        let progress = gtk::ProgressBar::builder().show_text(true).build();
        dialog.set_extra_child(Some(&progress));
        dialog.add_response("cancel", &gettext("_Cancel"));
        dialog.set_close_response("cancel");
        // Cancelling during the setup stops umu; once the game runs the dialog is gone.
        let (child, setting_up) = (self.child.clone(), self.setting_up.clone());
        dialog.connect_response(Some("cancel"), move |_, _| {
            if setting_up.get() {
                child.force_exit();
            }
        });
        let step = Rc::new(Cell::new(RuntimeSetup::Started));
        let download_dirs = download_dirs();
        let ticker = glib::timeout_add_local(PROGRESS_INTERVAL, {
            let (progress, step) = (progress.clone(), step.clone());
            move || {
                progress.pulse();
                progress.set_text(Some(&step_text(step.get(), &download_dirs)));
                glib::ControlFlow::Continue
            }
        });
        progress.set_text(Some(&step_text(RuntimeSetup::Started, &[])));
        // A launch without a window (Shell search, `--launch`) opens one to show this.
        if self.window.is_none() && self.app.active_window().is_none() {
            self.app.activate();
        }
        let parent = self.window.clone().or_else(|| self.app.active_window());
        dialog.present(parent.as_ref());
        Shown {
            dialog,
            progress,
            step,
            ticker: Some(ticker),
        }
    }
}

/// What the progress bar says at a setup step.
fn step_text(step: RuntimeSetup, download_dirs: &[PathBuf]) -> String {
    match step {
        RuntimeSetup::Downloading => match umu::downloaded_bytes(download_dirs) {
            Some(bytes) => {
                gettext("Downloading… {size}").replace("{size}", &glib::format_size(bytes))
            }
            None => gettext("Downloading…"),
        },
        RuntimeSetup::Unpacking => gettext("Unpacking…"),
        RuntimeSetup::Verifying => gettext("Checking…"),
        RuntimeSetup::Started | RuntimeSetup::Ready | RuntimeSetup::Offline => {
            gettext("Preparing…")
        }
    }
}

/// Where umu keeps its partial download: under the temporary folder, or umu's cache.
fn download_dirs() -> Vec<PathBuf> {
    vec![std::env::temp_dir(), glib::user_cache_dir().join("umu")]
}
