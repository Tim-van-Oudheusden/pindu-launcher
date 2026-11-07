//! `Monitor::devices`: `ManetteMonitorIter` is not introspectable, so gir skips it.

use glib::translate::*;

use crate::{Device, Monitor, ffi};

impl Monitor {
    /// The devices connected now.
    pub fn devices(&self) -> Vec<Device> {
        let mut devices = Vec::new();
        // SAFETY: `self` keeps the monitor alive while the iterator walks it. The iterator is
        // ours (transfer full) and freed once, after its last use. Each device it yields is
        // borrowed (transfer none), so `from_glib_none` takes a new reference to it.
        unsafe {
            let iter = ffi::manette_monitor_iterate(self.to_glib_none().0);
            let mut device = std::ptr::null_mut();
            while ffi::manette_monitor_iter_next(iter, &mut device) != glib::ffi::GFALSE {
                devices.push(from_glib_none(device));
            }
            ffi::manette_monitor_iter_free(iter);
        }
        devices
    }
}
