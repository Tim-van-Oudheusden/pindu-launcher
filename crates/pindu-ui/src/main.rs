//! The Pindu launcher binary: sets up gettext, picks GTK's renderer where GTK's own
//! choice fails, loads the resources and runs [`PinduApplication`].

use std::path::Path;

use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::renderer;
use pindu_ui::application::PinduApplication;
use pindu_ui::config::{GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR};

fn main() -> glib::ExitCode {
    // SAFETY: called first thing in main, before any other thread exists.
    unsafe { setlocale(LocaleCategory::LcAll, "") };
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let chosen = std::env::var_os(renderer::VARIABLE);
    if let Some(name) = renderer::renderer(Path::new(renderer::SYSFS_DRM), chosen.as_deref()) {
        // SAFETY: GTK and GIO have not started, so no other thread exists to read the
        // environment.
        unsafe { std::env::set_var(renderer::VARIABLE, name) };
        glib::g_info!(
            "pindu",
            "Asking GTK for its {name} renderer on this virtio-gpu"
        );
    }

    let resources = gio::Resource::load(format!("{PKGDATADIR}/resources.gresource"))
        .expect("Could not load resources");
    gio::resources_register(&resources);

    PinduApplication::new().run()
}
