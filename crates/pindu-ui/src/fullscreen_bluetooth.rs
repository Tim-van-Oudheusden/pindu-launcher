//! Quick Access's Bluetooth, where BlueZ has an adapter (`pindu_core::system::bluetooth`):
//! its entry, subtitled with the connected devices or how the radio is, and its level with
//! the radio's switch, the paired devices (each opening to Connect or Disconnect and
//! Forget, asked first) and the devices found nearby, which a press pairs and connects.
//! While the level is open discovery and the pairing agent run, and what the agent asks
//! shows in a dialog: a code to confirm (A confirms, B refuses) or a code to type on the
//! device. The window shows [`BluetoothService`]'s state and calls it; it never talks to
//! BlueZ itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::bluetooth::{BluetoothState, Busy, Device, Power, Prompt, Summary};

use crate::application::PinduApplication;
use crate::bluetooth_service::BluetoothService;
use crate::window::PinduWindow;

/// The icon of a device whose own icon the theme lacks.
const FALLBACK_ICON: &str = "bluetooth-active-symbolic";

impl PinduWindow {
    /// Adds the level's actions, and shows the entry as the section is and as it changes.
    pub(crate) fn setup_bluetooth(&self) {
        let device_action = |name: &str, act: fn(&BluetoothService, &str)| {
            gio::ActionEntry::builder(name)
                .parameter_type(Some(glib::VariantTy::STRING))
                .activate(move |window: &Self, _, target| {
                    if let (Some(service), Some(device)) = (
                        window.bluetooth_service(),
                        target.and_then(|target| target.str()),
                    ) {
                        act(&service, device);
                    }
                })
                .build()
        };
        let forget = gio::ActionEntry::builder("bluetooth-forget")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, target| {
                if let Some(device) = target.and_then(|target| target.str()) {
                    window.ask_forget(device);
                }
            })
            .build();
        self.add_action_entries([
            device_action("bluetooth-pair", BluetoothService::pair),
            device_action("bluetooth-connect", BluetoothService::connect),
            device_action("bluetooth-disconnect", BluetoothService::disconnect),
            forget,
        ]);
        let imp = self.imp();
        // Named as the device rows are, so focus finds them again after a refill.
        imp.quick_access_bluetooth_switch
            .set_widget_name("adapter:switch");
        imp.quick_access_bluetooth_blocked
            .set_widget_name("adapter:blocked");
        imp.quick_access_bluetooth_switch
            .connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |switch| {
                    // Showing the state sets the switch too; only the user's flip asks.
                    let Some(service) = window.bluetooth_service() else {
                        return;
                    };
                    if switch.is_active() != switch_on(service.state().power) {
                        service.set_powered(switch.is_active());
                    }
                }
            ));
        let placeholder = gtk::Label::builder()
            .label(gettext("Looking for devices…"))
            .wrap(true)
            .margin_top(12)
            .margin_bottom(12)
            .margin_start(12)
            .margin_end(12)
            .css_classes(["dim-label"])
            .build();
        imp.quick_access_bluetooth_found
            .set_placeholder(Some(&placeholder));
        let Some(service) = self.bluetooth_service() else {
            return;
        };
        service.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |state| window.show_bluetooth_state(state)
        ));
        self.show_bluetooth_state(&service.state());
    }

    /// The level opens: it shows the state as last reported, and discovery and the pairing
    /// agent start.
    pub(crate) fn show_bluetooth(&self) {
        let Some(service) = self.bluetooth_service() else {
            return;
        };
        self.fill_bluetooth(&service.state());
        service.open();
    }

    /// The level closes: discovery and the pairing agent stop.
    pub(crate) fn leave_bluetooth(&self) {
        if let Some(service) = self.bluetooth_service() {
            service.close();
        }
    }

    /// Where focus goes on the level: the switch, or what blocks the radio.
    pub(crate) fn bluetooth_focus(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        let switch = &imp.quick_access_bluetooth_switch;
        Some(if switch.is_visible() {
            switch.get().upcast()
        } else {
            imp.quick_access_bluetooth_blocked.get().upcast()
        })
    }

    fn bluetooth_service(&self) -> Option<BluetoothService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.bluetooth_service().clone())
    }

    /// Shows or hides the entry, with its subtitle, fills the level again while it is
    /// open, and shows or takes away the agent's prompt; it leaves for the entries once
    /// the section is gone.
    fn show_bluetooth_state(&self, state: &BluetoothState) {
        let imp = self.imp();
        imp.quick_access_bluetooth_row.set_visible(state.available);
        imp.quick_access_bluetooth_subtitle
            .set_label(&summary_text(&state.summary()));
        self.show_bluetooth_prompt(state.prompt.as_ref());
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::Bluetooth)) {
            return;
        }
        if !state.available {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::Main));
            return;
        }
        self.fill_bluetooth(state);
    }

    /// The level's rows for `state`, keeping focus on the row it was on and paired devices
    /// open as they were.
    fn fill_bluetooth(&self, state: &BluetoothState) {
        let imp = self.imp();
        let focused = self.bluetooth_focused_row();
        let paired = &imp.quick_access_bluetooth_paired;
        let expanded: Vec<glib::GString> = rows(paired)
            .filter_map(|row| row.downcast::<adw::ExpanderRow>().ok())
            .filter(|row| row.is_expanded())
            .map(|row| row.widget_name())
            .collect();

        let blocked = state.power == Power::Blocked;
        let switch = &imp.quick_access_bluetooth_switch;
        switch.set_visible(!blocked);
        imp.quick_access_bluetooth_blocked.set_visible(blocked);
        switch.set_active(switch_on(state.power));
        switch.set_sensitive(matches!(state.power, Power::On | Power::Off));

        paired.remove_all();
        for device in &state.paired {
            let row = paired_row(device, state);
            row.set_expanded(expanded.iter().any(|name| *name == row.widget_name()));
            paired.append(&row);
        }
        paired.set_visible(!state.paired.is_empty());
        imp.quick_access_bluetooth_paired_title
            .set_visible(!state.paired.is_empty());

        let found = &imp.quick_access_bluetooth_found;
        found.remove_all();
        for device in &state.found {
            found.append(&found_row(device, state));
        }
        let on = state.power == Power::On;
        found.set_visible(on);
        imp.quick_access_bluetooth_found_title.set_visible(on);

        if let Some(name) = focused {
            let lists = [
                imp.quick_access_bluetooth_adapter.get(),
                paired.get(),
                found.get(),
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
    fn bluetooth_focused_row(&self) -> Option<String> {
        let imp = self.imp();
        let focus = gtk::prelude::GtkWindowExt::focus(self)?;
        let lists = [
            imp.quick_access_bluetooth_adapter.get(),
            imp.quick_access_bluetooth_paired.get(),
            imp.quick_access_bluetooth_found.get(),
        ];
        if !lists.iter().any(|list| focus.is_ancestor(list)) {
            return None;
        }
        // The rows the level names; an expander row's header inside it has no name.
        std::iter::successors(Some(focus), |widget| widget.parent())
            .map(|widget| widget.widget_name().to_string())
            .find(|name| name.contains(':'))
    }

    /// Shows what the pairing agent asks in a dialog, or closes the dialog once it no
    /// longer asks.
    fn show_bluetooth_prompt(&self, prompt: Option<&Prompt>) {
        let imp = self.imp();
        let shown = imp.bluetooth_prompt.borrow().clone();
        if shown.as_ref().map(|(_, shown)| shown) == prompt {
            return;
        }
        if let Some((dialog, _)) = shown {
            imp.bluetooth_prompt.replace(None);
            dialog.force_close();
        }
        let Some(prompt) = prompt else {
            return;
        };
        let dialog = prompt_dialog(prompt);
        let answered = prompt.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, response| {
                    let Some(service) = window.bluetooth_service() else {
                        return;
                    };
                    // A dialog closing for a prompt gone or replaced answers nothing.
                    if service.state().prompt.as_ref() != Some(&answered) {
                        return;
                    }
                    // The dialog closes itself after its response; the answer taking the
                    // prompt away must not close it again.
                    window.imp().bluetooth_prompt.replace(None);
                    match answered {
                        Prompt::Confirm { .. } => service.confirm(response == "pair"),
                        Prompt::Show { .. } => service.dismiss(),
                        Prompt::EnterPin { .. } | Prompt::EnterPasskey { .. } => {
                            service.enter(None);
                        }
                    }
                }
            ),
        );
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |dialog| {
                let imp = window.imp();
                let ours = imp
                    .bluetooth_prompt
                    .borrow()
                    .as_ref()
                    .is_some_and(|(shown, _)| shown == dialog);
                if ours {
                    imp.bluetooth_prompt.replace(None);
                }
            }
        ));
        imp.bluetooth_prompt
            .replace(Some((dialog.clone(), prompt.clone())));
        dialog.present(Some(self));
    }

    /// Asks before forgetting a paired device: pairing it again takes a controller, and it
    /// may be the one in use.
    fn ask_forget(&self, device: &str) {
        let Some(service) = self.bluetooth_service() else {
            return;
        };
        let state = service.state();
        let Some(name) = state
            .paired
            .iter()
            .find(|paired| paired.path == device)
            .map(|paired| paired.display_name().to_owned())
        else {
            return;
        };
        let dialog = adw::AlertDialog::builder()
            // Translators: {device} is a Bluetooth device's name, such as a controller's.
            .heading(gettext("Forget {device}?").replace("{device}", &name))
            .body(gettext(
                "It disconnects, and has to be paired again before it can connect. If it is the controller in use, use another one to pair it.",
            ))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("forget", &gettext("_Forget")),
        ]);
        dialog.set_response_appearance("forget", adw::ResponseAppearance::Destructive);
        let device = device.to_owned();
        dialog.connect_response(Some("forget"), move |_, _| service.forget(&device));
        dialog.present(Some(self));
    }
}

/// Whether the switch shows the radio as on: on, or turning on.
fn switch_on(power: Power) -> bool {
    matches!(power, Power::On | Power::TurningOn)
}

/// What the entry says under its title.
fn summary_text(summary: &Summary) -> String {
    match summary {
        Summary::Connected(names) => names.join(", "),
        Summary::NotConnected => gettext("No devices connected"),
        Summary::Off => gettext("Off"),
        Summary::Blocked => gettext("Blocked"),
    }
}

/// What runs on `device`, if anything.
fn busy_on(device: &Device, state: &BluetoothState) -> Option<Busy> {
    state
        .pending
        .as_ref()
        .filter(|pending| pending.device == device.path)
        .map(|pending| pending.busy)
}

/// What a device's row says under its name.
fn status_text(device: &Device, state: &BluetoothState) -> String {
    match busy_on(device, state) {
        Some(Busy::Pairing) => gettext("Pairing…"),
        Some(Busy::Connecting) => gettext("Connecting…"),
        Some(Busy::Disconnecting) => gettext("Disconnecting…"),
        Some(Busy::Forgetting) => gettext("Forgetting…"),
        None if device.connected => gettext("Connected"),
        None if device.paired => gettext("Paired"),
        None => String::new(),
    }
}

/// The device's own icon, symbolic, where the theme has it.
fn device_icon(device: &Device) -> gtk::Image {
    let name = device
        .icon
        .as_deref()
        .map(|icon| format!("{icon}-symbolic"))
        .filter(|icon| {
            gtk::gdk::Display::default()
                .is_some_and(|display| gtk::IconTheme::for_display(&display).has_icon(icon))
        });
    gtk::Image::builder()
        .icon_name(name.as_deref().unwrap_or(FALLBACK_ICON))
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
}

/// A paired device: its name and status, opening to Connect or Disconnect and Forget.
fn paired_row(device: &Device, state: &BluetoothState) -> adw::ExpanderRow {
    let row = adw::ExpanderRow::builder()
        .title(device.display_name())
        .subtitle(status_text(device, state))
        .use_markup(false)
        .name(format!("device:{}", device.path))
        .build();
    row.add_prefix(&device_icon(device));
    if busy_on(device, state).is_some() {
        row.add_suffix(&adw::Spinner::new());
    }
    let target = device.path.to_variant();
    let (title, action, name) = if device.connected {
        (
            gettext("Disconnect"),
            "win.bluetooth-disconnect",
            format!("disconnect:{}", device.path),
        )
    } else {
        (
            gettext("Connect"),
            "win.bluetooth-connect",
            format!("connect:{}", device.path),
        )
    };
    let toggle = adw::ButtonRow::builder()
        .title(title)
        .action_name(action)
        .action_target(&target)
        .name(name)
        .build();
    let forget = adw::ButtonRow::builder()
        .title(gettext("Forget"))
        .action_name("win.bluetooth-forget")
        .action_target(&target)
        .name(format!("forget:{}", device.path))
        .css_classes(["destructive-action"])
        .build();
    row.add_row(&toggle);
    row.add_row(&forget);
    row
}

/// A device found nearby: selecting it pairs and connects it.
fn found_row(device: &Device, state: &BluetoothState) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .title(device.display_name())
        .subtitle(status_text(device, state))
        .use_markup(false)
        .activatable(true)
        .action_name("win.bluetooth-pair")
        .action_target(&device.path.to_variant())
        .name(format!("device:{}", device.path))
        .build();
    row.add_prefix(&device_icon(device));
    if busy_on(device, state).is_some() {
        row.add_suffix(&adw::Spinner::new());
    }
    row
}

/// The dialog for what the pairing agent asks.
fn prompt_dialog(prompt: &Prompt) -> adw::AlertDialog {
    let code_label = |code: &str| {
        gtk::Label::builder()
            .label(code)
            .selectable(false)
            .css_classes(["title-1", "numeric"])
            .build()
    };
    match prompt {
        Prompt::Confirm { device, passkey } => {
            let dialog = adw::AlertDialog::builder()
                // Translators: {device} is a Bluetooth device's name, such as a controller's.
                .heading(gettext("Pair With {device}?").replace("{device}", device))
                .body(gettext("Pair if the device shows this code too."))
                .extra_child(&code_label(passkey))
                .close_response("refuse")
                .default_response("pair")
                .build();
            dialog.add_responses(&[("refuse", &gettext("_Refuse")), ("pair", &gettext("_Pair"))]);
            dialog.set_response_appearance("pair", adw::ResponseAppearance::Suggested);
            dialog
        }
        Prompt::Show { device, code } => {
            let dialog = adw::AlertDialog::builder()
                // Translators: {device} is a Bluetooth device's name, such as a keyboard's.
                .heading(gettext("Type This Code on {device}").replace("{device}", device))
                .body(gettext("Then press Enter on it."))
                .extra_child(&code_label(code))
                .close_response("cancel")
                .default_response("cancel")
                .build();
            dialog.add_responses(&[("cancel", &gettext("_Cancel"))]);
            dialog
        }
        Prompt::EnterPin { device } | Prompt::EnterPasskey { device } => {
            let dialog = adw::AlertDialog::builder()
                // Translators: {device} is a Bluetooth device's name.
                .heading(gettext("{device} Asks for a Code").replace("{device}", device))
                .body(gettext(
                    "Typing a code with the controller is not possible yet, so this device cannot pair here.",
                ))
                .close_response("close")
                .default_response("close")
                .build();
            dialog.add_responses(&[("close", &gettext("_Close"))]);
            dialog
        }
    }
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
/// its place (Connect for Disconnect, the blocked note for the switch), then its device.
fn focus_candidates(name: &str) -> Vec<String> {
    let Some((kind, rest)) = name.split_once(':') else {
        return vec![name.to_owned()];
    };
    let swapped = match kind {
        "connect" => Some(format!("disconnect:{rest}")),
        "disconnect" => Some(format!("connect:{rest}")),
        "adapter" if rest == "switch" => Some("adapter:blocked".to_owned()),
        "adapter" => Some("adapter:switch".to_owned()),
        _ => None,
    };
    let device =
        matches!(kind, "connect" | "disconnect" | "forget").then(|| format!("device:{rest}"));
    std::iter::once(name.to_owned())
        .chain(swapped)
        .chain(device)
        .collect()
}
