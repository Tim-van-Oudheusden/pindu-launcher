//! The PipeWire and WirePlumber backend of the Sound Output section
//! (`pindu_core::system::audio`): runs `pw-dump` to read the outputs and `wpctl set-default`
//! to choose one, each as an argv list, off the main thread. Neither tool has a D-Bus
//! interface to call instead. In the Flatpak, which has neither tool, and wherever one is
//! not installed, it runs nothing and reports the section unavailable.

use std::ffi::OsStr;

use gtk::{gio, glib};
use pindu_core::system::audio::{
    AudioBackend, DUMP_COMMAND, Done, Dump, ReadDone, TOOLS, set_default_command,
};

use crate::permission_service::in_flatpak;

/// Runs `pw-dump` and `wpctl` from `PATH`.
#[derive(Debug, Default)]
pub struct WirePlumberBackend;

impl WirePlumberBackend {
    /// A backend over the tools on `PATH`.
    pub fn new() -> Self {
        Self
    }
}

impl AudioBackend for WirePlumberBackend {
    fn read(&self, done: ReadDone) {
        if !tools_available() {
            done(Dump::Unavailable);
            return;
        }
        glib::spawn_future_local(async move {
            done(match run(&DUMP_COMMAND).await {
                Ok(text) => Dump::Text(text),
                Err(error) => Dump::Failed(error),
            });
        });
    }

    fn set_default(&self, id: u32, done: Done) {
        let argv = set_default_command(id);
        glib::spawn_future_local(async move {
            done(run(&argv).await.map(drop));
        });
    }
}

/// Whether both tools can run: never in the Flatpak, and only when both are installed.
fn tools_available() -> bool {
    !in_flatpak()
        && TOOLS
            .iter()
            .all(|tool| glib::find_program_in_path(tool).is_some())
}

/// Runs `argv` and gives its standard output, or what went wrong: its standard error, or
/// how it ended when it wrote none.
async fn run(argv: &[impl AsRef<OsStr>]) -> Result<String, String> {
    let argv: Vec<&OsStr> = argv.iter().map(AsRef::as_ref).collect();
    let process = gio::Subprocess::newv(
        &argv,
        gio::SubprocessFlags::STDOUT_PIPE | gio::SubprocessFlags::STDERR_PIPE,
    )
    .map_err(|error| error.to_string())?;
    let (stdout, stderr) = process
        .communicate_utf8_future(None)
        .await
        .map_err(|error| error.to_string())?;
    if process.is_successful() {
        return Ok(stdout.map(String::from).unwrap_or_default());
    }
    let stderr = stderr.as_deref().map(str::trim).unwrap_or_default();
    if !stderr.is_empty() {
        return Err(stderr.to_owned());
    }
    let program = argv[0].to_string_lossy();
    Err(if process.has_exited() {
        format!("{program} exited with status {}", process.exit_status())
    } else {
        format!("{program} was stopped by signal {}", process.term_sig())
    })
}
