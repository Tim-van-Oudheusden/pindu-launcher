//! The Pindu OS backend of the System Update section (`pindu_core::system::update`): reads
//! os-release and the update status file Pindu OS writes, follows that file with a
//! `gio::FileMonitor`, starts the rollback unit through systemd and restarts through logind,
//! both on the system bus. Elsewhere os-release names another system, and nothing here
//! touches the bus. The Flatpak has no system bus and does not see `/run/pindu-os`, so the
//! section stays hidden there.

use std::cell::RefCell;
use std::io::ErrorKind;
use std::rc::Rc;

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use pindu_core::system::update::{
    Done, Facts, OS_RELEASE_FILES, ROLLBACK_UNIT, STATUS_FILE, UpdateBackend,
};

/// systemd's bus name, object and manager interface.
const SYSTEMD: &str = "org.freedesktop.systemd1";
const SYSTEMD_PATH: &str = "/org/freedesktop/systemd1";
const SYSTEMD_MANAGER: &str = "org.freedesktop.systemd1.Manager";

/// logind's bus name, object and manager interface.
const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";

/// The result systemd gives a job that ran to the end (`JobRemoved`).
const JOB_DONE: &str = "done";

/// Pindu OS's update status file, its rollback unit and logind's restart.
#[derive(Default)]
pub struct PinduOsBackend {
    /// Follows the status file once [`UpdateBackend::watch`] is called.
    monitor: RefCell<Option<gio::FileMonitor>>,
}

impl PinduOsBackend {
    /// A backend that has read nothing yet.
    pub fn new() -> Self {
        Self::default()
    }
}

impl UpdateBackend for PinduOsBackend {
    fn read(&self, done: Box<dyn FnOnce(Facts)>) {
        glib::spawn_future_local(async move {
            let facts = gio::spawn_blocking(read_facts).await.unwrap_or_default();
            done(facts);
        });
    }

    fn watch(&self, changed: Box<dyn Fn()>) {
        let file = gio::File::for_path(STATUS_FILE);
        let monitor = match file.monitor_file(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE) {
            Ok(monitor) => monitor,
            Err(error) => {
                glib::g_warning!("pindu", "Cannot follow {STATUS_FILE}: {error}");
                return;
            }
        };
        // Pindu OS writes a temporary file and renames it over this one, which shows up as
        // the file created.
        monitor.connect_changed(move |_, _, _, event| {
            if matches!(
                event,
                gio::FileMonitorEvent::Created
                    | gio::FileMonitorEvent::Deleted
                    | gio::FileMonitorEvent::ChangesDoneHint
            ) {
                changed();
            }
        });
        self.monitor.replace(Some(monitor));
    }

    fn roll_back(&self, done: Done) {
        glib::spawn_future_local(start_rollback(done));
    }

    fn restart(&self, done: Done) {
        glib::spawn_future_local(async move {
            done(reboot().await.map_err(bus_error_message));
        });
    }
}

/// Reads os-release and the status file; a file that is not there is `None`.
fn read_facts() -> Facts {
    Facts {
        os_release: OS_RELEASE_FILES.iter().find_map(|path| read_file(path)),
        status: read_file(STATUS_FILE),
    }
}

/// The text of `path`, or `None` when it is not there or cannot be read.
fn read_file(path: &str) -> Option<String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Some(text),
        Err(error) if error.kind() == ErrorKind::NotFound => None,
        Err(error) => {
            glib::g_warning!("pindu", "Cannot read {path}: {error}");
            None
        }
    }
}

/// Starts [`ROLLBACK_UNIT`] and reports to `done` once systemd has removed its job: `Ok`
/// when it ran to the end. The job is followed by the unit's name in `JobRemoved`, which
/// the subscription catches even if it comes before `StartUnit`'s reply.
async fn start_rollback(done: Done) {
    let bus = match gio::bus_get_future(gio::BusType::System).await {
        Ok(bus) => bus,
        Err(error) => return done(Err(bus_error_message(error))),
    };
    // systemd sends job signals only once a client has subscribed; a second subscription
    // is refused, which changes nothing.
    if let Err(error) = bus
        .call_future(
            Some(SYSTEMD),
            SYSTEMD_PATH,
            SYSTEMD_MANAGER,
            "Subscribe",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
    {
        glib::g_debug!("pindu", "systemd Subscribe: {error}");
    }
    let subscription = Rc::new(RefCell::new(None::<gio::SignalSubscription>));
    let done = Rc::new(RefCell::new(Some(done)));
    let finish = {
        let subscription = subscription.clone();
        Rc::new(move |result: Result<(), String>| {
            // Unsubscribe once this callback has returned.
            if let Some(subscription) = subscription.take() {
                glib::idle_add_local_once(move || drop(subscription));
            }
            if let Some(done) = done.take() {
                done(result);
            }
        })
    };
    let on_removed = finish.clone();
    subscription.replace(Some(bus.subscribe_to_signal(
        Some(SYSTEMD),
        Some(SYSTEMD_MANAGER),
        Some("JobRemoved"),
        Some(SYSTEMD_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some((_, _, unit, result)) =
                signal
                    .parameters
                    .get::<(u32, glib::variant::ObjectPath, String, String)>()
            else {
                return;
            };
            if unit != ROLLBACK_UNIT {
                return;
            }
            on_removed(if result == JOB_DONE {
                Ok(())
            } else {
                Err(format!("{ROLLBACK_UNIT}: {result}"))
            });
        },
    )));
    if let Err(error) = bus
        .call_future(
            Some(SYSTEMD),
            SYSTEMD_PATH,
            SYSTEMD_MANAGER,
            "StartUnit",
            Some(&(ROLLBACK_UNIT, "replace").to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
    {
        finish(Err(bus_error_message(error)));
    }
}

/// Asks logind to restart, letting polkit ask for a password where the session has an
/// agent for it.
async fn reboot() -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::System).await?;
    bus.call_future(
        Some(LOGIND),
        LOGIND_PATH,
        LOGIND_MANAGER,
        "Reboot",
        Some(&(true,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
    )
    .await?;
    Ok(())
}

/// The message of a bus error without the remote error's name in front of it.
fn bus_error_message(mut error: glib::Error) -> String {
    gio::DBusError::strip_remote_error(&mut error);
    error.message().to_owned()
}
