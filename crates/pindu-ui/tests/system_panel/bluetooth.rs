//! The Bluetooth section over [`BluezBackend`], against dbusmock's `bluez5` template.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::glib;
use gtk::glib::prelude::*;
use pindu_core::system::bluetooth::{
    Bluetooth, BluetoothState, Device, Failure, Power, Prompt, Summary,
};
use pindu_ui::bluez_backend::BluezBackend;

use crate::bus::{Mock, settle, system_bus, wait_until};

const BLUEZ: &str = "org.bluez";
/// The template's own interface, with its `Add…` methods, on its root object.
const BLUEZ_MOCK: &str = "org.bluez.Mock";
/// Where BlueZ's agent manager lives.
const BLUEZ_PATH: &str = "/org/bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";

/// `Device1.Pair` asking the default agent, which `@CLIENT@` exports, to confirm a code,
/// as BlueZ does for numeric comparison, and failing as BlueZ does when it refuses.
const REFUSED_PAIR: &str = r"
try:
    self.connection.call_blocking(
        '@CLIENT@', objects['/org/bluez'].default_agent, 'org.bluez.Agent1',
        'RequestConfirmation', 'ou', [dbus.ObjectPath(self.path), dbus.UInt32(12345)])
except dbus.exceptions.DBusException:
    raise dbus.exceptions.DBusException('Authentication Rejected',
                                        name='org.bluez.Error.AuthenticationRejected')
";

/// BlueZ with no adapter yet.
fn bluez() -> Mock {
    Mock::start("bluez5", BLUEZ, None)
}

/// Gives `bluez` an adapter, returning its path.
fn add_adapter(bluez: &Mock) -> String {
    let (adapter,) = bluez
        .call(
            "/",
            BLUEZ_MOCK,
            "AddAdapter",
            Some(("hci0", "pindu").to_variant()),
        )
        .get::<(String,)>()
        .expect("an adapter path");
    // The template's `StartDiscovery` (dbusmock 0.38.1) fails with a `KeyError` until a
    // discovery filter is set; BlueZ itself needs none.
    let filter: HashMap<String, glib::Variant> = HashMap::new();
    bluez.call(
        &adapter,
        ADAPTER,
        "SetDiscoveryFilter",
        Some((filter,).to_variant()),
    );
    adapter
}

/// Lets the adapter find a device named `name`, returning its path.
fn add_device(bluez: &Mock, address: &str, name: &str) -> String {
    bluez
        .call(
            "/",
            BLUEZ_MOCK,
            "AddDevice",
            Some(("hci0", address, name).to_variant()),
        )
        .get::<(String,)>()
        .expect("a device path")
        .0
}

/// The section started, with the failures it reports.
fn section() -> (Bluetooth, Rc<RefCell<Vec<Failure>>>) {
    let bluetooth = Bluetooth::new(Box::new(BluezBackend::new()));
    let failures = Rc::new(RefCell::new(Vec::new()));
    bluetooth.connect_failed({
        let failures = failures.clone();
        move |failure| failures.borrow_mut().push(failure.clone())
    });
    bluetooth.start();
    (bluetooth, failures)
}

/// The names of the devices `state` lists as paired and as found.
fn names(state: &BluetoothState) -> (Vec<&str>, Vec<&str>) {
    fn of(devices: &[Device]) -> Vec<&str> {
        devices.iter().map(Device::display_name).collect()
    }
    (of(&state.paired), of(&state.found))
}

/// Opens the level, waiting until the agent is BlueZ's default one and discovery runs.
fn open(bluetooth: &Bluetooth, bluez: &Mock, adapter: &str) {
    bluetooth.open();
    wait_until("the default agent", || {
        !bluez.calls(BLUEZ_PATH, "RequestDefaultAgent").is_empty()
    });
    wait_until("discovery", || {
        !bluez.calls(adapter, "StartDiscovery").is_empty()
    });
}

pub(crate) fn the_radio_discovery_pairing_and_connections_go_through_bluez() {
    let bluez = bluez();
    let adapter = add_adapter(&bluez);
    let pad = add_device(&bluez, "AA:BB:CC:DD:EE:01", "Pad");
    add_device(&bluez, "AA:BB:CC:DD:EE:02", "Headset");
    let (bluetooth, failures) = section();
    wait_until("the devices found", || bluetooth.state().found.len() == 2);
    assert!(bluetooth.state().available);
    assert_eq!(bluetooth.state().power, Power::On);
    assert_eq!(bluetooth.state().summary(), Summary::NotConnected);
    open(&bluetooth, &bluez, &adapter);

    bluetooth.set_powered(false);
    wait_until("the radio off", || bluetooth.state().power == Power::Off);
    assert_eq!(bluetooth.state().summary(), Summary::Off);
    bluetooth.set_powered(true);
    wait_until("the radio on", || bluetooth.state().power == Power::On);

    bluetooth.pair(&pad);
    wait_until("Pad paired, trusted and connected", || {
        let state = bluetooth.state();
        state.pending.is_none()
            && state
                .device(&pad)
                .is_some_and(|device| device.paired && device.trusted && device.connected)
    });
    assert_eq!(names(&bluetooth.state()), (vec!["Pad"], vec!["Headset"]));
    assert_eq!(
        bluetooth.state().summary(),
        Summary::Connected(vec!["Pad".to_owned()])
    );

    bluetooth.disconnect(&pad);
    wait_until("Pad disconnected", || {
        let state = bluetooth.state();
        state.pending.is_none() && state.device(&pad).is_some_and(|device| !device.connected)
    });

    bluetooth.close();
    wait_until("discovery stopped", || {
        !bluez.calls(&adapter, "StopDiscovery").is_empty()
    });
    wait_until("the agent unregistered", || {
        !bluez.calls(BLUEZ_PATH, "UnregisterAgent").is_empty()
    });
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

pub(crate) fn a_refused_confirmation_ends_the_pairing_quietly() {
    let bluez = bluez();
    let adapter = add_adapter(&bluez);
    let headset = add_device(&bluez, "AA:BB:CC:DD:EE:02", "Headset");
    let client = system_bus()
        .unique_name()
        .expect("a unique name on a message bus");
    bluez.add_method(
        &headset,
        DEVICE,
        "Pair",
        ("", ""),
        &REFUSED_PAIR.replace("@CLIENT@", &client),
    );
    let (bluetooth, failures) = section();
    wait_until("the device found", || bluetooth.state().found.len() == 1);
    open(&bluetooth, &bluez, &adapter);

    bluetooth.pair(&headset);
    wait_until("the code to confirm", || {
        bluetooth.state().prompt
            == Some(Prompt::Confirm {
                device: "Headset".to_owned(),
                passkey: "012345".to_owned(),
            })
    });
    bluetooth.confirm(false);
    wait_until("the pairing over", || bluetooth.state().pending.is_none());
    settle();

    assert_eq!(bluetooth.state().prompt, None);
    assert_eq!(names(&bluetooth.state()), (vec![], vec!["Headset"]));
    assert_eq!(bluez.calls(&headset, "Pair").len(), 1);
    assert!(
        failures.borrow().is_empty(),
        "refusing is not a failure: {:?}",
        failures.borrow()
    );
}

pub(crate) fn the_section_shows_once_bluez_has_an_adapter() {
    let (bluetooth, _) = section();
    settle();
    assert!(!bluetooth.state().available, "no BlueZ, no Bluetooth entry");

    let bluez = bluez();
    settle();
    assert!(
        !bluetooth.state().available,
        "no adapter, no Bluetooth entry"
    );

    add_adapter(&bluez);
    wait_until("the entry shown", || bluetooth.state().available);
    assert_eq!(bluetooth.state().power, Power::On);
}
