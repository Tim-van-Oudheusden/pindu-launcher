//! Quick Access's Sources: the Sources entry's level, which lists each imported source with
//! its icon, whether it was found on this computer and its Import switch, the same setting
//! Preferences, Sources changes (see `pindu_core::sources::sync::imported_source_states`).
//! A switch changes the setting through the sync service, which then syncs, so a source
//! turned off drops its games and one turned on brings them back, as on the desktop. The
//! states are read again as the level opens and after each sync, into the rows already
//! there, so the source a controller just switched keeps the focus and stays in view. A
//! source's folder and its sidebar row stay the desktop's. Below it, the copy order: the
//! source order a game's copies follow, with Move Up and Move Down on each source, the same
//! setting as Preferences' (see `crate::sources::bind_source_order`).

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::{Source, SourceState};
use pindu_core::panels::QuickAccessLevel;
use pindu_core::sources::sync::imported_source_states;

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;
use crate::sources::{bind_source_order, source_icon, source_name};

impl PinduQuickAccess {
    /// Reads the sources again after each sync while the level shows: a sync finds a
    /// source's folder, or no longer does. The copy order follows its setting.
    pub(crate) fn setup_sources(&self, application: &PinduApplication) {
        bind_source_order(&self.imp().source_order, self.settings());
        let quick_access = self.downgrade();
        application.sync_service().connect_finished(move |_| {
            if let Some(quick_access) = quick_access.upgrade()
                && quick_access.level() == Some(QuickAccessLevel::Sources)
            {
                quick_access.show_sources();
            }
        });
    }

    /// Shows the stored states in the Sources list. Focus on a source or on the copy order
    /// stays where it is; otherwise, as the level has just opened, the first source takes
    /// it.
    pub(crate) fn show_sources(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let sync = application.sync_service().clone();
        let quick_access = self.downgrade();
        glib::spawn_future_local(async move {
            let states = sync.source_states().await;
            let Some(quick_access) = quick_access.upgrade() else {
                return;
            };
            if quick_access.level() != Some(QuickAccessLevel::Sources) {
                return;
            }
            quick_access.fill_sources(&application, &imported_source_states(&states));
            let imp = quick_access.imp();
            let list = imp.sources.get();
            let kept = quick_access.focus_widget().is_some_and(|focus| {
                focus.is_ancestor(&list) || focus.is_ancestor(&*imp.source_order)
            });
            if !kept && let Some(row) = list.row_at_index(0) {
                row.grab_focus();
            }
        });
    }

    /// Sets each source's row to its stored state. The rows are built only when the sources
    /// listed change, which they do not after the first fill: building them again would take
    /// the focus off the source a switch just changed and scroll the list back to its top,
    /// as GTK scrolls to a row before it has a place.
    fn fill_sources(&self, application: &PinduApplication, states: &[SourceState]) {
        let imp = self.imp();
        let mut rows = imp.source_rows.borrow_mut();
        let same = rows
            .iter()
            .map(|(source, _, _)| *source)
            .eq(states.iter().map(|state| state.source));
        if !same {
            let list = imp.sources.get();
            list.remove_all();
            rows.clear();
            for state in states {
                let (row, handler) = source_row(application, state.source);
                list.append(&row);
                rows.push((state.source, row, handler));
            }
        }
        for ((_, row, handler), state) in rows.iter().zip(states) {
            row.set_subtitle(&if state.resolved_location.is_some() {
                gettext("Found on this computer")
            } else {
                gettext("Not found on this computer")
            });
            // The stored state is no change to make.
            row.block_signal(handler);
            row.set_active(state.enabled);
            row.unblock_signal(handler);
        }
    }
}

/// A source's row: its icon and name, and its Import switch, which turns its import on or
/// off and syncs. Whether it was found and whether it is on are `fill_sources`'.
fn source_row(
    application: &PinduApplication,
    source: Source,
) -> (adw::SwitchRow, glib::SignalHandlerId) {
    let row = adw::SwitchRow::builder().title(source_name(source)).build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(source_icon(source))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let sync = application.sync_service().clone();
    let handler =
        row.connect_active_notify(move |row| sync.set_source_enabled(source, row.is_active()));
    (row, handler)
}
