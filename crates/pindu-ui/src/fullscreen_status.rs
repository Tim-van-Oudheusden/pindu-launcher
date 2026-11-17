//! Quick Access's status header: the clock (see `fullscreen_clock`), the machine's battery
//! with its charge and charging state, from the power service and hidden without one, and
//! "Offline" while `gio::NetworkMonitor` reports no network.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::power::{Batteries, Charge};

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;

impl PinduQuickAccess {
    /// Follows the batteries and the network. The batteries are read again whenever the
    /// panel opens.
    pub(crate) fn setup_status(&self, application: &PinduApplication) {
        let power = application.power_service();
        power.connect_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |batteries| quick_access.show_battery(batteries)
        ));
        self.show_battery(&power.batteries());
        gio::NetworkMonitor::default()
            .bind_property("network-available", &*self.imp().offline, "visible")
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
        imp.battery.set_visible(shown.is_some());
        let Some((capacity, icon_name, charge)) = shown else {
            return;
        };
        imp.battery_icon.set_icon_name(Some(&icon_name));
        // Translators: a battery's charge; {percent} is a number from 0 to 100.
        let level = gettext("{percent} %").replace("{percent}", &capacity.to_string());
        imp.battery_level.set_label(&level);
        let state = match charge {
            Charge::Charging => Some(gettext("Charging")),
            Charge::Discharging => Some(gettext("Discharging")),
            Charge::NotCharging => Some(gettext("Plugged in, not charging")),
            Charge::Full => Some(gettext("Fully charged")),
            Charge::Unknown => None,
        };
        imp.battery.set_tooltip_text(state.as_deref());
    }
}
