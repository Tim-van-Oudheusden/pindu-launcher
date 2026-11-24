//! Quick Access's Sources: the Sources entry's level, which lists each imported source with
//! its icon, whether it was found on this computer and its Import switch, the same setting
//! Preferences, Sources changes (see `pindu_core::sources::sync::imported_source_states`),
//! then each external importer with where it runs and its own switch, off until the user
//! turns it on. A switch changes the setting through the sync service, which then syncs, so
//! a source turned off drops its games and one turned on brings them back, as on the
//! desktop. The states are read again as the level opens and after each sync, into the rows
//! already there, so the row a controller just switched keeps the focus and stays in view. A
//! source's folder and its sidebar row stay the desktop's. Below it, the copy order: the
//! source order a game's copies follow, with Move Up and Move Down on each source, the same
//! setting as Preferences' (see `crate::sources::bind_source_order`).

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::{Source, SourceFound, SourceState};
use pindu_core::panels::QuickAccessLevel;
use pindu_core::sources::external::listing::Listed;
use pindu_core::sources::sync::imported_source_states;

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;
use crate::sources::{bind_source_order, helper_summary, source_icon, source_name};

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

    /// Shows the stored states in the Sources list. Focus on a row or on the copy order
    /// stays where it is. Otherwise, as the level has just opened, the first row takes it,
    /// or the row at the focused one's place when the rows were built again.
    pub(crate) fn show_sources(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let sync = application.sync_service().clone();
        let quick_access = self.downgrade();
        glib::spawn_future_local(async move {
            let (states, helpers) = sync.sources().await;
            let Some(quick_access) = quick_access.upgrade() else {
                return;
            };
            if quick_access.level() != Some(QuickAccessLevel::Sources) {
                return;
            }
            let imp = quick_access.imp();
            let list = imp.sources.get();
            // Before its first fill the list is empty, so the copy order may have taken the
            // focus as the level opened.
            let filled = !imp.source_rows.borrow().is_empty();
            let focused = quick_access
                .focus_widget()
                .filter(|focus| focus.is_ancestor(&list))
                .and_then(|focus| {
                    focus
                        .ancestor(gtk::ListBoxRow::static_type())
                        .and_downcast::<gtk::ListBoxRow>()
                })
                .map_or(0, |row| row.index());
            quick_access.fill_sources(&application, &imported_source_states(&states), &helpers);
            let kept = quick_access.focus_widget().is_some_and(|focus| {
                focus.is_ancestor(&list) || (filled && focus.is_ancestor(&*imp.source_order))
            });
            if !kept && let Some(row) = list.row_at_index(focused).or_else(|| list.row_at_index(0))
            {
                row.grab_focus();
            }
        });
    }

    /// Sets each source's and each external importer's row to its stored state. The rows
    /// are built again only when the rows listed change, as an importer comes or goes:
    /// building them again would take the focus off the row a switch just changed and
    /// scroll the list back to its top, as GTK scrolls to a row before it has a place.
    fn fill_sources(
        &self,
        application: &PinduApplication,
        states: &[SourceState],
        helpers: &[Listed],
    ) {
        let imp = self.imp();
        let mut rows = imp.source_rows.borrow_mut();
        let keys = states
            .iter()
            .map(|state| SourceKey::Source(state.source))
            .chain(
                helpers
                    .iter()
                    .map(|helper| SourceKey::Helper(helper.id.clone())),
            );
        if !rows.iter().map(|(key, _, _)| key).cloned().eq(keys) {
            let list = imp.sources.get();
            list.remove_all();
            rows.clear();
            for state in states {
                let (row, handler) = source_row(application, state.source);
                list.append(&row);
                rows.push((SourceKey::Source(state.source), row, handler));
            }
            for helper in helpers {
                let (row, handler) = helper_row(application, helper);
                list.append(&row);
                rows.push((SourceKey::Helper(helper.id.clone()), row, handler));
            }
        }
        let (source_rows, helper_rows) = rows.split_at(states.len());
        for ((_, row, handler), state) in source_rows.iter().zip(states) {
            row.set_subtitle(&if state.found() == SourceFound::NotFound {
                gettext("Not found on this computer")
            } else {
                gettext("Found on this computer")
            });
            show_active(row, handler, state.enabled);
        }
        for ((_, row, handler), helper) in helper_rows.iter().zip(helpers) {
            row.set_subtitle(&helper_summary(helper));
            show_active(row, handler, helper.enabled);
        }
    }
}

/// What a Sources row stands for: a source Pindu imports itself, or an external importer
/// by its ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SourceKey {
    /// A built-in source.
    Source(Source),
    /// An external importer.
    Helper(String),
}

/// Sets a row's switch to the stored state with its handler blocked: the stored state is no
/// change to make.
fn show_active(row: &adw::SwitchRow, handler: &glib::SignalHandlerId, active: bool) {
    row.block_signal(handler);
    row.set_active(active);
    row.unblock_signal(handler);
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

/// An external importer's row: its name and icon, and its switch, which turns it on or off
/// and syncs. Whether it is experimental, where it runs, whether its source is on this
/// computer and whether it is on are `fill_sources`'.
fn helper_row(
    application: &PinduApplication,
    helper: &Listed,
) -> (adw::SwitchRow, glib::SignalHandlerId) {
    let row = adw::SwitchRow::builder()
        .title(&helper.name)
        .title_lines(1)
        .use_markup(false)
        .build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(source_icon(Source::External))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let sync = application.sync_service().clone();
    let id = helper.id.clone();
    let handler =
        row.connect_active_notify(move |row| sync.set_helper_enabled(id.clone(), row.is_active()));
    (row, handler)
}
