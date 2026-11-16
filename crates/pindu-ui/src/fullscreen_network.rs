//! Quick Access's Network, where NetworkManager runs (`pindu_core::system::network`): its
//! entry, subtitled with the connection in use, and its level with a row per wired and
//! Wi-Fi device and its state, Wi-Fi's switch, and the Wi-Fi networks in range, which a
//! scan looks for as the level opens. A new network is joined by selecting it, after its
//! passphrase where it needs one; a saved one opens to Connect and Forget (asked first).
//! The window shows [`NetworkService`]'s state and calls it; it never talks to
//! NetworkManager itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::network::{
    Availability, Busy, DeviceKind, DeviceRow, DeviceStatus, NetworkState, Problem, Prompt, Radio,
    Security, Signal, Summary, WifiNetwork,
};

use crate::application::PinduApplication;
use crate::network_service::NetworkService;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds the level's actions, and shows the entry as the network is and as it changes.
    pub(crate) fn setup_network(&self) {
        let join = gio::ActionEntry::builder("network-join")
            .parameter_type(Some(glib::VariantTy::BYTE_STRING))
            .activate(|window: &Self, _, target| {
                if let (Some(service), Some(ssid)) = (
                    window.network_service(),
                    target.and_then(|target| target.get::<Vec<u8>>()),
                ) {
                    service.join(&ssid);
                }
            })
            .build();
        let forget = gio::ActionEntry::builder("network-forget")
            .parameter_type(Some(glib::VariantTy::BYTE_STRING))
            .activate(|window: &Self, _, target| {
                if let Some(ssid) = target.and_then(|target| target.get::<Vec<u8>>()) {
                    window.ask_forget_network(&ssid);
                }
            })
            .build();
        self.add_action_entries([join, forget]);
        let imp = self.imp();
        // Named as the network rows are, so focus finds them again after a refill.
        imp.quick_access_wifi_switch.set_widget_name("radio:switch");
        imp.quick_access_wifi_blocked
            .set_widget_name("radio:blocked");
        imp.quick_access_wifi_switch
            .connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |switch| {
                    // Showing the state sets the switch too; only the user's flip asks.
                    let Some(service) = window.network_service() else {
                        return;
                    };
                    if switch.is_active() != (service.state().radio == Radio::On) {
                        service.set_wireless(switch.is_active());
                    }
                }
            ));
        let placeholder = gtk::Label::builder()
            .label(gettext("Looking for networks…"))
            .wrap(true)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["dim-label"])
            .build();
        imp.quick_access_wifi_networks
            .set_placeholder(Some(&placeholder));
        let Some(service) = self.network_service() else {
            return;
        };
        service.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |state| window.show_network_state(state)
        ));
        self.show_network_state(&service.state());
    }

    /// The level opens: it shows the network as last reported, and Wi-Fi looks for
    /// networks.
    pub(crate) fn show_network(&self) {
        let Some(service) = self.network_service() else {
            return;
        };
        self.fill_network(&service.state());
        service.open();
    }

    /// Where focus goes on the level: the first device, or else Wi-Fi's switch.
    pub(crate) fn network_focus(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        imp.quick_access_network_devices
            .row_at_index(0)
            .map(|row| row.upcast())
            .or_else(|| {
                let switch = &imp.quick_access_wifi_switch;
                let row: gtk::Widget = if switch.is_visible() {
                    switch.get().upcast()
                } else {
                    imp.quick_access_wifi_blocked.get().upcast()
                };
                imp.quick_access_wifi_radio.is_visible().then_some(row)
            })
    }

    fn network_service(&self) -> Option<NetworkService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.network_service().clone())
    }

    /// Shows or hides the entry, with its icon and subtitle, shows or takes away the
    /// passphrase prompt, and fills the level again while it is open; it leaves for the
    /// entries once the section is gone or not allowed.
    fn show_network_state(&self, state: &NetworkState) {
        let imp = self.imp();
        let row = &imp.quick_access_network_row;
        row.set_visible(state.availability != Availability::Hidden);
        row.set_sensitive(state.availability == Availability::Shown);
        if let Some(summary) = &state.summary {
            imp.quick_access_network_icon
                .set_icon_name(Some(summary_icon(summary)));
            imp.quick_access_network_subtitle
                .set_label(&summary_text(summary));
        }
        self.show_network_prompt(state.prompt.as_ref());
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::Network)) {
            return;
        }
        if state.availability != Availability::Shown {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::Main));
            return;
        }
        self.fill_network(state);
    }

    /// The level's rows for `state`, keeping focus on the row it was on and saved networks
    /// open as they were.
    fn fill_network(&self, state: &NetworkState) {
        let imp = self.imp();
        let focused = self.network_focused_row();

        let devices = &imp.quick_access_network_devices;
        devices.remove_all();
        for device in &state.devices {
            let several = state
                .devices
                .iter()
                .filter(|other| other.kind == device.kind)
                .count()
                > 1;
            devices.append(&device_row(device, several));
        }
        devices.set_visible(!state.devices.is_empty());

        let blocked = state.radio == Radio::Blocked;
        let switch = &imp.quick_access_wifi_switch;
        imp.quick_access_wifi_radio
            .set_visible(state.radio != Radio::Missing);
        switch.set_visible(!blocked);
        imp.quick_access_wifi_blocked.set_visible(blocked);
        switch.set_active(state.radio == Radio::On);

        let networks = &imp.quick_access_wifi_networks;
        let expanded: Vec<glib::GString> = rows(networks)
            .filter_map(|row| row.downcast::<adw::ExpanderRow>().ok())
            .filter(|row| row.is_expanded())
            .map(|row| row.widget_name())
            .collect();
        networks.remove_all();
        for network in &state.networks {
            let row = network_row(network, state);
            if let Some(expander) = row.downcast_ref::<adw::ExpanderRow>() {
                expander.set_expanded(expanded.iter().any(|name| *name == row.widget_name()));
            }
            networks.append(&row);
        }
        let on = state.radio == Radio::On;
        networks.set_visible(on);
        imp.quick_access_wifi_title.set_visible(on);

        if let Some(name) = focused {
            let lists = [
                devices.get(),
                imp.quick_access_wifi_radio.get(),
                networks.get(),
            ];
            let target = focus_candidates(&name).into_iter().find_map(|name| {
                lists
                    .iter()
                    .find_map(|list| find_named(list.upcast_ref(), &name))
                    .filter(|target| target.is_visible() && target.is_sensitive())
            });
            match target {
                Some(target) => {
                    target.grab_focus();
                }
                None => self.focus_panel(None),
            }
        }
    }

    /// The name of the level's row that has focus, which filling the level again takes
    /// away; `None` when focus is elsewhere.
    fn network_focused_row(&self) -> Option<String> {
        let imp = self.imp();
        let focus = gtk::prelude::GtkWindowExt::focus(self)?;
        let lists = [
            imp.quick_access_network_devices.get(),
            imp.quick_access_wifi_radio.get(),
            imp.quick_access_wifi_networks.get(),
        ];
        if !lists.iter().any(|list| focus.is_ancestor(list)) {
            return None;
        }
        // The rows the level names; an expander row's header inside it has no name.
        std::iter::successors(Some(focus), |widget| widget.parent())
            .map(|widget| widget.widget_name().to_string())
            .find(|name| name.contains(':'))
    }

    /// Asks for the passphrase the section wants in a dialog, or closes the dialog once it
    /// no longer wants one.
    fn show_network_prompt(&self, prompt: Option<&Prompt>) {
        let imp = self.imp();
        let shown = imp.network_prompt.borrow().clone();
        if shown.as_ref().map(|(_, shown)| shown) == prompt {
            return;
        }
        if let Some((dialog, _)) = shown {
            imp.network_prompt.replace(None);
            dialog.force_close();
        }
        let Some(prompt) = prompt else {
            return;
        };
        let (dialog, entry) = passphrase_dialog(prompt);
        let answered = prompt.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    let Some(service) = window.network_service() else {
                        return;
                    };
                    // A dialog closing for a prompt gone or replaced answers nothing.
                    if service.state().prompt.as_ref() != Some(&answered) {
                        return;
                    }
                    // The dialog closes itself after its response; the answer changing the
                    // prompt must not close it again.
                    window.imp().network_prompt.replace(None);
                    if response == "join" {
                        service.enter(Some(&entry.text()));
                    } else {
                        service.enter(None);
                    }
                    // A passphrase that cannot fit again leaves the prompt as it was, which
                    // tells no listener; ask again here.
                    window.show_network_prompt(service.state().prompt.as_ref());
                }
            ),
        );
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |dialog| {
                let imp = window.imp();
                let ours = imp
                    .network_prompt
                    .borrow()
                    .as_ref()
                    .is_some_and(|(shown, _)| shown == dialog);
                if ours {
                    imp.network_prompt.replace(None);
                }
            }
        ));
        imp.network_prompt
            .replace(Some((dialog.clone(), prompt.clone())));
        dialog.present(Some(self));
    }

    /// Asks before forgetting a saved network: joining it again takes its passphrase.
    fn ask_forget_network(&self, ssid: &[u8]) {
        let Some(service) = self.network_service() else {
            return;
        };
        let state = service.state();
        let Some(name) = state
            .networks
            .iter()
            .find(|network| network.ssid == ssid && network.saved)
            .map(|network| network.name.clone())
        else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            // Translators: {network} is a Wi-Fi network's name.
            .heading(gettext("Forget {network}?").replace("{network}", &name))
            .body(gettext(
                "Its passphrase is removed, and the console no longer joins it by itself.",
            ))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("forget", &gettext("_Forget")),
        ]);
        dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
        let ssid = ssid.to_vec();
        dialog.connect_response(Some("forget"), move |_, _| service.forget(&ssid));
        dialog.present(Some(self));
    }
}

/// The entry's icon for `summary`.
fn summary_icon(summary: &Summary) -> &'static str {
    match summary {
        Summary::Wired | Summary::Other(_) => "network-wired-symbolic",
        Summary::Wifi(_) => "network-wireless-symbolic",
        Summary::Offline | Summary::NotAllowed => "network-offline-symbolic",
    }
}

/// What the entry says under its title.
fn summary_text(summary: &Summary) -> String {
    match summary {
        Summary::Wired => gettext("Wired"),
        Summary::Wifi(name) | Summary::Other(name) => name.clone(),
        Summary::Offline => gettext("Offline"),
        Summary::NotAllowed => gettext("Not allowed on this system"),
    }
}

/// A device's row: what it is, and its state.
fn device_row(device: &DeviceRow, several: bool) -> adw::ActionRow {
    let kind = match device.kind {
        DeviceKind::Wired => gettext("Wired"),
        DeviceKind::Wifi => gettext("Wi-Fi"),
    };
    let title = if several {
        // Translators: {device} is "Wired" or "Wi-Fi", {interface} its name, such as wlan0.
        gettext("{device} ({interface})")
            .replace("{device}", &kind)
            .replace("{interface}", &device.interface)
    } else {
        kind
    };
    let status = match &device.status {
        DeviceStatus::Connected(Some(name)) => {
            // Translators: {network} is a Wi-Fi network's name.
            gettext("Connected to {network}").replace("{network}", name)
        }
        DeviceStatus::Connected(None) => gettext("Connected"),
        DeviceStatus::Connecting => gettext("Connecting…"),
        DeviceStatus::Disconnected => gettext("Disconnected"),
        DeviceStatus::Unplugged => gettext("Cable unplugged"),
        DeviceStatus::Off => gettext("Off"),
    };
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(status)
        .use_markup(false)
        .name(format!("device:{}", device.interface))
        .build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(match device.kind {
                DeviceKind::Wired => "network-wired-symbolic",
                DeviceKind::Wifi => "network-wireless-symbolic",
            })
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    row
}

/// What runs on `network`, if anything.
fn busy_on(network: &WifiNetwork, state: &NetworkState) -> Option<Busy> {
    state
        .pending
        .as_ref()
        .filter(|pending| pending.ssid == network.ssid)
        .map(|pending| pending.busy)
}

/// What a network's row says under its name.
fn network_status(network: &WifiNetwork, state: &NetworkState) -> String {
    match busy_on(network, state) {
        Some(Busy::Joining) => gettext("Connecting…"),
        Some(Busy::Forgetting) => gettext("Forgetting…"),
        None if network.in_use => gettext("Connected"),
        None if network.saved => gettext("Saved"),
        None if !network.joinable() => gettext("Set up on another device"),
        None => String::new(),
    }
}

/// A network's row: its signal, name and status, with a lock when it is secured and a
/// check when it is in use. A saved network opens to Connect and Forget; another one is
/// joined by selecting it, unless it is set up on another device.
fn network_row(network: &WifiNetwork, state: &NetworkState) -> gtk::Widget {
    let name = format!("network:{}", hex(&network.ssid));
    let target = network.ssid.to_variant();
    let row: adw::PreferencesRow = if network.saved {
        let row = adw::ExpanderRow::builder()
            .title(&network.name)
            .subtitle(network_status(network, state))
            .use_markup(false)
            .name(&name)
            .build();
        row.add_prefix(&signal_icon(network.signal()));
        add_suffixes(&row, network, state);
        if !network.in_use {
            row.add_row(
                &adw::ButtonRow::builder()
                    .title(gettext("Connect"))
                    .action_name("win.network-join")
                    .action_target(&target)
                    .name(format!("connect:{}", hex(&network.ssid)))
                    .build(),
            );
        }
        row.add_row(
            &adw::ButtonRow::builder()
                .title(gettext("Forget"))
                .action_name("win.network-forget")
                .action_target(&target)
                .name(format!("forget:{}", hex(&network.ssid)))
                .css_classes(["destructive-action"])
                .build(),
        );
        row.upcast()
    } else {
        let joinable = network.joinable() && !network.in_use;
        let row = adw::ActionRow::builder()
            .title(&network.name)
            .subtitle(network_status(network, state))
            .use_markup(false)
            .activatable(joinable)
            .sensitive(network.joinable())
            .name(&name)
            .build();
        if joinable {
            row.set_action_name(Some("win.network-join"));
            row.set_action_target_value(Some(&target));
        }
        row.add_prefix(&signal_icon(network.signal()));
        add_suffixes(&row, network, state);
        row.upcast()
    };
    row.upcast()
}

/// The lock, the check and the spinner a network's row ends with, as they apply.
fn add_suffixes(row: &impl IsA<adw::PreferencesRow>, network: &WifiNetwork, state: &NetworkState) {
    let add = |widget: &gtk::Widget| {
        if let Some(row) = row.dynamic_cast_ref::<adw::ExpanderRow>() {
            row.add_suffix(widget);
        } else if let Some(row) = row.dynamic_cast_ref::<adw::ActionRow>() {
            row.add_suffix(widget);
        }
    };
    let icon = |name: &str| -> gtk::Widget {
        gtk::Image::builder()
            .icon_name(name)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build()
            .upcast()
    };
    if busy_on(network, state).is_some() {
        add(adw::Spinner::new().upcast_ref());
    }
    if network.in_use {
        add(&icon("object-select-symbolic"));
    }
    if network.security != Security::Open {
        add(&icon("network-wireless-encrypted-symbolic"));
    }
}

/// The icon for a signal's strength.
fn signal_icon(signal: Signal) -> gtk::Image {
    let name = match signal {
        Signal::Excellent => "network-wireless-signal-excellent-symbolic",
        Signal::Good => "network-wireless-signal-good-symbolic",
        Signal::Ok => "network-wireless-signal-ok-symbolic",
        Signal::Weak => "network-wireless-signal-weak-symbolic",
        Signal::None => "network-wireless-signal-none-symbolic",
    };
    gtk::Image::builder()
        .icon_name(name)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
}

/// The dialog asking for a network's passphrase, with its entry.
fn passphrase_dialog(prompt: &Prompt) -> (adw::AlertDialog, gtk::PasswordEntry) {
    let body = match (prompt.problem, prompt.security) {
        (Some(Problem::Wrong), _) => gettext("Wrong passphrase. Try again."),
        (Some(Problem::Length), Security::WpaPersonal) => {
            gettext("A passphrase has 8 to 63 characters.")
        }
        _ => gettext("Enter the network's passphrase."),
    };
    let entry = gtk::PasswordEntry::builder()
        .show_peek_icon(true)
        .activates_default(true)
        .build();
    entry.update_property(&[gtk::accessible::Property::Label(&gettext("Passphrase"))]);
    let dialog = adw::AlertDialog::builder()
        // Translators: {network} is a Wi-Fi network's name.
        .heading(gettext("Join {network}").replace("{network}", &prompt.name))
        .body(body)
        .extra_child(&entry)
        .close_response("cancel")
        .default_response("join")
        .build();
    dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("join", &gettext("_Join"))]);
    dialog.set_response_appearance("join", adw::ResponseAppearance::Suggested);
    (dialog, entry)
}

/// `bytes` in hexadecimal, which names a network's rows whatever its SSID holds.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// The rows of `list`.
fn rows(list: &gtk::ListBox) -> impl Iterator<Item = gtk::Widget> {
    std::iter::successors(list.first_child(), |row| row.next_sibling())
}

/// The widget named `name` in `root`'s tree.
fn find_named(root: &gtk::Widget, name: &str) -> Option<gtk::Widget> {
    if root.widget_name() == name {
        return Some(root.clone());
    }
    std::iter::successors(root.first_child(), |child| child.next_sibling())
        .find_map(|child| find_named(&child, name))
}

/// Where focus goes back to after a refill, in order: the row it was on, the row that took
/// its place (the blocked note for the switch, and back), then its network.
fn focus_candidates(name: &str) -> Vec<String> {
    let Some((kind, rest)) = name.split_once(':') else {
        return vec![name.to_owned()];
    };
    let swapped = match (kind, rest) {
        ("radio", "switch") => Some("radio:blocked".to_owned()),
        ("radio", _) => Some("radio:switch".to_owned()),
        _ => None,
    };
    let network = matches!(kind, "connect" | "forget").then(|| format!("network:{rest}"));
    std::iter::once(name.to_owned())
        .chain(swapped)
        .chain(network)
        .collect()
}
