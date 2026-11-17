//! Quick Access's Network, where NetworkManager runs (`pindu_core::system::network`): its
//! entry, subtitled with the connection in use, and its level with a row per wired and
//! Wi-Fi device and its state, Wi-Fi's switch, and the Wi-Fi networks in range, which a
//! scan looks for as the level opens. A new network is joined by selecting it, after its
//! passphrase where it needs one, typed in the passphrase dialog with the on-screen
//! keyboard or a keyboard; a saved one opens to Connect and Forget (asked first).
//! The level is [`PinduNetworkLevel`], with its own Blueprint
//! (`data/ui/fullscreen_network.blp`); the window keeps the entry, which sits in Quick
//! Access's shared list, and the level's `win.network-join` and `win.network-forget`.
//! Both show [`NetworkService`]'s state and call it; neither talks to NetworkManager
//! itself.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::osk::Keyboard;
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::network::{
    Availability, Busy, DeviceKind, DeviceRow, DeviceStatus, NetworkState, Problem, Prompt, Radio,
    Security, Signal, Summary, WifiNetwork,
};

use crate::application::PinduApplication;
use crate::network_service::NetworkService;
use crate::secret_dialog::SecretDialog;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_network.ui")]
    pub struct PinduNetworkLevel {
        #[template_child]
        pub quick_access_network_devices: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_wifi_radio: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_wifi_switch: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub quick_access_wifi_blocked: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub quick_access_wifi_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_wifi_networks: TemplateChild<gtk::ListBox>,
        /// The dialog asking for a Wi-Fi passphrase, with the prompt it shows.
        pub network_prompt: RefCell<Option<(SecretDialog, Prompt)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduNetworkLevel {
        const NAME: &'static str = "PinduNetworkLevel";
        type Type = super::PinduNetworkLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduNetworkLevel {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PinduNetworkLevel {}

    impl BinImpl for PinduNetworkLevel {}
}

glib::wrapper! {
    /// Quick Access's Network level: the wired and Wi-Fi devices, Wi-Fi's switch and the
    /// Wi-Fi networks in range. It is the `"network"` page of the window's Quick Access
    /// stack.
    pub struct PinduNetworkLevel(ObjectSubclass<imp::PinduNetworkLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

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
                    let level = &window.imp().quick_access_network_level;
                    level.ask_forget(&ssid);
                }
            })
            .build();
        self.add_action_entries([join, forget]);
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
        self.imp().quick_access_network_level.focus_target()
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
        imp.quick_access_network_level
            .show_prompt(state.prompt.as_ref());
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::Network)) {
            return;
        }
        if state.availability != Availability::Shown {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::Main));
            return;
        }
        self.fill_network(state);
    }

    /// Fills the level for `state`, and puts focus back in the panel when the row it was on
    /// is gone.
    fn fill_network(&self, state: &NetworkState) {
        if self.imp().quick_access_network_level.fill(state) {
            self.focus_panel(None);
        }
    }
}

impl PinduNetworkLevel {
    /// Names the rows controller focus finds again after a refill, gives the networks
    /// their placeholder, and turns Wi-Fi on or off as the user flips its switch.
    fn setup(&self) {
        let imp = self.imp();
        // Named as the network rows are, so focus finds them again after a refill.
        imp.quick_access_wifi_switch.set_widget_name("radio:switch");
        imp.quick_access_wifi_blocked
            .set_widget_name("radio:blocked");
        imp.quick_access_wifi_switch
            .connect_active_notify(glib::clone!(
                #[weak(rename_to = level)]
                self,
                move |switch| {
                    // Showing the state sets the switch too; only the user's flip asks.
                    let Some(service) = level.network_service() else {
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
    }

    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 3] {
        let imp = self.imp();
        [
            imp.quick_access_network_devices.get(),
            imp.quick_access_wifi_radio.get(),
            imp.quick_access_wifi_networks.get(),
        ]
    }

    /// Where focus goes on the level: the first device, or else Wi-Fi's switch.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
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

    /// The service, through the application of the window the level is in.
    fn network_service(&self) -> Option<NetworkService> {
        self.root()
            .and_downcast::<gtk::Window>()
            .and_then(|window| window.application())
            .and_downcast::<PinduApplication>()
            .map(|application| application.network_service().clone())
    }

    /// The level's rows for `state`, keeping focus on the row it was on and saved networks
    /// open as they were. Returns whether focus was on a row that is gone, with no row to
    /// take its place, so the window must put it somewhere.
    pub(crate) fn fill(&self, state: &NetworkState) -> bool {
        let imp = self.imp();
        let focused = self.focused_row();

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
                None => return true,
            }
        }
        false
    }

    /// The name of the level's row that has focus, which filling the level again takes
    /// away; `None` when focus is elsewhere.
    fn focused_row(&self) -> Option<String> {
        let focus = gtk::prelude::RootExt::focus(&self.root()?)?;
        if !self.lists().iter().any(|list| focus.is_ancestor(list)) {
            return None;
        }
        // The rows the level names; an expander row's header inside it has no name.
        std::iter::successors(Some(focus), |widget| widget.parent())
            .map(|widget| widget.widget_name().to_string())
            .find(|name| name.contains(':'))
    }

    /// Asks for the passphrase the section wants in the passphrase dialog, or withdraws the
    /// dialog once the section no longer wants one.
    pub(crate) fn show_prompt(&self, prompt: Option<&Prompt>) {
        let imp = self.imp();
        let shown = imp.network_prompt.borrow().clone();
        if shown.as_ref().map(|(_, shown)| shown) == prompt {
            return;
        }
        if let Some((dialog, _)) = shown {
            imp.network_prompt.replace(None);
            dialog.withdraw();
        }
        let (Some(prompt), Some(window)) = (prompt, self.root().and_downcast::<PinduWindow>())
        else {
            return;
        };
        let dialog = window.ask_secret(
            // Translators: {network} is a Wi-Fi network's name.
            &gettext("Join {network}").replace("{network}", &prompt.name),
            &passphrase_message(prompt),
            Keyboard::passphrase(),
            glib::clone!(
                #[weak(rename_to = level)]
                self,
                move |passphrase: Option<String>| {
                    // The dialog closes itself after its answer; the answer changing the
                    // prompt must not withdraw it.
                    level.imp().network_prompt.replace(None);
                    let Some(service) = level.network_service() else {
                        return;
                    };
                    service.enter(passphrase.as_deref());
                    // A passphrase that cannot fit again leaves the prompt as it was, which
                    // tells no listener; ask again here.
                    level.show_prompt(service.state().prompt.as_ref());
                }
            ),
        );
        imp.network_prompt.replace(Some((dialog, prompt.clone())));
    }

    /// Asks before forgetting a saved network: joining it again takes its passphrase.
    pub(crate) fn ask_forget(&self, ssid: &[u8]) {
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
        dialog.present(self.root().as_ref());
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

/// What the passphrase dialog says above its entry: why it asks again, if it does.
fn passphrase_message(prompt: &Prompt) -> String {
    match (prompt.problem, prompt.security) {
        (Some(Problem::Wrong), _) => gettext("Wrong passphrase. Try again."),
        (Some(Problem::Length), Security::WpaPersonal) => {
            gettext("A passphrase has 8 to 63 characters.")
        }
        _ => gettext("Enter the network's passphrase."),
    }
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
