//! Quick Access's status header: the clock (see `fullscreen_clock`), the machine's battery
//! with its charge and charging state, from the power service and hidden without one, and
//! "Offline" while `gio::NetworkMonitor` reports no network.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::power::{Batteries, Charge};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Follows the batteries and the network, reads the batteries again whenever Quick
    /// Access opens, and runs the clock while it shows.
    pub(crate) fn setup_quick_access_status(&self, application: &PinduApplication) {
        let imp = self.imp();
        let power = application.power_service().clone();
        power.connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |batteries| window.show_battery(batteries)
        ));
        self.show_battery(&power.batteries());
        imp.quick_access.connect_show_sidebar_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |quick_access| {
                if quick_access.shows_sidebar() {
                    power.refresh();
                }
                window.update_clock();
            }
        ));
        gio::NetworkMonitor::default()
            .bind_property("network-available", &*imp.quick_access_offline, "visible")
            .invert_boolean()
            .sync_create()
            .build();
    }

    /// Shows the system battery's charge and charging state, or hides the battery when
    /// there is none or it reports no charge in percent.
    fn show_battery(&self, batteries: &Batteries) {
        let imp = self.imp();
        let shown = batteries
            .system
            .as_ref()
            .and_then(|battery| Some((battery.capacity?, battery.icon_name()?, battery.charge)));
        imp.quick_access_battery.set_visible(shown.is_some());
        let Some((capacity, icon_name, charge)) = shown else {
            return;
        };
        imp.quick_access_battery_icon
            .set_icon_name(Some(&icon_name));
        // Translators: a battery's charge; {percent} is a number from 0 to 100.
        let level = gettext("{percent} %").replace("{percent}", &capacity.to_string());
        imp.quick_access_battery_level.set_label(&level);
        let state = match charge {
            Charge::Charging => Some(gettext("Charging")),
            Charge::Discharging => Some(gettext("Discharging")),
            Charge::NotCharging => Some(gettext("Plugged in, not charging")),
            Charge::Full => Some(gettext("Fully charged")),
            Charge::Unknown => None,
        };
        imp.quick_access_battery.set_tooltip_text(state.as_deref());
    }
}
