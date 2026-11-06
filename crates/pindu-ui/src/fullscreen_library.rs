//! Fullscreen mode's library: the library page with tabs (All, Installed, Favourites and
//! one per collection) in place of the sidebar and the filter row, over the same grid and
//! models. The shoulder buttons switch tabs, each tab coming back on the cover the
//! controller was last on there, and the triggers cycle the sort order. Quick Access's
//! Filters narrow every tab, apart from the desktop window's filter row, with a Filtered
//! chip beside the tabs meanwhile.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::library::{self, Filters, SortMode, View};

use crate::application::PinduApplication;
use crate::window::{PinduWindow, sort_mode_from_name, sort_mode_name};

/// A toggle of fullscreen mode's filters: the part of [`Filters`] it turns on.
type FilterField = fn(&mut Filters) -> &mut bool;

impl PinduWindow {
    /// Keeps the tabs in step with the collections and the sort label with the order.
    pub(crate) fn setup_fullscreen_library(&self) {
        self.library().connect_collections_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_tabs()
        ));
        self.update_tabs();
        if let Some(action) = self
            .lookup_action("sort-mode")
            .and_downcast::<gio::SimpleAction>()
        {
            action.connect_state_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_sort_label()
            ));
        }
        self.update_sort_label();
        self.setup_fullscreen_filters();
    }

    /// Quick Access's Filters: each switch turns its filter on or off at once, and Clear
    /// Filters turns them all off.
    fn setup_fullscreen_filters(&self) {
        for (row, field) in self.filter_rows() {
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |row| {
                    let mut filters = window.fullscreen_filters();
                    *field(&mut filters) = row.is_active();
                    window.set_fullscreen_filters(filters);
                }
            ));
        }
        let clear = gio::ActionEntry::builder("clear-fullscreen-filters")
            .activate(|window: &Self, _, _| window.set_fullscreen_filters(Filters::default()))
            .build();
        self.add_action_entries([clear]);
    }

    /// The switches of Quick Access's Filters, each with the filter it turns on.
    fn filter_rows(&self) -> [(adw::SwitchRow, FilterField); 5] {
        let imp = self.imp();
        [
            (imp.quick_access_installed_row.get(), |filters| {
                &mut filters.installed
            }),
            (imp.quick_access_not_installed_row.get(), |filters| {
                &mut filters.not_installed
            }),
            (imp.quick_access_windows_row.get(), |filters| {
                &mut filters.windows
            }),
            (imp.quick_access_native_row.get(), |filters| {
                &mut filters.native
            }),
            (imp.quick_access_hidden_row.get(), |filters| {
                &mut filters.hidden
            }),
        ]
    }

    /// Fullscreen mode's filters, which last until the app quits.
    pub(crate) fn fullscreen_filters(&self) -> Filters {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.fullscreen_filters())
            .unwrap_or_default()
    }

    /// Applies `filters` to fullscreen mode's library and shows them on the switches.
    fn set_fullscreen_filters(&self, filters: Filters) {
        if filters == self.fullscreen_filters() {
            return;
        }
        if let Some(application) = self.application().and_downcast::<PinduApplication>() {
            application.set_fullscreen_filters(filters);
        }
        self.apply_filters();
        self.show_fullscreen_filters();
    }

    /// Sets Quick Access's switches to fullscreen mode's filters: each switch then finds
    /// its filter as it says, so nothing applies again.
    pub(crate) fn show_fullscreen_filters(&self) {
        let mut filters = self.fullscreen_filters();
        for (row, field) in self.filter_rows() {
            row.set_active(*field(&mut filters));
        }
    }

    /// Shows the tabs instead of the filter row in fullscreen mode, on a view that has a
    /// tab; the desktop window gets its filters back.
    pub(crate) fn show_fullscreen_library(&self, fullscreen: bool) {
        let imp = self.imp();
        imp.library_tabs.set_visible(fullscreen);
        imp.filter_row.set_visible(!fullscreen);
        if fullscreen && !self.tab_views().contains(&self.library().view()) {
            self.set_shown_view(View::All);
        }
        self.apply_filters();
    }

    /// One tab per view: All, Installed, Favourites, then each collection by its name.
    fn update_tabs(&self) {
        let imp = self.imp();
        while let Some(tab) = imp.tab_row.first_child() {
            imp.tab_row.remove(&tab);
        }
        let collections = self.library().collections();
        let tabs: Vec<(View, gtk::ToggleButton)> =
            library::fullscreen_tabs(collections.iter().map(|collection| collection.id))
                .into_iter()
                .map(|view| {
                    let label = match view {
                        View::All => gettext("All"),
                        View::Installed => gettext("Installed"),
                        View::Favourites => gettext("Favourites"),
                        View::Collection(id) => collections
                            .iter()
                            .find(|collection| collection.id == id)
                            .map(|collection| collection.name.clone())
                            .unwrap_or_default(),
                        _ => String::new(),
                    };
                    // The bar takes no focus, so the controller stays in the grid: the
                    // shoulder buttons switch tabs.
                    let tab = gtk::ToggleButton::builder()
                        .label(&label)
                        .use_underline(false)
                        .focus_on_click(false)
                        .css_classes(["flat", "library-tab"])
                        .build();
                    tab.connect_clicked(glib::clone!(
                        #[weak(rename_to = window)]
                        self,
                        move |_| window.show_tab(view)
                    ));
                    imp.tab_row.append(&tab);
                    (view, tab)
                })
                .collect();
        imp.tabs.replace(tabs);
        self.select_tab();
    }

    fn tab_views(&self) -> Vec<View> {
        self.imp()
            .tabs
            .borrow()
            .iter()
            .map(|(view, _)| *view)
            .collect()
    }

    /// Marks the tab of the shown view, if it has one, and scrolls it into view.
    pub(crate) fn select_tab(&self) {
        let imp = self.imp();
        let shown = self.library().view();
        let mut selected = None;
        for (view, tab) in imp.tabs.borrow().iter() {
            tab.set_active(*view == shown);
            if *view == shown {
                selected = Some(tab.clone());
            }
        }
        if let Some(tab) = selected {
            // Once the row has its size, after a rebuild.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    let imp = window.imp();
                    let bounds = tab.compute_bounds(&*imp.tab_row);
                    if let Some(bounds) = bounds {
                        let x = f64::from(bounds.x());
                        imp.tab_scroller
                            .hadjustment()
                            .clamp_page(x, x + f64::from(bounds.width()));
                    }
                }
            ));
        }
    }

    /// LB and RB: the tab before or after the shown one, on the cover the controller was
    /// last on there.
    pub(crate) fn step_tab(&self, forward: bool) -> bool {
        let tabs = self.tab_views();
        match library::step_tab(&tabs, self.library().view(), forward) {
            Some(view) => {
                self.show_tab(view);
                true
            }
            None => false,
        }
    }

    pub(crate) fn show_tab(&self, view: View) {
        self.set_shown_view(view);
        self.focus_page();
    }

    /// LT and RT: the sort order before or after the shown one, as the sort menu lists
    /// them, starting again from the first cover once the grid is sorted.
    pub(crate) fn cycle_sort(&self, forward: bool) {
        let Some(action) = self.lookup_action("sort-mode") else {
            return;
        };
        let mode = action
            .state()
            .and_then(|state| state.str().and_then(sort_mode_from_name))
            .unwrap_or_default()
            .cycled(forward);
        action.change_state(&sort_mode_name(mode).to_variant());
        if self.library().sort_model().pending() == 0 {
            self.focus_page();
        } else {
            self.imp().focus_first_after_sort.set(true);
        }
    }

    /// Whether the library page shows its tabs: in fullscreen mode, with nothing over it.
    pub(crate) fn shows_tabs(&self) -> bool {
        let imp = self.imp();
        self.is_fullscreen()
            && self.visible_dialog().is_none()
            && imp.navigation_view.visible_page().as_ref() == Some(&*imp.library_page)
    }

    fn update_sort_label(&self) {
        let mode = self
            .lookup_action("sort-mode")
            .and_then(|action| action.state())
            .and_then(|state| state.str().and_then(sort_mode_from_name))
            .unwrap_or_default();
        self.imp().sort_label.set_label(&sort_mode_label(mode));
    }
}

/// What the tabs call a sort order: the sort menu's item, without its mnemonic.
fn sort_mode_label(mode: SortMode) -> String {
    match mode {
        SortMode::LastPlayed => gettext("Last Played"),
        SortMode::NameAscending => gettext("Name A–Z"),
        SortMode::NameDescending => gettext("Name Z–A"),
        SortMode::RecentlyAdded => gettext("Recently Added"),
        SortMode::Playtime => gettext("Playtime"),
    }
}
