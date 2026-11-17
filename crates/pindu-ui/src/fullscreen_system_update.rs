//! Quick Access's System Update, on Pindu OS only (`pindu_core::system::update`): its entry,
//! subtitled with the installed version or that an update is ready, and its level with the
//! installed version, the update waiting for the next start, and Restart to Update, Roll
//! Back (asked first) or Restart, as bootc's status allows. The level is
//! [`PinduSystemUpdateLevel`], with its own Blueprint (`data/ui/fullscreen_system_update.blp`);
//! the window keeps the entry, which sits in Quick Access's shared list, and the level's
//! `win.restart-system` and `win.roll-back-system`. Both show [`SystemUpdateService`]'s
//! state and call it; neither reads the status file itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::update::{Busy, UpdateAction, UpdateState};

use crate::application::PinduApplication;
use crate::system_update_service::SystemUpdateService;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_system_update.ui"
    )]
    pub struct PinduSystemUpdateLevel {
        #[template_child]
        pub quick_access_system_versions: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub quick_access_rollback_note: TemplateChild<gtk::Label>,
        #[template_child]
        pub quick_access_system_actions: TemplateChild<gtk::ListBox>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduSystemUpdateLevel {
        const NAME: &'static str = "PinduSystemUpdateLevel";
        type Type = super::PinduSystemUpdateLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduSystemUpdateLevel {}

    impl WidgetImpl for PinduSystemUpdateLevel {}

    impl BinImpl for PinduSystemUpdateLevel {}
}

glib::wrapper! {
    /// Quick Access's System Update level: the installed version, the update waiting for
    /// the next start, and the actions bootc's status allows. It is the `"system-update"`
    /// page of the window's Quick Access stack.
    pub struct PinduSystemUpdateLevel(ObjectSubclass<imp::PinduSystemUpdateLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduWindow {
    /// Adds the level's actions, and shows the entry as the section is and as it changes.
    pub(crate) fn setup_system_update(&self) {
        let restart = gio::ActionEntry::builder("restart-system")
            .activate(|window: &Self, _, _| {
                if let Some(service) = window.system_update_service() {
                    service.restart();
                }
            })
            .build();
        let roll_back = gio::ActionEntry::builder("roll-back-system")
            .activate(|window: &Self, _, _| window.ask_roll_back())
            .build();
        self.add_action_entries([restart, roll_back]);
        let Some(service) = self.system_update_service() else {
            return;
        };
        service.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |state| window.show_system_update_state(state)
        ));
        self.show_system_update_state(&service.state());
    }

    /// The level opens: it shows the state as last read, and the status file is read
    /// again.
    pub(crate) fn show_system_update(&self) {
        let Some(service) = self.system_update_service() else {
            return;
        };
        self.imp()
            .quick_access_system_update_level
            .fill(&service.state());
        service.refresh();
    }

    /// Where focus goes on the level: the first action, or else the installed version.
    pub(crate) fn system_update_focus(&self) -> Option<gtk::Widget> {
        self.imp().quick_access_system_update_level.focus_target()
    }

    fn system_update_service(&self) -> Option<SystemUpdateService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.system_update_service().clone())
    }

    /// Shows or hides the entry, with its subtitle, and fills the level again while it is
    /// open; it leaves for the entries once the section is gone.
    fn show_system_update_state(&self, state: &UpdateState) {
        let imp = self.imp();
        imp.quick_access_system_update_row
            .set_visible(state.available);
        let subtitle = if state.update_ready() {
            gettext("Update ready")
        } else {
            state.installed.clone().unwrap_or_default()
        };
        imp.quick_access_system_update_subtitle
            .set_visible(!subtitle.is_empty());
        imp.quick_access_system_update_subtitle.set_label(&subtitle);
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::SystemUpdate)) {
            return;
        }
        if !state.available {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::Main));
            return;
        }
        let level = &imp.quick_access_system_update_level;
        let had_focus = level.has_focus();
        level.fill(state);
        if had_focus {
            self.focus_panel(None);
        }
    }

    /// Asks before rolling back: the console restarts into the previous version.
    fn ask_roll_back(&self) {
        let Some(service) = self.system_update_service() else {
            return;
        };
        let state = service.state();
        if !state.actions().contains(&UpdateAction::RollBack) {
            return;
        }
        let heading = match rollback_version(&state) {
            // Translators: {version} is an OS version, such as 45.20261101.0.
            Some(version) => gettext("Roll Back to {version}?").replace("{version}", version),
            None => gettext("Roll Back to the Previous Version?"),
        };
        let body = if state.update_ready() {
            gettext(
                "The console restarts into that version, and the update waiting for the next start is discarded. The version it runs now stays installed, so you can go back to it.",
            )
        } else {
            gettext(
                "The console restarts into that version. The version it runs now stays installed, so you can go back to it.",
            )
        };
        let dialog = adw::AlertDialog::builder()
            .heading(heading)
            .body(body)
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", &gettext("_Cancel")),
            ("roll-back", &gettext("_Roll Back")),
        ]);
        dialog.set_response_appearance("roll-back", adw::ResponseAppearance::Destructive);
        dialog.connect_response(
            Some("roll-back"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| {
                    if let Some(service) = window.system_update_service() {
                        service.roll_back();
                    }
                }
            ),
        );
        // Rolling back fills the level again while the dialog has focus, so the row the
        // dialog gives focus back to may be gone.
        dialog.connect_closed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                glib::idle_add_local_once(glib::clone!(
                    #[weak]
                    window,
                    move || {
                        let open = window.open_panel()
                            == Some(Panel::QuickAccess(QuickAccessLevel::SystemUpdate));
                        let level = &window.imp().quick_access_system_update_level;
                        if open && !level.has_focus() {
                            window.focus_panel(None);
                        }
                    }
                ));
            }
        ));
        dialog.present(Some(self));
    }
}

impl PinduSystemUpdateLevel {
    /// The level's lists, whose Up and Down go on to the next list at their ends.
    pub(crate) fn lists(&self) -> [gtk::ListBox; 2] {
        let imp = self.imp();
        [
            imp.quick_access_system_versions.get(),
            imp.quick_access_system_actions.get(),
        ]
    }

    /// Where focus goes on the level: the first action, or else the installed version.
    pub(crate) fn focus_target(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        let actions = &imp.quick_access_system_actions;
        let list = if actions.is_visible() {
            actions
        } else {
            &imp.quick_access_system_versions
        };
        list.row_at_index(0).map(|row| row.upcast())
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    pub(crate) fn has_focus(&self) -> bool {
        self.root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root))
            .is_some_and(|focus| self.lists().iter().any(|list| focus.is_ancestor(list)))
    }

    /// The level's rows for `state`: the versions, the note on a queued rollback, and the
    /// actions, or what runs.
    pub(crate) fn fill(&self, state: &UpdateState) {
        let imp = self.imp();
        let versions = &imp.quick_access_system_versions;
        versions.remove_all();
        versions.append(&version_row(
            &gettext("Installed Version"),
            state.installed.as_deref(),
        ));
        if state.update_ready() {
            let staged = state
                .status
                .staged
                .as_ref()
                .and_then(|staged| staged.version.as_deref());
            versions.append(&version_row(&gettext("Update Ready"), staged));
        }
        imp.quick_access_rollback_note
            .set_visible(state.status.rollback_queued);
        let actions = &imp.quick_access_system_actions;
        actions.remove_all();
        if let Some(busy) = state.busy {
            actions.append(&busy_row(busy));
        }
        for action in state.actions() {
            actions.append(&action_row(action, state));
        }
        actions.set_visible(actions.row_at_index(0).is_some());
    }
}

/// The version of the deployment to roll back to, when it has one.
fn rollback_version(state: &UpdateState) -> Option<&str> {
    state
        .status
        .rollback
        .as_ref()
        .and_then(|rollback| rollback.version.as_deref())
}

/// A row naming a version: `title`, with the version under it when there is one.
fn version_row(title: &str, version: Option<&str>) -> adw::ActionRow {
    adw::ActionRow::builder()
        .title(title)
        .subtitle(version.unwrap_or_default())
        .use_markup(false)
        .build()
}

/// The row of an action the level offers.
fn action_row(action: UpdateAction, state: &UpdateState) -> adw::ButtonRow {
    let (title, action, icon) = match action {
        UpdateAction::RestartToUpdate => (
            gettext("Restart to Update"),
            "win.restart-system",
            "system-reboot-symbolic",
        ),
        UpdateAction::Restart => (
            gettext("Restart"),
            "win.restart-system",
            "system-reboot-symbolic",
        ),
        UpdateAction::RollBack => (
            match rollback_version(state) {
                // Translators: {version} is an OS version, such as 45.20261101.0.
                Some(version) => gettext("Roll Back to {version}").replace("{version}", version),
                None => gettext("Roll Back to the Previous Version"),
            },
            "win.roll-back-system",
            "edit-undo-symbolic",
        ),
    };
    adw::ButtonRow::builder()
        .title(title)
        .use_markup(false)
        .start_icon_name(icon)
        .action_name(action)
        .build()
}

/// A row saying what runs, with a spinner.
fn busy_row(busy: Busy) -> adw::ActionRow {
    let title = match busy {
        Busy::RollingBack => gettext("Rolling Back…"),
        Busy::Restarting => gettext("Restarting…"),
    };
    let row = adw::ActionRow::builder().title(title).build();
    row.add_suffix(&adw::Spinner::new());
    row
}
