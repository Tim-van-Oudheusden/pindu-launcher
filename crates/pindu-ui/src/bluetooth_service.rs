//! Keeps the Bluetooth section (see `pindu_core::system::bluetooth`) for the shells, over
//! [`BluezBackend`]: the state they show, the switch, pairing, connecting, disconnecting
//! and forgetting for them to call, the level opening and closing, and a notification when
//! one of those fails.

use gettextrs::gettext;
use gtk::glib;
use pindu_core::notifications::Kind;
use pindu_core::system::bluetooth::{Bluetooth, BluetoothState, Failure};

use crate::bluez_backend::BluezBackend;
use crate::notification_service::NotificationService;

/// The Bluetooth section, following BlueZ from start-up on.
#[derive(Clone, Debug)]
pub struct BluetoothService {
    bluetooth: Bluetooth,
}

impl BluetoothService {
    /// Follows BlueZ now, posting what goes wrong to `notifications`.
    pub fn new(notifications: &NotificationService) -> Self {
        let bluetooth = Bluetooth::new(Box::new(BluezBackend::new()));
        let notifications = notifications.clone();
        bluetooth.connect_failed(move |failure| {
            let (title, error) = match failure {
                Failure::Power { on: true, error } => {
                    (gettext("Could not turn Bluetooth on"), error)
                }
                Failure::Power { on: false, error } => {
                    (gettext("Could not turn Bluetooth off"), error)
                }
                Failure::Pair { device, error } => (
                    // Translators: {device} is a Bluetooth device's name, such as a
                    // controller's.
                    gettext("Could not pair {device}").replace("{device}", device),
                    error,
                ),
                Failure::Connect { device, error } => (
                    // Translators: {device} is a Bluetooth device's name, such as a
                    // controller's.
                    gettext("Could not connect {device}").replace("{device}", device),
                    error,
                ),
                Failure::Disconnect { device, error } => (
                    // Translators: {device} is a Bluetooth device's name, such as a
                    // controller's.
                    gettext("Could not disconnect {device}").replace("{device}", device),
                    error,
                ),
                Failure::Forget { device, error } => (
                    // Translators: {device} is a Bluetooth device's name, such as a
                    // controller's.
                    gettext("Could not forget {device}").replace("{device}", device),
                    error,
                ),
                Failure::Discovery(error) => {
                    glib::g_warning!("pindu", "Bluetooth discovery: {error}");
                    return;
                }
                Failure::Agent(error) => {
                    glib::g_warning!("pindu", "Bluetooth pairing agent: {error}");
                    return;
                }
            };
            glib::g_warning!("pindu", "{title}: {error}");
            notifications.post(Kind::BluetoothFailed, &title, error, None);
        });
        bluetooth.start();
        Self { bluetooth }
    }

    /// The section as BlueZ last reported it.
    pub fn state(&self) -> BluetoothState {
        self.bluetooth.state()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&BluetoothState) + 'static) {
        self.bluetooth.connect_changed(listener);
    }

    /// The level opens: discovery and the pairing agent run.
    pub fn open(&self) {
        self.bluetooth.open();
    }

    /// The level closes: discovery and the pairing agent stop.
    pub fn close(&self) {
        self.bluetooth.close();
    }

    /// Turns the radio on or off.
    pub fn set_powered(&self, powered: bool) {
        self.bluetooth.set_powered(powered);
    }

    /// Pairs, trusts and connects a found device.
    pub fn pair(&self, device: &str) {
        self.bluetooth.pair(device);
    }

    /// Connects a paired device.
    pub fn connect(&self, device: &str) {
        self.bluetooth.connect(device);
    }

    /// Disconnects a paired device.
    pub fn disconnect(&self, device: &str) {
        self.bluetooth.disconnect(device);
    }

    /// Forgets a paired device. The shell asks first.
    pub fn forget(&self, device: &str) {
        self.bluetooth.forget(device);
    }

    /// Answers a code to confirm.
    pub fn confirm(&self, accept: bool) {
        self.bluetooth.confirm(accept);
    }

    /// Answers a PIN or passkey request with what was typed, or `None`.
    pub fn enter(&self, code: Option<String>) {
        self.bluetooth.enter(code);
    }

    /// Closes a shown code, giving up the pairing.
    pub fn dismiss(&self) {
        self.bluetooth.dismiss();
    }
}
