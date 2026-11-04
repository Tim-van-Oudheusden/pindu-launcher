//! The Preferences dialog.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;

use crate::permission_service;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/preferences.ui")]
    pub struct PreferencesDialog {
        #[template_child]
        pub host_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub command_row: TemplateChild<adw::ActionRow>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PreferencesDialog {
        const NAME: &'static str = "PinduPreferencesDialog";
        type Type = super::PreferencesDialog;
        type ParentType = adw::PreferencesDialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl PreferencesDialog {
        #[template_callback]
        fn copy_command(&self) {
            let dialog = self.obj();
            dialog
                .clipboard()
                .set_text(&permission_service::override_command());
            dialog.add_toast(adw::Toast::new(&gettext("Command copied")));
        }
    }

    impl ObjectImpl for PreferencesDialog {
        fn constructed(&self) {
            self.parent_constructed();
            let allowed = permission_service::host_spawn_allowed();
            self.host_status.set_label(&if allowed {
                gettext("Allowed")
            } else {
                gettext("Not allowed")
            });
            self.command_row.set_visible(!allowed);
            self.command_row
                .set_subtitle(&permission_service::override_command());
        }
    }
    impl WidgetImpl for PreferencesDialog {}
    impl AdwDialogImpl for PreferencesDialog {}
    impl PreferencesDialogImpl for PreferencesDialog {}
}

glib::wrapper! {
    pub struct PreferencesDialog(ObjectSubclass<imp::PreferencesDialog>)
        @extends adw::PreferencesDialog, adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl PreferencesDialog {
    pub fn new() -> Self {
        glib::Object::new()
    }
}

impl Default for PreferencesDialog {
    fn default() -> Self {
        Self::new()
    }
}
