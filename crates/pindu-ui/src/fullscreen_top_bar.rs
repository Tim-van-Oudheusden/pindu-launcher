//! Fullscreen mode's top bar, which a design turns on (`ThemeLayout::top_bar`): tabs for
//! Home and the library, at its start or in its middle (`ThemeLayout::top_bar_tabs`), the
//! shown page's tab checked, and a clock (see `fullscreen_clock`). It shows over Home and
//! the library, whose backdrop runs on under it on Home but in the Explore layout; the
//! controller reaches the tabs by moving up from the page.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::themes::{HomeLayout, TopBarTabs};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds `win.show-page`, whose state is the tag of the shown page and which the tabs
    /// show their page with; and lets the Down arrow from the tabs come back to Home's
    /// rows where they last had focus, as the controller does.
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
        let down = gtk::EventControllerKey::new();
        down.set_propagation_phase(gtk::PropagationPhase::Capture);
        down.connect_key_pressed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, state| {
                let down = matches!(key, gtk::gdk::Key::Down | gtk::gdk::Key::KP_Down);
                let modified = state.intersects(
                    gtk::gdk::ModifierType::CONTROL_MASK
                        | gtk::gdk::ModifierType::SHIFT_MASK
                        | gtk::gdk::ModifierType::ALT_MASK,
                );
                if down && !modified && window.leave_top_bar_down() {
                    glib::Propagation::Stop
                } else {
                    glib::Propagation::Proceed
                }
            }
        ));
        imp.top_bar.add_controller(down);
    }

    /// Down from the top bar's tabs while Home shows: the one-screen layouts' rows take
    /// focus where they last had it (`HomePage::enter_from_top`). `false` where the move
    /// is GTK's to make.
    pub(crate) fn leave_top_bar_down(&self) -> bool {
        let imp = self.imp();
        let on_home =
            imp.navigation_view.visible_page().as_ref() == Some(imp.home_page.upcast_ref());
        on_home
            && imp
                .top_bar
                .state_flags()
                .contains(gtk::StateFlags::FOCUS_WITHIN)
            && imp.home_page.enter_from_top()
    }

    /// Shows the bar while fullscreen mode shows Home or the library and the design in
    /// force wants it, with its tabs where the design puts them, checks the shown page's
    /// tab, and runs the clock while it shows. Home's backdrop runs on under the bar,
    /// except in the Explore layout, whose rows start under it.
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
        let layout = self
            .application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.style_service().layout())
            .unwrap_or_default();
        let show = self.is_fullscreen() && layout.top_bar && (on_home || on_library);
        imp.top_bar_view
            .set_extend_content_to_top_edge(on_home && layout.home != HomeLayout::Explore);
        let tabs = &*imp.top_bar_tabs;
        let centred = layout.top_bar_tabs == TopBarTabs::Center;
        // The tabs leave the place they are in before they take the other.
        if centred && imp.top_bar.center_widget().is_none() {
            imp.top_bar.set_start_widget(gtk::Widget::NONE);
            imp.top_bar.set_center_widget(Some(tabs));
        } else if !centred && imp.top_bar.start_widget().is_none() {
            imp.top_bar.set_center_widget(gtk::Widget::NONE);
            imp.top_bar.set_start_widget(Some(tabs));
        }
        imp.top_bar.set_visible(show);
        self.update_clock();
    }
}
