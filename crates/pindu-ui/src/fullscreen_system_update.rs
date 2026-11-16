//! Quick Access's System Update, on Pindu OS only (`pindu_core::system::update`): its entry,
//! subtitled with the installed version or that an update is ready, and its level with the
//! installed version, the update waiting for the next start, and Restart to Update, Roll
//! Back (asked first) or Restart, as bootc's status allows. The window shows
//! [`SystemUpdateService`]'s state and calls it; it never reads the status file itself.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::update::{Busy, UpdateAction, UpdateState};

use crate::application::PinduApplication;
use crate::system_update_service::SystemUpdateService;
use crate::window::PinduWindow;

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
        self.fill_system_update(&service.state());
        service.refresh();
    }

    /// Where focus goes on the level: the first action, or else the installed version.
    pub(crate) fn system_update_focus(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        let actions = &imp.quick_access_system_actions;
        let list = if actions.is_visible() {
            actions
        } else {
            &imp.quick_access_system_versions
        };
        list.row_at_index(0).map(|row| row.upcast())
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
        let had_focus = self.focus_in_system_update();
        self.fill_system_update(state);
        if had_focus {
            self.focus_panel(None);
        }
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    fn focus_in_system_update(&self) -> bool {
        let imp = self.imp();
        gtk::prelude::GtkWindowExt::focus(self).is_some_and(|focus| {
            focus.is_ancestor(&*imp.quick_access_system_versions)
                || focus.is_ancestor(&*imp.quick_access_system_actions)
        })
    }

    /// The level's rows for `state`: the versions, the note on a queued rollback, and the
    /// actions, or what runs.
    fn fill_system_update(&self, state: &UpdateState) {
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
                        if open && !window.focus_in_system_update() {
                            window.focus_panel(None);
                        }
                    }
                ));
            }
        ));
        dialog.present(Some(self));
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
