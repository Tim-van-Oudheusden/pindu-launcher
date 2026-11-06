//! Quick Access's Controls: the Controls entry's level, which lists what each button and
//! key does in fullscreen mode, in groups (see `pindu_core::controls`), with the glyphs of
//! the controller in use beside the keys that stand in for them, or the keys alone
//! without a controller. The list follows the controller in use and its button prompts.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::controls::{Control, Group, controls};
use pindu_core::gamepad::{Direction, Glyphs, Intent};
use pindu_core::hints;
use pindu_core::keys::Chord;
use pindu_core::panels::{Panel, QuickAccessLevel};

use crate::application::PinduApplication;
use crate::fullscreen_panels::step_out_of_list;
use crate::hint_bar::{GLYPH_SIZE, glyph_label};
use crate::window::PinduWindow;

impl PinduWindow {
    /// Follows the controller in use, whose glyphs the list shows.
    pub(crate) fn setup_controls(&self, application: &PinduApplication) {
        application
            .gamepad_service()
            .connect_active_layout_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    if window.open_panel() == Some(Panel::QuickAccess(QuickAccessLevel::Controls)) {
                        window.fill_controls();
                    }
                }
            ));
    }

    /// Fills the Controls level for the controller in use, keeping focus on the row it was
    /// on, or on the level's first row when that row is gone.
    pub(crate) fn fill_controls(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let imp = self.imp();
        let container = imp.quick_access_controls.get();
        let rows = |container: &gtk::Box| -> Vec<gtk::ListBoxRow> {
            std::iter::successors(container.first_child(), |child| child.next_sibling())
                .filter_map(|child| child.downcast::<gtk::ListBox>().ok())
                .flat_map(|list| {
                    std::iter::successors(list.first_child(), |row| row.next_sibling())
                        .filter_map(|row| row.downcast::<gtk::ListBoxRow>().ok())
                })
                .collect()
        };
        let focused = gtk::prelude::GtkWindowExt::focus(self)
            .filter(|focus| focus.is_ancestor(&container))
            .and_then(|focus| {
                let index = rows(&container)
                    .iter()
                    .position(|row| focus == *row || focus.is_ancestor(row))?;
                imp.shown_controls.borrow().get(index).copied()
            });
        while let Some(child) = container.first_child() {
            container.remove(&child);
        }
        let layout = application.gamepad_service().active_layout();
        let glyphs = layout.map(|layout| layout.glyphs);
        let mut shown = Vec::new();
        for section in controls(layout) {
            let heading = gtk::Label::builder()
                .label(group_label(section.group))
                .xalign(0.0)
                .css_classes(["heading"])
                .build();
            if container.first_child().is_some() {
                heading.set_margin_top(6);
            }
            container.append(&heading);
            let list = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            list.connect_keynav_failed(|list, direction| {
                step_out_of_list(list, direction);
                glib::Propagation::Stop
            });
            for control in &section.controls {
                list.append(&control_row(control, glyphs));
                shown.push(control.intent);
            }
            container.append(&list);
        }
        let index = focused.and_then(|intent| shown.iter().position(|&listed| listed == intent));
        imp.shown_controls.replace(shown);
        match index.and_then(|index| rows(&container).into_iter().nth(index)) {
            Some(row) => {
                row.grab_focus();
            }
            None if focused.is_some() => self.focus_panel(None),
            None => {}
        }
    }
}

/// A row of the list: the glyph of the button (with Hold before it for a held one) and the
/// keys that stand in for it, named for screen readers by the button, the keys and what
/// they do.
fn control_row(control: &Control, glyphs: Option<Glyphs>) -> adw::ActionRow {
    let action = action_label(control.intent);
    let row = adw::ActionRow::builder()
        .title(&action)
        .use_markup(false)
        .css_classes(["control"])
        .build();
    let button = control.button.zip(glyphs).map(|(button, glyphs)| {
        let shown = gtk::Box::builder()
            .spacing(6)
            .valign(gtk::Align::Center)
            .build();
        if control.held {
            let hold = gtk::Label::builder()
                .label(gettext("Hold"))
                .css_classes(["dim-label"])
                .build();
            shown.append(&hold);
        }
        let glyph = gtk::Image::builder()
            .icon_name(hints::glyph(glyphs, button))
            .pixel_size(GLYPH_SIZE)
            .css_classes(["control-glyph"])
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build();
        shown.append(&glyph);
        row.add_prefix(&shown);
        let name = glyph_label(hints::glyph_name(glyphs, button));
        if control.held {
            // Translators: a button held down, as Quick Access's Controls reads it to a
            // screen reader; {button} is its name, such as "B" or "Circle".
            gettext("Hold {button}").replace("{button}", &name)
        } else {
            name
        }
    });
    if !control.keys.is_empty() {
        let keys = gtk::Box::builder()
            .spacing(8)
            .valign(gtk::Align::Center)
            .css_classes(["control-keys"])
            .build();
        for (index, chord) in control.keys.iter().enumerate() {
            if index > 0 {
                // Translators: between two keys that do the same in Quick Access's
                // Controls, such as "Menu or Shift F10".
                let or = gtk::Label::builder()
                    .label(gettext("or"))
                    .css_classes(["dim-label"])
                    .build();
                keys.append(&or);
            }
            keys.append(&adw::ShortcutLabel::new(&chord.accelerator()));
        }
        row.add_suffix(&keys);
    }
    let keys = control
        .keys
        .iter()
        .map(|&chord| key_label(chord))
        .reduce(|first, second| {
            // Translators: two keys that do the same, as Quick Access's Controls reads
            // them to a screen reader, such as "Menu or Shift+F10".
            gettext("{first} or {second}")
                .replace("{first}", &first)
                .replace("{second}", &second)
        });
    let name = match (button, keys) {
        // Translators: a row of Quick Access's Controls as a screen reader reads it, such
        // as "A or Return: Select"; {button} is the controller button, {keys} the keys
        // that do the same and {action} what they do.
        (Some(button), Some(keys)) => gettext("{button} or {keys}: {action}")
            .replace("{button}", &button)
            .replace("{keys}", &keys),
        // Translators: a row of Quick Access's Controls as a screen reader reads it, such
        // as "Hold B: Cancel Launch".
        (Some(button), None) => gettext("{button}: {action}").replace("{button}", &button),
        // Translators: a row of Quick Access's Controls without a controller, as a screen
        // reader reads it, such as "Ctrl+F: Search".
        (None, Some(keys)) => gettext("{keys}: {action}").replace("{keys}", &keys),
        (None, None) => "{action}".to_owned(),
    }
    .replace("{action}", &action);
    // The bindings leave Accessible out of ActionRow's interfaces; its ListBoxRow has it.
    row.upcast_ref::<gtk::ListBoxRow>()
        .update_property(&[gtk::accessible::Property::Label(&name)]);
    row
}

/// The name of a key with its modifiers, as the desktop writes it ("Ctrl+Page Up").
fn key_label(chord: Chord) -> String {
    gtk::accelerator_parse(chord.accelerator())
        .map(|(key, modifiers)| gtk::accelerator_get_label(key, modifiers).into())
        .unwrap_or_else(|| chord.key.to_owned())
}

/// What Quick Access's Controls calls a group.
fn group_label(group: Group) -> String {
    match group {
        Group::Moving => gettext("Moving"),
        Group::Library => gettext("Library"),
        Group::Panels => gettext("Panels"),
        Group::Launching => gettext("Launching"),
    }
}

/// What Quick Access's Controls says an intent does.
fn action_label(intent: Intent) -> String {
    match intent {
        Intent::Navigate(Direction::Up) => gettext("Move Up"),
        Intent::Navigate(Direction::Down) => gettext("Move Down"),
        Intent::Navigate(Direction::Left) => gettext("Move Left"),
        Intent::Navigate(Direction::Right) => gettext("Move Right"),
        Intent::Activate => gettext("Select"),
        Intent::Back => gettext("Back"),
        Intent::BackHeld => gettext("Cancel Launch"),
        Intent::Search => gettext("Search"),
        Intent::PreviousTab => gettext("Previous Tab"),
        Intent::NextTab => gettext("Next Tab"),
        Intent::PreviousSort => gettext("Previous Sort Order"),
        Intent::NextSort => gettext("Next Sort Order"),
        Intent::Options => gettext("Game Options"),
        Intent::Menu => gettext("Menu"),
        // Guide opens the menu drawer too, and from the desktop it opens fullscreen mode.
        Intent::Home => gettext("Menu"),
        Intent::QuickAccess => gettext("Quick Access"),
    }
}
