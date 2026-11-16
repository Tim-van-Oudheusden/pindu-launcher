//! Keeps the Power section (see `pindu_core::system::power`) for the shells, over
//! [`LogindBackend`]: which of Suspend, Restart and Power Off show, the actions for them to
//! call, and a notification when logind refuses one, naming the programs that stop it.

use gettextrs::gettext;
use gtk::glib;
use pindu_core::notifications::Kind;
use pindu_core::system::power::{Failure, PowerAction, PowerState, SystemPower};

use crate::logind_backend::LogindBackend;
use crate::notification_service::NotificationService;

/// The Power section, read at start-up and kept up to date for the shells.
#[derive(Clone, Debug)]
pub struct SystemPowerService {
    power: SystemPower,
}

impl SystemPowerService {
    /// Reads the section now, posting what logind refuses to `notifications`.
    pub fn new(notifications: &NotificationService) -> Self {
        let power = SystemPower::new(Box::new(LogindBackend::new()));
        let notifications = notifications.clone();
        power.connect_failed(move |failure| match failure {
            Failure::Inhibited { action, holders } => {
                let holders = holders.join(", ");
                // Translators: {holders} names programs, such as "GNOME Software, Steam".
                let body = gettext("Stopped by {holders}").replace("{holders}", &holders);
                notifications.post(Kind::PowerInhibited, &refused(*action), &body, None);
            }
            Failure::Refused { action, error } => {
                glib::g_warning!("pindu", "logind refused {action:?}: {error}");
                notifications.post(Kind::SystemFailed, &refused(*action), error, None);
            }
        });
        power.refresh();
        Self { power }
    }

    /// The section as last read.
    pub fn state(&self) -> PowerState {
        self.power.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&PowerState) + 'static) {
        self.power.connect_changed(listener);
    }

    /// Suspends, restarts or powers off the machine. The shell asks first where
    /// `pindu_core::system::power::asks_first` says so.
    pub fn run(&self, action: PowerAction) {
        self.power.run(action);
    }
}

/// The title of a notification saying `action` did not happen.
fn refused(action: PowerAction) -> String {
    match action {
        PowerAction::Suspend => gettext("Could not suspend"),
        PowerAction::Restart => gettext("Could not restart"),
        PowerAction::PowerOff => gettext("Could not power off"),
    }
}
