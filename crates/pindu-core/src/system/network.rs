//! The network, for the Network entry of fullscreen mode's Quick Access: the state of each
//! wired and Wi-Fi device, the Wi-Fi networks in range, and joining and forgetting one,
//! all through NetworkManager. A [`NetworkBackend`] talks to NetworkManager and reports
//! what it says as [`Facts`] and device state changes; [`Network`] decides what shows and
//! runs the flows over it, so the join flow, a wrong passphrase included, is tested here
//! with a fake backend.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fmt;
use std::rc::{Rc, Weak};

/// The polkit action that lets the session activate and deactivate connections.
pub const NETWORK_CONTROL: &str = "org.freedesktop.NetworkManager.network-control";
/// The polkit action that lets the session add, change and delete connections every user
/// shares, as a joined network is so that it comes up at boot before anyone logs in.
pub const MODIFY_SYSTEM: &str = "org.freedesktop.NetworkManager.settings.modify.system";

/// NetworkManager's device states (`NMDeviceState`) that the section tells apart.
mod device_state {
    pub(super) const UNMANAGED: u32 = 10;
    pub(super) const UNAVAILABLE: u32 = 20;
    pub(super) const PREPARE: u32 = 40;
    pub(super) const SECONDARIES: u32 = 90;
    pub(super) const ACTIVATED: u32 = 100;
    pub(super) const FAILED: u32 = 120;
}

/// NetworkManager's reasons for a device state change (`NMDeviceStateReason`) that the
/// section tells apart.
mod reason {
    pub(super) const NO_SECRETS: u32 = 7;
    pub(super) const SUPPLICANT_DISCONNECT: u32 = 8;
    pub(super) const DHCP_START_FAILED: u32 = 15;
    pub(super) const DHCP_ERROR: u32 = 16;
    pub(super) const DHCP_FAILED: u32 = 17;
    pub(super) const SSID_NOT_FOUND: u32 = 53;
}

/// An access point's flags (`NM80211ApFlags`, `NM80211ApSecurityFlags`).
mod ap_flags {
    pub(super) const PRIVACY: u32 = 0x1;
    pub(super) const KEY_MGMT_PSK: u32 = 0x100;
    pub(super) const KEY_MGMT_802_1X: u32 = 0x200;
    pub(super) const KEY_MGMT_SAE: u32 = 0x400;
    pub(super) const KEY_MGMT_OWE: u32 = 0x800;
    pub(super) const KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;
}

/// The connection types NetworkManager names a wired and a Wi-Fi connection by.
const WIRED_TYPE: &str = "802-3-ethernet";
const WIFI_TYPE: &str = "802-11-wireless";
const WIFI_SECURITY: &str = "802-11-wireless-security";

/// What kind of device a row is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// An Ethernet port (`DeviceType` 1).
    Wired,
    /// A Wi-Fi card (`DeviceType` 2).
    Wifi,
}

/// A wired or Wi-Fi device as NetworkManager reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    /// Its object path, which the backend acts on.
    pub path: String,
    /// Wired or Wi-Fi.
    pub kind: DeviceKind,
    /// Its interface name, such as `wlan0`.
    pub interface: String,
    /// Its `NMDeviceState`.
    pub state: u32,
    /// The SSID of the access point a Wi-Fi device uses, while it uses one.
    pub active_ssid: Option<Vec<u8>>,
}

/// A Wi-Fi access point a device sees.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessPoint {
    /// Its object path.
    pub path: String,
    /// The object path of the device that sees it.
    pub device: String,
    /// Its network's name, as bytes; empty for a hidden network.
    pub ssid: Vec<u8>,
    /// Its signal strength, 0 to 100.
    pub strength: u8,
    /// Its `Flags`.
    pub flags: u32,
    /// Its `WpaFlags`.
    pub wpa_flags: u32,
    /// Its `RsnFlags`.
    pub rsn_flags: u32,
}

/// A saved Wi-Fi connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedConnection {
    /// Its object path under NetworkManager's settings.
    pub path: String,
    /// The SSID of the network it joins.
    pub ssid: Vec<u8>,
}

/// The connection NetworkManager routes through (`PrimaryConnection`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Primary {
    /// Its type, such as `802-3-ethernet` or `802-11-wireless`.
    pub connection_type: String,
    /// Its name.
    pub id: String,
    /// The object paths of the devices it runs on.
    pub devices: Vec<String>,
}

/// What the backend knows about NetworkManager.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// Whether NetworkManager is there to talk to: never in the Flatpak, which has no
    /// system bus.
    pub running: bool,
    /// What `GetPermissions` answered, polkit action to `yes`, `auth` or `no`.
    pub permissions: BTreeMap<String, String>,
    /// Whether Wi-Fi is switched on (`WirelessEnabled`).
    pub wireless_enabled: bool,
    /// Whether no hardware switch blocks Wi-Fi (`WirelessHardwareEnabled`).
    pub wireless_hardware_enabled: bool,
    /// The wired and Wi-Fi devices.
    pub devices: Vec<Device>,
    /// The access points the Wi-Fi devices see.
    pub access_points: Vec<AccessPoint>,
    /// The saved Wi-Fi connections.
    pub saved: Vec<SavedConnection>,
    /// The connection NetworkManager routes through, if any.
    pub primary: Option<Primary>,
}

/// Something the backend reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// What NetworkManager says now.
    Facts(Facts),
    /// A device changed state (its `StateChanged` signal), with the reason.
    DeviceState {
        /// The device's object path.
        device: String,
        /// Its new `NMDeviceState`.
        state: u32,
        /// The `NMDeviceStateReason`.
        reason: u32,
    },
}

/// Whether the section shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Availability {
    /// NetworkManager is not there: the Flatpak, or a system without it.
    #[default]
    Hidden,
    /// NetworkManager is there, but polkit does not let the session control it: the entry
    /// shows, insensitive, saying so.
    Denied,
    /// The section shows.
    Shown,
}

/// What a device row says about its device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceStatus {
    /// Connected; to the named Wi-Fi network for a Wi-Fi device.
    Connected(Option<String>),
    /// Connecting.
    Connecting,
    /// Not connected.
    Disconnected,
    /// A wired device with no cable in.
    Unplugged,
    /// A Wi-Fi device that cannot be used: its radio is off.
    Off,
}

/// A wired or Wi-Fi device's row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceRow {
    /// Wired or Wi-Fi.
    pub kind: DeviceKind,
    /// Its interface name.
    pub interface: String,
    /// What it does.
    pub status: DeviceStatus,
}

/// How the Wi-Fi radio is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Radio {
    /// There is no Wi-Fi device; the section has the wired state only.
    #[default]
    Missing,
    /// On.
    On,
    /// Switched off, and can be switched on.
    Off,
    /// Blocked by a hardware switch.
    Blocked,
}

/// Which security a Wi-Fi network needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    /// None.
    Open,
    /// WPA or WPA2 personal: a passphrase (`key-mgmt` `wpa-psk`).
    WpaPersonal,
    /// WPA3 personal: a passphrase (`key-mgmt` `sae`).
    Wpa3Personal,
    /// WPA enterprise (802.1X), which is set up on another device.
    Enterprise,
    /// Something else, such as WEP or Enhanced Open, which is set up on another device too.
    Unsupported,
}

impl Security {
    /// The security of an access point with these flags. A network that offers both WPA2
    /// and WPA3 personal is joined with WPA2, which every Wi-Fi card can do.
    pub fn from_flags(flags: u32, wpa_flags: u32, rsn_flags: u32) -> Self {
        let keys = wpa_flags | rsn_flags;
        if keys & (ap_flags::KEY_MGMT_802_1X | ap_flags::KEY_MGMT_EAP_SUITE_B_192) != 0 {
            Self::Enterprise
        } else if keys & ap_flags::KEY_MGMT_PSK != 0 {
            Self::WpaPersonal
        } else if keys & ap_flags::KEY_MGMT_SAE != 0 {
            Self::Wpa3Personal
        } else if keys & ap_flags::KEY_MGMT_OWE != 0 || flags & ap_flags::PRIVACY != 0 {
            Self::Unsupported
        } else {
            Self::Open
        }
    }

    /// Whether joining asks for a passphrase.
    pub fn needs_passphrase(self) -> bool {
        matches!(self, Self::WpaPersonal | Self::Wpa3Personal)
    }
}

/// How strong a network's signal is, which picks its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Signal {
    /// Almost nothing.
    None,
    /// Weak.
    Weak,
    /// OK.
    Ok,
    /// Good.
    Good,
    /// Excellent.
    Excellent,
}

impl Signal {
    /// The level of a strength from 0 to 100.
    pub fn from_strength(strength: u8) -> Self {
        match strength {
            81.. => Self::Excellent,
            56..=80 => Self::Good,
            31..=55 => Self::Ok,
            6..=30 => Self::Weak,
            _ => Self::None,
        }
    }
}

/// A Wi-Fi network in range: the access points with one SSID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiNetwork {
    /// Its SSID.
    pub ssid: Vec<u8>,
    /// Its SSID as text.
    pub name: String,
    /// The strongest signal of its access points, 0 to 100.
    pub strength: u8,
    /// What its strongest access point needs to be joined.
    pub security: Security,
    /// Whether a device is connected to it.
    pub in_use: bool,
    /// Whether it has a saved connection.
    pub saved: bool,
    /// The device that sees it best.
    pub device: String,
    /// Its strongest access point.
    pub access_point: String,
}

impl WifiNetwork {
    /// How strong its signal is.
    pub fn signal(&self) -> Signal {
        Signal::from_strength(self.strength)
    }

    /// Whether selecting it joins it: a saved network, or a new open or personal one.
    pub fn joinable(&self) -> bool {
        self.saved
            || matches!(
                self.security,
                Security::Open | Security::WpaPersonal | Security::Wpa3Personal
            )
    }
}

/// What the entry says under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// Online through a cable.
    Wired,
    /// Online through the named Wi-Fi network.
    Wifi(String),
    /// Online through another connection, by its name.
    Other(String),
    /// Not online.
    Offline,
    /// The system does not let the session change the network.
    NotAllowed,
}

/// What runs on a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// It is being joined.
    Joining,
    /// Its saved connection is being deleted.
    Forgetting,
}

/// A network something runs on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The network's SSID.
    pub ssid: Vec<u8>,
    /// What runs.
    pub busy: Busy,
}

/// Why the passphrase is asked for again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Problem {
    /// The network did not take it.
    Wrong,
    /// It cannot be a passphrase of the network's security: WPA takes 8 to 63 characters
    /// (or 64 hexadecimal digits), WPA3 at least one.
    Length,
}

/// A passphrase the section asks for, to join a network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prompt {
    /// The network's SSID.
    pub ssid: Vec<u8>,
    /// Its SSID as text.
    pub name: String,
    /// Its security.
    pub security: Security,
    /// Why it is asked again, if it is.
    pub problem: Option<Problem>,
}

/// Why joining failed, as NetworkManager said it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinError {
    /// NetworkManager refused to start, with its message.
    Refused(String),
    /// A saved passphrase was not taken.
    Passphrase,
    /// The network went out of range.
    OutOfRange,
    /// The network gave no address.
    NoAddress,
    /// Another `NMDeviceStateReason`.
    Failed(u32),
}

impl JoinError {
    /// The error for a device that failed with `reason`.
    fn from_reason(reason: u32) -> Self {
        match reason {
            reason::NO_SECRETS | reason::SUPPLICANT_DISCONNECT => Self::Passphrase,
            reason::SSID_NOT_FOUND => Self::OutOfRange,
            reason::DHCP_START_FAILED | reason::DHCP_ERROR | reason::DHCP_FAILED => Self::NoAddress,
            other => Self::Failed(other),
        }
    }
}

/// Something that went wrong, for the shells to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// A network could not be joined.
    Join {
        /// Its name.
        name: String,
        /// Why.
        error: JoinError,
    },
    /// A saved connection could not be deleted.
    Forget {
        /// Its network's name.
        name: String,
        /// NetworkManager's message.
        error: String,
    },
    /// Wi-Fi could not be switched on or off.
    Radio(String),
}

/// What the Network entry and level show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NetworkState {
    /// Whether the section shows.
    pub availability: Availability,
    /// The wired and Wi-Fi devices, wired first.
    pub devices: Vec<DeviceRow>,
    /// How the Wi-Fi radio is.
    pub radio: Radio,
    /// The Wi-Fi networks in range, strongest first; none while the radio is not on.
    pub networks: Vec<WifiNetwork>,
    /// What the entry says under its title.
    pub summary: Option<Summary>,
    /// The network something runs on, if any.
    pub pending: Option<Pending>,
    /// The passphrase being asked for, if one is.
    pub prompt: Option<Prompt>,
}

/// One setting's value, in the types NetworkManager's settings take.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    /// A string (`s`).
    Str(String),
    /// Bytes (`ay`).
    Bytes(Vec<u8>),
    /// An unsigned number (`u`).
    U32(u32),
    /// A boolean (`b`).
    Bool(bool),
    /// A list of strings (`as`).
    StrList(Vec<String>),
}

/// Connection settings (`a{sa{sv}}`): each setting's name with its keys and values.
pub type Settings = Vec<(&'static str, Vec<(&'static str, Value)>)>;

/// A new connection to add and activate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JoinRequest {
    /// Its settings.
    pub settings: Settings,
    /// The object path of the device to activate it on.
    pub device: String,
    /// The object path of the access point to join.
    pub access_point: String,
}

/// The settings of a new connection to a Wi-Fi network: shared by every user
/// (`connection.permissions` empty) so it comes up at boot before anyone logs in, with the
/// passphrase stored in it (`psk-flags` 0) for the networks that need one.
pub fn connection_settings(
    ssid: &[u8],
    name: &str,
    security: Security,
    passphrase: Option<&str>,
) -> Settings {
    let mut settings: Settings = vec![
        (
            "connection",
            vec![
                ("type", Value::Str(WIFI_TYPE.to_owned())),
                ("id", Value::Str(name.to_owned())),
                ("autoconnect", Value::Bool(true)),
                ("permissions", Value::StrList(Vec::new())),
            ],
        ),
        (
            WIFI_TYPE,
            vec![
                ("ssid", Value::Bytes(ssid.to_vec())),
                ("mode", Value::Str("infrastructure".to_owned())),
            ],
        ),
    ];
    let key_mgmt = match security {
        Security::WpaPersonal => "wpa-psk",
        Security::Wpa3Personal => "sae",
        Security::Open | Security::Enterprise | Security::Unsupported => return settings,
    };
    let mut keys = vec![
        ("key-mgmt", Value::Str(key_mgmt.to_owned())),
        ("psk-flags", Value::U32(0)),
    ];
    if let Some(passphrase) = passphrase {
        keys.push(("psk", Value::Str(passphrase.to_owned())));
    }
    settings.push((WIFI_SECURITY, keys));
    settings
}

/// Whether `passphrase` can be one for `security`.
pub fn passphrase_fits(security: Security, passphrase: &str) -> bool {
    match security {
        Security::WpaPersonal => match passphrase.len() {
            8..=63 => true,
            64 => passphrase.bytes().all(|byte| byte.is_ascii_hexdigit()),
            _ => false,
        },
        Security::Wpa3Personal => !passphrase.is_empty(),
        Security::Open | Security::Enterprise | Security::Unsupported => true,
    }
}

/// What a backend reports once an action ends: `Err` says why it did not run or failed.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;
/// What a backend reports once a new connection is added: its object path, or why not.
pub type Added = Box<dyn FnOnce(Result<String, String>)>;

/// Talks to NetworkManager for the Network section. Every callback runs on the main
/// thread.
pub trait NetworkBackend {
    /// Starts following NetworkManager, and calls `events` with what it says now and
    /// whenever that changes.
    fn start(&self, events: Box<dyn Fn(Event)>);

    /// Asks the Wi-Fi device at `device` to look for networks.
    fn scan(&self, device: &str);

    /// Switches Wi-Fi on or off.
    fn set_wireless(&self, enabled: bool, done: Done);

    /// Activates the saved connection at `connection` on the device at `device`.
    fn activate(&self, connection: &str, device: &str, done: Done);

    /// Adds the connection `request` describes and activates it.
    fn add_and_activate(&self, request: JoinRequest, done: Added);

    /// Deletes the saved connection at `connection`.
    fn delete(&self, connection: &str, done: Done);
}

/// A join that runs.
#[derive(Debug, Clone)]
struct Joining {
    network: WifiNetwork,
    /// The connection being activated, once known.
    connection: Option<String>,
    /// Whether the connection was added for this join, and goes if the join fails.
    new: bool,
    /// The reason the device failed with before the connection was known.
    failed: Option<u32>,
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&NetworkState)>;
/// Something a shell does when an action fails.
type FailureListener = Box<dyn Fn(&Failure)>;

/// The Network section over its backend: follows NetworkManager, and joins and forgets
/// Wi-Fi networks.
#[derive(Clone)]
pub struct Network {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn NetworkBackend>,
    facts: RefCell<Facts>,
    state: RefCell<NetworkState>,
    /// The network whose passphrase the prompt asks for.
    asking: RefCell<Option<WifiNetwork>>,
    joining: RefCell<Option<Joining>>,
    listeners: RefCell<Vec<Listener>>,
    failure_listeners: RefCell<Vec<FailureListener>>,
}

impl fmt::Debug for Network {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Network")
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl Network {
    /// The section over `backend`, hidden until [`start`](Self::start) hears from it, so
    /// the shells can connect to it first.
    pub fn new(backend: Box<dyn NetworkBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                facts: RefCell::default(),
                state: RefCell::default(),
                asking: RefCell::default(),
                joining: RefCell::default(),
                listeners: RefCell::default(),
                failure_listeners: RefCell::default(),
            }),
        }
    }

    /// The state as last reported.
    pub fn state(&self) -> NetworkState {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&NetworkState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever something goes wrong.
    pub fn connect_failed(&self, listener: impl Fn(&Failure) + 'static) {
        self.inner
            .failure_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Starts following NetworkManager.
    pub fn start(&self) {
        let weak = self.downgrade();
        self.inner.backend.start(Box::new(move |event| {
            let Some(network) = upgrade(&weak) else {
                return;
            };
            match event {
                Event::Facts(facts) => network.take(facts),
                Event::DeviceState {
                    device,
                    state,
                    reason,
                } => network.device_changed(&device, state, reason),
            }
        }));
    }

    /// The level opens: the Wi-Fi devices look for networks.
    pub fn open(&self) {
        {
            let state = self.inner.state.borrow();
            if state.availability != Availability::Shown || state.radio != Radio::On {
                return;
            }
        }
        let devices: Vec<String> = self
            .inner
            .facts
            .borrow()
            .devices
            .iter()
            .filter(|device| device.kind == DeviceKind::Wifi && listed(device))
            .filter(|device| device.state != device_state::UNAVAILABLE)
            .map(|device| device.path.clone())
            .collect();
        for device in devices {
            self.inner.backend.scan(&device);
        }
    }

    /// Joins the network `ssid`: activates its saved connection, joins an open one, or asks
    /// for the passphrase of a secured one. Nothing happens while a join or forget runs or
    /// a passphrase is asked for.
    pub fn join(&self, ssid: &[u8]) {
        let network = {
            let state = self.inner.state.borrow();
            if state.availability != Availability::Shown
                || state.pending.is_some()
                || state.prompt.is_some()
            {
                return;
            }
            let Some(network) = state.networks.iter().find(|network| network.ssid == ssid) else {
                return;
            };
            network.clone()
        };
        if network.in_use || !network.joinable() {
            return;
        }
        if network.saved {
            let Some(connection) = self.saved_connections(ssid).into_iter().next() else {
                return;
            };
            self.activate(network, connection);
        } else if network.security.needs_passphrase() {
            self.update(|state| {
                state.prompt = Some(Prompt {
                    ssid: network.ssid.clone(),
                    name: network.name.clone(),
                    security: network.security,
                    problem: None,
                });
            });
            self.inner.asking.replace(Some(network));
        } else {
            self.add(network, None);
        }
    }

    /// Answers the passphrase prompt: `None` cancels it. A passphrase that cannot be one
    /// for the network is asked for again.
    pub fn enter(&self, passphrase: Option<&str>) {
        let Some(network) = self.inner.asking.borrow().clone() else {
            return;
        };
        match passphrase {
            Some(passphrase) if !passphrase_fits(network.security, passphrase) => {
                self.update(|state| {
                    if let Some(prompt) = &mut state.prompt {
                        prompt.problem = Some(Problem::Length);
                    }
                });
            }
            passphrase => {
                self.inner.asking.take();
                self.update(|state| state.prompt = None);
                if let Some(passphrase) = passphrase {
                    self.add(network, Some(passphrase));
                }
            }
        }
    }

    /// Deletes the saved connections of the network `ssid`.
    pub fn forget(&self, ssid: &[u8]) {
        {
            let state = self.inner.state.borrow();
            if state.availability != Availability::Shown || state.pending.is_some() {
                return;
            }
        }
        let connections = self.saved_connections(ssid);
        if connections.is_empty() {
            return;
        }
        let name = self.name_of(ssid);
        self.update(|state| {
            state.pending = Some(Pending {
                ssid: ssid.to_vec(),
                busy: Busy::Forgetting,
            });
        });
        let left = Rc::new(Cell::new(connections.len()));
        for connection in connections {
            let weak = self.downgrade();
            let left = left.clone();
            let name = name.clone();
            self.inner.backend.delete(
                &connection,
                Box::new(move |result| {
                    let Some(network) = upgrade(&weak) else {
                        return;
                    };
                    if let Err(error) = result {
                        network.fail(&Failure::Forget { name, error });
                    }
                    left.set(left.get() - 1);
                    if left.get() == 0 {
                        network.update(|state| state.pending = None);
                    }
                }),
            );
        }
    }

    /// Switches Wi-Fi on or off, unless a hardware switch blocks it.
    pub fn set_wireless(&self, enabled: bool) {
        {
            let state = self.inner.state.borrow();
            if state.availability != Availability::Shown
                || !matches!(state.radio, Radio::On | Radio::Off)
            {
                return;
            }
        }
        let weak = self.downgrade();
        self.inner.backend.set_wireless(
            enabled,
            Box::new(move |result| {
                if let (Some(network), Err(error)) = (upgrade(&weak), result) {
                    network.fail(&Failure::Radio(error));
                }
            }),
        );
    }

    /// Takes what NetworkManager says now. A join and its prompt go with the section.
    fn take(&self, facts: Facts) {
        let read = read_state(&facts);
        self.inner.facts.replace(facts);
        let shown = read.availability == Availability::Shown;
        if !shown {
            self.inner.asking.take();
            self.inner.joining.take();
        }
        self.update(|state| {
            let pending = state.pending.take().filter(|_| shown);
            let prompt = state.prompt.take().filter(|_| shown);
            *state = NetworkState {
                pending,
                prompt,
                ..read
            };
        });
    }

    /// Activates the saved `connection` of `network`.
    fn activate(&self, network: WifiNetwork, connection: String) {
        self.begin(&network, Some(connection.clone()), false);
        let weak = self.downgrade();
        self.inner.backend.activate(
            &connection,
            &network.device,
            Box::new(move |result| {
                if let (Some(network), Err(error)) = (upgrade(&weak), result) {
                    network.refused(error);
                }
            }),
        );
    }

    /// Adds a connection to `network` and activates it.
    fn add(&self, network: WifiNetwork, passphrase: Option<&str>) {
        let request = JoinRequest {
            settings: connection_settings(
                &network.ssid,
                &network.name,
                network.security,
                passphrase,
            ),
            device: network.device.clone(),
            access_point: network.access_point.clone(),
        };
        self.begin(&network, None, true);
        let weak = self.downgrade();
        self.inner.backend.add_and_activate(
            request,
            Box::new(move |result| {
                let Some(network) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(connection) => network.added(connection),
                    Err(error) => network.refused(error),
                }
            }),
        );
    }

    fn begin(&self, network: &WifiNetwork, connection: Option<String>, new: bool) {
        self.inner.joining.replace(Some(Joining {
            network: network.clone(),
            connection,
            new,
            failed: None,
        }));
        self.update(|state| {
            state.pending = Some(Pending {
                ssid: network.ssid.clone(),
                busy: Busy::Joining,
            });
        });
    }

    /// The new connection is known: a failure reported before it ends the join now.
    fn added(&self, connection: String) {
        let failed = {
            let mut joining = self.inner.joining.borrow_mut();
            let Some(joining) = joining.as_mut() else {
                return;
            };
            joining.connection = Some(connection);
            joining.failed
        };
        if let Some(reason) = failed {
            self.join_failed(reason);
        }
    }

    /// NetworkManager refused to start the join.
    fn refused(&self, error: String) {
        let Some(joining) = self.inner.joining.take() else {
            return;
        };
        self.update(|state| state.pending = None);
        self.fail(&Failure::Join {
            name: joining.network.name,
            error: JoinError::Refused(error),
        });
    }

    /// A device changed state: the one joining ends the join once it is activated or has
    /// failed.
    fn device_changed(&self, device: &str, state: u32, reason: u32) {
        let known = {
            let mut joining = self.inner.joining.borrow_mut();
            let Some(joining) = joining
                .as_mut()
                .filter(|joining| joining.network.device == device)
            else {
                return;
            };
            if state == device_state::FAILED && joining.connection.is_none() {
                joining.failed = Some(reason);
                return;
            }
            joining.connection.is_some()
        };
        match state {
            device_state::ACTIVATED => {
                self.inner.joining.take();
                self.update(|state| state.pending = None);
            }
            device_state::FAILED if known => self.join_failed(reason),
            _ => {}
        }
    }

    /// The join failed for `reason`. A connection added for it is deleted, so nothing
    /// broken is left saved; a wrong passphrase is asked for again.
    fn join_failed(&self, reason: u32) {
        let Some(joining) = self.inner.joining.take() else {
            return;
        };
        let network = joining.network;
        if joining.new
            && let Some(connection) = &joining.connection
        {
            self.delete_added(connection, &network.name);
        }
        let error = JoinError::from_reason(reason);
        if joining.new && error == JoinError::Passphrase && network.security.needs_passphrase() {
            self.update(|state| {
                state.pending = None;
                state.prompt = Some(Prompt {
                    ssid: network.ssid.clone(),
                    name: network.name.clone(),
                    security: network.security,
                    problem: Some(Problem::Wrong),
                });
            });
            self.inner.asking.replace(Some(network));
            return;
        }
        self.update(|state| state.pending = None);
        self.fail(&Failure::Join {
            name: network.name,
            error,
        });
    }

    /// Deletes a connection a failed join added.
    fn delete_added(&self, connection: &str, name: &str) {
        let weak = self.downgrade();
        let name = name.to_owned();
        self.inner.backend.delete(
            connection,
            Box::new(move |result| {
                if let (Some(network), Err(error)) = (upgrade(&weak), result) {
                    network.fail(&Failure::Forget { name, error });
                }
            }),
        );
    }

    /// The object paths of the saved connections to `ssid`.
    fn saved_connections(&self, ssid: &[u8]) -> Vec<String> {
        self.inner
            .facts
            .borrow()
            .saved
            .iter()
            .filter(|saved| saved.ssid == ssid)
            .map(|saved| saved.path.clone())
            .collect()
    }

    /// The name the network `ssid` is listed by.
    fn name_of(&self, ssid: &[u8]) -> String {
        self.inner
            .state
            .borrow()
            .networks
            .iter()
            .find(|network| network.ssid == ssid)
            .map_or_else(|| ssid_name(ssid), |network| network.name.clone())
    }

    /// Changes the state with `change`, and tells the listeners if it changed.
    fn update(&self, change: impl FnOnce(&mut NetworkState)) {
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            let before = state.clone();
            change(&mut state);
            *state != before
        };
        if changed {
            let state = self.state();
            for listener in self.inner.listeners.borrow().iter() {
                listener(&state);
            }
        }
    }

    fn fail(&self, failure: &Failure) {
        for listener in self.inner.failure_listeners.borrow().iter() {
            listener(failure);
        }
    }

    fn downgrade(&self) -> Weak<Inner> {
        Rc::downgrade(&self.inner)
    }
}

/// An SSID as text.
fn ssid_name(ssid: &[u8]) -> String {
    String::from_utf8_lossy(ssid).into_owned()
}

/// Whether a device gets a row: NetworkManager manages it.
fn listed(device: &Device) -> bool {
    device.state > device_state::UNMANAGED
}

/// What a device's row says.
fn device_status(device: &Device) -> DeviceStatus {
    match (device.state, device.kind) {
        (device_state::ACTIVATED, DeviceKind::Wifi) => {
            DeviceStatus::Connected(device.active_ssid.as_deref().map(ssid_name))
        }
        (device_state::ACTIVATED, DeviceKind::Wired) => DeviceStatus::Connected(None),
        (device_state::PREPARE..=device_state::SECONDARIES, _) => DeviceStatus::Connecting,
        (device_state::UNAVAILABLE, DeviceKind::Wired) => DeviceStatus::Unplugged,
        (device_state::UNAVAILABLE, DeviceKind::Wifi) => DeviceStatus::Off,
        _ => DeviceStatus::Disconnected,
    }
}

/// What the section shows for `facts`, with nothing pending or asked for.
fn read_state(facts: &Facts) -> NetworkState {
    if !facts.running {
        return NetworkState::default();
    }
    let denied = [NETWORK_CONTROL, MODIFY_SYSTEM].into_iter().any(|action| {
        facts
            .permissions
            .get(action)
            .is_some_and(|answer| answer == "no")
    });
    let mut devices: Vec<&Device> = facts.devices.iter().filter(|d| listed(d)).collect();
    devices.sort_by_key(|device| device.kind == DeviceKind::Wifi);
    let wifi: Vec<&Device> = devices
        .iter()
        .copied()
        .filter(|device| device.kind == DeviceKind::Wifi)
        .collect();
    let radio = if wifi.is_empty() {
        Radio::Missing
    } else if !facts.wireless_hardware_enabled {
        Radio::Blocked
    } else if !facts.wireless_enabled {
        Radio::Off
    } else {
        Radio::On
    };
    NetworkState {
        availability: if denied {
            Availability::Denied
        } else {
            Availability::Shown
        },
        devices: devices
            .iter()
            .map(|device| DeviceRow {
                kind: device.kind,
                interface: device.interface.clone(),
                status: device_status(device),
            })
            .collect(),
        radio,
        networks: if radio == Radio::On {
            networks(facts, &wifi)
        } else {
            Vec::new()
        },
        summary: Some(if denied {
            Summary::NotAllowed
        } else {
            summary(facts)
        }),
        pending: None,
        prompt: None,
    }
}

/// The networks the Wi-Fi devices `wifi` see, each SSID once with its strongest access
/// point, strongest first and then by name.
fn networks(facts: &Facts, wifi: &[&Device]) -> Vec<WifiNetwork> {
    let mut by_ssid: BTreeMap<&[u8], &AccessPoint> = BTreeMap::new();
    for ap in &facts.access_points {
        if ap.ssid.is_empty() || !wifi.iter().any(|device| device.path == ap.device) {
            continue;
        }
        let strongest = by_ssid.entry(&ap.ssid).or_insert(ap);
        if ap.strength > strongest.strength {
            *strongest = ap;
        }
    }
    let mut networks: Vec<WifiNetwork> = by_ssid
        .into_values()
        .map(|ap| WifiNetwork {
            ssid: ap.ssid.clone(),
            name: ssid_name(&ap.ssid),
            strength: ap.strength,
            security: Security::from_flags(ap.flags, ap.wpa_flags, ap.rsn_flags),
            in_use: wifi.iter().any(|device| {
                device.state == device_state::ACTIVATED
                    && device.active_ssid.as_deref() == Some(ap.ssid.as_slice())
            }),
            saved: facts.saved.iter().any(|saved| saved.ssid == ap.ssid),
            device: ap.device.clone(),
            access_point: ap.path.clone(),
        })
        .collect();
    networks.sort_by(|a, b| {
        b.strength
            .cmp(&a.strength)
            .then_with(|| a.name.cmp(&b.name))
    });
    networks
}

/// What the entry says for the primary connection.
fn summary(facts: &Facts) -> Summary {
    let Some(primary) = &facts.primary else {
        return Summary::Offline;
    };
    match primary.connection_type.as_str() {
        WIRED_TYPE => Summary::Wired,
        WIFI_TYPE => Summary::Wifi(
            facts
                .devices
                .iter()
                .filter(|device| primary.devices.contains(&device.path))
                .find_map(|device| device.active_ssid.as_deref())
                .map_or_else(|| primary.id.clone(), ssid_name),
        ),
        _ => Summary::Other(primary.id.clone()),
    }
}

/// The section again from a backend's callback, unless it is gone.
fn upgrade(weak: &Weak<Inner>) -> Option<Network> {
    weak.upgrade().map(|inner| Network { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIRED: &str = "/org/freedesktop/NetworkManager/Devices/1";
    const WIFI: &str = "/org/freedesktop/NetworkManager/Devices/2";
    const HOME_AP: &str = "/org/freedesktop/NetworkManager/AccessPoint/1";
    const HOME_SAVED: &str = "/org/freedesktop/NetworkManager/Settings/1";
    const NEW: &str = "/org/freedesktop/NetworkManager/Settings/9";

    const PSK: u32 = ap_flags::KEY_MGMT_PSK;
    const SAE: u32 = ap_flags::KEY_MGMT_SAE;
    const EAP: u32 = ap_flags::KEY_MGMT_802_1X;
    /// `NMDeviceStateReason`'s SUPPLICANT_TIMEOUT, which says nothing about the passphrase.
    const SUPPLICANT_TIMEOUT: u32 = 11;

    /// Where the fake backend sends what NetworkManager says.
    type Sink = Box<dyn Fn(Event)>;

    fn device(path: &str, kind: DeviceKind, interface: &str, state: u32) -> Device {
        Device {
            path: path.to_owned(),
            kind,
            interface: interface.to_owned(),
            state,
            active_ssid: None,
        }
    }

    fn ap(path: &str, ssid: &str, strength: u8, rsn_flags: u32) -> AccessPoint {
        AccessPoint {
            path: path.to_owned(),
            device: WIFI.to_owned(),
            ssid: ssid.as_bytes().to_vec(),
            strength,
            flags: if rsn_flags == 0 { 0 } else { ap_flags::PRIVACY },
            wpa_flags: 0,
            rsn_flags,
        }
    }

    fn saved(path: &str, ssid: &str) -> SavedConnection {
        SavedConnection {
            path: path.to_owned(),
            ssid: ssid.as_bytes().to_vec(),
        }
    }

    fn permissions(answer: &str) -> BTreeMap<String, String> {
        [NETWORK_CONTROL, MODIFY_SYSTEM]
            .into_iter()
            .map(|action| (action.to_owned(), answer.to_owned()))
            .collect()
    }

    /// A machine with a wired port and a Wi-Fi card, Wi-Fi on, both disconnected, seeing
    /// four networks: Home (saved, WPA2), Cafe (open), Neighbour (WPA3) and Office
    /// (enterprise).
    fn facts() -> Facts {
        Facts {
            running: true,
            permissions: permissions("yes"),
            wireless_enabled: true,
            wireless_hardware_enabled: true,
            devices: vec![
                device(WIRED, DeviceKind::Wired, "enp3s0", 20),
                device(WIFI, DeviceKind::Wifi, "wlan0", 30),
            ],
            access_points: vec![
                ap(HOME_AP, "Home", 70, PSK),
                ap("/ap/2", "Cafe", 40, 0),
                ap("/ap/3", "Neighbour", 55, SAE),
                ap("/ap/4", "Office", 20, EAP),
            ],
            saved: vec![saved(HOME_SAVED, "Home")],
            primary: None,
        }
    }

    /// A backend that hands the test its event sink, records what it is asked to do and
    /// keeps each action's callback until the test finishes it.
    #[derive(Default)]
    struct Fake {
        events: RefCell<Option<Sink>>,
        scans: RefCell<Vec<String>>,
        radio: RefCell<Vec<(bool, Done)>>,
        activations: RefCell<Vec<(String, String, Done)>>,
        additions: RefCell<Vec<(JoinRequest, Added)>>,
        deletions: RefCell<Vec<(String, Done)>>,
    }

    struct FakeBackend(Rc<Fake>);

    impl NetworkBackend for FakeBackend {
        fn start(&self, events: Box<dyn Fn(Event)>) {
            self.0.events.replace(Some(events));
        }

        fn scan(&self, device: &str) {
            self.0.scans.borrow_mut().push(device.to_owned());
        }

        fn set_wireless(&self, enabled: bool, done: Done) {
            self.0.radio.borrow_mut().push((enabled, done));
        }

        fn activate(&self, connection: &str, device: &str, done: Done) {
            self.0
                .activations
                .borrow_mut()
                .push((connection.to_owned(), device.to_owned(), done));
        }

        fn add_and_activate(&self, request: JoinRequest, done: Added) {
            self.0.additions.borrow_mut().push((request, done));
        }

        fn delete(&self, connection: &str, done: Done) {
            self.0
                .deletions
                .borrow_mut()
                .push((connection.to_owned(), done));
        }
    }

    impl Fake {
        fn send(&self, event: Event) {
            let events = self.events.borrow();
            events.as_ref().expect("the section has started")(event);
        }

        fn report(&self, facts: Facts) {
            self.send(Event::Facts(facts));
        }

        fn device_state(&self, device: &str, state: u32, reason: u32) {
            self.send(Event::DeviceState {
                device: device.to_owned(),
                state,
                reason,
            });
        }

        fn finish_activation(&self, result: Result<(), String>) {
            let (_, _, done) = self.activations.borrow_mut().remove(0);
            done(result);
        }

        fn finish_addition(&self, result: Result<String, String>) -> JoinRequest {
            let (request, done) = self.additions.borrow_mut().remove(0);
            done(result);
            request
        }

        fn finish_deletion(&self, result: Result<(), String>) -> String {
            let (path, done) = self.deletions.borrow_mut().remove(0);
            done(result);
            path
        }
    }

    fn section(facts: Facts) -> (Network, Rc<Fake>, Rc<RefCell<Vec<Failure>>>) {
        let fake = Rc::new(Fake::default());
        let network = Network::new(Box::new(FakeBackend(fake.clone())));
        let failures = Rc::new(RefCell::new(Vec::new()));
        network.connect_failed({
            let failures = failures.clone();
            move |failure| failures.borrow_mut().push(failure.clone())
        });
        network.start();
        fake.report(facts);
        (network, fake, failures)
    }

    fn names(state: &NetworkState) -> Vec<&str> {
        state
            .networks
            .iter()
            .map(|network| network.name.as_str())
            .collect()
    }

    fn listed<'a>(state: &'a NetworkState, name: &str) -> &'a WifiNetwork {
        state
            .networks
            .iter()
            .find(|network| network.name == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
    }

    fn setting<'a>(settings: &'a Settings, name: &str, key: &str) -> Option<&'a Value> {
        settings
            .iter()
            .find(|(setting, _)| *setting == name)
            .and_then(|(_, values)| values.iter().find(|(k, _)| *k == key))
            .map(|(_, value)| value)
    }

    // Security and signal.

    #[test]
    fn an_access_point_without_privacy_is_open() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
    }

    #[test]
    fn psk_key_management_is_wpa_personal_on_wpa_or_rsn() {
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, PSK, 0),
            Security::WpaPersonal
        );
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, PSK),
            Security::WpaPersonal
        );
    }

    #[test]
    fn sae_alone_is_wpa3_personal_and_sae_with_psk_joins_as_wpa2() {
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, SAE),
            Security::Wpa3Personal
        );
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, SAE | PSK),
            Security::WpaPersonal
        );
    }

    #[test]
    fn eight_oh_two_one_x_or_suite_b_is_enterprise_even_beside_psk() {
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, EAP | PSK),
            Security::Enterprise
        );
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, ap_flags::KEY_MGMT_EAP_SUITE_B_192),
            Security::Enterprise
        );
    }

    #[test]
    fn privacy_without_wpa_is_wep_and_not_offered() {
        assert_eq!(
            Security::from_flags(ap_flags::PRIVACY, 0, 0),
            Security::Unsupported
        );
    }

    #[test]
    fn enhanced_open_alone_is_not_offered() {
        // NM80211ApSecurityFlags' KEY_MGMT_OWE.
        assert_eq!(Security::from_flags(0, 0, 0x800), Security::Unsupported);
    }

    #[test]
    fn only_personal_networks_need_a_passphrase() {
        assert!(Security::WpaPersonal.needs_passphrase());
        assert!(Security::Wpa3Personal.needs_passphrase());
        assert!(!Security::Open.needs_passphrase());
        assert!(!Security::Enterprise.needs_passphrase());
    }

    #[test]
    fn strength_maps_to_five_signal_levels() {
        assert_eq!(Signal::from_strength(0), Signal::None);
        assert_eq!(Signal::from_strength(5), Signal::None);
        assert_eq!(Signal::from_strength(6), Signal::Weak);
        assert_eq!(Signal::from_strength(31), Signal::Ok);
        assert_eq!(Signal::from_strength(56), Signal::Good);
        assert_eq!(Signal::from_strength(81), Signal::Excellent);
        assert_eq!(Signal::from_strength(100), Signal::Excellent);
    }

    // Passphrases and settings.

    #[test]
    fn a_wpa_passphrase_has_8_to_63_characters_or_64_hex_digits() {
        assert!(!passphrase_fits(Security::WpaPersonal, "1234567"));
        assert!(passphrase_fits(Security::WpaPersonal, "12345678"));
        assert!(passphrase_fits(Security::WpaPersonal, &"a".repeat(63)));
        assert!(!passphrase_fits(Security::WpaPersonal, &"g".repeat(64)));
        assert!(passphrase_fits(Security::WpaPersonal, &"0aF9".repeat(16)));
        assert!(!passphrase_fits(Security::WpaPersonal, &"a".repeat(65)));
    }

    #[test]
    fn a_wpa3_passphrase_only_has_to_be_there() {
        assert!(!passphrase_fits(Security::Wpa3Personal, ""));
        assert!(passphrase_fits(Security::Wpa3Personal, "short"));
    }

    #[test]
    fn a_new_secured_network_is_a_system_connection_that_stores_its_passphrase() {
        let settings =
            connection_settings(b"Home", "Home", Security::WpaPersonal, Some("hunter22"));

        assert_eq!(
            setting(&settings, "connection", "type"),
            Some(&Value::Str(WIFI_TYPE.to_owned()))
        );
        assert_eq!(
            setting(&settings, "connection", "id"),
            Some(&Value::Str("Home".to_owned()))
        );
        assert_eq!(
            setting(&settings, "connection", "permissions"),
            Some(&Value::StrList(Vec::new()))
        );
        assert_eq!(
            setting(&settings, "connection", "autoconnect"),
            Some(&Value::Bool(true))
        );
        assert_eq!(
            setting(&settings, WIFI_TYPE, "ssid"),
            Some(&Value::Bytes(b"Home".to_vec()))
        );
        assert_eq!(
            setting(&settings, WIFI_TYPE, "mode"),
            Some(&Value::Str("infrastructure".to_owned()))
        );
        assert_eq!(
            setting(&settings, WIFI_SECURITY, "key-mgmt"),
            Some(&Value::Str("wpa-psk".to_owned()))
        );
        assert_eq!(
            setting(&settings, WIFI_SECURITY, "psk"),
            Some(&Value::Str("hunter22".to_owned()))
        );
        assert_eq!(
            setting(&settings, WIFI_SECURITY, "psk-flags"),
            Some(&Value::U32(0))
        );
    }

    #[test]
    fn a_wpa3_network_uses_sae() {
        let settings = connection_settings(b"N", "N", Security::Wpa3Personal, Some("x"));

        assert_eq!(
            setting(&settings, WIFI_SECURITY, "key-mgmt"),
            Some(&Value::Str("sae".to_owned()))
        );
    }

    #[test]
    fn an_open_network_has_no_security_setting() {
        let settings = connection_settings(b"Cafe", "Cafe", Security::Open, None);

        assert!(settings.iter().all(|(name, _)| *name != WIFI_SECURITY));
        assert_eq!(
            setting(&settings, "connection", "permissions"),
            Some(&Value::StrList(Vec::new()))
        );
    }

    // Availability and state.

    #[test]
    fn the_section_is_hidden_until_the_backend_reports() {
        let fake = Rc::new(Fake::default());
        let network = Network::new(Box::new(FakeBackend(fake)));

        assert_eq!(network.state().availability, Availability::Hidden);
    }

    #[test]
    fn without_network_manager_the_section_is_hidden() {
        let (network, _, _) = section(Facts::default());

        assert_eq!(network.state().availability, Availability::Hidden);
        assert_eq!(network.state().summary, None);
    }

    #[test]
    fn polkit_saying_no_leaves_the_entry_saying_it_is_not_allowed() {
        for action in [NETWORK_CONTROL, MODIFY_SYSTEM] {
            let mut facts = facts();
            facts.permissions.insert(action.to_owned(), "no".to_owned());
            let (network, _, _) = section(facts);

            assert_eq!(
                network.state().availability,
                Availability::Denied,
                "{action}"
            );
            assert_eq!(
                network.state().summary,
                Some(Summary::NotAllowed),
                "{action}"
            );
        }
    }

    #[test]
    fn polkit_asking_for_a_password_or_not_answering_yet_counts_as_allowed() {
        let (asks, _, _) = section(Facts {
            permissions: permissions("auth"),
            ..facts()
        });
        let (unknown, _, _) = section(Facts {
            permissions: BTreeMap::new(),
            ..facts()
        });

        assert_eq!(asks.state().availability, Availability::Shown);
        assert_eq!(unknown.state().availability, Availability::Shown);
    }

    #[test]
    fn each_device_shows_its_state_wired_first() {
        let mut facts = facts();
        facts.devices = vec![
            device(WIFI, DeviceKind::Wifi, "wlan0", device_state::ACTIVATED),
            device(
                WIRED,
                DeviceKind::Wired,
                "enp3s0",
                device_state::UNAVAILABLE,
            ),
            device("/d/3", DeviceKind::Wired, "enp4s0", 30),
            device("/d/4", DeviceKind::Wired, "enp5s0", 70),
            device("/d/5", DeviceKind::Wired, "enp6s0", device_state::FAILED),
        ];
        facts.devices[0].active_ssid = Some(b"Home".to_vec());
        let (network, _, _) = section(facts);
        let state = network.state();

        let statuses: Vec<(&str, &DeviceStatus)> = state
            .devices
            .iter()
            .map(|row| (row.interface.as_str(), &row.status))
            .collect();
        assert_eq!(
            statuses,
            [
                ("enp3s0", &DeviceStatus::Unplugged),
                ("enp4s0", &DeviceStatus::Disconnected),
                ("enp5s0", &DeviceStatus::Connecting),
                ("enp6s0", &DeviceStatus::Disconnected),
                ("wlan0", &DeviceStatus::Connected(Some("Home".to_owned()))),
            ]
        );
    }

    #[test]
    fn unmanaged_devices_are_not_listed() {
        let mut facts = facts();
        facts.devices.push(device(
            "/d/3",
            DeviceKind::Wired,
            "docker0",
            device_state::UNMANAGED,
        ));
        let (network, _, _) = section(facts);

        assert!(
            network
                .state()
                .devices
                .iter()
                .all(|row| row.interface != "docker0")
        );
    }

    #[test]
    fn a_wifi_device_that_is_unavailable_is_off() {
        let mut facts = facts();
        facts.devices[1].state = device_state::UNAVAILABLE;
        let (network, _, _) = section(facts);

        assert_eq!(network.state().devices[1].status, DeviceStatus::Off);
    }

    #[test]
    fn the_summary_follows_the_primary_connection() {
        let cases = [
            (None, Summary::Offline),
            (
                Some(Primary {
                    connection_type: WIRED_TYPE.to_owned(),
                    id: "Wired connection 1".to_owned(),
                    devices: vec![WIRED.to_owned()],
                }),
                Summary::Wired,
            ),
            (
                Some(Primary {
                    connection_type: "vpn".to_owned(),
                    id: "Work VPN".to_owned(),
                    devices: vec![WIRED.to_owned()],
                }),
                Summary::Other("Work VPN".to_owned()),
            ),
        ];
        for (primary, summary) in cases {
            let (network, _, _) = section(Facts { primary, ..facts() });
            assert_eq!(network.state().summary, Some(summary));
        }
    }

    #[test]
    fn a_wifi_primary_connection_is_summed_up_by_the_network_name_not_the_connection_name() {
        let mut facts = facts();
        facts.devices[1].state = device_state::ACTIVATED;
        facts.devices[1].active_ssid = Some(b"Home".to_vec());
        facts.primary = Some(Primary {
            connection_type: WIFI_TYPE.to_owned(),
            id: "Home 1".to_owned(),
            devices: vec![WIFI.to_owned()],
        });
        let (network, _, _) = section(facts);

        assert_eq!(
            network.state().summary,
            Some(Summary::Wifi("Home".to_owned()))
        );
    }

    #[test]
    fn the_radio_is_missing_off_blocked_or_on() {
        let mut no_wifi = facts();
        no_wifi.devices.truncate(1);
        no_wifi.access_points.clear();
        let off = Facts {
            wireless_enabled: false,
            ..facts()
        };
        let blocked = Facts {
            wireless_enabled: false,
            wireless_hardware_enabled: false,
            ..facts()
        };

        assert_eq!(section(no_wifi).0.state().radio, Radio::Missing);
        assert_eq!(section(off).0.state().radio, Radio::Off);
        assert_eq!(section(blocked).0.state().radio, Radio::Blocked);
        assert_eq!(section(facts()).0.state().radio, Radio::On);
    }

    #[test]
    fn without_a_wifi_device_the_section_has_the_wired_state_only() {
        let mut facts = facts();
        facts.devices.truncate(1);
        let (network, _, _) = section(facts);

        assert_eq!(network.state().availability, Availability::Shown);
        assert_eq!(network.state().devices.len(), 1);
        assert!(network.state().networks.is_empty());
    }

    #[test]
    fn with_the_radio_off_no_networks_are_listed() {
        let (network, _, _) = section(Facts {
            wireless_enabled: false,
            ..facts()
        });

        assert!(network.state().networks.is_empty());
    }

    // The Wi-Fi list.

    #[test]
    fn networks_are_listed_strongest_first_each_once_by_ssid() {
        let mut facts = facts();
        facts.access_points.push(ap("/ap/5", "Cafe", 90, 0));
        let (network, _, _) = section(facts);
        let state = network.state();

        assert_eq!(names(&state), ["Cafe", "Home", "Neighbour", "Office"]);
        let cafe = listed(&state, "Cafe");
        assert_eq!(cafe.strength, 90);
        assert_eq!(cafe.access_point, "/ap/5");
        assert_eq!(cafe.device, WIFI);
    }

    #[test]
    fn networks_of_equal_strength_are_listed_by_name() {
        let mut facts = facts();
        facts.access_points = vec![ap("/a", "b", 50, 0), ap("/b", "a", 50, 0)];
        let (network, _, _) = section(facts);

        assert_eq!(names(&network.state()), ["a", "b"]);
    }

    #[test]
    fn hidden_networks_are_not_listed() {
        let mut facts = facts();
        facts.access_points.push(ap("/ap/5", "", 99, PSK));
        let (network, _, _) = section(facts);

        assert_eq!(network.state().networks.len(), 4);
    }

    #[test]
    fn a_network_carries_its_security_and_whether_it_is_saved() {
        let (network, _, _) = section(facts());
        let state = network.state();

        assert_eq!(listed(&state, "Home").security, Security::WpaPersonal);
        assert!(listed(&state, "Home").saved);
        assert_eq!(listed(&state, "Cafe").security, Security::Open);
        assert!(!listed(&state, "Cafe").saved);
        assert_eq!(listed(&state, "Neighbour").security, Security::Wpa3Personal);
        assert_eq!(listed(&state, "Office").security, Security::Enterprise);
    }

    #[test]
    fn the_network_a_device_is_connected_to_is_in_use() {
        let mut facts = facts();
        facts.devices[1].state = device_state::ACTIVATED;
        facts.devices[1].active_ssid = Some(b"Home".to_vec());
        let (network, _, _) = section(facts);
        let state = network.state();

        assert!(listed(&state, "Home").in_use);
        assert!(!listed(&state, "Cafe").in_use);
    }

    #[test]
    fn a_network_still_connecting_is_not_in_use_yet() {
        let mut facts = facts();
        facts.devices[1].state = 50;
        facts.devices[1].active_ssid = Some(b"Home".to_vec());
        let (network, _, _) = section(facts);

        assert!(!listed(&network.state(), "Home").in_use);
    }

    #[test]
    fn enterprise_networks_are_listed_but_only_joinable_when_saved() {
        let (network, _, _) = section(facts());
        assert!(!listed(&network.state(), "Office").joinable());

        let mut facts = facts();
        facts.saved.push(saved("/s/2", "Office"));
        let (network, _, _) = section(facts);
        assert!(listed(&network.state(), "Office").joinable());
    }

    #[test]
    fn open_and_personal_networks_are_joinable() {
        let (network, _, _) = section(facts());
        let state = network.state();

        for name in ["Home", "Cafe", "Neighbour"] {
            assert!(listed(&state, name).joinable(), "{name}");
        }
    }

    // Scanning and the radio.

    #[test]
    fn opening_the_level_scans_on_each_wifi_device() {
        let (network, fake, _) = section(facts());

        network.open();

        assert_eq!(*fake.scans.borrow(), [WIFI]);
    }

    #[test]
    fn opening_the_level_with_the_radio_off_or_access_denied_scans_nothing() {
        let (off, fake, _) = section(Facts {
            wireless_enabled: false,
            ..facts()
        });
        off.open();
        assert!(fake.scans.borrow().is_empty());

        let (denied, fake, _) = section(Facts {
            permissions: permissions("no"),
            ..facts()
        });
        denied.open();
        assert!(fake.scans.borrow().is_empty());
    }

    #[test]
    fn switching_wifi_on_asks_the_backend_and_reports_a_refusal() {
        let (network, fake, failures) = section(Facts {
            wireless_enabled: false,
            ..facts()
        });

        network.set_wireless(true);
        let (enabled, done) = fake.radio.borrow_mut().remove(0);
        done(Err("Not authorized".to_owned()));

        assert!(enabled);
        assert_eq!(
            *failures.borrow(),
            [Failure::Radio("Not authorized".to_owned())]
        );
    }

    #[test]
    fn a_blocked_radio_is_not_switched() {
        let (network, fake, _) = section(Facts {
            wireless_enabled: false,
            wireless_hardware_enabled: false,
            ..facts()
        });

        network.set_wireless(true);

        assert!(fake.radio.borrow().is_empty());
    }

    // Joining.

    #[test]
    fn joining_a_saved_network_activates_its_connection_on_the_device_that_sees_it() {
        let (network, fake, _) = section(facts());

        network.join(b"Home");

        let activations = fake.activations.borrow();
        assert_eq!(activations.len(), 1);
        assert_eq!(activations[0].0, HOME_SAVED);
        assert_eq!(activations[0].1, WIFI);
        drop(activations);
        assert_eq!(
            network.state().pending,
            Some(Pending {
                ssid: b"Home".to_vec(),
                busy: Busy::Joining,
            })
        );
        assert_eq!(network.state().prompt, None);
    }

    #[test]
    fn a_join_ends_once_the_device_is_activated() {
        let (network, fake, failures) = section(facts());
        network.join(b"Home");
        fake.finish_activation(Ok(()));

        fake.device_state(WIFI, 70, 0);
        assert!(network.state().pending.is_some());
        fake.device_state(WIFI, device_state::ACTIVATED, 0);

        assert_eq!(network.state().pending, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_refused_activation_is_reported_and_ends_the_join() {
        let (network, fake, failures) = section(facts());
        network.join(b"Home");

        fake.finish_activation(Err("Not authorized".to_owned()));

        assert_eq!(network.state().pending, None);
        assert_eq!(
            *failures.borrow(),
            [Failure::Join {
                name: "Home".to_owned(),
                error: JoinError::Refused("Not authorized".to_owned()),
            }]
        );
    }

    #[test]
    fn a_saved_network_whose_passphrase_fails_is_reported_and_kept() {
        let (network, fake, failures) = section(facts());
        network.join(b"Home");
        fake.finish_activation(Ok(()));

        fake.device_state(WIFI, device_state::FAILED, reason::NO_SECRETS);

        assert!(fake.deletions.borrow().is_empty());
        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending, None);
        assert_eq!(
            *failures.borrow(),
            [Failure::Join {
                name: "Home".to_owned(),
                error: JoinError::Passphrase,
            }]
        );
    }

    #[test]
    fn device_failure_reasons_become_join_errors() {
        assert_eq!(
            JoinError::from_reason(reason::NO_SECRETS),
            JoinError::Passphrase
        );
        assert_eq!(
            JoinError::from_reason(reason::SUPPLICANT_DISCONNECT),
            JoinError::Passphrase
        );
        assert_eq!(
            JoinError::from_reason(SUPPLICANT_TIMEOUT),
            JoinError::Failed(SUPPLICANT_TIMEOUT)
        );
        assert_eq!(
            JoinError::from_reason(reason::SSID_NOT_FOUND),
            JoinError::OutOfRange
        );
        for dhcp in [
            reason::DHCP_START_FAILED,
            reason::DHCP_ERROR,
            reason::DHCP_FAILED,
        ] {
            assert_eq!(JoinError::from_reason(dhcp), JoinError::NoAddress);
        }
        assert_eq!(JoinError::from_reason(2), JoinError::Failed(2));
    }

    #[test]
    fn state_changes_of_another_device_do_not_end_a_join() {
        let (network, fake, _) = section(facts());
        network.join(b"Home");
        fake.finish_activation(Ok(()));

        fake.device_state(WIRED, device_state::ACTIVATED, 0);
        fake.device_state(WIRED, device_state::FAILED, 2);

        assert!(network.state().pending.is_some());
    }

    #[test]
    fn joining_an_open_network_adds_a_connection_without_asking() {
        let (network, fake, _) = section(facts());

        network.join(b"Cafe");

        assert_eq!(network.state().prompt, None);
        let additions = fake.additions.borrow();
        assert_eq!(additions.len(), 1);
        let request = &additions[0].0;
        assert_eq!(request.device, WIFI);
        assert_eq!(request.access_point, "/ap/2");
        assert_eq!(
            request.settings,
            connection_settings(b"Cafe", "Cafe", Security::Open, None)
        );
    }

    #[test]
    fn joining_a_new_secured_network_asks_for_its_passphrase() {
        for (name, security) in [
            ("Neighbour", Security::Wpa3Personal),
            ("Home", Security::WpaPersonal),
        ] {
            let mut facts = facts();
            facts.saved.clear();
            let (network, fake, _) = section(facts);

            network.join(name.as_bytes());

            assert_eq!(
                network.state().prompt,
                Some(Prompt {
                    ssid: name.as_bytes().to_vec(),
                    name: name.to_owned(),
                    security,
                    problem: None,
                })
            );
            assert!(fake.additions.borrow().is_empty());
            assert!(fake.activations.borrow().is_empty());
        }
    }

    #[test]
    fn a_passphrase_joins_with_a_new_system_connection() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");

        network.enter(Some("correct horse"));

        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending.map(|p| p.busy), Some(Busy::Joining));
        let request = fake.finish_addition(Ok(NEW.to_owned()));
        assert_eq!(request.access_point, "/ap/3");
        assert_eq!(
            request.settings,
            connection_settings(
                b"Neighbour",
                "Neighbour",
                Security::Wpa3Personal,
                Some("correct horse")
            )
        );
    }

    #[test]
    fn a_passphrase_that_cannot_fit_is_asked_for_again() {
        let mut facts = facts();
        facts.saved.clear();
        let (network, fake, _) = section(facts);
        network.join(b"Home");

        network.enter(Some("short"));

        assert_eq!(
            network.state().prompt.and_then(|prompt| prompt.problem),
            Some(Problem::Length)
        );
        assert!(fake.additions.borrow().is_empty());
    }

    #[test]
    fn cancelling_the_passphrase_joins_nothing() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");

        network.enter(None);

        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending, None);
        assert!(fake.additions.borrow().is_empty());
    }

    #[test]
    fn a_wrong_passphrase_deletes_the_new_connection_and_asks_again() {
        for wrong in [reason::NO_SECRETS, reason::SUPPLICANT_DISCONNECT] {
            let (network, fake, failures) = section(facts());
            network.join(b"Neighbour");
            network.enter(Some("wrong one"));
            fake.finish_addition(Ok(NEW.to_owned()));

            fake.device_state(WIFI, device_state::FAILED, wrong);

            assert_eq!(fake.finish_deletion(Ok(())), NEW);
            assert_eq!(
                network.state().prompt,
                Some(Prompt {
                    ssid: b"Neighbour".to_vec(),
                    name: "Neighbour".to_owned(),
                    security: Security::Wpa3Personal,
                    problem: Some(Problem::Wrong),
                })
            );
            assert_eq!(network.state().pending, None);
            assert!(failures.borrow().is_empty());
        }
    }

    #[test]
    fn the_right_passphrase_after_a_wrong_one_joins() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");
        network.enter(Some("wrong one"));
        fake.finish_addition(Ok(NEW.to_owned()));
        fake.device_state(WIFI, device_state::FAILED, reason::NO_SECRETS);
        fake.finish_deletion(Ok(()));

        network.enter(Some("right one"));
        let request = fake.finish_addition(Ok("/s/10".to_owned()));
        fake.device_state(WIFI, device_state::ACTIVATED, 0);

        assert_eq!(
            setting(&request.settings, WIFI_SECURITY, "psk"),
            Some(&Value::Str("right one".to_owned()))
        );
        assert_eq!(network.state().pending, None);
        assert_eq!(network.state().prompt, None);
        assert!(fake.deletions.borrow().is_empty());
    }

    #[test]
    fn a_wrong_passphrase_reported_before_the_connection_is_known_is_still_cleaned_up() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");
        network.enter(Some("wrong one"));

        fake.device_state(WIFI, device_state::FAILED, reason::NO_SECRETS);
        fake.finish_addition(Ok(NEW.to_owned()));

        assert_eq!(fake.finish_deletion(Ok(())), NEW);
        assert_eq!(
            network.state().prompt.and_then(|prompt| prompt.problem),
            Some(Problem::Wrong)
        );
    }

    #[test]
    fn a_new_connection_that_fails_otherwise_is_deleted_and_reported() {
        let (network, fake, failures) = section(facts());
        network.join(b"Cafe");
        fake.finish_addition(Ok(NEW.to_owned()));

        fake.device_state(WIFI, device_state::FAILED, reason::DHCP_FAILED);

        assert_eq!(fake.finish_deletion(Ok(())), NEW);
        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending, None);
        assert_eq!(
            *failures.borrow(),
            [Failure::Join {
                name: "Cafe".to_owned(),
                error: JoinError::NoAddress,
            }]
        );
    }

    #[test]
    fn a_refused_new_connection_is_reported() {
        let (network, fake, failures) = section(facts());
        network.join(b"Cafe");

        fake.finish_addition(Err("Not authorized".to_owned()));

        assert_eq!(network.state().pending, None);
        assert!(fake.deletions.borrow().is_empty());
        assert_eq!(
            *failures.borrow(),
            [Failure::Join {
                name: "Cafe".to_owned(),
                error: JoinError::Refused("Not authorized".to_owned()),
            }]
        );
    }

    #[test]
    fn a_new_connection_that_cannot_be_deleted_is_reported() {
        let (network, fake, failures) = section(facts());
        network.join(b"Neighbour");
        network.enter(Some("wrong one"));
        fake.finish_addition(Ok(NEW.to_owned()));
        fake.device_state(WIFI, device_state::FAILED, reason::NO_SECRETS);

        fake.finish_deletion(Err("Not authorized".to_owned()));

        assert_eq!(
            *failures.borrow(),
            [Failure::Forget {
                name: "Neighbour".to_owned(),
                error: "Not authorized".to_owned(),
            }]
        );
    }

    #[test]
    fn nothing_else_joins_while_a_join_runs_or_a_passphrase_is_asked_for() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");

        network.join(b"Cafe");
        assert!(fake.additions.borrow().is_empty());

        network.enter(Some("passphrase"));
        network.join(b"Home");
        assert!(fake.activations.borrow().is_empty());
        assert_eq!(fake.additions.borrow().len(), 1);
    }

    #[test]
    fn networks_in_use_enterprise_or_unknown_ones_are_not_joined() {
        let mut facts = facts();
        facts.devices[1].state = device_state::ACTIVATED;
        facts.devices[1].active_ssid = Some(b"Home".to_vec());
        let (network, fake, _) = section(facts);

        network.join(b"Home");
        network.join(b"Office");
        network.join(b"Elsewhere");

        assert!(fake.activations.borrow().is_empty());
        assert!(fake.additions.borrow().is_empty());
        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending, None);
    }

    #[test]
    fn a_saved_enterprise_network_is_activated() {
        let mut facts = facts();
        facts.saved.push(saved("/s/2", "Office"));
        let (network, fake, _) = section(facts);

        network.join(b"Office");

        assert_eq!(fake.activations.borrow()[0].0, "/s/2");
    }

    #[test]
    fn a_join_and_its_prompt_go_when_network_manager_does() {
        let (network, fake, _) = section(facts());
        network.join(b"Neighbour");

        fake.report(Facts::default());

        assert_eq!(network.state().prompt, None);
        assert_eq!(network.state().pending, None);
    }

    #[test]
    fn a_new_report_keeps_the_join_that_runs() {
        let (network, fake, _) = section(facts());
        network.join(b"Home");

        fake.report(facts());

        assert!(network.state().pending.is_some());
    }

    // Forgetting.

    #[test]
    fn forgetting_a_saved_network_deletes_each_of_its_connections() {
        let mut facts = facts();
        facts.saved.push(saved("/s/2", "Home"));
        let (network, fake, failures) = section(facts);

        network.forget(b"Home");

        assert_eq!(
            network.state().pending,
            Some(Pending {
                ssid: b"Home".to_vec(),
                busy: Busy::Forgetting,
            })
        );
        assert_eq!(fake.finish_deletion(Ok(())), HOME_SAVED);
        assert!(network.state().pending.is_some());
        assert_eq!(fake.finish_deletion(Ok(())), "/s/2");
        assert_eq!(network.state().pending, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_refused_forget_is_reported() {
        let (network, fake, failures) = section(facts());
        network.forget(b"Home");

        fake.finish_deletion(Err("Not authorized".to_owned()));

        assert_eq!(network.state().pending, None);
        assert_eq!(
            *failures.borrow(),
            [Failure::Forget {
                name: "Home".to_owned(),
                error: "Not authorized".to_owned(),
            }]
        );
    }

    #[test]
    fn a_network_that_is_not_saved_has_nothing_to_forget() {
        let (network, fake, _) = section(facts());

        network.forget(b"Cafe");

        assert!(fake.deletions.borrow().is_empty());
        assert_eq!(network.state().pending, None);
    }

    // Listeners.

    #[test]
    fn listeners_hear_each_change_once_and_not_a_report_that_changes_nothing() {
        let (network, fake, _) = section(facts());
        let changes = Rc::new(Cell::new(0));
        network.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });

        fake.report(facts());
        assert_eq!(changes.get(), 0);

        let mut moved = facts();
        moved.access_points[0].strength = 10;
        fake.report(moved);
        assert_eq!(changes.get(), 1);
    }

    #[test]
    fn a_section_that_is_gone_ignores_late_callbacks() {
        let (network, fake, _) = section(facts());
        network.join(b"Cafe");
        drop(network);

        fake.finish_addition(Ok(NEW.to_owned()));
        fake.device_state(WIFI, device_state::FAILED, 2);

        assert!(fake.deletions.borrow().is_empty());
    }
}
