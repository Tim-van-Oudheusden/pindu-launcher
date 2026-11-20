//! The menu drawer's Power (`pindu_core::system::power`): its entry, last on the drawer's
//! main level, and its level with Suspend, Restart and Power Off, each greyed out with a
//! reason when the system does not allow it, then Exit Pindu Launcher, which quits. Natively
//! the actions ask logind on the system bus; in the Flatpak they ask the host's logind
//! through host spawn, and without it show greyed out, saying Preferences gives the access.
//! Restart and Power Off ask first while a game we started runs. As gamescope-session-plus's
//! client (`--session`, `pindu_core::gamescope::session`) the level also offers Switch to
//! Desktop, which runs the distribution's `os-session-select desktop` on the host and then
//! quits, ending the session; it asks first as Restart does. In the launcher's own session
//! quitting would leave the screen empty, so there Exit is left out. The level is
//! [`PinduPowerLevel`], with its own Blueprint (`data/ui/fullscreen_power.blp`); the window
//! keeps the entry, which sits in the drawer's shared list, the level's `win.power-*`
//! actions and the question asked first. Both show [`SystemPowerService`]'s state and the
//! window calls it; neither talks to logind itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamescope::session::{
    PowerLevelEntry, SwitchToDesktop, asks_before_switching, power_level,
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
    /// The menu drawer's Power level: Suspend, Restart and Power Off, as the system allows
    /// them, Switch to Desktop in a gamescope-session-plus session, and Exit Pindu Launcher
    /// outside the launcher's own session. It is the `"power"` page of the window's drawer
    /// stack.
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
            let state = service.state();
            self.imp()
                .drawer_panel
                .power_level()
                .fill(&self.power_level_entries(&state), state.sandboxed);
        }
    }

    /// The Power level's entries for `state`: what the system allows, Switch to Desktop for
    /// gamescope-session-plus's client, and Exit outside the launcher's own session.
    pub(crate) fn power_level_entries(&self, state: &PowerState) -> Vec<PowerLevelEntry> {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return Vec::new();
        };
        power_level(
            state,
            application.session().switch_to_desktop(),
            !application.runs_as_own_session(),
        )
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
    /// leaves for the drawer's entries once the level has nothing left to list.
    fn show_power_state(&self, state: &PowerState) {
        let entries = self.power_level_entries(state);
        let shown = !entries.is_empty();
        self.imp().drawer_panel.power_row().set_visible(shown);
        if self.open_panel() != Some(Panel::Drawer(DrawerLevel::Power)) {
            return;
        }
        if !shown {
            self.show_panel(Panel::Drawer(DrawerLevel::Main));
            return;
        }
        let level = self.imp().drawer_panel.power_level();
        let had_focus = level.has_focus();
        level.fill(&entries, state.sandboxed);
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
        glib::spawn_future_local(async move {
            match session_service::switch_to_desktop().await {
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

    /// The level's rows for `entries`; Suspend, Restart and Power Off not allowed say the
    /// app needs access outside the sandbox when `sandboxed`, or else that the system does
    /// not allow them.
    pub(crate) fn fill(&self, entries: &[PowerLevelEntry], sandboxed: bool) {
        let list = &self.imp().drawer_power;
        list.remove_all();
        for entry in entries {
            let row = match *entry {
                PowerLevelEntry::Power(entry) => power_row(entry, sandboxed),
                PowerLevelEntry::SwitchToDesktop(switch) => {
                    let allowed = if switch.allowed {
                        Ok(())
                    } else {
                        Err(needs_host_access())
                    };
                    level_row(
                        &gettext("Switch to Desktop"),
                        "user-desktop-symbolic",
                        "win.power-desktop",
                        allowed,
                    )
                }
                PowerLevelEntry::Exit => level_row(
                    &gettext("Exit Pindu Launcher"),
                    "application-exit-symbolic",
                    "app.quit",
                    Ok(()),
                ),
            };
            list.append(&row);
        }
    }
}

/// Why an entry is greyed out in the Flatpak without host spawn.
fn needs_host_access() -> String {
    gettext("Needs access outside the sandbox, in Preferences")
}

/// The row of an entry of the level: insensitive, saying why, when it is not allowed:
/// the app needs access outside the sandbox (`sandboxed`), or the system does not allow it.
fn power_row(entry: PowerEntry, sandboxed: bool) -> gtk::ListBoxRow {
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
    } else if sandboxed {
        Err(needs_host_access())
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
