//! The NetworkManager backend of the Network section (`pindu_core::system::network`), over
//! `org.freedesktop.NetworkManager` on the system bus with `gio`: an object manager client
//! follows the devices, access points, active and saved connections (and NetworkManager
//! coming and going), the devices' `StateChanged` signals tell the section how a join ends,
//! and method calls do what the section asks. The Flatpak has no system bus, so there the
//! section stays hidden and nothing touches the bus.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use pindu_core::system::network::{
    AccessPoint, Added, Device, DeviceKind, Done, Event, Facts, JoinRequest, NetworkBackend,
    Primary, SavedConnection, Value,
};

use crate::permission_service;

/// NetworkManager's bus name, the root its object manager lives on, and its own object.
const NM: &str = "org.freedesktop.NetworkManager";
const NM_ROOT: &str = "/org/freedesktop";
const NM_PATH: &str = "/org/freedesktop/NetworkManager";
const DEVICE: &str = "org.freedesktop.NetworkManager.Device";
const WIRELESS: &str = "org.freedesktop.NetworkManager.Device.Wireless";
const ACCESS_POINT: &str = "org.freedesktop.NetworkManager.AccessPoint";
const ACTIVE: &str = "org.freedesktop.NetworkManager.Connection.Active";
const CONNECTION: &str = "org.freedesktop.NetworkManager.Settings.Connection";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// The `DeviceType`s of a wired and a Wi-Fi device.
const TYPE_ETHERNET: u32 = 1;
const TYPE_WIFI: u32 = 2;

/// How long a join may take before NetworkManager answers, in milliseconds: it answers
/// once the activation has started, but polkit may ask first.
const JOIN_TIMEOUT: i32 = 60_000;

/// NetworkManager on the system bus.
#[derive(Default)]
pub struct NetworkManagerBackend {
    inner: Rc<Inner>,
}

/// Where the section hears what NetworkManager says.
type Events = Rc<dyn Fn(Event)>;

#[derive(Default)]
struct Inner {
    /// Follows NetworkManager's objects once [`NetworkBackend::start`] has connected.
    manager: RefCell<Option<gio::DBusObjectManagerClient>>,
    /// Where the section hears what NetworkManager says.
    events: RefCell<Option<Events>>,
    /// What `GetPermissions` answered last.
    permissions: RefCell<BTreeMap<String, String>>,
    /// The SSID of each saved connection read, `None` for one that is not Wi-Fi.
    saved: RefCell<HashMap<String, Option<Vec<u8>>>>,
    /// The saved connections whose settings are being read.
    reading: RefCell<HashSet<String>>,
    /// Whether a report waits for the main loop to be idle, so a burst of changes during a
    /// scan becomes one report.
    scheduled: Cell<bool>,
    /// The signals followed on the bus.
    subscriptions: RefCell<Vec<gio::SignalSubscription>>,
}

impl NetworkManagerBackend {
    /// A backend that follows nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The system bus connection NetworkManager is followed on, once there is one.
    fn connection(&self) -> Option<gio::DBusConnection> {
        self.inner
            .manager
            .borrow()
            .as_ref()
            .map(DBusObjectManagerClientExt::connection)
    }

    /// Calls `method` of `interface` on NetworkManager's object at `path`, reporting the
    /// reply, or why there is none, to `done`.
    fn call(
        &self,
        path: &str,
        interface: &'static str,
        method: &'static str,
        parameters: glib::Variant,
        timeout: i32,
        done: Box<dyn FnOnce(Result<glib::Variant, String>)>,
    ) {
        let Some(connection) = self.connection() else {
            done(Err("NetworkManager is not running".to_owned()));
            return;
        };
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            let result = connection
                .call_future(
                    Some(NM),
                    &path,
                    interface,
                    method,
                    Some(&parameters),
                    None,
                    gio::DBusCallFlags::NONE,
                    timeout,
                )
                .await;
            done(result.map_err(bus_error_message));
        });
    }
}

impl NetworkBackend for NetworkManagerBackend {
    fn start(&self, events: Box<dyn Fn(Event)>) {
        if permission_service::in_flatpak() {
            events(Event::Facts(Facts::default()));
            return;
        }
        let events: Events = Rc::from(events);
        self.inner.events.replace(Some(events.clone()));
        let inner = self.inner.clone();
        glib::spawn_future_local(async move {
            let manager = match gio::DBusObjectManagerClient::new_for_bus_future(
                gio::BusType::System,
                gio::DBusObjectManagerClientFlags::NONE,
                NM,
                NM_ROOT,
            )
            .await
            {
                Ok(manager) => manager,
                Err(error) => {
                    glib::g_debug!("pindu", "No NetworkManager on the system bus: {error}");
                    events(Event::Facts(Facts::default()));
                    return;
                }
            };
            inner.manager.replace(Some(manager.clone()));
            follow(&inner, &manager);
            read_permissions(&inner);
            schedule(&inner);
        });
    }

    fn scan(&self, device: &str) {
        let options: HashMap<String, glib::Variant> = HashMap::new();
        self.call(
            device,
            WIRELESS,
            "RequestScan",
            (options,).to_variant(),
            -1,
            Box::new(|result| {
                // NetworkManager refuses a scan soon after the last one, which changes
                // nothing: the list it has is recent.
                if let Err(error) = result {
                    glib::g_debug!("pindu", "Wi-Fi scan: {error}");
                }
            }),
        );
    }

    fn set_wireless(&self, enabled: bool, done: Done) {
        self.call(
            NM_PATH,
            PROPERTIES,
            "Set",
            (NM, "WirelessEnabled", enabled.to_variant()).to_variant(),
            -1,
            Box::new(move |result| done(result.map(|_| ()))),
        );
    }

    fn activate(&self, connection: &str, device: &str, done: Done) {
        let parameters = match (
            object_path(connection),
            object_path(device),
            object_path("/"),
        ) {
            (Ok(connection), Ok(device), Ok(specific)) => (connection, device, specific),
            (Err(error), _, _) | (_, Err(error), _) | (_, _, Err(error)) => {
                done(Err(error));
                return;
            }
        }
        .to_variant();
        self.call(
            NM_PATH,
            NM,
            "ActivateConnection",
            parameters,
            JOIN_TIMEOUT,
            Box::new(move |result| done(result.map(|_| ()))),
        );
    }

    fn add_and_activate(&self, request: JoinRequest, done: Added) {
        let settings: HashMap<&str, HashMap<&str, glib::Variant>> = request
            .settings
            .iter()
            .map(|(name, values)| {
                let values = values
                    .iter()
                    .map(|(key, value)| (*key, setting_variant(value)))
                    .collect();
                (*name, values)
            })
            .collect();
        let options: HashMap<&str, glib::Variant> =
            HashMap::from([("persist", "disk".to_variant())]);
        let (device, access_point) = match (
            object_path(&request.device),
            object_path(&request.access_point),
        ) {
            (Ok(device), Ok(access_point)) => (device, access_point),
            (Err(error), _) | (_, Err(error)) => {
                done(Err(error));
                return;
            }
        };
        let parameters = (settings, device, access_point, options).to_variant();
        self.call(
            NM_PATH,
            NM,
            "AddAndActivateConnection2",
            parameters,
            JOIN_TIMEOUT,
            Box::new(move |result| {
                done(result.and_then(|reply| {
                    reply
                        .try_child_value(0)
                        .and_then(|path| path.str().map(str::to_owned))
                        .ok_or_else(|| "NetworkManager gave no connection".to_owned())
                }));
            }),
        );
    }

    fn delete(&self, connection: &str, done: Done) {
        self.call(
            connection,
            CONNECTION,
            "Delete",
            ().to_variant(),
            -1,
            Box::new(move |result| done(result.map(|_| ()))),
        );
    }
}

/// Reports to the section whenever NetworkManager's objects change, it comes or goes, its
/// permissions may have changed or a saved connection was changed, and passes each
/// device's state changes on.
fn follow(inner: &Rc<Inner>, manager: &gio::DBusObjectManagerClient) {
    let weak = Rc::downgrade(inner);
    let changed = move || {
        if let Some(inner) = weak.upgrade() {
            schedule(&inner);
        }
    };
    manager.connect_object_added({
        let changed = changed.clone();
        move |_, _| changed()
    });
    manager.connect_object_removed({
        let changed = changed.clone();
        move |_, _| changed()
    });
    manager.connect_interface_added({
        let changed = changed.clone();
        move |_, _, _| changed()
    });
    manager.connect_interface_removed({
        let changed = changed.clone();
        move |_, _, _| changed()
    });
    manager.connect_local("interface-proxy-properties-changed", false, {
        let changed = changed.clone();
        move |_| {
            changed();
            None
        }
    });
    // NetworkManager starting or stopping: what it allows and what it saved are read again.
    let weak = Rc::downgrade(inner);
    manager.connect_notify_local(Some("name-owner"), move |_, _| {
        if let Some(inner) = weak.upgrade() {
            inner.saved.borrow_mut().clear();
            read_permissions(&inner);
            schedule(&inner);
        }
    });

    let bus = manager.connection();
    let mut subscriptions = inner.subscriptions.borrow_mut();
    let weak = Rc::downgrade(inner);
    subscriptions.push(bus.subscribe_to_signal(
        Some(NM),
        Some(NM),
        Some("CheckPermissions"),
        Some(NM_PATH),
        None,
        gio::DBusSignalFlags::NONE,
        move |_| {
            if let Some(inner) = weak.upgrade() {
                read_permissions(&inner);
            }
        },
    ));
    let weak = Rc::downgrade(inner);
    subscriptions.push(bus.subscribe_to_signal(
        Some(NM),
        Some(CONNECTION),
        Some("Updated"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            if let Some(inner) = weak.upgrade() {
                inner.saved.borrow_mut().remove(signal.object_path);
                schedule(&inner);
            }
        },
    ));
    let weak = Rc::downgrade(inner);
    subscriptions.push(bus.subscribe_to_signal(
        Some(NM),
        Some(DEVICE),
        Some("StateChanged"),
        None,
        None,
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let Some((state, _, reason)) = signal.parameters.get::<(u32, u32, u32)>() else {
                return;
            };
            let events = inner.events.borrow().clone();
            if let Some(events) = events {
                events(Event::DeviceState {
                    device: signal.object_path.to_owned(),
                    state,
                    reason,
                });
            }
        },
    ));
}

/// Reports to the section once the main loop is idle.
fn schedule(inner: &Rc<Inner>) {
    if inner.scheduled.replace(true) {
        return;
    }
    let weak = Rc::downgrade(inner);
    glib::idle_add_local_once(move || {
        if let Some(inner) = weak.upgrade() {
            inner.scheduled.set(false);
            report(&inner);
        }
    });
}

/// Tells the section what NetworkManager says now.
fn report(inner: &Rc<Inner>) {
    let Some(manager) = inner.manager.borrow().clone() else {
        return;
    };
    let facts = read_facts(inner, &manager);
    let events = inner.events.borrow().clone();
    if let Some(events) = events {
        events(Event::Facts(facts));
    }
}

/// Asks NetworkManager what polkit lets the session do, and reports it.
fn read_permissions(inner: &Rc<Inner>) {
    let Some(manager) = inner.manager.borrow().clone() else {
        return;
    };
    if manager.name_owner().is_none() {
        inner.permissions.borrow_mut().clear();
        return;
    }
    let bus = manager.connection();
    let weak = Rc::downgrade(inner);
    glib::spawn_future_local(async move {
        let reply = bus
            .call_future(
                Some(NM),
                NM_PATH,
                NM,
                "GetPermissions",
                None,
                Some(glib::VariantTy::new("(a{ss})").expect("a valid type")),
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        let Some(inner) = weak.upgrade() else {
            return;
        };
        match reply.map(|reply| reply.get::<(HashMap<String, String>,)>()) {
            Ok(Some((permissions,))) => {
                inner.permissions.replace(permissions.into_iter().collect());
                schedule(&inner);
            }
            Ok(None) => {}
            Err(error) => glib::g_debug!("pindu", "NetworkManager GetPermissions: {error}"),
        }
    });
}

/// Reads the settings of the saved connection at `path`, and reports once it has them.
fn read_saved(inner: &Rc<Inner>, bus: gio::DBusConnection, path: String) {
    if !inner.reading.borrow_mut().insert(path.clone()) {
        return;
    }
    let weak = Rc::downgrade(inner);
    glib::spawn_future_local(async move {
        let reply = bus
            .call_future(
                Some(NM),
                &path,
                CONNECTION,
                "GetSettings",
                None,
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        let Some(inner) = weak.upgrade() else {
            return;
        };
        inner.reading.borrow_mut().remove(&path);
        let ssid = match reply {
            Ok(reply) => wifi_ssid(&reply),
            Err(error) => {
                glib::g_debug!("pindu", "NetworkManager GetSettings {path}: {error}");
                None
            }
        };
        inner.saved.borrow_mut().insert(path, ssid);
        schedule(&inner);
    });
}

/// The SSID in `GetSettings`' reply (`(a{sa{sv}})`), or `None` for a connection that is not
/// Wi-Fi.
fn wifi_ssid(reply: &glib::Variant) -> Option<Vec<u8>> {
    let (settings,) = reply.get::<(HashMap<String, HashMap<String, glib::Variant>>,)>()?;
    settings
        .get("802-11-wireless")?
        .get("ssid")?
        .get::<Vec<u8>>()
}

/// What NetworkManager's objects say now, as the object manager has them; saved
/// connections not read yet are read, and reported once they are.
fn read_facts(inner: &Rc<Inner>, manager: &gio::DBusObjectManagerClient) -> Facts {
    if manager.name_owner().is_none() {
        return Facts::default();
    }
    let property = |path: &str, interface: &str, name: &str| -> Option<glib::Variant> {
        DBusObjectManagerExt::interface(manager, path, interface)
            .and_downcast::<gio::DBusProxy>()
            .and_then(|proxy| proxy.cached_property(name))
    };
    let flag = |name: &str| {
        property(NM_PATH, NM, name)
            .and_then(|value| value.get::<bool>())
            .unwrap_or(false)
    };
    let ssid_of =
        |path: &str| property(path, ACCESS_POINT, "Ssid").and_then(|value| value.get::<Vec<u8>>());

    let mut devices = Vec::new();
    let mut access_points = Vec::new();
    let mut saved_paths = Vec::new();
    for object in manager.objects() {
        let path = object.object_path();
        if DBusObjectExt::interface(&object, CONNECTION).is_some() {
            saved_paths.push(path.to_string());
        }
        let Some(kind) = property(&path, DEVICE, "DeviceType")
            .and_then(|value| value.get::<u32>())
            .and_then(|device_type| match device_type {
                TYPE_ETHERNET => Some(DeviceKind::Wired),
                TYPE_WIFI => Some(DeviceKind::Wifi),
                _ => None,
            })
        else {
            continue;
        };
        let mut active_ssid = None;
        if kind == DeviceKind::Wifi {
            active_ssid = property(&path, WIRELESS, "ActiveAccessPoint")
                .and_then(|value| value.str().map(str::to_owned))
                .filter(|ap| ap != "/")
                .and_then(|ap| ssid_of(&ap));
            let aps = property(&path, WIRELESS, "AccessPoints")
                .and_then(|value| value.get::<Vec<glib::variant::ObjectPath>>())
                .unwrap_or_default();
            for ap in aps {
                let ap = ap.as_str();
                let number = |name: &str| {
                    property(ap, ACCESS_POINT, name)
                        .and_then(|value| value.get::<u32>())
                        .unwrap_or(0)
                };
                access_points.push(AccessPoint {
                    path: ap.to_owned(),
                    device: path.to_string(),
                    ssid: ssid_of(ap).unwrap_or_default(),
                    strength: property(ap, ACCESS_POINT, "Strength")
                        .and_then(|value| value.get::<u8>())
                        .unwrap_or(0),
                    flags: number("Flags"),
                    wpa_flags: number("WpaFlags"),
                    rsn_flags: number("RsnFlags"),
                });
            }
        }
        devices.push(Device {
            path: path.to_string(),
            kind,
            interface: property(&path, DEVICE, "Interface")
                .and_then(|value| value.get::<String>())
                .unwrap_or_default(),
            state: property(&path, DEVICE, "State")
                .and_then(|value| value.get::<u32>())
                .unwrap_or(0),
            active_ssid,
        });
    }

    let mut saved = Vec::new();
    {
        let mut cache = inner.saved.borrow_mut();
        cache.retain(|path, _| saved_paths.contains(path));
        for path in saved_paths {
            match cache.get(&path) {
                Some(Some(ssid)) => saved.push(SavedConnection {
                    path,
                    ssid: ssid.clone(),
                }),
                Some(None) => {}
                None => read_saved(inner, manager.connection(), path),
            }
        }
    }

    let primary = property(NM_PATH, NM, "PrimaryConnection")
        .and_then(|value| value.str().map(str::to_owned))
        .filter(|path| path != "/")
        .map(|path| Primary {
            connection_type: property(&path, ACTIVE, "Type")
                .and_then(|value| value.get::<String>())
                .unwrap_or_default(),
            id: property(&path, ACTIVE, "Id")
                .and_then(|value| value.get::<String>())
                .unwrap_or_default(),
            devices: property(&path, ACTIVE, "Devices")
                .and_then(|value| value.get::<Vec<glib::variant::ObjectPath>>())
                .unwrap_or_default()
                .iter()
                .map(|device| device.as_str().to_owned())
                .collect(),
        });

    Facts {
        running: true,
        permissions: inner.permissions.borrow().clone(),
        wireless_enabled: flag("WirelessEnabled"),
        wireless_hardware_enabled: flag("WirelessHardwareEnabled"),
        devices,
        access_points,
        saved,
        primary,
    }
}

/// A setting's value as NetworkManager takes it.
fn setting_variant(value: &Value) -> glib::Variant {
    match value {
        Value::Str(text) => text.to_variant(),
        Value::Bytes(bytes) => bytes.to_variant(),
        Value::U32(number) => number.to_variant(),
        Value::Bool(flag) => flag.to_variant(),
        Value::StrList(texts) => texts.to_variant(),
    }
}

/// `path` as a D-Bus object path, or why it is not one.
fn object_path(path: &str) -> Result<glib::variant::ObjectPath, String> {
    glib::variant::ObjectPath::try_from(path.to_owned())
        .map_err(|_| format!("{path:?} is not an object path"))
}

/// The message of a bus error without the remote error's name in front of it.
fn bus_error_message(mut error: glib::Error) -> String {
    gio::DBusError::strip_remote_error(&mut error);
    error.message().to_owned()
}
