//! The menu drawer's Power (`pindu_core::system::power`): its entry after Quit, shown where
//! logind lets the session suspend, restart or power off, and its level with Suspend,
//! Restart and Power Off, each greyed out with a reason when the system does not allow it.
//! Restart and Power Off ask first while a game we started runs. The window shows
//! [`SystemPowerService`]'s state and calls it; it never talks to logind itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{DrawerLevel, Panel};
use pindu_core::system::power::{PowerAction, PowerEntry, PowerState, asks_first};

use crate::application::PinduApplication;
use crate::system_power_service::SystemPowerService;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds the entry's and the level's actions, and shows the entry as the section is and
    /// as it changes.
    pub(crate) fn setup_power(&self) {
        let power = gio::ActionEntry::builder("drawer-power")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::Drawer(DrawerLevel::Power));
            })
            .build();
        let suspend = gio::ActionEntry::builder("power-suspend")
            .activate(|window: &Self, _, _| window.press_power(PowerAction::Suspend))
            .build();
        let restart = gio::ActionEntry::builder("power-restart")
            .activate(|window: &Self, _, _| window.press_power(PowerAction::Restart))
            .build();
        let power_off = gio::ActionEntry::builder("power-off")
            .activate(|window: &Self, _, _| window.press_power(PowerAction::PowerOff))
            .build();
        self.add_action_entries([power, suspend, restart, power_off]);
        let Some(service) = self.system_power_service() else {
            return;
        };
        service.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |state| window.show_power_state(state)
        ));
        self.show_power_state(&service.state());
    }

    /// The level opens: it lists the actions as last read.
    pub(crate) fn show_power(&self) {
        if let Some(service) = self.system_power_service() {
            self.fill_power(&service.state());
        }
    }

    /// Where focus goes on the level: the first action the system allows, or else the
    /// first one.
    pub(crate) fn power_focus(&self) -> Option<gtk::Widget> {
        let list = &self.imp().drawer_power;
        std::iter::successors(list.first_child(), |row| row.next_sibling())
            .find(|row| row.is_sensitive())
            .or_else(|| list.first_child())
    }

    fn system_power_service(&self) -> Option<SystemPowerService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.system_power_service().clone())
    }

    /// Shows or hides the drawer's entry, and fills the level again while it is open; it
    /// leaves for the drawer's entries once the section is gone.
    fn show_power_state(&self, state: &PowerState) {
        self.imp().drawer_power_row.set_visible(state.shown());
        if self.open_panel() != Some(Panel::Drawer(DrawerLevel::Power)) {
            return;
        }
        if !state.shown() {
            self.show_panel(Panel::Drawer(DrawerLevel::Main));
            return;
        }
        let had_focus = self.focus_in_power();
        self.fill_power(state);
        if had_focus {
            self.focus_panel(None);
        }
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    fn focus_in_power(&self) -> bool {
        let imp = self.imp();
        gtk::prelude::GtkWindowExt::focus(self)
            .is_some_and(|focus| focus.is_ancestor(&*imp.drawer_power))
    }

    /// The level's rows for `state`.
    fn fill_power(&self, state: &PowerState) {
        let list = &self.imp().drawer_power;
        list.remove_all();
        for entry in &state.entries {
            list.append(&power_row(*entry));
        }
    }

    /// Runs `action`, asking first while a game we started runs when it would end it.
    fn press_power(&self, action: PowerAction) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let sessions = application.session_service();
        if !asks_first(action, sessions.any_playing()) {
            self.run_power(action);
            return;
        }
        let running = sessions.sessions();
        let Some((id, _)) = running.first() else {
            return;
        };
        let game = self
            .library()
            .game_title(id)
            .unwrap_or_else(|| id.as_str().to_owned());
        self.ask_power(action, &game);
    }

    /// Asks before restarting or powering off while `game` runs: its unsaved progress is
    /// lost.
    fn ask_power(&self, action: PowerAction, game: &str) {
        let (heading, confirm) = match action {
            PowerAction::PowerOff => (
                // Translators: {game} is the title of the game that runs.
                gettext("Power off while {game} is running?"),
                gettext("_Power Off"),
            ),
            PowerAction::Restart | PowerAction::Suspend => (
                // Translators: {game} is the title of the game that runs.
                gettext("Restart while {game} is running?"),
                gettext("_Restart"),
            ),
        };
        let dialog = adw::AlertDialog::builder()
            .heading(heading.replace("{game}", game))
            .body(gettext("Unsaved progress is lost."))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("confirm", &confirm)]);
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            Some("confirm"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| window.run_power(action)
            ),
        );
        dialog.present(Some(self));
    }

    /// Closes the drawer and asks the system to do `action`.
    fn run_power(&self, action: PowerAction) {
        self.close_panel();
        if let Some(service) = self.system_power_service() {
            service.run(action);
        }
    }
}

/// The row of an entry of the level: insensitive, saying why, when the system does not
/// allow it.
fn power_row(entry: PowerEntry) -> gtk::ListBoxRow {
    let (title, icon, action) = match entry.action {
        PowerAction::Suspend => (
            gettext("Suspend"),
            "media-playback-pause-symbolic",
            "win.power-suspend",
        ),
        PowerAction::Restart => (
            gettext("Restart"),
            "system-reboot-symbolic",
            "win.power-restart",
        ),
        PowerAction::PowerOff => (
            gettext("Power Off"),
            "system-shutdown-symbolic",
            "win.power-off",
        ),
    };
    let content = gtk::Box::builder().spacing(16).build();
    content.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let labels = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .hexpand(true)
        .build();
    labels.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    if !entry.allowed {
        labels.append(
            &gtk::Label::builder()
                .label(gettext("Not allowed on this system"))
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dimmed"])
                .build(),
        );
    }
    content.append(&labels);
    let row = gtk::ListBoxRow::builder()
        .child(&content)
        .sensitive(entry.allowed)
        .build();
    row.set_action_name(Some(action));
    row
}
