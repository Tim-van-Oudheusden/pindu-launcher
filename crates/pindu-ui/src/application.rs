use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};

use crate::config::{APP_ID, RESOURCE_BASE_PATH, VERSION};
use crate::sync_service::SyncService;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct PinduApplication {
        /// Shared by every shell, so there is one import at a time.
        pub sync_service: SyncService,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduApplication {
        const NAME: &'static str = "PinduApplication";
        type Type = super::PinduApplication;
        type ParentType = adw::Application;
    }

    impl ObjectImpl for PinduApplication {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            obj.setup_actions();
            obj.set_accels_for_action("app.quit", &["<primary>q"]);
        }
    }

    impl ApplicationImpl for PinduApplication {
        fn activate(&self) {
            let application = self.obj();
            let window = application.active_window().unwrap_or_else(|| {
                let window = PinduWindow::new(
                    application.upcast_ref::<adw::Application>(),
                    &self.sync_service,
                );
                // The first window of a run syncs every source.
                self.sync_service.sync();
                window.upcast()
            });
            window.present();
        }
    }

    impl GtkApplicationImpl for PinduApplication {}
    impl AdwApplicationImpl for PinduApplication {}
}

glib::wrapper! {
    pub struct PinduApplication(ObjectSubclass<imp::PinduApplication>)
        @extends gio::Application, gtk::Application, adw::Application,
        @implements gio::ActionGroup, gio::ActionMap;
}

impl PinduApplication {
    pub fn new() -> Self {
        glib::Object::builder()
            .property("application-id", APP_ID)
            .property("resource-base-path", RESOURCE_BASE_PATH)
            .build()
    }

    fn setup_actions(&self) {
        let quit_action = gio::ActionEntry::builder("quit")
            .activate(move |app: &Self, _, _| app.quit())
            .build();
        let about_action = gio::ActionEntry::builder("about")
            .activate(move |app: &Self, _, _| app.show_about())
            .build();
        let import_action = gio::ActionEntry::builder("import")
            .activate(move |app: &Self, _, _| app.imp().sync_service.sync())
            .build();
        self.add_action_entries([quit_action, about_action, import_action]);
    }

    fn show_about(&self) {
        let about = adw::AboutDialog::builder()
            .application_name(gettext("Pindu launcher"))
            .application_icon(APP_ID)
            .developer_name("Tim van Oudheusden")
            .version(VERSION)
            .website("https://github.com/Tim-van-Oudheusden/pindu-launcher")
            .issue_url("https://github.com/Tim-van-Oudheusden/pindu-launcher/issues")
            .developers(["Tim van Oudheusden"])
            // Translators: Replace "translator-credits" with your name/username, and optionally an email or URL.
            .translator_credits(gettext("translator-credits"))
            .license_type(gtk::License::Gpl30)
            .build();
        about.present(self.active_window().as_ref());
    }
}

impl Default for PinduApplication {
    fn default() -> Self {
        Self::new()
    }
}
