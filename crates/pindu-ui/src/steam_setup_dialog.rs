//! The Setting Up Steam screen (`data/ui/steam_setup_dialog.blp`), which covers the window
//! while a Steam that has never run sets itself up (`steam_setup_service`): what Steam is
//! doing and how far its download is, with Cancel; once the set-up failed or stopped, why,
//! with Try Again. It only shows a [`State`] of `pindu_core::steam_setup`; Cancel, Escape
//! and B close it, which the service takes as giving up.

use std::cell::RefCell;
use std::fmt;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::steam_setup::{Failure, State, Step};

/// What Try Again does.
#[derive(Default)]
pub struct TryAgain(RefCell<Option<Box<dyn Fn()>>>);

impl fmt::Debug for TryAgain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TryAgain")
            .field("set", &self.0.borrow().is_some())
            .finish()
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/steam_setup_dialog.ui")]
    pub struct SteamSetupDialog {
        #[template_child]
        pub status: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub progress: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub try_again_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub cancel_button: TemplateChild<gtk::Button>,
        pub try_again: TryAgain,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SteamSetupDialog {
        const NAME: &'static str = "PinduSteamSetupDialog";
        type Type = super::SteamSetupDialog;
        type ParentType = adw::Dialog;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl SteamSetupDialog {
        #[template_callback]
        fn try_again(&self) {
            if let Some(try_again) = self.try_again.0.borrow().as_ref() {
                try_again();
            }
        }
    }

    impl ObjectImpl for SteamSetupDialog {}
    impl WidgetImpl for SteamSetupDialog {}
    impl AdwDialogImpl for SteamSetupDialog {}
}

glib::wrapper! {
    /// The Setting Up Steam screen.
    pub struct SteamSetupDialog(ObjectSubclass<imp::SteamSetupDialog>)
        @extends adw::Dialog, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::ShortcutManager;
}

impl SteamSetupDialog {
    /// The screen, whose Try Again calls `try_again`.
    pub fn new(try_again: impl Fn() + 'static) -> Self {
        let dialog: Self = glib::Object::new();
        dialog.imp().try_again.0.replace(Some(Box::new(try_again)));
        dialog.show_state(State::Working(Step::Starting), None);
        dialog
    }

    /// Shows the set-up at `state`, with the download `progress` from 0 to 1 when known.
    pub fn show_state(&self, state: State, progress: Option<f64>) {
        let imp = self.imp();
        let (icon, title, description) = match state {
            State::Working(_) | State::Ready | State::Cancelled => (
                "pindu-source-steam-symbolic",
                gettext("Setting Up Steam"),
                gettext(
                    "Steam downloads its own update before you can sign in. This can take \
                     several minutes.",
                ),
            ),
            State::Failed(Failure::Offline) => (
                "network-offline-symbolic",
                gettext("No Internet Connection"),
                gettext(
                    "Steam needs the internet to download its update. Connect to the internet, \
                     then try again.",
                ),
            ),
            State::Failed(Failure::Download) => (
                "dialog-warning-symbolic",
                gettext("Steam Could Not Update"),
                gettext("Steam could not download its update. Try again in a while."),
            ),
            State::TimedOut => (
                "dialog-warning-symbolic",
                gettext("Steam Is Not Responding"),
                gettext(
                    "Steam has shown no progress for several minutes. Try again, or cancel \
                     and come back later.",
                ),
            ),
        };
        imp.status.set_icon_name(Some(icon));
        imp.status.set_title(&title);
        imp.status.set_description(Some(&description));
        let working = match state {
            State::Working(step) => {
                imp.progress.set_text(Some(&step_text(step)));
                if let Some(fraction) = progress {
                    imp.progress.set_fraction(fraction);
                }
                true
            }
            _ => false,
        };
        imp.progress.set_visible(working);
        let offers_try_again = !working;
        if imp.try_again_button.is_visible() != offers_try_again {
            imp.try_again_button.set_visible(offers_try_again);
            let focus = if offers_try_again {
                &imp.try_again_button
            } else {
                &imp.cancel_button
            };
            focus.grab_focus();
        }
    }

    /// Moves the bar on while Steam's step has no measure of how far it is.
    pub fn pulse(&self) {
        self.imp().progress.pulse();
    }
}

/// What the progress bar says at a step.
fn step_text(step: Step) -> String {
    match step {
        Step::Starting => gettext("Starting Steam…"),
        Step::Checking => gettext("Looking for Steam’s update…"),
        Step::Downloading { done, total } if total > 0 => gettext("Downloading… {done} of {total}")
            .replace("{done}", &glib::format_size(done.saturating_mul(1024)))
            .replace("{total}", &glib::format_size(total.saturating_mul(1024))),
        Step::Downloading { .. } => gettext("Downloading…"),
        Step::Unpacking => gettext("Unpacking…"),
        Step::Installing => gettext("Installing…"),
        Step::Launching => gettext("Opening the sign-in…"),
    }
}
