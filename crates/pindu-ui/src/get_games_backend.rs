//! The backend of Get Games' jobs (`pindu_core::get_games`): runs the host's `flatpak` as
//! argv lists, one install or removal at a time with its output read line by line, asks it
//! questions in the C locale, and measures the free space of the installation. The native
//! build only: the Flatpak build cannot run the host's `flatpak`.

use std::cell::RefCell;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::add_store::{Done, Line};
use pindu_core::get_games::{Answer, Free, GetGamesBackend};

use crate::add_store_backend::run;

/// `SIGTERM`, which `flatpak` ends on, undoing what it had not finished.
const SIGTERM: i32 = 15;

/// Runs `flatpak` from `PATH` against the installation in `dir`.
#[derive(Debug)]
pub struct FlatpakJobs {
    /// The install or removal that runs, to stop it.
    running: Rc<RefCell<Option<gio::Subprocess>>>,
    /// The installation's folder, `/var/lib/flatpak` or the user's, whose disk the free
    /// space is measured on.
    dir: PathBuf,
}

impl FlatpakJobs {
    /// A backend for the installation in `dir`, with nothing running.
    pub fn new(dir: PathBuf) -> Self {
        Self {
            running: Rc::default(),
            dir,
        }
    }
}

impl GetGamesBackend for FlatpakJobs {
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

    fn query(&self, argv: Vec<String>, answer: Answer) {
        glib::spawn_future_local(async move {
            answer(query(&argv).await);
        });
    }

    fn free_space(&self, free: Free) {
        let file = gio::File::for_path(&self.dir);
        glib::spawn_future_local(async move {
            let info = file
                .query_filesystem_info_future(
                    gio::FILE_ATTRIBUTE_FILESYSTEM_FREE,
                    glib::Priority::DEFAULT,
                )
                .await;
            free(
                info.ok()
                    .map(|info| info.attribute_uint64(gio::FILE_ATTRIBUTE_FILESYSTEM_FREE)),
            );
        });
    }
}

/// Runs `argv` in the C locale with its standard input closed, and gives what it printed
/// on standard output, or on failure what it printed on standard error (else on standard
/// output, else how it ended).
pub(crate) async fn query(argv: &[String]) -> Result<String, String> {
    let launcher = gio::SubprocessLauncher::new(
        gio::SubprocessFlags::STDIN_PIPE
            | gio::SubprocessFlags::STDOUT_PIPE
            | gio::SubprocessFlags::STDERR_PIPE,
    );
    launcher.setenv("LC_ALL", "C.UTF-8", true);
    let args: Vec<&OsStr> = argv.iter().map(OsStr::new).collect();
    let process = launcher.spawn(&args).map_err(|error| error.to_string())?;
    let (stdout, stderr) = process
        .communicate_utf8_future(None)
        .await
        .map_err(|error| error.to_string())?;
    let stdout = stdout.map(String::from).unwrap_or_default();
    if process.is_successful() {
        return Ok(stdout);
    }
    let stderr = stderr.map(String::from).unwrap_or_default();
    Err(if !stderr.trim().is_empty() {
        stderr
    } else if !stdout.trim().is_empty() {
        stdout
    } else {
        format!(
            "{} ended without saying why",
            argv.first().map_or("flatpak", String::as_str)
        )
    })
}
