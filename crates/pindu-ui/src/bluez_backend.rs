//! The BlueZ backend of the Bluetooth section (`pindu_core::system::bluetooth`), over
//! `org.bluez` on the system bus with `gio`: an object manager client follows the adapter
//! and its devices (and BlueZ coming and going), method calls and property writes do what
//! the section asks, and an exported `org.bluez.Agent1` hands BlueZ's pairing requests to
//! the section while it is registered. The Flatpak has no system bus, so there the section
//! stays hidden and nothing touches the bus.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use pindu_core::system::bluetooth::{
    Adapter, AgentRequest, BluetoothBackend, Device, Done, Facts, Reply,
};

use crate::permission_service;

/// BlueZ's bus name.
const BLUEZ: &str = "org.bluez";
/// The object BlueZ's agent manager lives on.
const BLUEZ_PATH: &str = "/org/bluez";
const ADAPTER: &str = "org.bluez.Adapter1";
const DEVICE: &str = "org.bluez.Device1";
const AGENT_MANAGER: &str = "org.bluez.AgentManager1";
const AGENT: &str = "org.bluez.Agent1";
const PROPERTIES: &str = "org.freedesktop.DBus.Properties";

/// Where the launcher exports its pairing agent.
const AGENT_PATH: &str = "/io/github/tim_van_oudheusden/pindu_launcher/bluetooth_agent";
/// The agent's capability: it can show a code and take one typed with the on-screen
/// keyboard.
const CAPABILITY: &str = "KeyboardDisplay";
/// The error an agent answers a refused request with.
const REJECTED: &str = "org.bluez.Error.Rejected";

/// How long pairing may take, in milliseconds: the user may have a code to confirm or
/// type, and BlueZ ends a pairing that takes too long itself.
const PAIR_TIMEOUT: i32 = 120_000;
/// How long connecting may take, in milliseconds: a device asleep takes a while to answer.
const CONNECT_TIMEOUT: i32 = 60_000;

/// `org.bluez.Agent1` as BlueZ calls it.
const AGENT_XML: &str = r#"<node>
  <interface name="org.bluez.Agent1">
    <method name="Release"/>
    <method name="RequestPinCode">
      <arg name="device" type="o" direction="in"/>
      <arg name="pincode" type="s" direction="out"/>
    </method>
    <method name="DisplayPinCode">
      <arg name="device" type="o" direction="in"/>
      <arg name="pincode" type="s" direction="in"/>
    </method>
    <method name="RequestPasskey">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="out"/>
    </method>
    <method name="DisplayPasskey">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="in"/>
      <arg name="entered" type="q" direction="in"/>
    </method>
    <method name="RequestConfirmation">
      <arg name="device" type="o" direction="in"/>
      <arg name="passkey" type="u" direction="in"/>
    </method>
    <method name="RequestAuthorization">
      <arg name="device" type="o" direction="in"/>
    </method>
    <method name="AuthorizeService">
      <arg name="device" type="o" direction="in"/>
      <arg name="uuid" type="s" direction="in"/>
    </method>
    <method name="Cancel"/>
  </interface>
</node>"#;

/// BlueZ on the system bus.
#[derive(Default)]
pub struct BluezBackend {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Follows BlueZ's objects once [`BluetoothBackend::start`] has connected.
    manager: RefCell<Option<gio::DBusObjectManagerClient>>,
    /// The exported agent while it is registered.
    agent: RefCell<Option<(gio::DBusConnection, gio::RegistrationId)>>,
}

impl BluezBackend {
    /// A backend that follows nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// The system bus connection BlueZ is followed on, once there is one.
    fn connection(&self) -> Option<gio::DBusConnection> {
        self.inner
            .manager
            .borrow()
            .as_ref()
            .map(DBusObjectManagerClientExt::connection)
    }

    /// Calls `method` of `interface` on BlueZ's object at `path`, reporting to `done`.
    fn call(
        &self,
        path: &str,
        interface: &'static str,
        method: &'static str,
        parameters: Option<glib::Variant>,
        timeout: i32,
        done: Done,
    ) {
        let Some(connection) = self.connection() else {
            done(Err("BlueZ is not running".to_owned()));
            return;
        };
        let path = path.to_owned();
        glib::spawn_future_local(async move {
            let result = connection
                .call_future(
                    Some(BLUEZ),
                    &path,
                    interface,
                    method,
                    parameters.as_ref(),
                    None,
                    gio::DBusCallFlags::NONE,
                    timeout,
                )
                .await;
            done(result.map(|_| ()).map_err(bus_error_message));
        });
    }

    /// Sets the boolean property `name` of `interface` on BlueZ's object at `path`.
    fn set_flag(&self, path: &str, interface: &str, name: &str, value: bool, done: Done) {
        let parameters = (interface, name, value.to_variant()).to_variant();
        self.call(path, PROPERTIES, "Set", Some(parameters), -1, done);
    }
}

impl BluetoothBackend for BluezBackend {
    fn start(&self, changed: Box<dyn Fn(Facts)>) {
        if permission_service::in_flatpak() {
            changed(Facts::default());
            return;
        }
        let inner = self.inner.clone();
        glib::spawn_future_local(async move {
            let manager = match gio::DBusObjectManagerClient::new_for_bus_future(
                gio::BusType::System,
                gio::DBusObjectManagerClientFlags::NONE,
                BLUEZ,
                "/",
            )
            .await
            {
                Ok(manager) => manager,
                Err(error) => {
                    glib::g_debug!("pindu", "No BlueZ on the system bus: {error}");
                    changed(Facts::default());
                    return;
                }
            };
            let changed = Rc::new(changed);
            let report = move |manager: &gio::DBusObjectManagerClient| changed(read_facts(manager));
            let report = Rc::new(report);
            manager.connect_object_added({
                let report = report.clone();
                move |manager, _| report(manager)
            });
            manager.connect_object_removed({
                let report = report.clone();
                move |manager, _| report(manager)
            });
            manager.connect_interface_added({
                let report = report.clone();
                move |manager, _, _| report(manager)
            });
            manager.connect_interface_removed({
                let report = report.clone();
                move |manager, _, _| report(manager)
            });
            // BlueZ starting or stopping.
            manager.connect_notify_local(Some("name-owner"), {
                let report = report.clone();
                move |manager, _| report(manager)
            });
            manager.connect_local("interface-proxy-properties-changed", false, {
                let report = report.clone();
                move |values| {
                    if let Some(manager) = values
                        .first()
                        .and_then(|value| value.get::<gio::DBusObjectManagerClient>().ok())
                    {
                        report(&manager);
                    }
                    None
                }
            });
            report(&manager);
            inner.manager.replace(Some(manager));
        });
    }

    fn set_powered(&self, adapter: &str, powered: bool, done: Done) {
        self.set_flag(adapter, ADAPTER, "Powered", powered, done);
    }

    fn start_discovery(&self, adapter: &str, done: Done) {
        self.call(adapter, ADAPTER, "StartDiscovery", None, -1, done);
    }

    fn stop_discovery(&self, adapter: &str, done: Done) {
        self.call(adapter, ADAPTER, "StopDiscovery", None, -1, done);
    }

    fn register_agent(&self, requests: Box<dyn Fn(AgentRequest)>, done: Done) {
        let (Some(connection), Some(manager)) =
            (self.connection(), self.inner.manager.borrow().clone())
        else {
            done(Err("BlueZ is not running".to_owned()));
            return;
        };
        let info =
            match gio::DBusNodeInfo::for_xml(AGENT_XML).map(|node| node.lookup_interface(AGENT)) {
                Ok(Some(info)) => info,
                Ok(None) => {
                    done(Err(format!("{AGENT} is not described")));
                    return;
                }
                Err(error) => {
                    done(Err(error.to_string()));
                    return;
                }
            };
        // An agent left from before BlueZ restarted is still exported.
        unexport_agent(&self.inner);
        let id = connection
            .register_object(AGENT_PATH, &info)
            .method_call(move |_, sender, _, _, method, parameters, invocation| {
                // Only BlueZ asks the agent anything; another client on the bus could
                // otherwise put a code in front of the user.
                let owner = manager.name_owner();
                if sender.is_none() || sender != owner.as_deref() {
                    invocation.return_dbus_error(REJECTED, "Only BlueZ talks to this agent");
                    return;
                }
                handle_agent_call(&requests, method, &parameters, invocation);
            })
            .build();
        let id = match id {
            Ok(id) => id,
            Err(error) => {
                done(Err(error.to_string()));
                return;
            }
        };
        self.inner.agent.replace(Some((connection.clone(), id)));
        let inner = self.inner.clone();
        glib::spawn_future_local(async move {
            let path = glib::variant::ObjectPath::try_from(AGENT_PATH.to_owned())
                .expect("the agent's path is an object path");
            let mut result = connection
                .call_future(
                    Some(BLUEZ),
                    BLUEZ_PATH,
                    AGENT_MANAGER,
                    "RegisterAgent",
                    Some(&(path.clone(), CAPABILITY).to_variant()),
                    None,
                    gio::DBusCallFlags::NONE,
                    -1,
                )
                .await;
            if result.is_ok() {
                result = connection
                    .call_future(
                        Some(BLUEZ),
                        BLUEZ_PATH,
                        AGENT_MANAGER,
                        "RequestDefaultAgent",
                        Some(&(path,).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        -1,
                    )
                    .await;
            }
            if result.is_err() {
                unexport_agent(&inner);
            }
            done(result.map(|_| ()).map_err(bus_error_message));
        });
    }

    fn unregister_agent(&self, done: Done) {
        let inner = self.inner.clone();
        let path = glib::variant::ObjectPath::try_from(AGENT_PATH.to_owned())
            .expect("the agent's path is an object path");
        self.call(
            BLUEZ_PATH,
            AGENT_MANAGER,
            "UnregisterAgent",
            Some((path,).to_variant()),
            -1,
            Box::new(move |result| {
                unexport_agent(&inner);
                done(result);
            }),
        );
    }

    fn pair(&self, device: &str, done: Done) {
        self.call(device, DEVICE, "Pair", None, PAIR_TIMEOUT, done);
    }

    fn cancel_pairing(&self, device: &str) {
        self.call(
            device,
            DEVICE,
            "CancelPairing",
            None,
            -1,
            Box::new(|result| {
                if let Err(error) = result {
                    glib::g_debug!("pindu", "CancelPairing: {error}");
                }
            }),
        );
    }

    fn trust(&self, device: &str, done: Done) {
        self.set_flag(device, DEVICE, "Trusted", true, done);
    }

    fn connect(&self, device: &str, done: Done) {
        self.call(device, DEVICE, "Connect", None, CONNECT_TIMEOUT, done);
    }

    fn disconnect(&self, device: &str, done: Done) {
        self.call(device, DEVICE, "Disconnect", None, -1, done);
    }

    fn remove(&self, adapter: &str, device: &str, done: Done) {
        let Ok(device) = glib::variant::ObjectPath::try_from(device.to_owned()) else {
            done(Err(format!("{device} is not an object path")));
            return;
        };
        self.call(
            adapter,
            ADAPTER,
            "RemoveDevice",
            Some((device,).to_variant()),
            -1,
            done,
        );
    }
}

/// Takes the agent off the bus.
fn unexport_agent(inner: &Inner) {
    if let Some((connection, id)) = inner.agent.take()
        && let Err(error) = connection.unregister_object(id)
    {
        glib::g_debug!("pindu", "Unexporting the Bluetooth agent: {error}");
    }
}

/// The first adapter BlueZ has, by path, and its devices.
fn read_facts(manager: &gio::DBusObjectManagerClient) -> Facts {
    let objects = manager.objects();
    let proxies = |interface: &str| -> Vec<gio::DBusProxy> {
        objects
            .iter()
            .filter_map(|object| DBusObjectExt::interface(object, interface))
            .filter_map(|proxy| proxy.downcast::<gio::DBusProxy>().ok())
            .collect()
    };
    let Some(adapter) = proxies(ADAPTER)
        .into_iter()
        .min_by_key(|proxy| proxy.object_path())
    else {
        return Facts::default();
    };
    let adapter = Adapter {
        path: adapter.object_path().to_string(),
        powered: flag(&adapter, "Powered"),
        power_state: text(&adapter, "PowerState"),
    };
    let devices = proxies(DEVICE)
        .iter()
        .filter(|proxy| text(proxy, "Adapter").as_deref() == Some(adapter.path.as_str()))
        .map(|proxy| Device {
            path: proxy.object_path().to_string(),
            address: text(proxy, "Address").unwrap_or_default(),
            name: text(proxy, "Name"),
            alias: text(proxy, "Alias"),
            icon: text(proxy, "Icon"),
            paired: flag(proxy, "Paired"),
            trusted: flag(proxy, "Trusted"),
            connected: flag(proxy, "Connected"),
        })
        .collect();
    Facts {
        adapter: Some(adapter),
        devices,
    }
}

/// A boolean property as last cached; `false` when BlueZ has not given it.
fn flag(proxy: &gio::DBusProxy, name: &str) -> bool {
    proxy
        .cached_property(name)
        .and_then(|value| value.get::<bool>())
        .unwrap_or(false)
}

/// A string or object path property as last cached.
fn text(proxy: &gio::DBusProxy, name: &str) -> Option<String> {
    proxy
        .cached_property(name)
        .and_then(|value| value.str().map(str::to_owned))
}

/// Hands a call BlueZ makes on the agent to `requests`, answering it once the section
/// does, or at once for the calls that need no answer.
fn handle_agent_call(
    requests: &dyn Fn(AgentRequest),
    method: &str,
    parameters: &glib::Variant,
    invocation: gio::DBusMethodInvocation,
) {
    let device = || {
        parameters
            .try_child_value(0)
            .and_then(|value| value.str().map(str::to_owned))
            .unwrap_or_default()
    };
    match method {
        "Release" => invocation.return_value(None),
        "Cancel" => {
            requests(AgentRequest::Cancel);
            invocation.return_value(None);
        }
        "RequestPinCode" => requests(AgentRequest::PinCode {
            device: device(),
            reply: answer(invocation, |pin: String| Some((pin,).to_variant())),
        }),
        "DisplayPinCode" => {
            if let Some((_, pin)) = parameters.get::<(glib::variant::ObjectPath, String)>() {
                requests(AgentRequest::DisplayPinCode {
                    device: device(),
                    pin,
                });
            }
            invocation.return_value(None);
        }
        "RequestPasskey" => requests(AgentRequest::Passkey {
            device: device(),
            reply: answer(invocation, |passkey: u32| Some((passkey,).to_variant())),
        }),
        "DisplayPasskey" => {
            if let Some((_, passkey, _)) = parameters.get::<(glib::variant::ObjectPath, u32, u16)>()
            {
                requests(AgentRequest::DisplayPasskey {
                    device: device(),
                    passkey,
                });
            }
            invocation.return_value(None);
        }
        "RequestConfirmation" => {
            let Some((_, passkey)) = parameters.get::<(glib::variant::ObjectPath, u32)>() else {
                invocation.return_dbus_error(REJECTED, "No passkey");
                return;
            };
            requests(AgentRequest::Confirm {
                device: device(),
                passkey,
                reply: answer(invocation, |()| None),
            });
        }
        "RequestAuthorization" | "AuthorizeService" => requests(AgentRequest::Authorize {
            device: device(),
            reply: answer(invocation, |()| None),
        }),
        _ => invocation.return_dbus_error(
            "org.freedesktop.DBus.Error.UnknownMethod",
            &format!("{AGENT} has no method {method}"),
        ),
    }
}

/// What answers `invocation`: the value `output` makes of the section's answer, or
/// [`REJECTED`] when the section refuses.
fn answer<T: 'static>(
    invocation: gio::DBusMethodInvocation,
    output: impl FnOnce(T) -> Option<glib::Variant> + 'static,
) -> Reply<T> {
    Box::new(move |value| match value {
        Some(value) => invocation.return_value(output(value).as_ref()),
        None => invocation.return_dbus_error(REJECTED, "Refused"),
    })
}

/// The message of a bus error without the remote error's name in front of it.
fn bus_error_message(mut error: glib::Error) -> String {
    gio::DBusError::strip_remote_error(&mut error);
    error.message().to_owned()
}
