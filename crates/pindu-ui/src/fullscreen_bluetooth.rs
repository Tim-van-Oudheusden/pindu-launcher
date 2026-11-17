//! Quick Access's Bluetooth, where BlueZ has an adapter (`pindu_core::system::bluetooth`):
//! its entry, subtitled with the connected devices or how the radio is, and its level with
//! the radio's switch, the paired devices (each opening to Connect or Disconnect and
//! Forget, asked first) and the devices found nearby, which a press pairs and connects.
//! While the level is open discovery and the pairing agent run, and what the agent asks
//! shows in a dialog: a code to confirm (A confirms, B refuses), a code to type on the
//! device, or a PIN or passkey to type here in the passphrase dialog, with the on-screen
//! keyboard or a keyboard. The level is [`PinduBluetoothLevel`], with its own Blueprint
//! (`data/ui/fullscreen_bluetooth.blp`); Quick Access keeps the entry, which sits in its
//! shared list, and adds the level's `win.bluetooth-*` actions to its host. Both show
//! [`BluetoothService`]'s state and call it; neither talks to BlueZ itself.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::osk::Keyboard;
use pindu_core::panels::QuickAccessLevel;
use pindu_core::system::bluetooth::{BluetoothState, Busy, Device, Power, Prompt, Summary};

use crate::application::PinduApplication;
use crate::bluetooth_service::BluetoothService;
use crate::quick_access::PinduQuickAccess;
use crate::secret_dialog::SecretDialog;
use crate::window::PinduWindow;

/// The icon of a device whose own icon the theme lacks.
const FALLBACK_ICON: &str = "bluetooth-active-symbolic";

/// What one of the level's device actions asks of the service, for a device.
type DeviceAction = fn(&BluetoothService, &str);

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_bluetooth.ui"
    )]
    pub struct PinduBluetoothLevel {
        #[template_child]
        pub quick_access_bluetooth_adapter: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_bluetooth_switch: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub quick_access_bluetooth_blocked: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub quick_access_bluetooth_paired_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_bluetooth_paired: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_bluetooth_found_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_bluetooth_found: TemplateChild<gtk::ListBox>,
        /// The dialog showing what Bluetooth's pairing agent asks, with what it shows.
        pub bluetooth_prompt: RefCell<Option<(adw::Dialog, Prompt)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduBluetoothLevel {
        const NAME: &'static str = "PinduBluetoothLevel";
        type Type = super::PinduBluetoothLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduBluetoothLevel {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().setup();
        }
    }

    impl WidgetImpl for PinduBluetoothLevel {}

    impl BinImpl for PinduBluetoothLevel {}
}

glib::wrapper! {
    /// Quick Access's Bluetooth level: the radio's switch, the paired devices and the
    /// devices found nearby. It is the `"bluetooth"` page of Quick Access's stack.
    pub struct PinduBluetoothLevel(ObjectSubclass<imp::PinduBluetoothLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduQuickAccess {
    /// Adds the level's actions, and shows the entry as the section is and as it changes.
    pub(crate) fn setup_bluetooth(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        let device_actions: [(&str, DeviceAction); 3] = [
            ("bluetooth-pair", BluetoothService::pair),
            ("bluetooth-connect", BluetoothService::connect),
            ("bluetooth-disconnect", BluetoothService::disconnect),
        ];
        for (name, act) in device_actions {
            self.add_host_action(
                actions,
                name,
                Some(glib::VariantTy::STRING),
                move |quick_access, target| {
                    if let (Some(service), Some(device)) = (
                        quick_access.bluetooth_service(),
                        target.and_then(|target| target.str()),
                    ) {
                        act(&service, device);
                    }
                },
            );
        }
        self.add_host_action(
            actions,
            "bluetooth-forget",
            Some(glib::VariantTy::STRING),
            |quick_access, target| {
                if let Some(device) = target.and_then(|target| target.str()) {
                    quick_access.imp().bluetooth_level.ask_forget(device);
                }
            },
        );
        let service = application.bluetooth_service();
        service.connect_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |state| quick_access.show_bluetooth_state(state)
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

    fn bluetooth_service(&self) -> Option<BluetoothService> {
        self.application()
            .map(|application| application.bluetooth_service().clone())
    }

    /// Shows or hides the entry, with its subtitle, fills the level again while it is
    /// open, and shows or takes away the agent's prompt; it leaves for the entries once
    /// the section is gone.
    fn show_bluetooth_state(&self, state: &BluetoothState) {
        let imp = self.imp();
        imp.bluetooth_row.set_visible(state.available);
        imp.bluetooth_subtitle
            .set_label(&summary_text(&state.summary()));
        imp.bluetooth_level.show_prompt(state.prompt.as_ref());
        if self.level() != Some(QuickAccessLevel::Bluetooth) {
            return;
        }
        if !state.available {
            self.request_level(QuickAccessLevel::Main);
            return;
        }
        self.fill_bluetooth(state);
    }

    /// Fills the level for `state`, and puts focus back in the panel when the row it was on
    /// is gone.
    fn fill_bluetooth(&self, state: &BluetoothState) {
        if self.imp().bluetooth_level.fill(state) {
            self.focus_level(None);
        }
    }
}

impl PinduBluetoothLevel {
    /// Names the rows controller focus finds again after a refill, gives the found devices
    /// their placeholder, and turns the radio on or off as the user flips its switch.
    fn setup(&self) {
        let imp = self.imp();
        // Named as the device rows are, so focus finds them again after a refill.
        imp.quick_access_bluetooth_switch
            .set_widget_name("adapter:switch");
        imp.quick_access_bluetooth_blocked
            .set_widget_name("adapter:blocked");
        imp.quick_access_bluetooth_switch
            .connect_active_notify(glib::clone!(
                #[weak(rename_to = level)]
                self,
                move |switch| {
                    // Showing the state sets the switch too; only the user's flip asks.
                    let Some(service) = level.bluetooth_service() else {
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
    }

    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 3] {
        let imp = self.imp();
        [
            imp.quick_access_bluetooth_adapter.get(),
            imp.quick_access_bluetooth_paired.get(),
            imp.quick_access_bluetooth_found.get(),
        ]
    }

    /// Where focus goes on the level: the switch, or what blocks the radio.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        let switch = &imp.quick_access_bluetooth_switch;
        Some(if switch.is_visible() {
            switch.get().upcast()
        } else {
            imp.quick_access_bluetooth_blocked.get().upcast()
        })
    }

    /// The service, through the application of the window the level is in.
    fn bluetooth_service(&self) -> Option<BluetoothService> {
        self.root()
            .and_downcast::<gtk::Window>()
            .and_then(|window| window.application())
            .and_downcast::<PinduApplication>()
            .map(|application| application.bluetooth_service().clone())
    }

    /// The level's rows for `state`, keeping focus on the row it was on and paired devices
    /// open as they were. Returns whether focus was on a row that is gone, with no row to
    /// take its place, so the window must put it somewhere.
    pub(crate) fn fill(&self, state: &BluetoothState) -> bool {
        let imp = self.imp();
        let focused = self.focused_row();
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

    /// Shows what the pairing agent asks in a dialog, or closes the dialog once it no
    /// longer asks.
    pub(crate) fn show_prompt(&self, prompt: Option<&Prompt>) {
        let imp = self.imp();
        let shown = imp.bluetooth_prompt.borrow().clone();
        if shown.as_ref().map(|(_, shown)| shown) == prompt {
            return;
        }
        if let Some((dialog, _)) = shown {
            imp.bluetooth_prompt.replace(None);
            match dialog.downcast_ref::<SecretDialog>() {
                Some(secret) => secret.withdraw(),
                None => dialog.force_close(),
            }
        }
        let Some(prompt) = prompt else {
            return;
        };
        let dialog = match prompt {
            Prompt::Confirm { device, passkey } => {
                Some(self.present_alert(prompt, confirm_dialog(device, passkey)))
            }
            Prompt::Show { device, code } => {
                Some(self.present_alert(prompt, show_dialog(device, code)))
            }
            Prompt::EnterPin { device } => {
                self.ask_code(device, &gettext("Type the device's PIN."))
            }
            Prompt::EnterPasskey { device } => self.ask_code(
                device,
                &gettext("Type the six-digit code the device shows."),
            ),
        };
        if let Some(dialog) = dialog {
            imp.bluetooth_prompt.replace(Some((dialog, prompt.clone())));
        }
    }

    /// Presents `dialog` for `prompt`, a code to confirm or to type on the device, and
    /// answers the prompt with its response.
    fn present_alert(&self, prompt: &Prompt, dialog: adw::AlertDialog) -> adw::Dialog {
        let answered = prompt.clone();
        dialog.connect_response(
            None,
            glib::clone!(
                #[weak(rename_to = level)]
                self,
                move |_, response| {
                    let Some(service) = level.bluetooth_service() else {
                        return;
                    };
                    // A dialog closing for a prompt gone or replaced answers nothing.
                    if service.state().prompt.as_ref() != Some(&answered) {
                        return;
                    }
                    // The dialog closes itself after its response; the answer taking the
                    // prompt away must not close it again.
                    level.imp().bluetooth_prompt.replace(None);
                    if matches!(answered, Prompt::Confirm { .. }) {
                        service.confirm(response == "pair");
                    } else {
                        service.dismiss();
                    }
                }
            ),
        );
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = level)]
            self,
            move |dialog| {
                let imp = level.imp();
                let ours = imp
                    .bluetooth_prompt
                    .borrow()
                    .as_ref()
                    .is_some_and(|(shown, _)| shown == dialog.upcast_ref::<adw::Dialog>());
                if ours {
                    imp.bluetooth_prompt.replace(None);
                }
            }
        ));
        // On the window the level is in.
        dialog.present(self.root().as_ref());
        dialog.upcast()
    }

    /// Asks for the PIN or passkey `device` wants in the passphrase dialog, saying
    /// `message`, and answers the agent with what is typed, or refuses once it is
    /// cancelled. The code keyboard starts on the digits and shows what it types: the
    /// device shows the code anyway.
    fn ask_code(&self, device: &str, message: &str) -> Option<adw::Dialog> {
        let window = self.root().and_downcast::<PinduWindow>()?;
        let dialog = window.ask_secret(
            // Translators: {device} is a Bluetooth device's name.
            &gettext("Pair With {device}").replace("{device}", device),
            message,
            Keyboard::code(),
            glib::clone!(
                #[weak(rename_to = level)]
                self,
                move |code: Option<String>| {
                    // The dialog closes itself after its answer; the answer taking the
                    // prompt away must not withdraw it.
                    level.imp().bluetooth_prompt.replace(None);
                    if let Some(service) = level.bluetooth_service() {
                        service.enter(code);
                    }
                }
            ),
        );
        Some(dialog.upcast())
    }

    /// Asks before forgetting a paired device: pairing it again takes a controller, and it
    /// may be the one in use.
    pub(crate) fn ask_forget(&self, device: &str) {
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
        dialog.present(self.root().as_ref());
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

/// The dialog for a code both sides show, which A confirms and B refuses.
fn confirm_dialog(device: &str, passkey: &str) -> adw::AlertDialog {
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

/// The dialog for a code to type on the device.
fn show_dialog(device: &str, code: &str) -> adw::AlertDialog {
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

/// A code, large, in a dialog.
fn code_label(code: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(code)
        .selectable(false)
        .css_classes(["title-1", "numeric"])
        .build()
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
