//! The menu drawer's Power (`pindu_core::system::power`): its entry after Quit, shown where
//! logind lets the session suspend, restart or power off, and its level with Suspend,
//! Restart and Power Off, each greyed out with a reason when the system does not allow it.
//! Restart and Power Off ask first while a game we started runs. As gamescope-session-plus's
//! client (`--session`, `pindu_core::gamescope::session`) the level also offers Switch to
//! Desktop, which runs the distribution's `os-session-select desktop` on the host and then
//! quits, ending the session; it asks first as Restart does. The level is
//! [`PinduPowerLevel`], with its own Blueprint (`data/ui/fullscreen_power.blp`); the window
//! keeps the entry, which sits in the drawer's shared list, the level's `win.power-*`
//! actions and the question asked first. Both show [`SystemPowerService`]'s state and the
//! window calls it; neither talks to logind itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamescope::session::{
    SwitchToDesktop, asks_before_switching, power_shown, switch_to_desktop_command,
};
use pindu_core::panels::{DrawerLevel, Panel};
use pindu_core::system::power::{PowerAction, PowerEntry, PowerState, asks_first};

use crate::application::PinduApplication;
use crate::session_service;
use crate::system_power_service::SystemPowerService;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_power.ui")]
    pub struct PinduPowerLevel {
        #[template_child]
        pub drawer_power: TemplateChild<gtk::ListBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduPowerLevel {
        const NAME: &'static str = "PinduPowerLevel";
        type Type = super::PinduPowerLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduPowerLevel {}

    impl WidgetImpl for PinduPowerLevel {}

    impl BinImpl for PinduPowerLevel {}
}

glib::wrapper! {
    /// The menu drawer's Power level: Suspend, Restart and Power Off, as logind allows them,
    /// and Switch to Desktop in a gamescope-session-plus session. It is the `"power"` page
    /// of the window's drawer stack.
    pub struct PinduPowerLevel(ObjectSubclass<imp::PinduPowerLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

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
        let desktop = gio::ActionEntry::builder("power-desktop")
            .activate(|window: &Self, _, _| window.press_switch_to_desktop())
            .build();
        self.add_action_entries([power, suspend, restart, power_off, desktop]);
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
            self.imp()
                .drawer_power_level
                .fill(&service.state(), self.switch_to_desktop());
        }
    }

    /// Where focus goes on the level: the first action the system allows, or else the
    /// first one.
    pub(crate) fn power_focus(&self) -> Option<gtk::Widget> {
        self.imp().drawer_power_level.focus_target()
    }

    fn system_power_service(&self) -> Option<SystemPowerService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.system_power_service().clone())
    }

    /// The level's Switch to Desktop, offered to gamescope-session-plus's client.
    fn switch_to_desktop(&self) -> Option<SwitchToDesktop> {
        self.application()
            .and_downcast::<PinduApplication>()
            .and_then(|application| application.session().switch_to_desktop())
    }

    /// Shows or hides the drawer's entry, and fills the level again while it is open; it
    /// leaves for the drawer's entries once the section is gone.
    fn show_power_state(&self, state: &PowerState) {
        let switch = self.switch_to_desktop();
        let shown = power_shown(state, switch);
        self.imp().drawer_power_row.set_visible(shown);
        if self.open_panel() != Some(Panel::Drawer(DrawerLevel::Power)) {
            return;
        }
        if !shown {
            self.show_panel(Panel::Drawer(DrawerLevel::Main));
            return;
        }
        let level = &self.imp().drawer_power_level;
        let had_focus = level.has_focus();
        level.fill(state, switch);
        if had_focus {
            self.focus_panel(None);
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
        self.ask_first(&heading.replace("{game}", game), &confirm, move |window| {
            window.run_power(action);
        });
    }

    /// Asks `heading` before something that ends the running game, and calls `confirmed`
    /// when `confirm` is pressed.
    fn ask_first(&self, heading: &str, confirm: &str, confirmed: impl Fn(&Self) + 'static) {
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(gettext("Unsaved progress is lost."))
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[("cancel", &gettext("_Cancel")), ("confirm", confirm)]);
        dialog.set_response_appearance("confirm", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            Some("confirm"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| confirmed(&window)
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

    /// Switches to the desktop session, asking first while a game we started runs: the
    /// session, and the game with it, ends.
    fn press_switch_to_desktop(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let sessions = application.session_service();
        if !asks_before_switching(sessions.any_playing()) {
            self.run_switch_to_desktop();
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
        // Translators: {game} is the title of the game that runs.
        let heading = gettext("Switch to the desktop while {game} is running?");
        self.ask_first(
            &heading.replace("{game}", &game),
            &gettext("_Switch to Desktop"),
            Self::run_switch_to_desktop,
        );
    }

    /// Closes the drawer and runs `os-session-select desktop` on the host; once it
    /// succeeds the app quits, which ends the session.
    fn run_switch_to_desktop(&self) {
        self.close_panel();
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        if !self
            .switch_to_desktop()
            .is_some_and(|switch| switch.allowed)
        {
            return;
        }
        let message = gettext("Could not switch to the desktop");
        let failed = glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |error: glib::Error| {
                glib::g_warning!("pindu", "Could not switch to the desktop: {error}");
                window.add_toast(adw::Toast::new(&message));
            }
        );
        let child = match session_service::spawn(&switch_to_desktop_command(), None) {
            Ok(child) => child,
            Err(error) => return failed(error),
        };
        glib::spawn_future_local(async move {
            match child.wait_check_future().await {
                Ok(()) => application.quit(),
                Err(error) => failed(error),
            }
        });
    }
}

impl PinduPowerLevel {
    /// Where focus goes on the level: the first action the system allows, or else the
    /// first one.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        let list = &self.imp().drawer_power;
        std::iter::successors(list.first_child(), |row| row.next_sibling())
            .find(|row| row.is_sensitive())
            .or_else(|| list.first_child())
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    pub(crate) fn has_focus(&self) -> bool {
        let list = &self.imp().drawer_power;
        self.root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root))
            .is_some_and(|focus| focus.is_ancestor(&**list))
    }

    /// The level's rows for `state`, then Switch to Desktop when `switch` offers it.
    pub(crate) fn fill(&self, state: &PowerState, switch: Option<SwitchToDesktop>) {
        let list = &self.imp().drawer_power;
        list.remove_all();
        for entry in &state.entries {
            list.append(&power_row(*entry));
        }
        if let Some(switch) = switch {
            let allowed = if switch.allowed {
                Ok(())
            } else {
                Err(gettext("Needs access outside the sandbox, in Preferences"))
            };
            list.append(&level_row(
                &gettext("Switch to Desktop"),
                "user-desktop-symbolic",
                "win.power-desktop",
                allowed,
            ));
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
    let allowed = if entry.allowed {
        Ok(())
    } else {
        Err(gettext("Not allowed on this system"))
    };
    level_row(&title, icon, action, allowed)
}

/// A row of the level running `action`: insensitive, saying why (`allowed`'s error), when
/// it is not allowed.
fn level_row(
    title: &str,
    icon: &str,
    action: &str,
    allowed: Result<(), String>,
) -> gtk::ListBoxRow {
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
    if let Err(reason) = &allowed {
        labels.append(
            &gtk::Label::builder()
                .label(reason)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["caption", "dimmed"])
                .build(),
        );
    }
    content.append(&labels);
    let row = gtk::ListBoxRow::builder()
        .child(&content)
        .sensitive(allowed.is_ok())
        .build();
    row.set_action_name(Some(action));
    row
}
