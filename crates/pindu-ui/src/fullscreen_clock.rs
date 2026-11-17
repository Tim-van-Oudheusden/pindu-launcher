//! Fullscreen mode's clocks, in the top bar and in Quick Access's header: the time as the
//! desktop's `clock-format` setting shows it, read through the settings portal. Each is a
//! `Clock`, which ticks while it runs.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::clock::{self, ClockFormat};

use crate::window::PinduWindow;

const PORTAL: &str = "org.freedesktop.portal.Desktop";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const SETTINGS: &str = "org.freedesktop.portal.Settings";
const INTERFACE: &str = "org.gnome.desktop.interface";
const CLOCK_FORMAT: &str = "clock-format";

/// A label that shows the time of day in the desktop's clock format, follows the format as
/// it changes, and moves on each minute while it runs.
#[derive(Debug, Clone)]
pub(crate) struct Clock(Rc<ClockInner>);

#[derive(Debug)]
struct ClockInner {
    label: gtk::Label,
    format: Cell<ClockFormat>,
    /// The next tick while the clock runs.
    tick: RefCell<Option<glib::SourceId>>,
    /// The portal's notice of format changes.
    format_changes: RefCell<Option<gio::SignalSubscription>>,
}

impl Clock {
    /// A clock shown on `label`, following the clock format; it runs once told to.
    pub(crate) fn new(label: &gtk::Label) -> Self {
        let clock = Self(Rc::new(ClockInner {
            label: label.clone(),
            format: Cell::new(ClockFormat::default()),
            tick: RefCell::new(None),
            format_changes: RefCell::new(None),
        }));
        let weak = Rc::downgrade(&clock.0);
        glib::spawn_future_local(async move { follow_clock_format(weak).await });
        clock
    }

    /// Runs the clock, or stops it.
    pub(crate) fn run(&self, running: bool) {
        if running {
            if self.0.tick.borrow().is_none() {
                tick(&self.0);
            }
        } else if let Some(tick) = self.0.tick.take() {
            tick.remove();
        }
    }

    /// The time of day `time` as the clock shows it, in the desktop's clock format.
    pub(crate) fn text_at(&self, time: &glib::DateTime) -> String {
        clock_text_at(time, self.0.format.get())
    }
}

impl PinduWindow {
    /// The top bar's clock, following the clock format.
    pub(crate) fn setup_clock(&self) {
        let imp = self.imp();
        let _ = imp.top_bar_clock.set(Clock::new(&imp.clock));
        self.update_clock();
    }

    /// Runs the top bar's clock while the top bar shows, and stops it otherwise.
    pub(crate) fn update_clock(&self) {
        let imp = self.imp();
        if let Some(clock) = imp.top_bar_clock.get() {
            clock.run(imp.top_bar.is_visible());
        }
    }
}

/// Shows the time and comes back when the next minute starts.
fn tick(clock: &Rc<ClockInner>) {
    show_time(clock);
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    let delay = clock::until_next_minute(
        u32::try_from(now.second()).unwrap_or_default(),
        u32::try_from(now.microsecond()).unwrap_or_default(),
    );
    let weak = Rc::downgrade(clock);
    let source = glib::timeout_add_local_once(delay, move || {
        if let Some(clock) = weak.upgrade() {
            // This timeout is done: forget it rather than remove it.
            clock.tick.take();
            tick(&clock);
        }
    });
    clock.tick.replace(Some(source));
}

fn show_time(clock: &ClockInner) {
    let Ok(now) = glib::DateTime::now_local() else {
        return;
    };
    clock
        .label
        .set_label(&clock_text_at(&now, clock.format.get()));
}

/// The time of day `time` in `format`.
fn clock_text_at(time: &glib::DateTime, format: ClockFormat) -> String {
    let (am, pm) = (period_name(0), period_name(12));
    clock::clock_text(
        u32::try_from(time.hour()).unwrap_or_default(),
        u32::try_from(time.minute()).unwrap_or_default(),
        format,
        [&am, &pm],
    )
}

/// Reads `clock-format` through the settings portal, which the sandbox lets every app use,
/// and follows its changes. Without the portal the clock shows 24 hours.
async fn follow_clock_format(clock: Weak<ClockInner>) {
    let Ok(bus) = gio::bus_get_future(gio::BusType::Session).await else {
        return;
    };
    let changed = clock.clone();
    let subscription = bus.subscribe_to_signal(
        Some(PORTAL),
        Some(SETTINGS),
        Some("SettingChanged"),
        Some(PORTAL_PATH),
        Some(INTERFACE),
        gio::DBusSignalFlags::NONE,
        move |signal| {
            let parameters = signal.parameters;
            if parameters.n_children() == 3
                && parameters.child_value(1).str() == Some(CLOCK_FORMAT)
                && let Some(clock) = changed.upgrade()
            {
                set_clock_format(&clock, &parameters.child_value(2));
            }
        },
    );
    let Some(inner) = clock.upgrade() else {
        return;
    };
    inner.format_changes.replace(Some(subscription));
    drop(inner);
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
        Ok(reply) if reply.n_children() == 1 => {
            if let Some(clock) = clock.upgrade() {
                set_clock_format(&clock, &reply.child_value(0));
            }
        }
        Ok(_) => {}
        Err(error) => glib::g_debug!("pindu", "No clock format from the portal: {error}"),
    }
}

/// Takes the clock format from a `clock-format` value, a string in a variant.
fn set_clock_format(clock: &ClockInner, value: &glib::Variant) {
    let value = value.as_variant().unwrap_or_else(|| value.clone());
    clock
        .format
        .set(ClockFormat::from_setting(value.str().unwrap_or_default()));
    show_time(clock);
}

/// The locale's name for the half of the day `hour` falls in ("AM" or "PM" in English).
fn period_name(hour: i32) -> String {
    glib::DateTime::from_local(2000, 1, 1, hour, 0, 0.0)
        .and_then(|time| time.format("%p"))
        .map(String::from)
        .unwrap_or_default()
}
