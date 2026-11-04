use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};

use crate::config::APP_ID;
use crate::sync_service::SyncService;

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/window.ui")]
    pub struct PinduWindow {
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub sync_spinner: TemplateChild<adw::Spinner>,
        pub settings: OnceCell<gio::Settings>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduWindow {
        const NAME: &'static str = "PinduWindow";
        type Type = super::PinduWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().restore_window_state();
        }
    }

    impl WidgetImpl for PinduWindow {}

    impl WindowImpl for PinduWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_window_state();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for PinduWindow {}
    impl AdwApplicationWindowImpl for PinduWindow {}
}

glib::wrapper! {
    pub struct PinduWindow(ObjectSubclass<imp::PinduWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl PinduWindow {
    pub fn new(application: &adw::Application, sync_service: &SyncService) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window.observe_sync(sync_service);
        window
    }

    /// Shows a header spinner while importing, and one toast summarising any problems.
    fn observe_sync(&self, sync_service: &SyncService) {
        sync_service
            .bind_property("syncing", &*self.imp().sync_spinner, "visible")
            .sync_create()
            .build();
        sync_service.connect_finished(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |service| window.report_problems(&service.problems())
        ));
    }

    fn report_problems(&self, problems: &[String]) {
        if problems.is_empty() {
            return;
        }
        let count = problems.len() as u32;
        let toast = adw::Toast::builder()
            .title(
                ngettext("One import problem", "{count} import problems", count)
                    .replace("{count}", &count.to_string()),
            )
            .button_label(gettext("Details"))
            .timeout(10)
            .build();
        let problems = problems.to_vec();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_problems(&problems)
        ));
        self.imp().toast_overlay.add_toast(toast);
    }

    /// One dialog listing every problem of the last import.
    fn show_problems(&self, problems: &[String]) {
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Some Games Could Not Be Imported"))
            .body(problems.join("\n"))
            .build();
        dialog.add_response("close", &gettext("_Close"));
        dialog.set_default_response(Some("close"));
        dialog.present(Some(self));
    }

    fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get_or_init(|| gio::Settings::new(APP_ID))
    }

    fn restore_window_state(&self) {
        let settings = self.settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("window-maximized") {
            self.maximize();
        }
    }

    fn save_window_state(&self) {
        let settings = self.settings();
        let (width, height) = self.default_size();
        // Window state is best effort: a read-only settings backend must not block closing.
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("window-maximized", self.is_maximized());
    }
}
