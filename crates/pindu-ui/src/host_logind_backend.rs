//! The Power section's backend in the Flatpak (`pindu_core::system::power`), whose sandbox
//! has no system bus: with the host-spawn override it asks the host's logind through
//! `busctl --system call`, run on the host with `flatpak-spawn --host`
//! ([`session_service::spawn_captured`]), and reads the answers with
//! `pindu_core::system::power::busctl`. `CanSuspend`, `CanReboot` and `CanPowerOff` say
//! what the session may do, `Suspend`, `Reboot` and `PowerOff` do it, and `ListInhibitors`
//! names who stops it. It does not follow logind coming and going on the bus; the section
//! reads it again when it is refreshed.

use gtk::glib;
use pindu_core::launch::Command;
use pindu_core::system::power::{Done, Facts, Inhibitor, PowerAction, PowerBackend, busctl};

use crate::session_service;

/// What a command run on the host wrote, and how it ended.
struct Output {
    /// Whether it exited with status 0.
    success: bool,
    /// Its exit status, or -1 when it did not exit by itself.
    status: i32,
    /// What it wrote to its output.
    stdout: String,
    /// What it wrote to its errors.
    stderr: String,
}

/// logind on the host, through `busctl`.
#[derive(Debug, Default)]
pub struct HostLogindBackend;

impl HostLogindBackend {
    /// The backend.
    pub fn new() -> Self {
        Self
    }
}

impl PowerBackend for HostLogindBackend {
    fn read(&self, done: Box<dyn FnOnce(Facts)>) {
        glib::spawn_future_local(async move {
            let mut answers = [None; 3];
            for (answer, action) in answers.iter_mut().zip(PowerAction::ALL) {
                *answer = match run_on_host(&busctl::can_command(action)).await {
                    Ok(output) if output.success => busctl::parse_answer(&output.stdout),
                    Ok(output) => {
                        let error = busctl::error_message(&output.stderr, output.status);
                        glib::g_debug!("pindu", "host logind {action:?}: {error}");
                        None
                    }
                    Err(error) => {
                        glib::g_debug!("pindu", "Cannot run busctl on the host: {error}");
                        None
                    }
                };
            }
            done(busctl::facts(answers));
        });
    }

    /// Follows nothing: `busctl` cannot tell when logind comes or goes.
    fn watch(&self, _changed: Box<dyn Fn()>) {}

    fn run(&self, action: PowerAction, done: Done) {
        glib::spawn_future_local(async move {
            let result = match run_on_host(&busctl::run_command(action)).await {
                Ok(output) if output.success => Ok(()),
                Ok(output) => Err(busctl::error_message(&output.stderr, output.status)),
                Err(error) => Err(error.message().to_owned()),
            };
            done(result);
        });
    }

    fn inhibitors(&self, done: Box<dyn FnOnce(Vec<Inhibitor>)>) {
        glib::spawn_future_local(async move {
            let inhibitors = match run_on_host(&busctl::inhibitors_command()).await {
                Ok(output) if output.success => busctl::parse_inhibitors(&output.stdout),
                Ok(output) => {
                    let error = busctl::error_message(&output.stderr, output.status);
                    glib::g_debug!("pindu", "host logind ListInhibitors: {error}");
                    Vec::new()
                }
                Err(error) => {
                    glib::g_debug!("pindu", "Cannot run busctl on the host: {error}");
                    Vec::new()
                }
            };
            done(inhibitors);
        });
    }
}

/// Runs `command` on the host and waits for it, with what it wrote.
async fn run_on_host(command: &Command) -> Result<Output, glib::Error> {
    let child = session_service::spawn_captured(command)?;
    let (stdout, stderr) = child.communicate_utf8_future(None).await?;
    let status = if child.has_exited() {
        child.exit_status()
    } else {
        -1
    };
    Ok(Output {
        success: child.is_successful(),
        status,
        stdout: stdout.map(String::from).unwrap_or_default(),
        stderr: stderr.map(String::from).unwrap_or_default(),
    })
}
