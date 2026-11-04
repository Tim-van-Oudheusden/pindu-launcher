//! `ManetteEvent`, a boxed union that gir cannot generate.

use glib::translate::*;

use crate::{EventType, ffi};

glib::wrapper! {
    /// One input event from a device: a button, an axis or a hat.
    #[derive(Debug)]
    pub struct Event(Boxed<ffi::ManetteEvent>);

    match fn {
        copy => |ptr| ffi::manette_event_copy(ptr),
        free => |ptr| ffi::manette_event_free(ptr),
        type_ => || ffi::manette_event_get_type(),
    }
}

impl Event {
    pub fn event_type(&self) -> EventType {
        unsafe { from_glib(ffi::manette_event_get_event_type(self.to_glib_none().0)) }
    }

    /// The mapped button of a button event (a Linux `BTN_*` code).
    pub fn button(&self) -> Option<u16> {
        let mut button = 0;
        let found = unsafe { ffi::manette_event_get_button(self.to_glib_none().0, &mut button) };
        (found != glib::ffi::GFALSE).then_some(button)
    }

    /// The mapped axis (a Linux `ABS_*` code) and its value from -1 to 1 of an axis event.
    pub fn absolute(&self) -> Option<(u16, f64)> {
        let (mut axis, mut value) = (0, 0.0);
        let found = unsafe {
            ffi::manette_event_get_absolute(self.to_glib_none().0, &mut axis, &mut value)
        };
        (found != glib::ffi::GFALSE).then_some((axis, value))
    }

    /// The mapped hat axis and its value (-1, 0 or 1) of a hat event.
    pub fn hat(&self) -> Option<(u16, i8)> {
        let (mut axis, mut value) = (0, 0);
        let found =
            unsafe { ffi::manette_event_get_hat(self.to_glib_none().0, &mut axis, &mut value) };
        (found != glib::ffi::GFALSE).then_some((axis, value))
    }
}
