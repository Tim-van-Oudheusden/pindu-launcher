//! Keeps the System Update section (see `pindu_core::system::update`) for the shells, over
//! [`PinduOsBackend`]: the state they show, Restart and Roll Back for them to call, and a
//! notification when the system refuses one.

use gettextrs::gettext;
use gtk::glib;
use pindu_core::notifications::Kind;
use pindu_core::system::update::{Failure, STATUS_FILE, SystemUpdate, UpdateState};

use crate::notification_service::NotificationService;
use crate::pindu_os_backend::PinduOsBackend;

/// The System Update section, read at start-up and kept up to date for the shells.
#[derive(Clone, Debug)]
pub struct SystemUpdateService {
    update: SystemUpdate,
}

impl SystemUpdateService {
    /// Reads the section now, posting what goes wrong to `notifications`.
    pub fn new(notifications: &NotificationService) -> Self {
        let update = SystemUpdate::new(Box::new(PinduOsBackend::new()));
        let notifications = notifications.clone();
        update.connect_failed(move |failure| match failure {
            Failure::Status(error) => {
                glib::g_warning!("pindu", "Cannot read {STATUS_FILE}: {error}");
            }
            Failure::RollBack(error) => {
                glib::g_warning!("pindu", "Could not roll back: {error}");
                notifications.post(
                    Kind::SystemFailed,
                    &gettext("Could not roll back"),
                    error,
                    None,
                );
            }
            Failure::Restart(error) => {
                glib::g_warning!("pindu", "Could not restart: {error}");
                notifications.post(
                    Kind::SystemFailed,
                    &gettext("Could not restart"),
                    error,
                    None,
                );
            }
        });
        update.refresh();
        Self { update }
    }

    /// The section as last read.
    pub fn state(&self) -> UpdateState {
        self.update.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&UpdateState) + 'static) {
        self.update.connect_changed(listener);
    }

    /// Reads the section again: its level opens.
    pub fn refresh(&self) {
        self.update.refresh();
    }

    /// Restarts into the staged update or the queued rollback.
    pub fn restart(&self) {
        self.update.restart();
    }

    /// Rolls back to the previous version, then restarts. The shell asks first.
    pub fn roll_back(&self) {
        self.update.roll_back();
    }
}
