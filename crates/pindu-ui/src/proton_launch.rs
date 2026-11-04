//! Runs Windows games through umu and Proton inside the sandbox, as our child. umu's log is
//! read as it comes, so a first start, which downloads the Steam Linux Runtime, shows its
//! progress, and a missing internet connection is said plainly instead of the game
//! silently not starting.

use std::cell::{Cell, RefCell};
use std::ffi::OsStr;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::launch::Command;
use pindu_core::model::{Game, LaunchConfig};
use pindu_core::paths::{default_prefixes_dir, default_tools_dir};
use pindu_core::proton::{self, ProtonError};
use pindu_core::sources::location::HostDirs;
use pindu_core::umu::{self, RuntimeSetup};

use crate::launch_service::now;
use crate::library_service::LibraryService;

/// How often the setup dialog's progress moves.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(250);

/// Starts a Windows game with the Proton build and prefix its launch config names, or the
/// newest build and a prefix of its own. The app stays up while the game runs, since the
/// game writes its log to us.
pub fn play(
    app: &gtk::Application,
    window: Option<&gtk::Window>,
    game: Game,
    config: LaunchConfig,
    library: LibraryService,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let (app, window) = (app.clone(), window.cloned());
    let hold = app.hold();
    glib::spawn_future_local(async move {
        let found = {
            let game = game.clone();
            gio::spawn_blocking(move || {
                proton::command(&game, &config, &builds(), &default_prefixes_dir())
            })
            .await
        };
        let title = game.title.clone();
        let command = match found {
            Ok(Ok(command)) => command,
            Ok(Err(error)) => {
                toast(adw::Toast::new(&cannot_start_message(&title, &error)));
                return;
            }
            Err(_) => {
                toast(adw::Toast::new(&could_not_start(&title)));
                return;
            }
        };
        let toast = Rc::new(toast);
        let started = {
            let (toast, title) = (toast.clone(), title.clone());
            move || {
                let (library, id) = (library.clone(), game.id.clone());
                glib::spawn_future_local(async move { library.mark_played(&id, now()).await });
                toast(adw::Toast::new(
                    &gettext("Starting “{title}”").replace("{title}", &title),
                ));
            }
        };
        run(&app, window, &title, &command, &*toast, started).await;
        drop(hold);
    });
}

/// Runs a game's installer through umu in the prefix the game will use, with the named
/// Proton build or the newest one, and waits until it exits. Returns whether it ran.
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
    let found = gio::spawn_blocking(move || {
        proton::installer_command(&installer, proton_version.as_deref(), &prefix, &builds())
    })
    .await;
    match found {
        Ok(Ok(command)) => run(app, window, &title, &command, &toast, || {}).await,
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

/// Runs a program through umu as our child and follows its log until umu exits. The
/// Steam Linux Runtime setup shows in a dialog; `started` runs once umu reports the
/// program starting. Returns whether it started; when it did not, the dialog or a toast
/// says so.
async fn run(
    app: &gtk::Application,
    window: Option<gtk::Window>,
    title: &str,
    command: &Command,
    toast: &dyn Fn(adw::Toast),
    started: impl FnOnce(),
) -> bool {
    let child = match spawn(command) {
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
        match umu::setup_event(&line) {
            Some(RuntimeSetup::Ready) => {
                if let Some(started) = started.take() {
                    setup.close();
                    started();
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

/// Runs the command inside the sandbox, with its log piped to us.
fn spawn(command: &Command) -> Result<gio::Subprocess, glib::Error> {
    let launcher = gio::SubprocessLauncher::new(gio::SubprocessFlags::STDERR_PIPE);
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
