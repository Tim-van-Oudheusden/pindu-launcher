//! Screen reader announcements that are skipped, rather than crash the app, where GTK has
//! no accessibility backend that can announce.

use gtk::ATContext;
use gtk::ffi::gtk_accessible_get_at_context;
use gtk::glib::translate::from_glib_full;
use gtk::prelude::*;
use pindu_core::accessibility::announces_with;

/// Asks screen readers to read `message` out at `priority`, when the accessibility
/// context of `accessible` can announce.
///
/// Without an accessibility bus GTK gives every widget its test context, whose class has
/// no `announce` function, and `gtk_accessible_announce` calls it unchecked: a crash
/// (#729). Every announcement goes through here so it only reaches GTK where a backend
/// such as AT-SPI, the one Orca listens on, implements it.
pub(crate) fn announce(
    accessible: &impl IsA<gtk::Accessible>,
    message: &str,
    priority: gtk::AccessibleAnnouncementPriority,
) {
    if context_type(accessible.upcast_ref()).is_some_and(announces_with) {
        accessible.announce(message, priority);
    }
}

/// The GType name of the accessibility context of `accessible`, or `None` when it has
/// none: with `GTK_A11Y=none`, or while the widget is being destroyed.
fn context_type(accessible: &gtk::Accessible) -> Option<&'static str> {
    let raw = accessible.as_ptr();
    // SAFETY: `raw` points to a valid `GtkAccessible` for the length of the call, and
    // `gtk_accessible_get_at_context` returns a new reference or NULL, which `Option`
    // takes ownership of. The safe `AccessibleExt::at_context` assumes it is never NULL.
    let context: Option<ATContext> = unsafe { from_glib_full(gtk_accessible_get_at_context(raw)) };
    context.map(|context| context.type_().name())
}
