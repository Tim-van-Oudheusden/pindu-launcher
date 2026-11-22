//! The menu drawer's power dock (`pindu_core::gamescope::session::power_dock`): a row of
//! round buttons at the bottom of its main level, always in view under the scrolling list.
//! The ways out of the launcher sit at its start: Exit Fullscreen Mode, Switch to Desktop
//! and Exit Pindu Launcher, which quits. The machine's sit at its end: Suspend, Restart and
//! Power Off, each dimmed with a reason when the system does not offer it, which still
//! takes focus but does nothing. A line over the dock names the focused button. Natively
//! the machine's actions ask logind on the system bus; in the Flatpak they ask the host's
//! logind through host spawn, and without it show dimmed, saying Preferences gives the
//! access. Restart and Power Off ask first while a game we started runs. As
//! gamescope-session-plus's client (`--session`, `pindu_core::gamescope::session`) the
//! dock also offers Switch to Desktop, which runs the distribution's `os-session-select
//! desktop` on the host and then quits, ending the session; it asks first as Restart does.
//! In the launcher's own session quitting would leave the screen empty, so there Exit is
//! left out. The dock is [`PinduPowerDock`], with its own Blueprint
//! (`data/ui/fullscreen_power.blp`); the window keeps the buttons' `win.power-*` actions
//! and the question asked first. Both show [`SystemPowerService`]'s state and the window
//! calls it; neither talks to logind itself.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamescope::session::{
    DockEntry, DockGroup, SwitchToDesktop, asks_before_switching, dock_landing,
};
use pindu_core::system::power::{PowerAction, PowerEntry, PowerState, asks_first};

use crate::application::PinduApplication;
use crate::session_service;
use crate::system_power_service::SystemPowerService;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_power.ui")]
    pub struct PinduPowerDock {
        #[template_child]
        pub label: TemplateChild<gtk::Label>,
        #[template_child]
        pub leaving: TemplateChild<gtk::Box>,
        #[template_child]
        pub machine: TemplateChild<gtk::Box>,
        /// The buttons, start to end.
        pub buttons: RefCell<Vec<gtk::Button>>,
        /// The button Up from the top of the menu lands on.
        pub landing: RefCell<Option<gtk::Button>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduPowerDock {
        const NAME: &'static str = "PinduPowerDock";
        type Type = super::PinduPowerDock;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduPowerDock {
        fn constructed(&self) {
            self.parent_constructed();
            // The line names the focused button, and is empty once focus leaves the dock.
            let focus = gtk::EventControllerFocus::new();
            focus.connect_leave(glib::clone!(
                #[weak(rename_to = label)]
                self.label,
                move |_| label.set_label("")
            ));
            self.obj().add_controller(focus);
        }
    }

    impl WidgetImpl for PinduPowerDock {}

    impl BinImpl for PinduPowerDock {}
}

glib::wrapper! {
    /// The menu drawer's power dock: Exit Fullscreen Mode, Switch to Desktop in a
    /// gamescope-session-plus session and Exit Pindu Launcher outside the launcher's own
    /// session at its start; Suspend, Restart and Power Off, as the system offers them, at
    /// its end. It sits at the bottom of the drawer's main level.
    pub struct PinduPowerDock(ObjectSubclass<imp::PinduPowerDock>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduWindow {
    /// Adds the dock's actions, and fills the dock as the system's power state is and as
    /// it changes.
    pub(crate) fn setup_power(&self) {
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
        self.add_action_entries([suspend, restart, power_off, desktop]);
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

    /// The drawer opens: the dock shows the actions as last read.
    pub(crate) fn show_power(&self) {
        if let Some(service) = self.system_power_service() {
            self.show_power_state(&service.state());
        }
    }

    fn system_power_service(&self) -> Option<SystemPowerService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.system_power_service().clone())
    }

    /// The dock's Switch to Desktop, offered to gamescope-session-plus's client.
    fn switch_to_desktop(&self) -> Option<SwitchToDesktop> {
        self.application()
            .and_downcast::<PinduApplication>()
            .and_then(|application| application.session().switch_to_desktop())
    }

    /// Fills the drawer's dock for `state`: what the system offers, Switch to Desktop for
    /// gamescope-session-plus's client, and Exit outside the launcher's own session.
    fn show_power_state(&self, state: &PowerState) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        self.imp().drawer_panel.fill_dock(
            state,
            application.session().switch_to_desktop(),
            !application.runs_as_own_session(),
        );
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

impl PinduPowerDock {
    /// The dock's buttons for `entries`, start to end, its leaving group at its start and
    /// its machine group at its end, and where Up from the top of the menu lands. Suspend,
    /// Restart and Power Off not allowed say the app needs access outside the sandbox when
    /// `sandboxed`, or else that the system does not allow them. The dock shows while it
    /// has buttons. A button that had focus gives it to the one now in its place.
    pub(crate) fn fill(&self, entries: &[DockEntry], sandboxed: bool) {
        let imp = self.imp();
        let focused = self.focused();
        for group in [&imp.leaving, &imp.machine] {
            while let Some(child) = group.first_child() {
                group.remove(&child);
            }
        }
        let buttons: Vec<gtk::Button> = entries
            .iter()
            .map(|entry| {
                let button = self.dock_button(*entry, sandboxed);
                match entry.group() {
                    DockGroup::Leaving => imp.leaving.append(&button),
                    DockGroup::Machine => imp.machine.append(&button),
                }
                button
            })
            .collect();
        imp.landing
            .replace(dock_landing(entries).and_then(|index| buttons.get(index).cloned()));
        let refocus = focused.and_then(|index| buttons.get(index).or(buttons.last()).cloned());
        self.set_visible(!buttons.is_empty());
        imp.buttons.replace(buttons);
        if let Some(button) = refocus {
            button.grab_focus();
        }
    }

    /// The index of the button that has focus, if one does.
    pub(crate) fn focused(&self) -> Option<usize> {
        self.imp()
            .buttons
            .borrow()
            .iter()
            .position(|button| button.is_focus())
    }

    /// The button at `index`, start to end.
    pub(crate) fn button(&self, index: usize) -> Option<gtk::Widget> {
        self.imp()
            .buttons
            .borrow()
            .get(index)
            .map(|button| button.clone().upcast())
    }

    /// The button Up from the top of the menu lands on
    /// (`pindu_core::gamescope::session::dock_landing`).
    pub(crate) fn landing(&self) -> Option<gtk::Widget> {
        self.imp()
            .landing
            .borrow()
            .clone()
            .map(|button| button.upcast())
    }

    /// A round button for `entry`: its icon, with its name as its tooltip and accessible
    /// label. One the system does not offer is dimmed, has no action, so A does nothing,
    /// and is exposed as disabled with the reason as its description; it still takes
    /// focus, so the line over the dock can give the reason.
    fn dock_button(&self, entry: DockEntry, sandboxed: bool) -> gtk::Button {
        let (name, icon, action) = dock_entry(entry);
        let image = gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        let button = gtk::Button::builder()
            .child(&image)
            .tooltip_text(&name)
            .css_classes(["circular", "dock-button"])
            .build();
        button.update_property(&[gtk::accessible::Property::Label(&name)]);
        let line = match unavailable_reason(entry, sandboxed) {
            None => {
                button.set_action_name(Some(action));
                name
            }
            Some(reason) => {
                image.add_css_class("dimmed");
                button.update_state(&[gtk::accessible::State::Disabled(true)]);
                button.update_property(&[gtk::accessible::Property::Description(&reason)]);
                // Translators: what the line over the power dock says on an action the
                // system does not offer: its name, then why.
                gettext("{name} — {reason}")
                    .replace("{name}", &name)
                    .replace("{reason}", &reason)
            }
        };
        let focus = gtk::EventControllerFocus::new();
        focus.connect_enter(glib::clone!(
            #[weak(rename_to = label)]
            self.imp().label,
            move |_| label.set_label(&line)
        ));
        button.add_controller(focus);
        button
    }
}

/// The name, icon and action of a dock entry.
fn dock_entry(entry: DockEntry) -> (String, &'static str, &'static str) {
    match entry {
        DockEntry::ExitFullscreen => (
            gettext("Exit Fullscreen Mode"),
            "view-restore-symbolic",
            "win.leave-fullscreen",
        ),
        DockEntry::SwitchToDesktop(_) => (
            gettext("Switch to Desktop"),
            "user-desktop-symbolic",
            "win.power-desktop",
        ),
        DockEntry::Exit => (
            gettext("Exit Pindu Launcher"),
            "application-exit-symbolic",
            "app.quit",
        ),
        DockEntry::Power(PowerEntry { action, .. }) => match action {
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
        },
    }
}

/// Why `entry` is dimmed, when the system does not offer it: the app needs access outside
/// the sandbox (`sandboxed`, and always for Switch to Desktop), or the system does not
/// allow it.
fn unavailable_reason(entry: DockEntry, sandboxed: bool) -> Option<String> {
    if entry.allowed() {
        None
    } else if sandboxed || matches!(entry, DockEntry::SwitchToDesktop(_)) {
        Some(gettext("Needs access outside the sandbox, in Preferences"))
    } else {
        Some(gettext("Not allowed on this system"))
    }
}
