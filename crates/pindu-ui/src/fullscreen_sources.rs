//! Quick Access's Sources: the Sources entry's level, which lists each imported source with
//! its icon, whether it was found on this computer and its Import switch, the same setting
//! Preferences, Sources changes (see `pindu_core::sources::sync::imported_source_states`),
//! then each external importer with where it runs and its own switch, off until the user
//! turns it on. A switch changes the setting through the sync service, which then syncs, so
//! a source turned off drops its games and one turned on brings them back, as on the
//! desktop. The list is read again as the level opens and after each sync. A source's
//! folder and its sidebar row stay the desktop's. Below it, the copy order: the source order
//! a game's copies follow, with Move Up and Move Down on each source, the same setting as
//! Preferences' (see `crate::sources::bind_source_order`).

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::{Source, SourceState};
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

    /// Fills the Sources list from the stored states, keeping focus on the source it was on,
    /// or giving it to the first source when the level has just opened.
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
            let focus = quick_access.focus_widget();
            // Focus on the copy order stays there.
            let ordering = focus
                .as_ref()
                .is_some_and(|focus| focus.is_ancestor(&*imp.source_order));
            let focused = focus
                .filter(|focus| focus.is_ancestor(&list))
                .and_then(|focus| {
                    focus
                        .ancestor(gtk::ListBoxRow::static_type())
                        .and_downcast::<gtk::ListBoxRow>()
                })
                .map_or(0, |row| row.index());
            list.remove_all();
            for state in imported_source_states(&states) {
                list.append(&source_row(&application, &state));
            }
            for helper in &helpers {
                list.append(&helper_row(&application, helper));
            }
            if !ordering && let Some(row) = list.row_at_index(focused) {
                row.grab_focus();
            }
        });
    }
}

/// A source's row: its icon and name, whether it was found on this computer, and its Import
/// switch, which turns its import on or off and syncs.
fn source_row(application: &PinduApplication, state: &SourceState) -> adw::SwitchRow {
    let source = state.source;
    let found = if state.resolved_location.is_some() {
        gettext("Found on this computer")
    } else {
        gettext("Not found on this computer")
    };
    let row = adw::SwitchRow::builder()
        .title(source_name(source))
        .subtitle(found)
        .active(state.enabled)
        .build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(source_icon(source))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let sync = application.sync_service().clone();
    row.connect_active_notify(move |row| sync.set_source_enabled(source, row.is_active()));
    row
}

/// An external importer's row: its name, whether it is experimental, where it runs and
/// whether its source is on this computer, and its switch, which turns it on or off and
/// syncs.
fn helper_row(application: &PinduApplication, helper: &Listed) -> adw::SwitchRow {
    let row = adw::SwitchRow::builder()
        .title(&helper.name)
        .title_lines(1)
        .subtitle(helper_summary(helper))
        .use_markup(false)
        .active(helper.enabled)
        .build();
    row.add_prefix(
        &gtk::Image::builder()
            .icon_name(source_icon(Source::External))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let sync = application.sync_service().clone();
    let id = helper.id.clone();
    row.connect_active_notify(move |row| sync.set_helper_enabled(id.clone(), row.is_active()));
    row
}
