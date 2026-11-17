//! Quick Access's Sound Output (`pindu_core::system::audio`): its entry, subtitled with the
//! output in use and shown only where the tools run and find an output, and its level,
//! which lists the outputs as radio rows with a check on the default one; choosing one
//! makes it the default. The level is [`PinduSoundOutputLevel`], with its own Blueprint
//! (`data/ui/fullscreen_sound_output.blp`); the window keeps the entry, which sits in Quick
//! Access's shared list, and `win.sound-output`. Both show
//! [`AudioOutputService`](crate::audio_output_service::AudioOutputService)'s state and the
//! window calls it; neither runs `pw-dump` or `wpctl` itself.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::system::audio::{AudioState, Output};

use crate::application::PinduApplication;
use crate::window::PinduWindow;

/// The `win.sound-output` state while no output is the default: PipeWire's invalid id,
/// which no output has.
const NO_OUTPUT: u32 = u32::MAX;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(
        resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/fullscreen_sound_output.ui"
    )]
    pub struct PinduSoundOutputLevel {
        #[template_child]
        pub quick_access_sound_outputs: TemplateChild<gtk::ListBox>,
        /// The outputs the level lists, in its rows' order.
        pub sound_outputs: RefCell<Vec<Output>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduSoundOutputLevel {
        const NAME: &'static str = "PinduSoundOutputLevel";
        type Type = super::PinduSoundOutputLevel;
        type ParentType = adw::Bin;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduSoundOutputLevel {}

    impl WidgetImpl for PinduSoundOutputLevel {}

    impl BinImpl for PinduSoundOutputLevel {}
}

glib::wrapper! {
    /// Quick Access's Sound Output level: the outputs as radio rows, with a check on the
    /// default one. It is the `"sound-output"` page of the window's Quick Access stack.
    pub struct PinduSoundOutputLevel(ObjectSubclass<imp::PinduSoundOutputLevel>)
        @extends adw::Bin, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl PinduWindow {
    /// Adds `win.sound-output`, whose state is the default output and which the level's
    /// radio rows choose through, shows the entry as the section is and as it changes, and
    /// reads the outputs again whenever Quick Access opens.
    pub(crate) fn setup_sound_output(&self, application: &PinduApplication) {
        let service = application.audio_output_service().clone();
        let action = gio::SimpleAction::new_stateful(
            "sound-output",
            Some(glib::VariantTy::UINT32),
            &default_output(&service.state()).to_variant(),
        );
        action.connect_change_state(glib::clone!(
            #[strong]
            service,
            move |_, state| {
                if let Some(id) = state.and_then(u32::from_variant) {
                    service.choose(id);
                }
            }
        ));
        self.add_action(&action);
        service.connect_changed(glib::clone!(
            #[weak]
            action,
            #[weak(rename_to = window)]
            self,
            move |state| {
                action.set_state(&default_output(state).to_variant());
                window.show_sound_output_state(state);
            }
        ));
        self.show_sound_output_state(&service.state());
        self.imp()
            .quick_access
            .connect_show_sidebar_notify(move |quick_access| {
                if quick_access.shows_sidebar() {
                    service.refresh();
                }
            });
    }

    /// The level opens: it lists the outputs as last read, and they are read again.
    pub(crate) fn show_sound_output(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let service = application.audio_output_service();
        self.imp()
            .quick_access_sound_output_level
            .fill(&service.state().outputs.outputs);
        service.refresh();
    }

    /// Where focus goes on the level: the output in use, or else the first one.
    pub(crate) fn sound_output_focus(&self) -> Option<gtk::Widget> {
        let checked = self.action_state("sound-output");
        self.imp()
            .quick_access_sound_output_level
            .focus_target(checked.as_ref())
    }

    /// Shows or hides the entry, with the output in use under it, and lists the outputs
    /// again while the level is open and they changed; it leaves for the entries once the
    /// section is gone.
    fn show_sound_output_state(&self, state: &AudioState) {
        let imp = self.imp();
        imp.quick_access_sound_output_row
            .set_visible(state.available);
        let in_use = state
            .outputs
            .in_use()
            .map(|output| output.description.as_str())
            .unwrap_or_default();
        imp.quick_access_sound_output_subtitle
            .set_visible(!in_use.is_empty());
        imp.quick_access_sound_output_subtitle.set_label(in_use);
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::SoundOutput)) {
            return;
        }
        if !state.available {
            self.show_panel(Panel::QuickAccess(QuickAccessLevel::Main));
            return;
        }
        // The checks follow `win.sound-output`; only other outputs need the rows again.
        let level = &imp.quick_access_sound_output_level;
        if !level.lists_outputs(&state.outputs.outputs) {
            let had_focus = level.has_focus();
            level.fill(&state.outputs.outputs);
            if had_focus {
                self.focus_panel(None);
            }
        }
    }
}

impl PinduSoundOutputLevel {
    /// Where focus goes on the level: the row whose check has the `checked` target, the
    /// output in use, or else the first row.
    pub(crate) fn focus_target(&self, checked: Option<&glib::Variant>) -> Option<gtk::Widget> {
        let list = &self.imp().quick_access_sound_outputs;
        std::iter::successors(list.first_child(), |row| row.next_sibling())
            .find(|row| {
                row.downcast_ref::<adw::ActionRow>()
                    .and_then(adw::ActionRow::activatable_widget)
                    .and_downcast::<gtk::CheckButton>()
                    .and_then(|check| check.action_target_value())
                    .as_ref()
                    == checked
            })
            .or_else(|| list.row_at_index(0).map(|row| row.upcast()))
    }

    /// Whether the level lists `outputs`, in their order.
    pub(crate) fn lists_outputs(&self, outputs: &[Output]) -> bool {
        *self.imp().sound_outputs.borrow() == outputs
    }

    /// Whether focus is on one of the level's rows, which filling it again takes away.
    pub(crate) fn has_focus(&self) -> bool {
        let list = &self.imp().quick_access_sound_outputs;
        self.root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root))
            .is_some_and(|focus| focus.is_ancestor(&**list))
    }

    /// One radio row per output, in PipeWire's order.
    pub(crate) fn fill(&self, outputs: &[Output]) {
        let imp = self.imp();
        let list = &imp.quick_access_sound_outputs;
        list.remove_all();
        for output in outputs {
            list.append(&output_row(output));
        }
        imp.sound_outputs.replace(outputs.to_vec());
    }
}

/// The `win.sound-output` state for `state`: the default output's id, or [`NO_OUTPUT`].
fn default_output(state: &AudioState) -> u32 {
    state.outputs.default.unwrap_or(NO_OUTPUT)
}

/// A row of the level: the output's description, with a check on `win.sound-output` for
/// its id, which the row's activation sets.
fn output_row(output: &Output) -> adw::ActionRow {
    let check = gtk::CheckButton::builder()
        .action_name("win.sound-output")
        .action_target(&output.id.to_variant())
        .can_focus(false)
        .build();
    let row = adw::ActionRow::builder()
        .title(&output.description)
        .use_markup(false)
        .activatable_widget(&check)
        .build();
    row.add_prefix(&check);
    row
}
