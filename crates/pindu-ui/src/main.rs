use gettextrs::{LocaleCategory, bind_textdomain_codeset, bindtextdomain, setlocale, textdomain};
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_ui::application::PinduApplication;
use pindu_ui::config::{GETTEXT_PACKAGE, LOCALEDIR, PKGDATADIR};

fn main() -> glib::ExitCode {
    // SAFETY: called first thing in main, before any other thread exists.
    unsafe { setlocale(LocaleCategory::LcAll, "") };
    bindtextdomain(GETTEXT_PACKAGE, LOCALEDIR).expect("Unable to bind the text domain");
    bind_textdomain_codeset(GETTEXT_PACKAGE, "UTF-8")
        .expect("Unable to set the text domain encoding");
    textdomain(GETTEXT_PACKAGE).expect("Unable to switch to the text domain");

    let resources = gio::Resource::load(format!("{PKGDATADIR}/resources.gresource"))
        .expect("Could not load resources");
    gio::resources_register(&resources);

    PinduApplication::new().run()
}
