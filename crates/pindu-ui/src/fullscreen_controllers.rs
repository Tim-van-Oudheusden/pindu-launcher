//! Quick Access's Controllers: the Controllers entry's level, which lists each connected
//! pad with the glyphs it shows and its battery where one can be told to be its (see
//! `pindu_core::controllers`), rumbles a pad when its row is selected, and holds the Button
//! Prompts and Swap Confirm and Back settings as radio rows. The list follows pads as they
//! are plugged in and out, and the batteries as they are read.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::controllers::{Controller, controllers};
use pindu_core::gamepad::{DeviceId, Glyphs, PROMPTS_KEY, SWAP_KEY};
use pindu_core::panels::{Panel, QuickAccessLevel};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

impl PinduWindow {
    /// The settings' actions the radio rows choose through, Identify, and following the
    /// pads and the batteries.
    pub(crate) fn setup_controllers(&self, application: &PinduApplication) {
        let settings = self.settings();
        self.add_action(&settings.create_action(PROMPTS_KEY));
        self.add_action(&settings.create_action(SWAP_KEY));
        let identify = gio::ActionEntry::builder("identify-controller")
            .parameter_type(Some(glib::VariantTy::UINT64))
            .activate(|window: &Self, _, target| {
                let device = target.and_then(u64::from_variant).map(DeviceId);
                let application = window.application().and_downcast::<PinduApplication>();
                if let (Some(device), Some(application)) = (device, application) {
                    application.gamepad_service().identify(device);
                }
            })
            .build();
        self.add_action_entries([identify]);
        let power = application.power_service().clone();
        application
            .gamepad_service()
            .connect_pads_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // A pad just plugged in may bring a battery.
                    power.refresh();
                    window.controllers_changed();
                }
            ));
        application.power_service().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.controllers_changed()
        ));
    }

    /// A pad came or went, the settings changed its glyphs, or the batteries were read:
    /// the level shows it while it is open.
    fn controllers_changed(&self) {
        if self.open_panel() == Some(Panel::QuickAccess(QuickAccessLevel::Controllers)) {
            self.fill_controllers();
        }
    }

    /// Fills the Controllers list, keeping focus on the pad it was on, or on the level's
    /// first item when that pad is gone.
    pub(crate) fn fill_controllers(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let list = self.imp().quick_access_controllers.get();
        let focused = gtk::prelude::GtkWindowExt::focus(self)
            .filter(|focus| focus.is_ancestor(&list))
            .and_then(|focus| {
                focus
                    .ancestor(gtk::ListBoxRow::static_type())
                    .and_downcast::<gtk::ListBoxRow>()
            })
            .map(|row| row.action_target_value());
        list.remove_all();
        let gamepad = application.gamepad_service();
        let pads = gamepad.pads();
        let shown = controllers(
            pads.iter().map(|(device, pad)| (*device, pad)),
            gamepad.controller_settings(),
            &application.power_service().batteries().peripherals,
        );
        if shown.is_empty() {
            let row = adw::ActionRow::builder()
                .title(gettext("No Controllers Connected"))
                .build();
            list.append(&row);
        }
        for controller in &shown {
            list.append(&controller_row(controller));
        }
        let Some(focused) = focused else {
            return;
        };
        let same = std::iter::successors(list.first_child(), |row| row.next_sibling())
            .filter_map(|row| row.downcast::<gtk::ListBoxRow>().ok())
            .find(|row| row.is_sensitive() && row.action_target_value() == focused);
        match same {
            Some(row) => {
                row.grab_focus();
            }
            None => self.focus_panel(None),
        }
    }
}

/// A pad's row: its name, the glyphs it shows and its battery; selecting it rumbles the pad,
/// and a pad that cannot rumble leaves its row unavailable.
fn controller_row(controller: &Controller) -> adw::ActionRow {
    let name = if controller.name.is_empty() {
        gettext("Controller")
    } else {
        controller.name.clone()
    };
    let row = adw::ActionRow::builder()
        .title(name)
        .subtitle(glyphs_label(controller.glyphs))
        .use_markup(false)
        .activatable(true)
        .action_name("win.identify-controller")
        .action_target(&controller.device.0.to_variant())
        .sensitive(controller.rumble)
        .build();
    if controller.rumble {
        row.set_tooltip_text(Some(&gettext("Rumble to tell which controller this is")));
    }
    let icon = gtk::Image::builder()
        .icon_name("input-gaming-symbolic")
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    row.add_prefix(&icon);
    let battery = controller
        .battery
        .as_ref()
        .and_then(|battery| Some((battery.capacity?, battery.icon_name()?)));
    if let Some((capacity, icon_name)) = battery {
        let shown = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        shown.append(
            &gtk::Image::builder()
                .icon_name(icon_name)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
        // Translators: a battery's charge; {percent} is a number from 0 to 100.
        let level = gettext("{percent} %").replace("{percent}", &capacity.to_string());
        shown.append(
            &gtk::Label::builder()
                .label(level)
                .css_classes(["numeric"])
                .build(),
        );
        row.add_suffix(&shown);
    }
    row
}

/// What Quick Access calls a set of glyphs, without naming a console: the same words as
/// its Button Prompts rows.
fn glyphs_label(glyphs: Glyphs) -> String {
    match glyphs {
        Glyphs::Letters | Glyphs::DeckLetters => gettext("Letters"),
        Glyphs::Shapes => gettext("Shapes"),
        Glyphs::NintendoLetters => gettext("Letters, A on the Right"),
    }
}
