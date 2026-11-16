//! Keeps the Network section (see `pindu_core::system::network`) for the shells, over
//! [`NetworkManagerBackend`]: the state they show, the joins, the passphrase answer and
//! Forget for them to call, and a notification when something fails.

use gettextrs::gettext;
use gtk::glib;
use pindu_core::notifications::Kind;
use pindu_core::system::network::{Failure, JoinError, Network, NetworkState};

use crate::networkmanager_backend::NetworkManagerBackend;
use crate::notification_service::NotificationService;

/// The Network section, followed from start-up for the shells.
#[derive(Clone, Debug)]
pub struct NetworkService {
    network: Network,
}

impl NetworkService {
    /// Starts following NetworkManager, posting what goes wrong to `notifications`.
    pub fn new(notifications: &NotificationService) -> Self {
        let network = Network::new(Box::new(NetworkManagerBackend::new()));
        let notifications = notifications.clone();
        network.connect_failed(move |failure| {
            let (title, body) = match failure {
                Failure::Join { name, error } => {
                    glib::g_warning!("pindu", "Could not join {name}: {error:?}");
                    (
                        // Translators: {network} is a Wi-Fi network's name.
                        gettext("Could not join {network}").replace("{network}", name),
                        join_error_text(error),
                    )
                }
                Failure::Forget { name, error } => {
                    glib::g_warning!("pindu", "Could not forget {name}: {error}");
                    (
                        // Translators: {network} is a Wi-Fi network's name.
                        gettext("Could not forget {network}").replace("{network}", name),
                        error.clone(),
                    )
                }
                Failure::Radio(error) => {
                    glib::g_warning!("pindu", "Could not switch Wi-Fi: {error}");
                    (gettext("Could not switch Wi-Fi"), error.clone())
                }
            };
            notifications.post(Kind::NetworkFailed, &title, &body, None);
        });
        network.start();
        Self { network }
    }

    /// The section as last reported.
    pub fn state(&self) -> NetworkState {
        self.network.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&NetworkState) + 'static) {
        self.network.connect_changed(listener);
    }

    /// The level opens: Wi-Fi looks for networks.
    pub fn open(&self) {
        self.network.open();
    }

    /// Joins the network `ssid`, asking for its passphrase if it needs one.
    pub fn join(&self, ssid: &[u8]) {
        self.network.join(ssid);
    }

    /// Answers the passphrase prompt; `None` cancels it.
    pub fn enter(&self, passphrase: Option<&str>) {
        self.network.enter(passphrase);
    }

    /// Forgets the network `ssid`. The shell asks first.
    pub fn forget(&self, ssid: &[u8]) {
        self.network.forget(ssid);
    }

    /// Switches Wi-Fi on or off.
    pub fn set_wireless(&self, enabled: bool) {
        self.network.set_wireless(enabled);
    }
}

/// What a notification says about why a join failed.
fn join_error_text(error: &JoinError) -> String {
    match error {
        JoinError::Refused(message) => message.clone(),
        JoinError::Passphrase => gettext("The saved passphrase was not accepted."),
        JoinError::OutOfRange => gettext("The network is out of range."),
        JoinError::NoAddress => gettext("The network gave no address."),
        // Translators: {reason} is a number NetworkManager gives for a failed connection.
        JoinError::Failed(reason) => gettext("The connection failed (reason {reason}).")
            .replace("{reason}", &reason.to_string()),
    }
}
