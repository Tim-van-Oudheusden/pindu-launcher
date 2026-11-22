//! Quick Access's Controller section: it lists each connected pad with the glyphs it shows
//! and its battery where one can be told to be its (see `pindu_core::controllers`),
//! rumbles a pad when its row is selected, and holds the Button Prompts, Swap Confirm and
//! Back, Repeat Delay, Repeat Rate and Stick Dead Zone settings as choice rows, with the
//! Hold to Confirm switch after Swap Confirm and Back, then the Controls entry. The list
//! follows pads as they are plugged in and out, and the batteries as they are read.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::controllers::{Controller, controllers};
use pindu_core::gamepad::{
    DEAD_ZONE_KEY, DeviceId, Glyphs, PROMPTS_KEY, REPEAT_DELAY_KEY, REPEAT_RATE_KEY, SWAP_KEY,
};
use pindu_core::hold::HOLD_TO_CONFIRM_KEY;
use pindu_core::panels::QuickAccessSection;

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;
use crate::widgets::ChoiceOption;

impl PinduQuickAccess {
    /// The choice rows on the settings' actions, Hold to Confirm bound to its setting,
    /// Identify, and following the pads and the batteries.
    pub(crate) fn setup_controllers(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        let imp = self.imp();
        let explained = |target, label: String, detail: String| {
            ChoiceOption::new(target, label).with_detail(detail)
        };
        self.add_choice(
            actions,
            PROMPTS_KEY,
            &imp.prompts_row,
            vec![
                explained(
                    "auto",
                    gettext("Automatic"),
                    gettext("As the controller shows them"),
                ),
                ChoiceOption::new("letters", gettext("Letters")),
                ChoiceOption::new("shapes", gettext("Shapes")),
                ChoiceOption::new("nintendo-letters", gettext("Letters, A on the Right")),
            ],
        );
        self.add_choice(
            actions,
            SWAP_KEY,
            &imp.swap_row,
            vec![
                explained(
                    "auto",
                    gettext("Automatic"),
                    gettext("Swapped when A is on the right"),
                ),
                explained("off", gettext("Off"), gettext("The bottom button confirms")),
                explained("on", gettext("On"), gettext("The right button confirms")),
            ],
        );
        self.settings()
            .bind(HOLD_TO_CONFIRM_KEY, &*imp.hold_to_confirm_row, "active")
            .build();
        self.add_choice(
            actions,
            REPEAT_DELAY_KEY,
            &imp.repeat_delay_row,
            vec![
                explained("short", gettext("Short"), gettext("After 0.2 seconds")),
                explained("normal", gettext("Normal"), gettext("After 0.3 seconds")),
                explained("long", gettext("Long"), gettext("After 0.6 seconds")),
            ],
        );
        self.add_choice(
            actions,
            REPEAT_RATE_KEY,
            &imp.repeat_rate_row,
            vec![
                explained("slow", gettext("Slow"), gettext("10 moves a second")),
                explained("normal", gettext("Normal"), gettext("20 moves a second")),
                explained("fast", gettext("Fast"), gettext("About 33 moves a second")),
            ],
        );
        self.add_choice(
            actions,
            DEAD_ZONE_KEY,
            &imp.dead_zone_row,
            vec![
                explained(
                    "small",
                    gettext("Small"),
                    gettext("A light push moves focus"),
                ),
                explained("normal", gettext("Normal"), gettext("A push halfway")),
                explained(
                    "large",
                    gettext("Large"),
                    gettext("A firm push, for a stick that drifts"),
                ),
            ],
        );
        self.add_host_action(
            actions,
            "identify-controller",
            Some(glib::VariantTy::UINT64),
            |quick_access, target| {
                let device = target.and_then(u64::from_variant).map(DeviceId);
                if let (Some(device), Some(application)) = (device, quick_access.application()) {
                    application.gamepad_service().identify(device);
                }
            },
        );
        let power = application.power_service().clone();
        application
            .gamepad_service()
            .connect_pads_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || {
                    // A pad just plugged in may bring a battery.
                    power.refresh();
                    quick_access.controllers_changed();
                }
            ));
        application.power_service().connect_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_| quick_access.controllers_changed()
        ));
    }

    /// A pad came or went, the settings changed its glyphs, or the batteries were read:
    /// the section shows it while it shows.
    fn controllers_changed(&self) {
        if self.shows_section(QuickAccessSection::Controller) {
            self.fill_controllers();
        }
    }

    /// Fills the Controllers list, keeping focus on the pad it was on, or on the section's
    /// first item when that pad is gone.
    pub(crate) fn fill_controllers(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let list = self.imp().controllers.get();
        let focused = self
            .focus_widget()
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
            None => self.focus_level(None),
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
