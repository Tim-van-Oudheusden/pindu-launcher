//! Fullscreen mode's top bar, which a design turns on (`ThemeLayout::top_bar`): tabs for
//! Home and the library, the shown page's tab checked, and a clock (see
//! `fullscreen_clock`). It shows over Home and the library, whose backdrop runs on under
//! it on Home; the controller reaches the tabs by moving up from the page.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds `win.show-page`, whose state is the tag of the shown page and which the tabs
    /// show their page with.
    pub(crate) fn setup_top_bar(&self) {
        let imp = self.imp();
        let show_page = gio::ActionEntry::builder("show-page")
            .parameter_type(Some(glib::VariantTy::STRING))
            .state(String::new().to_variant())
            .activate(
                |window: &Self, _, target| match target.and_then(|target| target.str()) {
                    Some("home") => window.show_home(),
                    Some("library") => window.show_library(),
                    _ => {}
                },
            )
            .build();
        self.add_action_entries([show_page]);
        imp.navigation_view
            .connect_visible_page_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_fullscreen_top_bar()
            ));
    }

    /// Shows the bar while fullscreen mode shows Home or the library and the design in
    /// force wants it, checks the shown page's tab, and runs the clock while it shows.
    pub(crate) fn update_fullscreen_top_bar(&self) {
        let imp = self.imp();
        let page = imp.navigation_view.visible_page();
        let tag = page
            .as_ref()
            .and_then(|page| page.tag())
            .unwrap_or_default();
        if let Some(action) = self
            .lookup_action("show-page")
            .and_downcast::<gio::SimpleAction>()
        {
            action.set_state(&tag.to_variant());
        }
        let on_home = page.as_ref() == Some(imp.home_page.upcast_ref());
        let on_library = page.as_ref() == Some(&*imp.library_page);
        let wanted = self
            .application()
            .and_downcast::<PinduApplication>()
            .is_some_and(|application| application.style_service().layout().top_bar);
        let show = self.is_fullscreen() && wanted && (on_home || on_library);
        imp.top_bar_view.set_extend_content_to_top_edge(on_home);
        imp.top_bar.set_visible(show);
        self.update_clock();
    }
}
