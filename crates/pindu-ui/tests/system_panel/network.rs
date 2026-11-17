//! The Network section over [`NetworkManagerBackend`], against dbusmock's
//! `networkmanager` template.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib::prelude::*;
use pindu_core::system::Availability;
use pindu_core::system::network::{
    DeviceKind, DeviceRow, DeviceStatus, Failure, Network, NetworkState, Problem, Prompt, Radio,
    Security, Summary,
};
use pindu_ui::networkmanager_backend::NetworkManagerBackend;

use crate::bus::{MOCK, Mock, object_path, settle, wait_until};

const NM: &str = "org.freedesktop.NetworkManager";
/// Where the template's own object, with its `Add…` methods, lives.
const ROOT: &str = "/org/freedesktop";
const MANAGER: &str = "/org/freedesktop/NetworkManager";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";

/// `NMDeviceState`s.
const DISCONNECTED: i32 = 30;
const ACTIVATED: i32 = 100;
/// `NM80211ApSecurityFlags`' `KEY_MGMT_PSK`.
const KEY_MGMT_PSK: u32 = 0x100;

/// The passphrase the mock's WPA network takes.
const PASSPHRASE: &str = "correct horse";

/// `AddAndActivateConnection2`, which the template lacks: it adds and activates the
/// connection as the template's `AddAndActivateConnection` does, setting the device's
/// access point and the primary connection as NetworkManager does. A WPA connection with
/// another passphrase is added, and the device fails with `NO_SECRETS` (7) before the call
/// answers, as a wrong passphrase does. That connection gets a path of its own, as
/// NetworkManager never gives a deleted connection's path to another.
const ADD_AND_ACTIVATE: &str = r"
settings, device, access_point, options = args
security = settings.get('802-11-wireless-security', {})
if security and security.get('psk') != 'correct horse':
    self.failed_joins = getattr(self, 'failed_joins', 0) + 1
    ssid = ''.join(chr(byte) for byte in settings['802-11-wireless']['ssid'])
    connection = self.AddWiFiConnection(device, f'failed{self.failed_joins}', ssid, '')
    objects[device].Set('org.freedesktop.NetworkManager.Device', 'State', dbus.UInt32(120))
    objects[device].EmitSignal('org.freedesktop.NetworkManager.Device', 'StateChanged', 'uuu',
                               [dbus.UInt32(120), dbus.UInt32(40), dbus.UInt32(7)])
    ret = (dbus.ObjectPath(connection), dbus.ObjectPath('/'), {})
else:
    objects[device].Set('org.freedesktop.NetworkManager.Device.Wireless', 'ActiveAccessPoint',
                        access_point)
    connection, active = self.add_and_activate_connection(self, settings, device, access_point)
    self.Set('org.freedesktop.NetworkManager', 'PrimaryConnection', active)
    ret = (connection, active, {})
";

/// NetworkManager with a wired device that is connected, and a Wi-Fi device that is not
/// and sees an open network and a WPA one. Returns the mock and the Wi-Fi device's path.
fn networkmanager() -> (Mock, String) {
    let nm = Mock::start("networkmanager", NM, None);
    nm.call(
        ROOT,
        MOCK,
        "AddEthernetDevice",
        Some(("eth0", "eth0", ACTIVATED).to_variant()),
    );
    let (wifi,) = nm
        .call(
            ROOT,
            MOCK,
            "AddWiFiDevice",
            Some(("wlan0", "wlan0", DISCONNECTED).to_variant()),
        )
        .get::<(String,)>()
        .expect("a device path");
    for (name, ssid, strength, security) in [
        ("cafe", "Cafe", 80_u8, 0),
        ("home", "Home", 60, KEY_MGMT_PSK),
    ] {
        nm.call(
            ROOT,
            MOCK,
            "AddAccessPoint",
            Some(
                (
                    wifi.as_str(),
                    name,
                    ssid,
                    "00:11:22:33:44:55",
                    2_u32,
                    2412_u32,
                    54_000_u32,
                    strength,
                    security,
                )
                    .to_variant(),
            ),
        );
    }
    // Properties NetworkManager has and the template leaves out.
    nm.call(
        &wifi,
        MOCK,
        "AddProperty",
        Some((WIRELESS, "ActiveAccessPoint", object_path("/")).to_variant()),
    );
    nm.call(
        MANAGER,
        MOCK,
        "AddProperty",
        Some((NM, "PrimaryConnection", object_path("/")).to_variant()),
    );
    nm.add_method(
        MANAGER,
        NM,
        "AddAndActivateConnection2",
        ("a{sa{sv}}ooa{sv}", "ooa{sv}"),
        ADD_AND_ACTIVATE,
    );
    (nm, wifi)
}

/// The section started, with the failures it reports.
fn section() -> (Network, Rc<RefCell<Vec<Failure>>>) {
    let network = Network::new(Box::new(NetworkManagerBackend::new()));
    let failures = Rc::new(RefCell::new(Vec::new()));
    network.connect_failed({
        let failures = failures.clone();
        move |failure| failures.borrow_mut().push(failure.clone())
    });
    network.start();
    (network, failures)
}

/// Whether `state` lists both of the mock's networks.
fn both_listed(state: &NetworkState) -> bool {
    state.radio == Radio::On && state.networks.len() == 2
}

/// The listed network named `name`: in use, saved.
fn listed_as(network: &Network, name: &str) -> Option<(bool, bool)> {
    network
        .state()
        .networks
        .iter()
        .find(|listed| listed.name == name)
        .map(|listed| (listed.in_use, listed.saved))
}

pub(crate) fn devices_and_networks_are_listed_and_scanned() {
    let (nm, wifi) = networkmanager();
    let (network, failures) = section();
    wait_until("the networks", || both_listed(&network.state()));

    let state = network.state();
    assert_eq!(state.availability, Availability::Shown);
    assert_eq!(
        state.devices,
        [
            DeviceRow {
                kind: DeviceKind::Wired,
                interface: "eth0".to_owned(),
                status: DeviceStatus::Connected(None),
            },
            DeviceRow {
                kind: DeviceKind::Wifi,
                interface: "wlan0".to_owned(),
                status: DeviceStatus::Disconnected,
            },
        ]
    );
    let listed: Vec<_> = state
        .networks
        .iter()
        .map(|network| (network.name.as_str(), network.strength, network.security))
        .collect();
    assert_eq!(
        listed,
        [
            ("Cafe", 80, Security::Open),
            ("Home", 60, Security::WpaPersonal)
        ]
    );
    assert_eq!(state.summary, Some(Summary::Offline));

    network.open();
    wait_until("a Wi-Fi scan", || {
        !nm.calls(&wifi, "RequestScan").is_empty()
    });
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

pub(crate) fn an_open_network_is_joined() {
    let (_nm, _) = networkmanager();
    let (network, failures) = section();
    wait_until("the networks", || both_listed(&network.state()));

    network.join(b"Cafe");
    assert_eq!(
        network.state().prompt,
        None,
        "an open network needs no passphrase"
    );
    wait_until("Cafe in use", || {
        network.state().pending.is_none() && listed_as(&network, "Cafe") == Some((true, true))
    });
    assert_eq!(
        network.state().summary,
        Some(Summary::Wifi("Cafe".to_owned()))
    );
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

pub(crate) fn a_wrong_passphrase_is_asked_again_and_leaves_nothing_saved() {
    let (_nm, _) = networkmanager();
    let (network, failures) = section();
    wait_until("the networks", || both_listed(&network.state()));

    network.join(b"Home");
    let asked = |problem| {
        Some(Prompt {
            ssid: b"Home".to_vec(),
            name: "Home".to_owned(),
            security: Security::WpaPersonal,
            problem,
        })
    };
    assert_eq!(network.state().prompt, asked(None));

    network.enter(Some("wrong passphrase"));
    wait_until("the passphrase asked again", || {
        network.state().prompt == asked(Some(Problem::Wrong))
    });
    wait_until("the failed connection deleted", || {
        listed_as(&network, "Home") == Some((false, false))
    });
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());

    network.enter(Some(PASSPHRASE));
    wait_until("Home in use", || {
        network.state().pending.is_none() && listed_as(&network, "Home") == Some((true, true))
    });
    assert_eq!(network.state().prompt, None);
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

pub(crate) fn wifi_is_switched_off_and_on() {
    let (_nm, _) = networkmanager();
    let (network, failures) = section();
    wait_until("the networks", || both_listed(&network.state()));

    network.set_wireless(false);
    wait_until("Wi-Fi off", || {
        let state = network.state();
        state.radio == Radio::Off && state.networks.is_empty()
    });
    network.set_wireless(true);
    wait_until("Wi-Fi on", || both_listed(&network.state()));
    assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
}

pub(crate) fn polkit_saying_no_leaves_the_entry_denied() {
    let nm = Mock::start("networkmanager", NM, None);
    nm.add_method(
        MANAGER,
        NM,
        "GetPermissions",
        ("", "a{ss}"),
        "ret = {'org.freedesktop.NetworkManager.network-control': 'no', \
         'org.freedesktop.NetworkManager.settings.modify.system': 'yes'}",
    );
    let (network, _) = section();
    wait_until("the entry denied", || {
        network.state().availability == Availability::Denied
    });
    assert_eq!(network.state().summary, Some(Summary::NotAllowed));
}

pub(crate) fn the_section_shows_once_networkmanager_appears() {
    let (network, _) = section();
    settle();
    assert_eq!(network.state().availability, Availability::Hidden);

    let _nm = Mock::start("networkmanager", NM, None);
    wait_until("the entry shown", || {
        network.state().availability == Availability::Shown
    });
    assert_eq!(network.state().radio, Radio::Missing);
}
