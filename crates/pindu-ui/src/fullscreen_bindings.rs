//! Quick Access's Change Buttons: the last entry of its Controller section, and its level,
//! which lists the functions a controller button can be given
//! (`pindu_core::gamepad::REBINDABLE_INTENTS`), each with the glyph of its button on the
//! controller in use, then Reset to Defaults. Choosing a function shows "Press a button…"
//! and gives it the next button pressed, swapping with the function that had it; Guide, or
//! a few seconds without a press, leaves it as it was. The gamepad service waits for the
//! press and keeps the choice, which the hints and What Each Button Does follow at once.
//! Guide, the D-pad, the left stick and the keys stay as they are. Over a running game the
//! section leaves the entry out.

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::gamepad::{DeviceKind, Intent, Layout, REBINDABLE_INTENTS};
use pindu_core::hints;
use pindu_core::panels::QuickAccessLevel;

use crate::application::PinduApplication;
use crate::fullscreen_controls::action_label;
use crate::hint_bar::{GLYPH_SIZE, glyph_label};
use crate::quick_access::PinduQuickAccess;

impl PinduQuickAccess {
    /// Adds Reset to Defaults' action to `actions`, gives a chosen function the next button
    /// pressed, and lists the buttons again as the controller in use or the bindings
    /// change.
    pub(crate) fn setup_bindings(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        self.add_host_action(actions, "bindings-reset", None, |quick_access, _| {
            if let Some(application) = quick_access.application() {
                application.gamepad_service().reset_bindings();
            }
            // Without a controller no layout changes to list them again.
            quick_access.fill_bindings();
        });
        self.imp().bindings.connect_row_activated(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_, row| {
                let intent = usize::try_from(row.index())
                    .ok()
                    .and_then(|index| REBINDABLE_INTENTS.get(index).copied());
                if let Some(intent) = intent {
                    quick_access.rebind_function(intent);
                }
            }
        ));
        application
            .gamepad_service()
            .connect_active_layout_notify(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |_| {
                    if quick_access.level() == Some(QuickAccessLevel::Bindings) {
                        quick_access.fill_bindings();
                    }
                }
            ));
    }

    /// The level closes: a function still waiting for a button keeps its own.
    pub(crate) fn leave_bindings(&self) {
        if let Some(application) = self.application() {
            application.gamepad_service().cancel_rebind();
        }
    }

    /// Waits for the button to give `intent`, showing "Press a button…" on its row until
    /// one is pressed or the wait ends.
    fn rebind_function(&self, intent: Intent) {
        let Some(application) = self.application() else {
            return;
        };
        application.gamepad_service().rebind_next_press(
            intent,
            glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |_| quick_access.fill_bindings()
            ),
        );
        self.fill_bindings();
    }

    /// Lists each function with its button, or "Press a button…" on the one waiting for
    /// one, keeping focus on the row it was on.
    pub(crate) fn fill_bindings(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let service = application.gamepad_service();
        let list = self.imp().bindings.get();
        let focused = self
            .focus_widget()
            .and_then(|focus| focus.ancestor(gtk::ListBoxRow::static_type()))
            .and_downcast::<gtk::ListBoxRow>()
            .filter(|row| row.parent().as_ref() == Some(list.upcast_ref()))
            .map(|row| row.index());
        // Without a controller the buttons show as on a generic pad.
        let layout = service
            .active_layout()
            .unwrap_or_else(|| Layout::resolve(DeviceKind::Generic, service.controller_settings()));
        let waiting = service.rebinding();
        list.remove_all();
        for intent in REBINDABLE_INTENTS {
            list.append(&binding_row(intent, layout, waiting == Some(intent)));
        }
        if let Some(row) = focused.and_then(|index| list.row_at_index(index)) {
            row.grab_focus();
        }
    }
}

/// A function's row: what it does, then the glyph of its button on a pad with `layout`, or
/// "Press a button…" while it is `waiting` for one.
fn binding_row(intent: Intent, layout: Layout, waiting: bool) -> gtk::ListBoxRow {
    let action = action_label(intent);
    let content = gtk::Box::builder().spacing(16).build();
    content.append(
        &gtk::Label::builder()
            .label(&action)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    let button = if waiting {
        let press = gettext("Press a button…");
        content.append(
            &gtk::Label::builder()
                .label(&press)
                .css_classes(["dim-label"])
                .build(),
        );
        press
    } else {
        let button = layout.button(intent);
        content.append(
            &gtk::Image::builder()
                .icon_name(hints::glyph(layout.glyphs, button))
                .pixel_size(GLYPH_SIZE)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
        glyph_label(hints::glyph_name(layout.glyphs, button))
    };
    let row = gtk::ListBoxRow::builder().child(&content).build();
    // Translators: a row of Quick Access's Change Buttons as a screen reader reads it,
    // such as "Search: Y"; {action} is the function and {button} the controller button
    // that does it, or "Press a button…" while the row waits for one.
    let name = gettext("{action}: {button}")
        .replace("{action}", &action)
        .replace("{button}", &button);
    row.update_property(&[gtk::accessible::Property::Label(&name)]);
    row
}
