//! The backend of the "add a store" flow (`pindu_core::add_store`): finds the installed
//! stores off the main thread, runs the host's `flatpak` as argv lists, one at a time,
//! with its output read line by line, and opens a store's `appstream://` page through
//! GTK's URI launcher, which goes through the OpenURI portal in the Flatpak.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::add_store::{AddStoreBackend, Detected, Done, Line, installed_stores};
use pindu_core::sources::location::HostDirs;

/// `SIGTERM`, which `flatpak` ends on, undoing what it had not finished.
const SIGTERM: i32 = 15;

/// Runs `flatpak` from `PATH`, and opens pages with the default handler.
#[derive(Debug, Default)]
pub struct FlatpakBackend {
    /// The command that runs, to stop it.
    running: Rc<RefCell<Option<gio::Subprocess>>>,
}

impl FlatpakBackend {
    /// A backend with nothing running.
    pub fn new() -> Self {
        Self::default()
    }
}

impl AddStoreBackend for FlatpakBackend {
    fn detect(&self, done: Detected) {
        glib::spawn_future_local(async move {
            let stores = gio::spawn_blocking(|| installed_stores(&HostDirs::from_env()))
                .await
                .unwrap_or_default();
            done(stores);
        });
    }

    fn run(&self, argv: Vec<String>, line: Line, done: Done) {
        let running = self.running.clone();
        glib::spawn_future_local(async move {
            let result = run(&argv, &running, line).await;
            running.replace(None);
            done(result);
        });
    }

    fn stop(&self) {
        if let Some(process) = self.running.borrow().as_ref() {
            process.send_signal(SIGTERM);
        }
    }

    fn open(&self, uri: String, done: Done) {
        let parent = gio::Application::default()
            .and_downcast::<gtk::Application>()
            .and_then(|application| application.active_window());
        gtk::UriLauncher::new(&uri).launch(
            parent.as_ref(),
            gio::Cancellable::NONE,
            move |result| done(result.map_err(|error| error.to_string())),
        );
    }
}

/// Runs `argv`, with its standard error merged into its standard output, giving `line`
/// each line; on failure the error is everything it printed, for
/// `pindu_core::add_store::error_message` to pick from, or how it ended when it printed
/// nothing.
async fn run(
    argv: &[String],
    running: &RefCell<Option<gio::Subprocess>>,
    line: Line,
) -> Result<(), String> {
    let args: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    let process = gio::Subprocess::newv(
        &args,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_MERGE,
    )
    .map_err(|error| error.to_string())?;
    running.replace(Some(process.clone()));
    let mut printed = String::new();
    if let Some(stdout) = process.stdout_pipe() {
        let reader = gio::DataInputStream::new(&stdout);
        while let Ok(Some(text)) = reader.read_line_utf8_future(glib::Priority::DEFAULT).await {
            line(&text);
            printed.push_str(&text);
            printed.push('\n');
        }
    }
    process
        .wait_future()
        .await
        .map_err(|error| error.to_string())?;
    if process.is_successful() {
        return Ok(());
    }
    if !printed.trim().is_empty() {
        return Err(printed);
    }
    let program = argv.first().map_or("flatpak", String::as_str);
    Err(if process.has_exited() {
        format!("{program} exited with status {}", process.exit_status())
    } else {
        format!("{program} was stopped by signal {}", process.term_sig())
    })
}
