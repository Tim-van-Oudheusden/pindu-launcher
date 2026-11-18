//! The logind backend of the Power section (`pindu_core::system::power`), over
//! `org.freedesktop.login1` on the system bus with `gio`: `CanSuspend`, `CanReboot` and
//! `CanPowerOff` say what the session may do, `Suspend`, `Reboot` and `PowerOff` do it,
//! `ListInhibitors` names who stops it, and `NameOwnerChanged` says when logind comes or
//! goes. The Flatpak has no system bus, so there this backend touches nothing; the section
//! asks the host's logind through `host_logind_backend` instead.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::gio::prelude::*;
use gtk::{gio, glib};
use pindu_core::system::Permission;
use pindu_core::system::power::{Done, Facts, Inhibitor, PowerAction, PowerBackend};

use crate::permission_service;

/// logind's bus name, object and manager interface.
const LOGIND: &str = "org.freedesktop.login1";
const LOGIND_PATH: &str = "/org/freedesktop/login1";
const LOGIND_MANAGER: &str = "org.freedesktop.login1.Manager";

/// The bus itself, which says who owns a name and what it can activate.
const DBUS: &str = "org.freedesktop.DBus";
const DBUS_PATH: &str = "/org/freedesktop/DBus";

/// An inhibitor as `ListInhibitors` lists it: what, who, why, mode, user and process.
type ListedInhibitor = (String, String, String, String, u32, u32);

/// logind on the system bus.
#[derive(Default)]
pub struct LogindBackend {
    /// Follows logind's owner once [`PowerBackend::watch`] has connected.
    subscription: Rc<RefCell<Option<gio::SignalSubscription>>>,
}

impl LogindBackend {
    /// A backend that follows nothing yet.
    pub fn new() -> Self {
        Self::default()
    }
}

impl PowerBackend for LogindBackend {
    fn read(&self, done: Box<dyn FnOnce(Facts)>) {
        if permission_service::in_flatpak() {
            done(Facts::default());
            return;
        }
        glib::spawn_future_local(async move {
            done(read_facts().await);
        });
    }

    fn watch(&self, changed: Box<dyn Fn()>) {
        let subscription = self.subscription.clone();
        glib::spawn_future_local(async move {
            let bus = match gio::bus_get_future(gio::BusType::System).await {
                Ok(bus) => bus,
                Err(error) => {
                    glib::g_debug!("pindu", "Cannot follow logind: {error}");
                    return;
                }
            };
            subscription.replace(Some(bus.subscribe_to_signal(
                Some(DBUS),
                Some(DBUS),
                Some("NameOwnerChanged"),
                Some(DBUS_PATH),
                Some(LOGIND),
                gio::DBusSignalFlags::NONE,
                move |_| changed(),
            )));
        });
    }

    fn run(&self, action: PowerAction, done: Done) {
        let method = match action {
            PowerAction::Suspend => "Suspend",
            PowerAction::Restart => "Reboot",
            PowerAction::PowerOff => "PowerOff",
        };
        glib::spawn_future_local(async move {
            done(call_manager(method).await.map_err(bus_error_message));
        });
    }

    fn inhibitors(&self, done: Box<dyn FnOnce(Vec<Inhibitor>)>) {
        glib::spawn_future_local(async move {
            done(list_inhibitors().await);
        });
    }
}

/// What logind lets the session do. No system bus, or no logind, is reported as such; a
/// `Can*` call that fails counts as `na`, which hides its entry.
async fn read_facts() -> Facts {
    let bus = match gio::bus_get_future(gio::BusType::System).await {
        Ok(bus) => bus,
        Err(error) => {
            glib::g_debug!("pindu", "No system bus: {error}");
            return Facts::default();
        }
    };
    if !has_owner(&bus).await && !activatable(&bus).await {
        return Facts {
            system_bus: true,
            ..Facts::default()
        };
    }
    Facts {
        system_bus: true,
        logind: true,
        can_suspend: can(&bus, "CanSuspend").await,
        can_reboot: can(&bus, "CanReboot").await,
        can_power_off: can(&bus, "CanPowerOff").await,
        sandboxed: false,
    }
}

/// Whether logind has an owner on the bus.
async fn has_owner(bus: &gio::DBusConnection) -> bool {
    let reply = bus
        .call_future(
            Some(DBUS),
            DBUS_PATH,
            DBUS,
            "NameHasOwner",
            Some(&(LOGIND,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    match reply {
        Ok(reply) => reply.get::<(bool,)>().is_some_and(|(owned,)| owned),
        Err(error) => {
            glib::g_debug!("pindu", "NameHasOwner: {error}");
            false
        }
    }
}

/// Whether the bus can start logind when it is called.
async fn activatable(bus: &gio::DBusConnection) -> bool {
    let reply = bus
        .call_future(
            Some(DBUS),
            DBUS_PATH,
            DBUS,
            "ListActivatableNames",
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    match reply {
        Ok(reply) => reply
            .get::<(Vec<String>,)>()
            .is_some_and(|(names,)| names.iter().any(|name| name == LOGIND)),
        Err(error) => {
            glib::g_debug!("pindu", "ListActivatableNames: {error}");
            false
        }
    }
}

/// What logind's `method`, one of `CanSuspend`, `CanReboot` and `CanPowerOff`, answers.
async fn can(bus: &gio::DBusConnection, method: &str) -> Permission {
    let reply = bus
        .call_future(
            Some(LOGIND),
            LOGIND_PATH,
            LOGIND_MANAGER,
            method,
            None,
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await;
    match reply {
        Ok(reply) => reply
            .get::<(String,)>()
            .map_or(Permission::Na, |(answer,)| Permission::parse(&answer)),
        Err(error) => {
            glib::g_debug!("pindu", "logind {method}: {error}");
            Permission::Na
        }
    }
}

/// Calls logind's `method` with `interactive` on, letting polkit ask for a password where
/// the session has an agent for it.
async fn call_manager(method: &str) -> Result<(), glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::System).await?;
    bus.call_future(
        Some(LOGIND),
        LOGIND_PATH,
        LOGIND_MANAGER,
        method,
        Some(&(true,).to_variant()),
        None,
        gio::DBusCallFlags::NONE,
        -1,
    )
    .await?;
    Ok(())
}

/// logind's inhibitors; none when it cannot be asked.
async fn list_inhibitors() -> Vec<Inhibitor> {
    let listed = match ask_inhibitors().await {
        Ok(reply) => reply.get::<(Vec<ListedInhibitor>,)>(),
        Err(error) => {
            glib::g_debug!("pindu", "logind ListInhibitors: {error}");
            None
        }
    };
    let Some((listed,)) = listed else {
        return Vec::new();
    };
    listed
        .into_iter()
        .map(|(what, who, why, mode, _, _)| Inhibitor {
            what,
            who,
            why,
            mode,
        })
        .collect()
}

/// Calls logind's `ListInhibitors`.
async fn ask_inhibitors() -> Result<glib::Variant, glib::Error> {
    let bus = gio::bus_get_future(gio::BusType::System).await?;
    bus.call_future(
        Some(LOGIND),
        LOGIND_PATH,
        LOGIND_MANAGER,
        "ListInhibitors",
        None,
        None,
        gio::DBusCallFlags::NONE,
        -1,
    )
    .await
}

/// The message of a bus error without the remote error's name in front of it.
fn bus_error_message(mut error: glib::Error) -> String {
    gio::DBusError::strip_remote_error(&mut error);
    error.message().to_owned()
}
