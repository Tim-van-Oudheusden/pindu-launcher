//! Bluetooth, for the Bluetooth entry of fullscreen mode's Quick Access: the adapter with
//! its switch, the paired devices, and while the level is open the devices found nearby,
//! which a press pairs, trusts and connects. BlueZ does the work; a [`BluetoothBackend`]
//! talks to it, and [`Bluetooth`] decides what shows and runs the flows over it: discovery
//! and the pairing agent only while the level is open, and the agent's requests (a code to
//! confirm, a code to show, a PIN to type) as a [`Prompt`] for the shells.

use std::cell::{Cell, RefCell};
use std::cmp::Ordering;
use std::fmt;
use std::rc::{Rc, Weak};

/// BlueZ's `PowerState` while rfkill blocks the radio: the switch cannot turn it on.
const OFF_BLOCKED: &str = "off-blocked";
/// BlueZ's `PowerState` while the adapter turns on.
const OFF_ENABLING: &str = "off-enabling";
/// BlueZ's `PowerState` while the adapter turns off.
const ON_DISABLING: &str = "on-disabling";

/// The longest PIN a Bluetooth device takes, in characters.
const PIN_MAX: usize = 16;
/// The largest passkey: six decimal digits.
const PASSKEY_MAX: u32 = 999_999;

/// The adapter as the backend reads it (`org.bluez.Adapter1`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Adapter {
    /// Its object path, such as `/org/bluez/hci0`.
    pub path: String,
    /// `Powered`: whether the radio is on.
    pub powered: bool,
    /// `PowerState`, such as `on`, `off` or `off-blocked`, where BlueZ has it.
    pub power_state: Option<String>,
}

/// A device the adapter knows (`org.bluez.Device1`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Device {
    /// Its object path, such as `/org/bluez/hci0/dev_00_11_22_33_44_55`.
    pub path: String,
    /// Its address, such as `00:11:22:33:44:55`.
    pub address: String,
    /// `Name`, which the device itself gives; many found devices have none.
    pub name: Option<String>,
    /// `Alias`: the name to show, which BlueZ sets to the name or the address.
    pub alias: Option<String>,
    /// `Icon`, a freedesktop icon name such as `input-gaming` or `audio-headset`.
    pub icon: Option<String>,
    /// `Paired`.
    pub paired: bool,
    /// `Trusted`: whether it may connect by itself.
    pub trusted: bool,
    /// `Connected`.
    pub connected: bool,
}

impl Device {
    /// The name to show: its alias, else its name, else its address.
    pub fn display_name(&self) -> &str {
        self.alias
            .as_deref()
            .or(self.name.as_deref())
            .unwrap_or(&self.address)
    }
}

/// What the backend reads from BlueZ: the adapter and its devices. Without BlueZ on the
/// system bus, or without an adapter, there is no adapter, and the section is hidden.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// The adapter, when BlueZ has one.
    pub adapter: Option<Adapter>,
    /// The adapter's devices, in any order.
    pub devices: Vec<Device>,
}

/// Where the adapter's radio stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Power {
    /// On.
    On,
    /// Off: the switch turns it on.
    #[default]
    Off,
    /// Turning on.
    TurningOn,
    /// Turning off.
    TurningOff,
    /// Off and blocked by rfkill (airplane mode or a hardware switch): the switch cannot
    /// turn it on.
    Blocked,
}

impl Power {
    /// Reads the adapter's `PowerState`, or `Powered` where BlueZ has no `PowerState`.
    fn of(adapter: &Adapter) -> Self {
        match adapter.power_state.as_deref() {
            Some(OFF_BLOCKED) => Self::Blocked,
            Some(OFF_ENABLING) => Self::TurningOn,
            Some(ON_DISABLING) => Self::TurningOff,
            _ if adapter.powered => Self::On,
            _ => Self::Off,
        }
    }
}

/// What runs on a device on the user's say-so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// It is being paired.
    Pairing,
    /// It is being trusted and connected.
    Connecting,
    /// It is being disconnected.
    Disconnecting,
    /// It is being removed.
    Forgetting,
}

/// A device with what runs on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The device's object path.
    pub device: String,
    /// What runs.
    pub busy: Busy,
}

/// What the Quick Access entry says under its title.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Summary {
    /// The names of the connected devices.
    Connected(Vec<String>),
    /// On, with nothing connected.
    NotConnected,
    /// The radio is off.
    Off,
    /// The radio is blocked.
    Blocked,
}

/// What the pairing agent asks of the user, for the shells to show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Prompt {
    /// Numeric comparison: the device shows `passkey` too; A confirms, B refuses.
    Confirm {
        /// The device's name.
        device: String,
        /// The six-digit code, zero-padded.
        passkey: String,
    },
    /// A code to type on the device (a keyboard).
    Show {
        /// The device's name.
        device: String,
        /// The code: a six-digit passkey, zero-padded, or a PIN.
        code: String,
    },
    /// A PIN to type here, for a device from before Bluetooth 2.1.
    EnterPin {
        /// The device's name.
        device: String,
    },
    /// A six-digit passkey to type here, which the device shows.
    EnterPasskey {
        /// The device's name.
        device: String,
    },
}

/// What the Bluetooth entry and level show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BluetoothState {
    /// Whether the entry shows: BlueZ is there and has an adapter.
    pub available: bool,
    /// Where the radio stands.
    pub power: Power,
    /// The paired devices, by name.
    pub paired: Vec<Device>,
    /// The devices found that are not paired and give a name, by name.
    pub found: Vec<Device>,
    /// What runs on a device, if anything; nothing else starts meanwhile.
    pub pending: Option<Pending>,
    /// What the pairing agent asks, if anything.
    pub prompt: Option<Prompt>,
}

impl BluetoothState {
    /// What the entry says under its title.
    pub fn summary(&self) -> Summary {
        let connected: Vec<String> = self
            .paired
            .iter()
            .filter(|device| device.connected)
            .map(|device| device.display_name().to_owned())
            .collect();
        if !connected.is_empty() {
            return Summary::Connected(connected);
        }
        match self.power {
            Power::On | Power::TurningOn => Summary::NotConnected,
            Power::Off | Power::TurningOff => Summary::Off,
            Power::Blocked => Summary::Blocked,
        }
    }

    /// The device at `path`, paired or found.
    pub fn device(&self, path: &str) -> Option<&Device> {
        self.paired
            .iter()
            .chain(&self.found)
            .find(|device| device.path == path)
    }
}

/// What the agent hands back to BlueZ: `None` refuses.
pub type Reply<T> = Box<dyn FnOnce(Option<T>)>;

/// A request BlueZ makes of the pairing agent (`org.bluez.Agent1`), with what answers it.
pub enum AgentRequest {
    /// `RequestConfirmation`: confirm that the device shows `passkey`.
    Confirm {
        /// The device's object path.
        device: String,
        /// The code both sides show.
        passkey: u32,
        /// Answers BlueZ.
        reply: Reply<()>,
    },
    /// `DisplayPasskey`: show `passkey` to type on the device.
    DisplayPasskey {
        /// The device's object path.
        device: String,
        /// The code to type.
        passkey: u32,
    },
    /// `DisplayPinCode`: show `pin` to type on the device.
    DisplayPinCode {
        /// The device's object path.
        device: String,
        /// The PIN to type.
        pin: String,
    },
    /// `RequestPinCode`: the PIN for the device.
    PinCode {
        /// The device's object path.
        device: String,
        /// Answers BlueZ with the PIN.
        reply: Reply<String>,
    },
    /// `RequestPasskey`: the passkey the device shows.
    Passkey {
        /// The device's object path.
        device: String,
        /// Answers BlueZ with the passkey.
        reply: Reply<u32>,
    },
    /// `RequestAuthorization` or `AuthorizeService`: may the device pair or connect.
    Authorize {
        /// The device's object path.
        device: String,
        /// Answers BlueZ.
        reply: Reply<()>,
    },
    /// `Cancel`: BlueZ gave up on the request it made last.
    Cancel,
}

impl fmt::Debug for AgentRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Confirm {
                device, passkey, ..
            } => f
                .debug_struct("Confirm")
                .field("device", device)
                .field("passkey", passkey)
                .finish_non_exhaustive(),
            Self::DisplayPasskey { device, passkey } => f
                .debug_struct("DisplayPasskey")
                .field("device", device)
                .field("passkey", passkey)
                .finish(),
            Self::DisplayPinCode { device, pin } => f
                .debug_struct("DisplayPinCode")
                .field("device", device)
                .field("pin", pin)
                .finish(),
            Self::PinCode { device, .. } => f
                .debug_struct("PinCode")
                .field("device", device)
                .finish_non_exhaustive(),
            Self::Passkey { device, .. } => f
                .debug_struct("Passkey")
                .field("device", device)
                .finish_non_exhaustive(),
            Self::Authorize { device, .. } => f
                .debug_struct("Authorize")
                .field("device", device)
                .finish_non_exhaustive(),
            Self::Cancel => f.write_str("Cancel"),
        }
    }
}

/// What a backend reports once a call ends: `Err` says why it failed.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;

/// Talks to BlueZ on the system bus for the Bluetooth section. Every callback runs on the
/// main thread.
pub trait BluetoothBackend {
    /// Follows BlueZ: reports what it has to `changed` now, and again whenever BlueZ
    /// appears or goes away, or the adapter or a device changes.
    fn start(&self, changed: Box<dyn Fn(Facts)>);

    /// Turns the adapter at `adapter` on or off (`Powered`).
    fn set_powered(&self, adapter: &str, powered: bool, done: Done);

    /// Starts looking for devices (`StartDiscovery`).
    fn start_discovery(&self, adapter: &str, done: Done);

    /// Stops looking for devices (`StopDiscovery`).
    fn stop_discovery(&self, adapter: &str, done: Done);

    /// Registers the pairing agent with capability `KeyboardDisplay` and asks to be the
    /// default one, handing BlueZ's requests to `requests` until it is unregistered.
    fn register_agent(&self, requests: Box<dyn Fn(AgentRequest)>, done: Done);

    /// Unregisters the pairing agent.
    fn unregister_agent(&self, done: Done);

    /// Pairs with the device at `device` (`Pair`).
    fn pair(&self, device: &str, done: Done);

    /// Gives up pairing with the device at `device` (`CancelPairing`).
    fn cancel_pairing(&self, device: &str);

    /// Lets the device at `device` connect by itself (`Trusted = true`).
    fn trust(&self, device: &str, done: Done);

    /// Connects the device at `device` (`Connect`).
    fn connect(&self, device: &str, done: Done);

    /// Disconnects the device at `device` (`Disconnect`).
    fn disconnect(&self, device: &str, done: Done);

    /// Removes the device at `device` from the adapter at `adapter` (`RemoveDevice`),
    /// which unpairs it.
    fn remove(&self, adapter: &str, device: &str, done: Done);
}

/// Something that went wrong, for the shells to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The adapter did not turn on or off.
    Power {
        /// Whether it was to turn on.
        on: bool,
        /// Why.
        error: String,
    },
    /// A device did not pair.
    Pair {
        /// Its name.
        device: String,
        /// Why.
        error: String,
    },
    /// A device did not connect.
    Connect {
        /// Its name.
        device: String,
        /// Why.
        error: String,
    },
    /// A device did not disconnect.
    Disconnect {
        /// Its name.
        device: String,
        /// Why.
        error: String,
    },
    /// A device was not removed.
    Forget {
        /// Its name.
        device: String,
        /// Why.
        error: String,
    },
    /// Discovery did not start or stop.
    Discovery(String),
    /// The pairing agent was not registered or unregistered.
    Agent(String),
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&BluetoothState)>;
/// Something a shell does when an action fails.
type FailureListener = Box<dyn Fn(&Failure)>;

/// Where a session with BlueZ (discovery, the agent) stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Session {
    /// Not asked for.
    #[default]
    Off,
    /// Asked for, without an answer yet.
    Starting,
    /// Running.
    On,
    /// Refused; not asked for again until the level opens again or the radio is cycled.
    Failed,
}

/// The agent's request waiting for the user's answer.
enum Waiting {
    /// A yes or no.
    Confirm(Reply<()>),
    /// A PIN.
    Pin(Reply<String>),
    /// A passkey.
    Passkey(Reply<u32>),
}

/// The Bluetooth section over its backend: follows BlueZ, keeps discovery and the agent
/// running while the level is open, and runs the switch, pairing, connecting,
/// disconnecting and forgetting.
#[derive(Clone)]
pub struct Bluetooth {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn BluetoothBackend>,
    facts: RefCell<Facts>,
    pending: RefCell<Option<Pending>>,
    prompt: RefCell<Option<Prompt>>,
    waiting: RefCell<Option<Waiting>>,
    /// Whether the user refused or cancelled a prompt of the pairing that runs, so its
    /// failure is not reported.
    refused: Cell<bool>,
    /// Whether the level is open.
    open: Cell<bool>,
    discovery: Cell<Session>,
    agent: Cell<Session>,
    listeners: RefCell<Vec<Listener>>,
    failure_listeners: RefCell<Vec<FailureListener>>,
}

impl fmt::Debug for Bluetooth {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bluetooth")
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl Bluetooth {
    /// The section over `backend`, hidden until [`start`](Self::start) hears from BlueZ,
    /// so the shells can connect to it first.
    pub fn new(backend: Box<dyn BluetoothBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                facts: RefCell::default(),
                pending: RefCell::default(),
                prompt: RefCell::default(),
                waiting: RefCell::default(),
                refused: Cell::new(false),
                open: Cell::new(false),
                discovery: Cell::default(),
                agent: Cell::default(),
                listeners: RefCell::default(),
                failure_listeners: RefCell::default(),
            }),
        }
    }

    /// Follows BlueZ from now on.
    pub fn start(&self) {
        let weak = self.downgrade();
        self.inner.backend.start(Box::new(move |facts| {
            if let Some(bluetooth) = upgrade(&weak) {
                bluetooth.take(facts);
            }
        }));
    }

    /// The state as BlueZ last reported it.
    pub fn state(&self) -> BluetoothState {
        let facts = self.inner.facts.borrow();
        let Some(adapter) = &facts.adapter else {
            return BluetoothState::default();
        };
        let (mut paired, mut found): (Vec<Device>, Vec<Device>) = facts
            .devices
            .iter()
            .filter(|device| device.paired || device.name.is_some())
            .cloned()
            .partition(|device| device.paired);
        paired.sort_by(by_name);
        found.sort_by(by_name);
        BluetoothState {
            available: true,
            power: Power::of(adapter),
            paired,
            found,
            pending: self.inner.pending.borrow().clone(),
            prompt: self.inner.prompt.borrow().clone(),
        }
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&BluetoothState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever something goes wrong.
    pub fn connect_failed(&self, listener: impl Fn(&Failure) + 'static) {
        self.inner
            .failure_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// The level opens: the pairing agent is registered and, with the radio on,
    /// discovery starts.
    pub fn open(&self) {
        if self.inner.open.replace(true) {
            return;
        }
        for session in [&self.inner.agent, &self.inner.discovery] {
            if session.get() == Session::Failed {
                session.set(Session::Off);
            }
        }
        self.run_sessions();
    }

    /// The level closes: discovery stops, the agent is unregistered, and a prompt goes
    /// away refused.
    pub fn close(&self) {
        if !self.inner.open.replace(false) {
            return;
        }
        if let Some(adapter) = self.adapter_path()
            && self.inner.discovery.get() == Session::On
        {
            self.stop_discovery(&adapter);
        }
        if self.inner.discovery.get() != Session::Starting {
            self.inner.discovery.set(Session::Off);
        }
        if self.inner.agent.get() == Session::On {
            self.unregister_agent();
        }
        if self.inner.agent.get() != Session::Starting {
            self.inner.agent.set(Session::Off);
        }
        if self.inner.prompt.borrow().is_some() || self.inner.waiting.borrow().is_some() {
            self.inner.refused.set(true);
            self.drop_prompt();
            self.changed();
        }
    }

    /// Turns the radio on or off; a blocked radio stays off.
    pub fn set_powered(&self, powered: bool) {
        let state = self.state();
        if !state.available || state.power == Power::Blocked {
            return;
        }
        let Some(adapter) = self.adapter_path() else {
            return;
        };
        let weak = self.downgrade();
        self.inner.backend.set_powered(
            &adapter,
            powered,
            Box::new(move |result| {
                if let (Some(bluetooth), Err(error)) = (upgrade(&weak), result) {
                    bluetooth.fail(&Failure::Power { on: powered, error });
                }
            }),
        );
    }

    /// Pairs with a found device, then trusts and connects it.
    pub fn pair(&self, device: &str) {
        let Some(found) = self.idle_device(device, false, true) else {
            return;
        };
        self.inner.refused.set(false);
        self.set_pending(device, Busy::Pairing);
        let weak = self.downgrade();
        let path = found.path.clone();
        self.inner.backend.pair(
            device,
            Box::new(move |result| {
                let Some(bluetooth) = upgrade(&weak) else {
                    return;
                };
                bluetooth.drop_prompt();
                match result {
                    Ok(()) => bluetooth.trust_and_connect(&path),
                    Err(error) => {
                        bluetooth.inner.pending.replace(None);
                        bluetooth.changed();
                        if !bluetooth.inner.refused.get() {
                            bluetooth.fail(&Failure::Pair {
                                device: bluetooth.name_of(&path),
                                error,
                            });
                        }
                    }
                }
            }),
        );
    }

    /// Connects a paired device, trusting it first so it reconnects by itself.
    pub fn connect(&self, device: &str) {
        if self.idle_device(device, true, true).is_none() {
            return;
        }
        self.trust_and_connect(device);
    }

    /// Disconnects a paired device.
    pub fn disconnect(&self, device: &str) {
        if self.idle_device(device, true, true).is_none() {
            return;
        }
        self.set_pending(device, Busy::Disconnecting);
        let path = device.to_owned();
        let weak = self.downgrade();
        self.inner.backend.disconnect(
            device,
            Box::new(move |result| {
                if let Some(bluetooth) = upgrade(&weak) {
                    bluetooth.finish(
                        result,
                        |device, error| Failure::Disconnect { device, error },
                        &path,
                    );
                }
            }),
        );
    }

    /// Forgets a paired device: BlueZ removes it, which unpairs it. The shell asks first.
    pub fn forget(&self, device: &str) {
        if self.idle_device(device, true, false).is_none() {
            return;
        }
        let Some(adapter) = self.adapter_path() else {
            return;
        };
        self.set_pending(device, Busy::Forgetting);
        let path = device.to_owned();
        let weak = self.downgrade();
        self.inner.backend.remove(
            &adapter,
            device,
            Box::new(move |result| {
                if let Some(bluetooth) = upgrade(&weak) {
                    bluetooth.finish(
                        result,
                        |device, error| Failure::Forget { device, error },
                        &path,
                    );
                }
            }),
        );
    }

    /// Answers [`Prompt::Confirm`]: `true` confirms the code, `false` refuses it.
    pub fn confirm(&self, accept: bool) {
        if !matches!(*self.inner.prompt.borrow(), Some(Prompt::Confirm { .. })) {
            return;
        }
        let waiting = self.inner.waiting.take();
        self.inner.prompt.replace(None);
        if !accept {
            self.inner.refused.set(true);
        }
        if let Some(Waiting::Confirm(reply)) = waiting {
            reply(accept.then_some(()));
        }
        self.changed();
    }

    /// Answers [`Prompt::EnterPin`] or [`Prompt::EnterPasskey`] with what the user typed,
    /// or `None` when they cancelled. A PIN longer than 16 characters, or a passkey that
    /// is not up to six digits, refuses.
    pub fn enter(&self, code: Option<String>) {
        let waiting = match *self.inner.prompt.borrow() {
            Some(Prompt::EnterPin { .. } | Prompt::EnterPasskey { .. }) => {
                self.inner.waiting.take()
            }
            _ => return,
        };
        self.inner.prompt.replace(None);
        match waiting {
            Some(Waiting::Pin(reply)) => {
                let pin = code.filter(|pin| !pin.is_empty() && pin.chars().count() <= PIN_MAX);
                self.inner.refused.set(pin.is_none());
                reply(pin);
            }
            Some(Waiting::Passkey(reply)) => {
                let passkey = code
                    .filter(|code| {
                        (1..=6).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_digit())
                    })
                    .and_then(|code| code.parse::<u32>().ok())
                    .filter(|passkey| *passkey <= PASSKEY_MAX);
                self.inner.refused.set(passkey.is_none());
                reply(passkey);
            }
            _ => {}
        }
        self.changed();
    }

    /// The user closes [`Prompt::Show`]: pairing is given up.
    pub fn dismiss(&self) {
        if !matches!(*self.inner.prompt.borrow(), Some(Prompt::Show { .. })) {
            return;
        }
        self.inner.prompt.replace(None);
        self.inner.refused.set(true);
        let pairing = self
            .inner
            .pending
            .borrow()
            .as_ref()
            .filter(|pending| pending.busy == Busy::Pairing)
            .map(|pending| pending.device.clone());
        if let Some(device) = pairing {
            self.inner.backend.cancel_pairing(&device);
        }
        self.changed();
    }

    /// Trusts the device unless it is trusted already, then connects it.
    fn trust_and_connect(&self, device: &str) {
        self.set_pending(device, Busy::Connecting);
        let trusted = self
            .inner
            .facts
            .borrow()
            .devices
            .iter()
            .any(|known| known.path == device && known.trusted);
        if trusted {
            self.connect_now(device);
            return;
        }
        let path = device.to_owned();
        let weak = self.downgrade();
        self.inner.backend.trust(
            device,
            Box::new(move |result| {
                let Some(bluetooth) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(()) => bluetooth.connect_now(&path),
                    Err(error) => bluetooth.finish(
                        Err(error),
                        |device, error| Failure::Connect { device, error },
                        &path,
                    ),
                }
            }),
        );
    }

    fn connect_now(&self, device: &str) {
        let path = device.to_owned();
        let weak = self.downgrade();
        self.inner.backend.connect(
            device,
            Box::new(move |result| {
                if let Some(bluetooth) = upgrade(&weak) {
                    bluetooth.finish(
                        result,
                        |device, error| Failure::Connect { device, error },
                        &path,
                    );
                }
            }),
        );
    }

    /// Ends what runs on `device`, reporting an error as `failure` makes it.
    fn finish(
        &self,
        result: Result<(), String>,
        failure: impl FnOnce(String, String) -> Failure,
        device: &str,
    ) {
        self.inner.pending.replace(None);
        self.changed();
        if let Err(error) = result {
            self.fail(&failure(self.name_of(device), error));
        }
    }

    /// The device at `path` when nothing runs, paired or found as `paired` says, and the
    /// radio is on where `needs_power` says it has to be.
    fn idle_device(&self, path: &str, paired: bool, needs_power: bool) -> Option<Device> {
        let state = self.state();
        if state.pending.is_some() || (needs_power && state.power != Power::On) {
            return None;
        }
        let list = if paired { &state.paired } else { &state.found };
        list.iter().find(|device| device.path == path).cloned()
    }

    /// Takes what BlueZ reports, and starts or ends discovery and the agent as it allows.
    fn take(&self, facts: Facts) {
        if *self.inner.facts.borrow() == facts {
            return;
        }
        let available = facts.adapter.is_some();
        let powered = facts
            .adapter
            .as_ref()
            .is_some_and(|adapter| Power::of(adapter) == Power::On);
        self.inner.facts.replace(facts);
        if !available {
            // BlueZ went away, and its sessions and what ran with it.
            self.inner.agent.set(Session::Off);
            self.inner.discovery.set(Session::Off);
            self.inner.pending.replace(None);
            self.drop_prompt();
        } else if !powered && self.inner.discovery.get() != Session::Starting {
            // BlueZ ends discovery as the radio turns off.
            self.inner.discovery.set(Session::Off);
        }
        self.run_sessions();
        self.changed();
    }

    /// Registers the agent and starts discovery while the level is open, as far as BlueZ
    /// allows.
    fn run_sessions(&self) {
        if !self.inner.open.get() {
            return;
        }
        let state = self.state();
        if !state.available {
            return;
        }
        if self.inner.agent.get() == Session::Off {
            self.register_agent();
        }
        if state.power == Power::On
            && self.inner.discovery.get() == Session::Off
            && let Some(adapter) = self.adapter_path()
        {
            self.start_discovery(&adapter);
        }
    }

    fn register_agent(&self) {
        self.inner.agent.set(Session::Starting);
        let requests = {
            let weak = self.downgrade();
            Box::new(move |request| {
                if let Some(bluetooth) = upgrade(&weak) {
                    bluetooth.request(request);
                }
            })
        };
        let weak = self.downgrade();
        self.inner.backend.register_agent(
            requests,
            Box::new(move |result| {
                let Some(bluetooth) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(()) if bluetooth.inner.open.get() => {
                        bluetooth.inner.agent.set(Session::On);
                    }
                    // The level closed meanwhile.
                    Ok(()) => bluetooth.unregister_agent(),
                    Err(error) => {
                        bluetooth.inner.agent.set(Session::Failed);
                        bluetooth.fail(&Failure::Agent(error));
                    }
                }
            }),
        );
    }

    fn unregister_agent(&self) {
        self.inner.agent.set(Session::Off);
        let weak = self.downgrade();
        self.inner.backend.unregister_agent(Box::new(move |result| {
            if let (Some(bluetooth), Err(error)) = (upgrade(&weak), result) {
                bluetooth.fail(&Failure::Agent(error));
            }
        }));
    }

    fn start_discovery(&self, adapter: &str) {
        self.inner.discovery.set(Session::Starting);
        let path = adapter.to_owned();
        let weak = self.downgrade();
        self.inner.backend.start_discovery(
            adapter,
            Box::new(move |result| {
                let Some(bluetooth) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(()) if bluetooth.inner.open.get() => {
                        bluetooth.inner.discovery.set(Session::On);
                    }
                    // The level closed meanwhile.
                    Ok(()) => bluetooth.stop_discovery(&path),
                    Err(error) => {
                        bluetooth.inner.discovery.set(Session::Failed);
                        bluetooth.fail(&Failure::Discovery(error));
                    }
                }
            }),
        );
    }

    fn stop_discovery(&self, adapter: &str) {
        self.inner.discovery.set(Session::Off);
        let weak = self.downgrade();
        self.inner.backend.stop_discovery(
            adapter,
            Box::new(move |result| {
                if let (Some(bluetooth), Err(error)) = (upgrade(&weak), result) {
                    bluetooth.fail(&Failure::Discovery(error));
                }
            }),
        );
    }

    /// Acts on a request of the pairing agent.
    fn request(&self, request: AgentRequest) {
        match request {
            AgentRequest::Confirm {
                device,
                passkey,
                reply,
            } => self.ask(
                Prompt::Confirm {
                    device: self.name_of(&device),
                    passkey: format!("{passkey:06}"),
                },
                Some(Waiting::Confirm(reply)),
            ),
            AgentRequest::DisplayPasskey { device, passkey } => self.ask(
                Prompt::Show {
                    device: self.name_of(&device),
                    code: format!("{passkey:06}"),
                },
                None,
            ),
            AgentRequest::DisplayPinCode { device, pin } => self.ask(
                Prompt::Show {
                    device: self.name_of(&device),
                    code: pin,
                },
                None,
            ),
            AgentRequest::PinCode { device, reply } => self.ask(
                Prompt::EnterPin {
                    device: self.name_of(&device),
                },
                Some(Waiting::Pin(reply)),
            ),
            AgentRequest::Passkey { device, reply } => self.ask(
                Prompt::EnterPasskey {
                    device: self.name_of(&device),
                },
                Some(Waiting::Passkey(reply)),
            ),
            AgentRequest::Authorize { device, reply } => {
                let pairing = self
                    .inner
                    .pending
                    .borrow()
                    .as_ref()
                    .is_some_and(|pending| pending.device == device);
                let paired = self
                    .inner
                    .facts
                    .borrow()
                    .devices
                    .iter()
                    .any(|known| known.path == device && known.paired);
                reply((pairing || paired).then_some(()));
            }
            AgentRequest::Cancel => {
                if self.inner.prompt.borrow().is_some() || self.inner.waiting.borrow().is_some() {
                    self.drop_prompt();
                    self.changed();
                }
            }
        }
    }

    /// Shows `prompt`, refusing a request still waiting.
    fn ask(&self, prompt: Prompt, waiting: Option<Waiting>) {
        self.drop_prompt();
        self.inner.prompt.replace(Some(prompt));
        self.inner.waiting.replace(waiting);
        self.changed();
    }

    /// Takes the prompt away, refusing its request if it waits for an answer.
    fn drop_prompt(&self) {
        self.inner.prompt.replace(None);
        match self.inner.waiting.take() {
            Some(Waiting::Confirm(reply)) => reply(None),
            Some(Waiting::Pin(reply)) => reply(None),
            Some(Waiting::Passkey(reply)) => reply(None),
            None => {}
        }
    }

    fn set_pending(&self, device: &str, busy: Busy) {
        self.inner.pending.replace(Some(Pending {
            device: device.to_owned(),
            busy,
        }));
        self.changed();
    }

    fn adapter_path(&self) -> Option<String> {
        self.inner
            .facts
            .borrow()
            .adapter
            .as_ref()
            .map(|adapter| adapter.path.clone())
    }

    /// The name of the device at `path`, or its address taken from the path.
    fn name_of(&self, path: &str) -> String {
        if let Some(device) = self
            .inner
            .facts
            .borrow()
            .devices
            .iter()
            .find(|device| device.path == path)
        {
            return device.display_name().to_owned();
        }
        let last = path.rsplit('/').next().unwrap_or(path);
        last.strip_prefix("dev_").unwrap_or(last).replace('_', ":")
    }

    fn changed(&self) {
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
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

/// Orders devices by name, case aside, then by address.
fn by_name(a: &Device, b: &Device) -> Ordering {
    a.display_name()
        .to_lowercase()
        .cmp(&b.display_name().to_lowercase())
        .then_with(|| a.address.cmp(&b.address))
}

/// The section again from a backend's callback, unless it is gone.
fn upgrade(weak: &Weak<Inner>) -> Option<Bluetooth> {
    weak.upgrade().map(|inner| Bluetooth { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ADAPTER: &str = "/org/bluez/hci0";

    /// What the section asked the backend to do.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Call {
        SetPowered(bool),
        StartDiscovery,
        StopDiscovery,
        RegisterAgent,
        UnregisterAgent,
        Pair(String),
        CancelPairing(String),
        Trust(String),
        Connect(String),
        Disconnect(String),
        Remove(String),
    }

    /// Where the section hears from BlueZ.
    type Reports = Box<dyn Fn(Facts)>;
    /// Where the agent hands on BlueZ's requests.
    type Requests = Box<dyn Fn(AgentRequest)>;
    /// What the section answered BlueZ: `None` until it has, then the answer.
    type Answer<T> = Rc<RefCell<Option<Option<T>>>>;

    /// A backend that records each call and keeps its `Done` until the test finishes it,
    /// and hands the test BlueZ's side: reports and the agent's requests.
    #[derive(Default)]
    struct Fake {
        calls: RefCell<Vec<Call>>,
        dones: RefCell<Vec<(Call, Done)>>,
        changed: RefCell<Option<Reports>>,
        agent: RefCell<Option<Requests>>,
    }

    struct FakeBackend(Rc<Fake>);

    impl FakeBackend {
        fn record(&self, call: Call, done: Done) {
            self.0.calls.borrow_mut().push(call.clone());
            self.0.dones.borrow_mut().push((call, done));
        }
    }

    impl BluetoothBackend for FakeBackend {
        fn start(&self, changed: Box<dyn Fn(Facts)>) {
            self.0.changed.replace(Some(changed));
        }

        fn set_powered(&self, adapter: &str, powered: bool, done: Done) {
            assert_eq!(adapter, ADAPTER);
            self.record(Call::SetPowered(powered), done);
        }

        fn start_discovery(&self, adapter: &str, done: Done) {
            assert_eq!(adapter, ADAPTER);
            self.record(Call::StartDiscovery, done);
        }

        fn stop_discovery(&self, adapter: &str, done: Done) {
            assert_eq!(adapter, ADAPTER);
            self.record(Call::StopDiscovery, done);
        }

        fn register_agent(&self, requests: Box<dyn Fn(AgentRequest)>, done: Done) {
            self.0.agent.replace(Some(requests));
            self.record(Call::RegisterAgent, done);
        }

        fn unregister_agent(&self, done: Done) {
            self.record(Call::UnregisterAgent, done);
        }

        fn pair(&self, device: &str, done: Done) {
            self.record(Call::Pair(device.to_owned()), done);
        }

        fn cancel_pairing(&self, device: &str) {
            self.0
                .calls
                .borrow_mut()
                .push(Call::CancelPairing(device.to_owned()));
        }

        fn trust(&self, device: &str, done: Done) {
            self.record(Call::Trust(device.to_owned()), done);
        }

        fn connect(&self, device: &str, done: Done) {
            self.record(Call::Connect(device.to_owned()), done);
        }

        fn disconnect(&self, device: &str, done: Done) {
            self.record(Call::Disconnect(device.to_owned()), done);
        }

        fn remove(&self, adapter: &str, device: &str, done: Done) {
            assert_eq!(adapter, ADAPTER);
            self.record(Call::Remove(device.to_owned()), done);
        }
    }

    impl Fake {
        /// BlueZ reports `facts`.
        fn report(&self, facts: Facts) {
            let changed = self.changed.borrow();
            changed.as_ref().expect("the section follows BlueZ")(facts);
        }

        /// The pairing agent gets `request`.
        fn ask(&self, request: AgentRequest) {
            let agent = self.agent.borrow();
            agent.as_ref().expect("the agent is registered")(request);
        }

        /// Ends the oldest call still running that is `call`.
        fn finish(&self, call: &Call, result: Result<(), String>) {
            let done = {
                let mut dones = self.dones.borrow_mut();
                let index = dones
                    .iter()
                    .position(|(running, _)| running == call)
                    .unwrap_or_else(|| panic!("{call:?} does not run"));
                dones.remove(index).1
            };
            done(result);
        }

        /// Takes the calls made so far.
        fn calls(&self) -> Vec<Call> {
            self.calls.take()
        }
    }

    fn adapter(powered: bool) -> Adapter {
        Adapter {
            path: ADAPTER.to_owned(),
            powered,
            power_state: Some(if powered { "on" } else { "off" }.to_owned()),
        }
    }

    fn path(address: &str) -> String {
        format!("{ADAPTER}/dev_{}", address.replace(':', "_"))
    }

    /// A device found nearby that gives `name`.
    fn found(address: &str, name: &str) -> Device {
        Device {
            path: path(address),
            address: address.to_owned(),
            name: Some(name.to_owned()),
            alias: Some(name.to_owned()),
            ..Device::default()
        }
    }

    /// A paired and trusted device, connected as `connected` says.
    fn paired(address: &str, name: &str, connected: bool) -> Device {
        Device {
            paired: true,
            trusted: true,
            connected,
            ..found(address, name)
        }
    }

    fn powered(devices: Vec<Device>) -> Facts {
        Facts {
            adapter: Some(adapter(true)),
            devices,
        }
    }

    const PAD: &str = "00:11:22:33:44:01";
    const HEADSET: &str = "00:11:22:33:44:02";

    /// A gamepad found nearby and a headset paired, not connected.
    fn nearby() -> Facts {
        powered(vec![
            found(PAD, "Wireless Controller"),
            Device {
                trusted: false,
                ..paired(HEADSET, "Headset", false)
            },
        ])
    }

    fn section(facts: Facts) -> (Bluetooth, Rc<Fake>, Rc<RefCell<Vec<Failure>>>) {
        let fake = Rc::new(Fake::default());
        let bluetooth = Bluetooth::new(Box::new(FakeBackend(fake.clone())));
        let failures = Rc::new(RefCell::new(Vec::new()));
        bluetooth.connect_failed({
            let failures = failures.clone();
            move |failure| failures.borrow_mut().push(failure.clone())
        });
        bluetooth.start();
        fake.report(facts);
        (bluetooth, fake, failures)
    }

    /// The section with its level open, the agent registered and discovery running.
    fn opened(facts: Facts) -> (Bluetooth, Rc<Fake>, Rc<RefCell<Vec<Failure>>>) {
        let (bluetooth, fake, failures) = section(facts);
        bluetooth.open();
        fake.finish(&Call::RegisterAgent, Ok(()));
        if fake.calls().contains(&Call::StartDiscovery) {
            fake.finish(&Call::StartDiscovery, Ok(()));
        }
        (bluetooth, fake, failures)
    }

    /// What `reply` hands BlueZ, once the test has given it to the section.
    fn answer<T: 'static>() -> (Reply<T>, Answer<T>) {
        let answer = Rc::new(RefCell::new(None));
        let reply: Reply<T> = Box::new({
            let answer = answer.clone();
            move |value| {
                answer.replace(Some(value));
            }
        });
        (reply, answer)
    }

    #[test]
    fn without_bluez_or_an_adapter_the_section_is_hidden_and_asks_nothing() {
        let (bluetooth, fake, failures) = section(Facts::default());

        bluetooth.open();

        assert!(!bluetooth.state().available);
        assert!(fake.calls().is_empty());
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn paired_devices_come_first_then_found_ones_that_give_a_name_each_by_name() {
        let unnamed = Device {
            name: None,
            alias: Some("4C-11-22-33-44-55".to_owned()),
            ..found("4C:11:22:33:44:55", "")
        };
        let (bluetooth, _, _) = section(powered(vec![
            found("00:00:00:00:00:01", "Zeta Pad"),
            unnamed,
            paired("00:00:00:00:00:02", "beta buds", false),
            found("00:00:00:00:00:03", "alpha speaker"),
            paired("00:00:00:00:00:04", "Alpha Pad", true),
        ]));
        let state = bluetooth.state();
        let names = |devices: &[Device]| -> Vec<String> {
            devices
                .iter()
                .map(|device| device.display_name().to_owned())
                .collect()
        };

        assert!(state.available);
        assert_eq!(names(&state.paired), ["Alpha Pad", "beta buds"]);
        assert_eq!(names(&state.found), ["alpha speaker", "Zeta Pad"]);
    }

    #[test]
    fn a_device_without_an_alias_shows_its_name_or_else_its_address() {
        let named = Device {
            alias: None,
            ..found(PAD, "Pad")
        };
        let bare = Device {
            name: None,
            ..named.clone()
        };

        assert_eq!(named.display_name(), "Pad");
        assert_eq!(bare.display_name(), PAD);
    }

    #[test]
    fn the_power_state_says_whether_the_radio_is_on_off_turning_or_blocked() {
        let power = |powered, state: Option<&str>| {
            Power::of(&Adapter {
                path: ADAPTER.to_owned(),
                powered,
                power_state: state.map(str::to_owned),
            })
        };

        assert_eq!(power(true, Some("on")), Power::On);
        assert_eq!(power(false, Some("off")), Power::Off);
        assert_eq!(power(false, Some("off-blocked")), Power::Blocked);
        assert_eq!(power(false, Some("off-enabling")), Power::TurningOn);
        assert_eq!(power(true, Some("on-disabling")), Power::TurningOff);
        // BlueZ before PowerState has only Powered.
        assert_eq!(power(true, None), Power::On);
        assert_eq!(power(false, None), Power::Off);
    }

    #[test]
    fn the_entry_names_the_connected_devices_or_says_how_the_radio_is() {
        let summary = |facts: Facts| section(facts).0.state().summary();
        let blocked = Adapter {
            power_state: Some("off-blocked".to_owned()),
            ..adapter(false)
        };

        assert_eq!(
            summary(powered(vec![
                paired(PAD, "Pad", true),
                paired(HEADSET, "Headset", true),
                found("00:00:00:00:00:09", "Speaker"),
            ])),
            Summary::Connected(vec!["Headset".to_owned(), "Pad".to_owned()])
        );
        assert_eq!(summary(nearby()), Summary::NotConnected);
        assert_eq!(
            summary(Facts {
                adapter: Some(adapter(false)),
                devices: Vec::new(),
            }),
            Summary::Off
        );
        assert_eq!(
            summary(Facts {
                adapter: Some(blocked),
                devices: Vec::new(),
            }),
            Summary::Blocked
        );
    }

    #[test]
    fn opening_the_level_registers_the_agent_and_starts_discovery() {
        let (bluetooth, fake, _) = section(nearby());
        assert!(fake.calls().is_empty());

        bluetooth.open();

        assert_eq!(fake.calls(), [Call::RegisterAgent, Call::StartDiscovery]);
    }

    #[test]
    fn closing_the_level_stops_discovery_and_unregisters_the_agent() {
        let (bluetooth, fake, failures) = opened(nearby());

        bluetooth.close();

        assert_eq!(fake.calls(), [Call::StopDiscovery, Call::UnregisterAgent]);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn discovery_and_the_agent_that_start_after_the_level_closed_are_ended() {
        let (bluetooth, fake, _) = section(nearby());
        bluetooth.open();
        fake.calls();

        bluetooth.close();
        assert!(
            fake.calls().is_empty(),
            "nothing to end before BlueZ answers"
        );
        fake.finish(&Call::StartDiscovery, Ok(()));
        fake.finish(&Call::RegisterAgent, Ok(()));

        assert_eq!(fake.calls(), [Call::StopDiscovery, Call::UnregisterAgent]);
    }

    #[test]
    fn reopening_before_bluez_answers_keeps_what_it_starts() {
        let (bluetooth, fake, _) = section(nearby());
        bluetooth.open();
        bluetooth.close();
        bluetooth.open();
        fake.calls();

        fake.finish(&Call::StartDiscovery, Ok(()));
        fake.finish(&Call::RegisterAgent, Ok(()));

        assert!(fake.calls().is_empty());
        bluetooth.close();
        assert_eq!(fake.calls(), [Call::StopDiscovery, Call::UnregisterAgent]);
    }

    #[test]
    fn with_the_radio_off_discovery_waits_until_it_is_on() {
        let (bluetooth, fake, _) = section(Facts {
            adapter: Some(adapter(false)),
            devices: Vec::new(),
        });

        bluetooth.open();
        assert_eq!(fake.calls(), [Call::RegisterAgent]);
        fake.report(nearby());

        assert_eq!(fake.calls(), [Call::StartDiscovery]);
    }

    #[test]
    fn the_radio_turned_off_and_on_again_starts_discovery_again() {
        let (_bluetooth, fake, _) = opened(nearby());

        fake.report(Facts {
            adapter: Some(adapter(false)),
            ..nearby()
        });
        assert!(fake.calls().is_empty());
        fake.report(nearby());

        assert_eq!(fake.calls(), [Call::StartDiscovery]);
    }

    #[test]
    fn discovery_that_fails_is_reported_and_not_retried_until_the_level_opens_again() {
        let (bluetooth, fake, failures) = section(nearby());
        bluetooth.open();
        fake.finish(&Call::RegisterAgent, Ok(()));
        fake.finish(&Call::StartDiscovery, Err("Busy".to_owned()));
        fake.calls();

        fake.report(powered(vec![found(PAD, "Pad")]));
        assert!(fake.calls().is_empty());
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Discovery("Busy".to_owned())]
        );

        bluetooth.close();
        assert_eq!(fake.calls(), [Call::UnregisterAgent], "nothing to stop");
        bluetooth.open();
        assert_eq!(fake.calls(), [Call::RegisterAgent, Call::StartDiscovery]);
    }

    #[test]
    fn bluez_going_away_hides_the_section_and_coming_back_registers_again() {
        let (bluetooth, fake, _) = opened(nearby());

        fake.report(Facts::default());
        assert!(!bluetooth.state().available);
        assert!(fake.calls().is_empty());
        fake.report(nearby());

        assert!(bluetooth.state().available);
        assert_eq!(fake.calls(), [Call::RegisterAgent, Call::StartDiscovery]);
    }

    #[test]
    fn the_same_report_again_changes_nothing() {
        let (bluetooth, fake, _) = section(nearby());
        let changes = Rc::new(Cell::new(0));
        bluetooth.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });

        fake.report(nearby());
        assert_eq!(changes.get(), 0);
        fake.report(powered(vec![found(PAD, "Wireless Controller")]));
        assert_eq!(changes.get(), 1);
    }

    #[test]
    fn the_switch_turns_the_radio_on_and_off_and_reports_a_refusal() {
        let (bluetooth, fake, failures) = section(Facts {
            adapter: Some(adapter(false)),
            devices: Vec::new(),
        });

        bluetooth.set_powered(true);
        assert_eq!(fake.calls(), [Call::SetPowered(true)]);
        fake.finish(&Call::SetPowered(true), Err("Not Ready".to_owned()));

        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Power {
                on: true,
                error: "Not Ready".to_owned(),
            }]
        );
    }

    #[test]
    fn a_blocked_radio_is_not_switched() {
        let (bluetooth, fake, _) = section(Facts {
            adapter: Some(Adapter {
                power_state: Some("off-blocked".to_owned()),
                ..adapter(false)
            }),
            devices: Vec::new(),
        });

        bluetooth.set_powered(true);

        assert_eq!(bluetooth.state().power, Power::Blocked);
        assert!(fake.calls().is_empty());
    }

    #[test]
    fn pairing_a_found_device_pairs_trusts_and_connects_it() {
        let (bluetooth, fake, failures) = opened(nearby());
        let pad = path(PAD);
        let pending = |busy| {
            Some(Pending {
                device: path(PAD),
                busy,
            })
        };

        bluetooth.pair(&pad);
        assert_eq!(fake.calls(), [Call::Pair(pad.clone())]);
        assert_eq!(bluetooth.state().pending, pending(Busy::Pairing));
        fake.finish(&Call::Pair(pad.clone()), Ok(()));
        assert_eq!(fake.calls(), [Call::Trust(pad.clone())]);
        assert_eq!(bluetooth.state().pending, pending(Busy::Connecting));
        fake.finish(&Call::Trust(pad.clone()), Ok(()));
        assert_eq!(fake.calls(), [Call::Connect(pad.clone())]);
        fake.finish(&Call::Connect(pad), Ok(()));

        assert_eq!(bluetooth.state().pending, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_pairing_that_fails_is_reported_with_the_device_name() {
        let (bluetooth, fake, failures) = opened(nearby());
        let pad = path(PAD);

        bluetooth.pair(&pad);
        fake.finish(&Call::Pair(pad), Err("Authentication Failed".to_owned()));

        assert_eq!(bluetooth.state().pending, None);
        assert!(
            fake.calls()
                .iter()
                .all(|call| !matches!(call, Call::Trust(_)))
        );
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Pair {
                device: "Wireless Controller".to_owned(),
                error: "Authentication Failed".to_owned(),
            }]
        );
    }

    #[test]
    fn a_device_that_pairs_but_does_not_connect_is_reported() {
        let (bluetooth, fake, failures) = opened(nearby());
        let pad = path(PAD);

        bluetooth.pair(&pad);
        fake.finish(&Call::Pair(pad.clone()), Ok(()));
        fake.finish(&Call::Trust(pad.clone()), Ok(()));
        fake.finish(&Call::Connect(pad), Err("Page Timeout".to_owned()));

        assert_eq!(bluetooth.state().pending, None);
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Connect {
                device: "Wireless Controller".to_owned(),
                error: "Page Timeout".to_owned(),
            }]
        );
    }

    #[test]
    fn numeric_comparison_shows_the_code_and_confirming_it_answers_bluez() {
        let (bluetooth, fake, _) = opened(nearby());
        bluetooth.pair(&path(PAD));
        let (reply, answer) = answer();

        fake.ask(AgentRequest::Confirm {
            device: path(PAD),
            passkey: 4_321,
            reply,
        });
        assert_eq!(
            bluetooth.state().prompt,
            Some(Prompt::Confirm {
                device: "Wireless Controller".to_owned(),
                passkey: "004321".to_owned(),
            })
        );
        bluetooth.confirm(true);

        assert_eq!(*answer.borrow(), Some(Some(())));
        assert_eq!(bluetooth.state().prompt, None);
    }

    #[test]
    fn a_refused_code_fails_the_pairing_without_a_report() {
        let (bluetooth, fake, failures) = opened(nearby());
        let pad = path(PAD);
        bluetooth.pair(&pad);
        let (reply, answer) = answer();
        fake.ask(AgentRequest::Confirm {
            device: pad.clone(),
            passkey: 123_456,
            reply,
        });

        bluetooth.confirm(false);
        fake.finish(&Call::Pair(pad), Err("Authentication Rejected".to_owned()));

        assert_eq!(*answer.borrow(), Some(None));
        assert_eq!(bluetooth.state().pending, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_shown_passkey_stays_until_pairing_ends() {
        let (bluetooth, fake, _) = opened(nearby());
        let pad = path(PAD);
        bluetooth.pair(&pad);

        fake.ask(AgentRequest::DisplayPasskey {
            device: pad.clone(),
            passkey: 42,
        });
        assert_eq!(
            bluetooth.state().prompt,
            Some(Prompt::Show {
                device: "Wireless Controller".to_owned(),
                code: "000042".to_owned(),
            })
        );
        fake.finish(&Call::Pair(pad), Ok(()));

        assert_eq!(bluetooth.state().prompt, None);
    }

    #[test]
    fn a_shown_pin_is_shown_as_it_is() {
        let (bluetooth, fake, _) = opened(nearby());

        fake.ask(AgentRequest::DisplayPinCode {
            device: path(PAD),
            pin: "0815".to_owned(),
        });

        assert_eq!(
            bluetooth.state().prompt,
            Some(Prompt::Show {
                device: "Wireless Controller".to_owned(),
                code: "0815".to_owned(),
            })
        );
    }

    #[test]
    fn closing_a_shown_code_cancels_the_pairing_without_a_report() {
        let (bluetooth, fake, failures) = opened(nearby());
        let pad = path(PAD);
        bluetooth.pair(&pad);
        fake.calls();
        fake.ask(AgentRequest::DisplayPasskey {
            device: pad.clone(),
            passkey: 42,
        });

        bluetooth.dismiss();
        fake.finish(
            &Call::Pair(pad.clone()),
            Err("Authentication Canceled".to_owned()),
        );

        assert_eq!(fake.calls(), [Call::CancelPairing(pad)]);
        assert_eq!(bluetooth.state().prompt, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_pin_request_waits_for_the_pin_typed() {
        let (bluetooth, fake, _) = opened(nearby());
        let (reply, answer) = answer();

        fake.ask(AgentRequest::PinCode {
            device: path(PAD),
            reply,
        });
        assert_eq!(
            bluetooth.state().prompt,
            Some(Prompt::EnterPin {
                device: "Wireless Controller".to_owned(),
            })
        );
        bluetooth.enter(Some("0000".to_owned()));

        assert_eq!(*answer.borrow(), Some(Some("0000".to_owned())));
        assert_eq!(bluetooth.state().prompt, None);
    }

    #[test]
    fn an_empty_or_too_long_pin_or_a_cancelled_one_refuses() {
        for typed in [None, Some(String::new()), Some("1".repeat(17))] {
            let (bluetooth, fake, _) = opened(nearby());
            let (reply, answer) = answer();
            fake.ask(AgentRequest::PinCode {
                device: path(PAD),
                reply,
            });

            bluetooth.enter(typed.clone());

            assert_eq!(*answer.borrow(), Some(None), "{typed:?}");
        }
    }

    #[test]
    fn a_passkey_is_up_to_six_digits() {
        for (typed, passkey) in [
            ("012345", Some(12_345)),
            ("7", Some(7)),
            ("1234567", None),
            ("12a4", None),
            ("-1", None),
            ("", None),
        ] {
            let (bluetooth, fake, _) = opened(nearby());
            let (reply, answer) = answer();
            fake.ask(AgentRequest::Passkey {
                device: path(PAD),
                reply,
            });
            assert_eq!(
                bluetooth.state().prompt,
                Some(Prompt::EnterPasskey {
                    device: "Wireless Controller".to_owned(),
                })
            );

            bluetooth.enter(Some(typed.to_owned()));

            assert_eq!(*answer.borrow(), Some(passkey), "{typed:?}");
        }
    }

    #[test]
    fn bluez_cancelling_a_request_takes_its_prompt_away() {
        let (bluetooth, fake, _) = opened(nearby());
        let (reply, answer) = answer();
        fake.ask(AgentRequest::Confirm {
            device: path(PAD),
            passkey: 1,
            reply,
        });

        fake.ask(AgentRequest::Cancel);

        assert_eq!(bluetooth.state().prompt, None);
        assert_eq!(*answer.borrow(), Some(None));
    }

    #[test]
    fn closing_the_level_refuses_the_request_waiting() {
        let (bluetooth, fake, _) = opened(nearby());
        let (reply, answer) = answer::<String>();
        fake.ask(AgentRequest::PinCode {
            device: path(PAD),
            reply,
        });

        bluetooth.close();

        assert_eq!(bluetooth.state().prompt, None);
        assert_eq!(*answer.borrow(), Some(None));
    }

    #[test]
    fn a_request_for_an_unknown_device_names_it_by_its_address() {
        let (bluetooth, fake, _) = opened(nearby());

        fake.ask(AgentRequest::DisplayPasskey {
            device: path("AA:BB:CC:DD:EE:FF"),
            passkey: 1,
        });

        assert!(matches!(
            bluetooth.state().prompt,
            Some(Prompt::Show { device, .. }) if device == "AA:BB:CC:DD:EE:FF"
        ));
    }

    #[test]
    fn only_paired_devices_or_the_one_pairing_are_authorized() {
        let (bluetooth, fake, _) = opened(nearby());
        let authorize = |device: &str| {
            let (reply, answer) = answer();
            fake.ask(AgentRequest::Authorize {
                device: device.to_owned(),
                reply,
            });
            answer.take().expect("the agent answers at once")
        };

        assert_eq!(authorize(&path(HEADSET)), Some(()));
        assert_eq!(authorize(&path(PAD)), None);
        bluetooth.pair(&path(PAD));
        assert_eq!(authorize(&path(PAD)), Some(()));
    }

    #[test]
    fn connecting_a_paired_device_trusts_it_first_so_it_reconnects_by_itself() {
        let (bluetooth, fake, _) = opened(nearby());
        let headset = path(HEADSET);

        bluetooth.connect(&headset);
        assert_eq!(fake.calls(), [Call::Trust(headset.clone())]);
        fake.finish(&Call::Trust(headset.clone()), Ok(()));

        assert_eq!(fake.calls(), [Call::Connect(headset)]);
    }

    #[test]
    fn connecting_a_trusted_device_connects_it_at_once() {
        let (bluetooth, fake, _) = opened(powered(vec![paired(PAD, "Pad", false)]));
        let pad = path(PAD);

        bluetooth.connect(&pad);

        assert_eq!(fake.calls(), [Call::Connect(pad)]);
    }

    #[test]
    fn a_paired_device_disconnects_and_a_refusal_is_reported() {
        let (bluetooth, fake, failures) = opened(powered(vec![paired(PAD, "Pad", true)]));
        let pad = path(PAD);

        bluetooth.disconnect(&pad);
        assert_eq!(fake.calls(), [Call::Disconnect(pad.clone())]);
        assert_eq!(
            bluetooth.state().pending.map(|pending| pending.busy),
            Some(Busy::Disconnecting)
        );
        fake.finish(&Call::Disconnect(pad), Err("Not Connected".to_owned()));

        assert_eq!(bluetooth.state().pending, None);
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Disconnect {
                device: "Pad".to_owned(),
                error: "Not Connected".to_owned(),
            }]
        );
    }

    #[test]
    fn forgetting_removes_a_paired_device_even_with_the_radio_off() {
        let (bluetooth, fake, failures) = section(Facts {
            adapter: Some(adapter(false)),
            devices: vec![paired(PAD, "Pad", false)],
        });
        let pad = path(PAD);

        bluetooth.forget(&pad);
        assert_eq!(fake.calls(), [Call::Remove(pad.clone())]);
        assert_eq!(
            bluetooth.state().pending.map(|pending| pending.busy),
            Some(Busy::Forgetting)
        );
        fake.finish(&Call::Remove(pad), Ok(()));

        assert_eq!(bluetooth.state().pending, None);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn found_devices_are_not_forgotten_or_connected_and_paired_ones_not_paired() {
        let (bluetooth, fake, _) = opened(nearby());

        bluetooth.forget(&path(PAD));
        bluetooth.connect(&path(PAD));
        bluetooth.disconnect(&path(PAD));
        bluetooth.pair(&path(HEADSET));

        assert!(fake.calls().is_empty());
    }

    #[test]
    fn with_the_radio_off_nothing_pairs_or_connects() {
        let (bluetooth, fake, _) = section(Facts {
            adapter: Some(adapter(false)),
            ..nearby()
        });

        bluetooth.pair(&path(PAD));
        bluetooth.connect(&path(HEADSET));

        assert!(fake.calls().is_empty());
    }

    #[test]
    fn nothing_else_starts_while_a_device_is_busy() {
        let (bluetooth, fake, _) = opened(nearby());
        bluetooth.pair(&path(PAD));
        fake.calls();

        bluetooth.connect(&path(HEADSET));
        bluetooth.forget(&path(HEADSET));

        assert!(fake.calls().is_empty());
    }

    #[test]
    fn an_agent_that_fails_to_register_is_reported_and_not_retried_until_the_level_opens_again() {
        let (bluetooth, fake, failures) = section(nearby());
        bluetooth.open();
        fake.finish(&Call::StartDiscovery, Ok(()));
        fake.finish(&Call::RegisterAgent, Err("Already Exists".to_owned()));
        fake.calls();

        fake.report(powered(vec![found(PAD, "Pad")]));
        assert!(fake.calls().is_empty());
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Agent("Already Exists".to_owned())]
        );

        bluetooth.close();
        assert_eq!(
            fake.calls(),
            [Call::StopDiscovery],
            "no agent to unregister"
        );
        bluetooth.open();
        assert_eq!(fake.calls(), [Call::RegisterAgent, Call::StartDiscovery]);
    }

    #[test]
    fn discovery_or_the_agent_that_fails_to_end_on_close_is_reported() {
        let (bluetooth, fake, failures) = opened(nearby());

        bluetooth.close();
        fake.finish(&Call::StopDiscovery, Err("Not Authorized".to_owned()));
        fake.finish(&Call::UnregisterAgent, Err("Does Not Exist".to_owned()));

        assert_eq!(
            failures.borrow().as_slice(),
            [
                Failure::Discovery("Not Authorized".to_owned()),
                Failure::Agent("Does Not Exist".to_owned()),
            ]
        );
    }

    #[test]
    fn a_device_that_cannot_be_trusted_is_reported_and_not_connected() {
        let (bluetooth, fake, failures) = opened(nearby());
        let headset = path(HEADSET);
        bluetooth.connect(&headset);
        fake.calls();

        fake.finish(&Call::Trust(headset), Err("Failed".to_owned()));

        assert!(fake.calls().is_empty());
        assert_eq!(bluetooth.state().pending, None);
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Connect {
                device: "Headset".to_owned(),
                error: "Failed".to_owned(),
            }]
        );
    }

    #[test]
    fn forgetting_that_fails_is_reported_with_the_device_name() {
        let (bluetooth, fake, failures) = opened(powered(vec![paired(PAD, "Pad", false)]));
        let pad = path(PAD);

        bluetooth.forget(&pad);
        fake.finish(&Call::Remove(pad), Err("Does Not Exist".to_owned()));

        assert_eq!(bluetooth.state().pending, None);
        assert_eq!(
            failures.borrow().as_slice(),
            [Failure::Forget {
                device: "Pad".to_owned(),
                error: "Does Not Exist".to_owned(),
            }]
        );
    }

    #[test]
    fn a_new_request_refuses_the_passkey_request_waiting() {
        let (bluetooth, fake, _) = opened(nearby());
        let (reply, answer) = answer::<u32>();
        fake.ask(AgentRequest::Passkey {
            device: path(PAD),
            reply,
        });

        fake.ask(AgentRequest::DisplayPasskey {
            device: path(PAD),
            passkey: 42,
        });

        assert_eq!(*answer.borrow(), Some(None));
        assert_eq!(
            bluetooth.state().prompt,
            Some(Prompt::Show {
                device: "Wireless Controller".to_owned(),
                code: "000042".to_owned(),
            })
        );
    }

    #[test]
    fn devices_whose_names_differ_only_in_case_are_ordered_by_address() {
        let (bluetooth, _, _) = section(powered(vec![
            found("00:00:00:00:00:01", "pad"),
            paired("00:00:00:00:00:04", "Buds", false),
            found("00:00:00:00:00:02", "Pad"),
            paired("00:00:00:00:00:03", "buds", false),
        ]));
        let state = bluetooth.state();
        let addresses = |devices: &[Device]| -> Vec<String> {
            devices
                .iter()
                .map(|device| device.address.clone())
                .collect()
        };

        assert_eq!(
            addresses(&state.paired),
            ["00:00:00:00:00:03", "00:00:00:00:00:04"]
        );
        assert_eq!(
            addresses(&state.found),
            ["00:00:00:00:00:01", "00:00:00:00:00:02"]
        );
    }
}
