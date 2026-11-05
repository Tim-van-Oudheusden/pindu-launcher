//! Fullscreen mode's top bar, which a design turns on (`ThemeLayout::top_bar`): tabs for
//! Home and the library, the shown page's tab checked, and a clock that follows the
//! desktop's `clock-format` setting, read through the settings portal. It shows over Home
//! and the library, whose backdrop runs on under it on Home; the controller reaches the
//! tabs by moving up from the page.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::clock::{self, ClockFormat};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const INTERFACE: &str = "org.gnome.desktop.interface";
const CLOCK_FORMAT: &str = "clock-format";

impl PinduWindow {
    /// Adds `win.show-page`, whose state is the tag of the shown page and which the tabs
    /// show their page with, and follows the clock format.
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
        glib::spawn_future_local(glib::clone!(
            #[weak(rename_to = window)]
            self,
            async move { window.follow_clock_format().await }
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
        if show {
            if imp.clock_tick.borrow().is_none() {
                self.tick_clock();
            }
        } else if let Some(tick) = imp.clock_tick.take() {
            tick.remove();
        }
    }

    /// Shows the time and comes back when the next minute starts.
    fn tick_clock(&self) {
        self.show_time();
        let Ok(now) = glib::DateTime::now_local() else {
            return;
        };
        let delay = clock::until_next_minute(
            u32::try_from(now.second()).unwrap_or_default(),
            u32::try_from(now.microsecond()).unwrap_or_default(),
        );
        let tick = glib::timeout_add_local_once(
            delay,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // This timeout is done: forget it rather than remove it.
                    window.imp().clock_tick.take();
                    window.tick_clock();
                }
            ),
        );
        self.imp().clock_tick.replace(Some(tick));
    }

    fn show_time(&self) {
        let Ok(now) = glib::DateTime::now_local() else {
            return;
        };
        let (am, pm) = (period_name(0), period_name(12));
        let text = clock::clock_text(
            u32::try_from(now.hour()).unwrap_or_default(),
            u32::try_from(now.minute()).unwrap_or_default(),
            self.imp().clock_format.get(),
            [&am, &pm],
        );
        self.imp().clock.set_label(&text);
    }

    /// Reads `clock-format` through the settings portal, which the sandbox lets every app
    /// use, and follows its changes. Without the portal the clock shows 24 hours.
    async fn follow_clock_format(&self) {
        let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
            return;
        };
        let subscription = bus.subscribe_to_signal(
            Some(PORTAL),
            Some(SETTINGS),
            Some("SettingChanged"),
            Some(PORTAL_PATH),
            Some(INTERFACE),
            gio::DBusSignalFlags::NONE,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |signal| {
                    let parameters = signal.parameters;
                    if parameters.n_children() == 3
                        && parameters.child_value(1).str() == Some(CLOCK_FORMAT)
                    {
                        window.set_clock_format(&parameters.child_value(2));
                    }
                }
            ),
        );
        self.imp().clock_format_changes.replace(Some(subscription));
        let reply = bus
            .call_future(
                Some(PORTAL),
                PORTAL_PATH,
                SETTINGS,
                "ReadOne",
                Some(&(INTERFACE, CLOCK_FORMAT).to_variant()),
                None,
                gio::DBusCallFlags::NONE,
                -1,
            )
            .await;
        match reply {
            Ok(reply) if reply.n_children() == 1 => self.set_clock_format(&reply.child_value(0)),
            Ok(_) => {}
            Err(error) => glib::g_debug!("pindu", "No clock format from the portal: {error}"),
        }
    }

    /// Takes the clock format from a `clock-format` value, a string in a variant.
    fn set_clock_format(&self, value: &glib::Variant) {
        let value = value.as_variant().unwrap_or_else(|| value.clone());
        let format = ClockFormat::from_setting(value.str().unwrap_or_default());
        self.imp().clock_format.set(format);
        self.show_time();
    }
}

/// The locale's name for the half of the day `hour` falls in ("AM" or "PM" in English).
fn period_name(hour: i32) -> String {
    glib::DateTime::from_local(2000, 1, 1, hour, 0, 0.0)
        .and_then(|time| time.format("%p"))
        .map(String::from)
        .unwrap_or_default()
}
