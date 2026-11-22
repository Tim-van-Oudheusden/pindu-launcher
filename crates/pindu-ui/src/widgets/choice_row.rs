//! A choice row: one setting whose options Left and Right step through in place, applied at
//! once, for Quick Access, where a drop-down's popover would be dismissed by a controller
//! press and radio rows take a stop each. The setting's name is at the start, its chosen
//! option between two arrows at the end, and the option's second line, if it has one, under
//! the name. The row is bound to a stateful action and a target per option, as radio rows
//! would be, so choosing an option activates the action with its target.
//!
//! The row is one focus stop. Left and Right (swapped in right-to-left text) choose the
//! option before or after and stop at the first and the last, where the arrow that leads
//! nowhere is dimmed; they never move focus off the row. Activating the row (Enter, a click,
//! a controller's A) chooses the next option, wrapping from the last to the first. The
//! pointer and touch can press either arrow, which never takes focus. The steps are
//! `pindu_core::choice`.

use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gdk, gio, glib, pango};
use pindu_core::choice;

/// One option of a [`ChoiceRow`].
#[derive(Clone, Debug)]
pub struct ChoiceOption {
    /// The action target that chooses it, a string.
    pub target: &'static str,
    /// What the row shows between its arrows while it is chosen.
    pub label: String,
    /// The second line under the setting's name while it is chosen, if it has one.
    pub detail: Option<String>,
}

impl ChoiceOption {
    /// An option chosen with `target` and shown as `label`, without a second line.
    pub fn new(target: &'static str, label: String) -> Self {
        Self {
            target,
            label,
            detail: None,
        }
    }

    /// The option with `detail` as its second line.
    pub fn with_detail(self, detail: String) -> Self {
        Self {
            detail: Some(detail),
            ..self
        }
    }
}

/// The chosen option's place between the arrows: a stack of a label per option, as wide as
/// the widest. A box beside it asks its width for the box's height, for which a wrapping
/// label answers the narrowest width that fits, wrapping a short option needlessly; this
/// asks the stack's width for no height instead, so an option wraps only when the row
/// cannot fit it on one line.
mod value {
    use gtk::glib;
    use gtk::prelude::*;
    use gtk::subclass::prelude::*;

    mod imp {
        use super::*;

        #[derive(Debug)]
        pub struct ChoiceValue {
            pub stack: gtk::Stack,
        }

        impl Default for ChoiceValue {
            fn default() -> Self {
                Self {
                    stack: gtk::Stack::builder()
                        .hhomogeneous(true)
                        .vhomogeneous(false)
                        .build(),
                }
            }
        }

        #[glib::object_subclass]
        impl ObjectSubclass for ChoiceValue {
            const NAME: &'static str = "PinduChoiceValue";
            type Type = super::ChoiceValue;
            type ParentType = gtk::Widget;
        }

        impl ObjectImpl for ChoiceValue {
            fn constructed(&self) {
                self.parent_constructed();
                self.stack.set_parent(&*self.obj());
            }

            fn dispose(&self) {
                self.stack.unparent();
            }
        }

        impl WidgetImpl for ChoiceValue {
            fn request_mode(&self) -> gtk::SizeRequestMode {
                gtk::SizeRequestMode::HeightForWidth
            }

            fn measure(
                &self,
                orientation: gtk::Orientation,
                for_size: i32,
            ) -> (i32, i32, i32, i32) {
                match orientation {
                    gtk::Orientation::Horizontal => {
                        let (minimum, natural, _, _) = self.stack.measure(orientation, -1);
                        (minimum, natural, -1, -1)
                    }
                    _ => self.stack.measure(orientation, for_size),
                }
            }

            fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
                self.stack.allocate(width, height, baseline, None);
            }
        }
    }

    glib::wrapper! {
        pub struct ChoiceValue(ObjectSubclass<imp::ChoiceValue>)
            @extends gtk::Widget,
            @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
    }

    impl Default for ChoiceValue {
        fn default() -> Self {
            glib::Object::builder()
                .property("valign", gtk::Align::Center)
                .build()
        }
    }

    impl ChoiceValue {
        /// The stack of the options' labels.
        pub fn stack(&self) -> &gtk::Stack {
            &self.imp().stack
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct ChoiceRow {
        pub action: RefCell<Option<gio::Action>>,
        pub handlers: RefCell<Vec<glib::SignalHandlerId>>,
        pub options: RefCell<Vec<ChoiceOption>>,
        /// The option shown, to announce only a change.
        pub shown: Cell<Option<usize>>,
        /// Set while the row activates the action itself.
        pub choosing: Cell<bool>,
        pub previous: gtk::Button,
        pub next: gtk::Button,
        /// A label per option, as wide as the widest, so the arrows stay put.
        pub values: value::ChoiceValue,
    }

    impl Default for ChoiceRow {
        fn default() -> Self {
            let arrow = |icon: &str, label: String| {
                let button = gtk::Button::builder()
                    .icon_name(icon)
                    .valign(gtk::Align::Center)
                    .focusable(false)
                    .focus_on_click(false)
                    .css_classes(["flat", "circular", "choice-arrow"])
                    .build();
                button.update_property(&[gtk::accessible::Property::Label(&label)]);
                button
            };
            Self {
                action: RefCell::default(),
                handlers: RefCell::default(),
                options: RefCell::default(),
                shown: Cell::default(),
                choosing: Cell::default(),
                previous: arrow("pan-start-symbolic", gettext("Previous Option")),
                next: arrow("pan-end-symbolic", gettext("Next Option")),
                values: value::ChoiceValue::default(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for ChoiceRow {
        const NAME: &'static str = "PinduChoiceRow";
        type Type = super::ChoiceRow;
        type ParentType = adw::ActionRow;
    }

    impl ObjectImpl for ChoiceRow {
        fn constructed(&self) {
            self.parent_constructed();
            let row = self.obj();
            row.add_css_class("choice-row");
            row.set_activatable(true);
            row.set_use_markup(false);
            // The name wraps between words, as the option does, rather than break a word
            // to leave the option its width at a large text size.
            if let Some(title) = descendants(row.upcast_ref())
                .filter_map(|widget| widget.downcast::<gtk::Label>().ok())
                .find(|label| label.has_css_class("title"))
            {
                title.set_wrap_mode(pango::WrapMode::Word);
            }
            let control = gtk::Box::builder()
                .spacing(6)
                .valign(gtk::Align::Center)
                .build();
            control.append(&self.previous);
            control.append(&self.values);
            control.append(&self.next);
            row.add_suffix(&control);
            self.previous.connect_clicked(glib::clone!(
                #[weak]
                row,
                move |_| {
                    row.step(false);
                }
            ));
            self.next.connect_clicked(glib::clone!(
                #[weak]
                row,
                move |_| {
                    row.step(true);
                }
            ));
            // The arrow keys step as the controller's Left and Right do, rather than move
            // focus sideways.
            let keys = gtk::EventControllerKey::new();
            keys.connect_key_pressed(glib::clone!(
                #[weak]
                row,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, key, _, state| {
                    let modifiers = gdk::ModifierType::CONTROL_MASK
                        | gdk::ModifierType::SHIFT_MASK
                        | gdk::ModifierType::ALT_MASK;
                    if state.intersects(modifiers) {
                        return glib::Propagation::Proceed;
                    }
                    let right = match key {
                        gdk::Key::Left | gdk::Key::KP_Left => false,
                        gdk::Key::Right | gdk::Key::KP_Right => true,
                        _ => return glib::Propagation::Proceed,
                    };
                    row.adjust(right);
                    glib::Propagation::Stop
                }
            ));
            row.add_controller(keys);
        }

        fn dispose(&self) {
            self.obj().unbind();
        }
    }

    impl WidgetImpl for ChoiceRow {}
    impl ListBoxRowImpl for ChoiceRow {}
    impl PreferencesRowImpl for ChoiceRow {}

    impl ActionRowImpl for ChoiceRow {
        fn activate(&self) {
            ActionRowImplExt::parent_activate(self);
            self.obj().cycle();
        }
    }
}

glib::wrapper! {
    /// A row that shows one setting's chosen option and steps through its options in place.
    pub struct ChoiceRow(ObjectSubclass<imp::ChoiceRow>)
        @extends adw::ActionRow, adw::PreferencesRow, gtk::ListBoxRow, gtk::Widget,
        @implements gtk::Accessible, gtk::Actionable, gtk::Buildable, gtk::ConstraintTarget;
}

impl Default for ChoiceRow {
    fn default() -> Self {
        glib::Object::new()
    }
}

impl ChoiceRow {
    /// Binds the row to `action`, a stateful action with a string state, and `options`, in
    /// the order Left and Right step through them. The row shows the option whose target is
    /// the action's state, follows that state, and goes insensitive while the action is
    /// disabled.
    pub fn bind(&self, action: &impl IsA<gio::Action>, options: Vec<ChoiceOption>) {
        self.unbind();
        let imp = self.imp();
        for (index, option) in options.iter().enumerate() {
            let label = gtk::Label::builder()
                .label(&option.label)
                .wrap(true)
                .wrap_mode(pango::WrapMode::Word)
                .justify(gtk::Justification::Center)
                .css_classes(["choice-value"])
                .build();
            imp.values
                .stack()
                .add_named(&label, Some(&index.to_string()));
        }
        imp.options.replace(options);
        let action = action.upcast_ref::<gio::Action>().clone();
        let state = action.connect_notify_local(
            Some("state"),
            glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |_, _| row.show_state()
            ),
        );
        let enabled = action.connect_notify_local(
            Some("enabled"),
            glib::clone!(
                #[weak(rename_to = row)]
                self,
                move |_, _| row.show_state()
            ),
        );
        imp.handlers.replace(vec![state, enabled]);
        imp.action.replace(Some(action));
        imp.shown.set(None);
        self.show_state();
    }

    /// Left (`right` false) or Right, swapped in right-to-left text: chooses the option
    /// before or after the chosen one. Says whether it chose another, which it does not at
    /// an end.
    pub fn adjust(&self, right: bool) -> bool {
        let rtl = self.direction() == gtk::TextDirection::Rtl;
        self.step(right != rtl)
    }

    /// Chooses the option after the chosen one (`forward`) or before it; says whether it
    /// chose another.
    pub fn step(&self, forward: bool) -> bool {
        let (index, count) = self.position();
        let target = if forward {
            choice::next(index, count)
        } else {
            choice::previous(index, count)
        };
        target.is_some_and(|target| self.choose(target))
    }

    /// Chooses the next option, or the first after the last; says whether it chose one.
    pub fn cycle(&self) -> bool {
        let (index, count) = self.position();
        choice::cycle(index, count).is_some_and(|target| self.choose(target))
    }

    /// The chosen option's place among the options (their number when none is chosen), and
    /// their number.
    fn position(&self) -> (usize, usize) {
        let imp = self.imp();
        let options = imp.options.borrow();
        let count = options.len();
        let state = imp.action.borrow().as_ref().and_then(gio::Action::state);
        let index = state
            .and_then(|state| state.str().map(str::to_owned))
            .and_then(|state| options.iter().position(|option| option.target == state))
            .unwrap_or(count);
        (index, count)
    }

    /// Activates the action with the target of the option at `index`; says whether it did.
    fn choose(&self, index: usize) -> bool {
        let imp = self.imp();
        let Some(target) = imp.options.borrow().get(index).map(|option| option.target) else {
            return false;
        };
        let Some(action) = imp.action.borrow().clone() else {
            return false;
        };
        if !action.is_enabled() || self.position().0 == index {
            return false;
        }
        imp.choosing.set(true);
        action.activate(Some(&target.to_variant()));
        imp.choosing.set(false);
        true
    }

    /// Shows the chosen option, its second line and which arrows lead somewhere, tells
    /// assistive technology the value, and announces a change the row made or that came
    /// while it has focus.
    fn show_state(&self) {
        let imp = self.imp();
        let (index, count) = self.position();
        let enabled = imp
            .action
            .borrow()
            .as_ref()
            .is_some_and(gio::Action::is_enabled);
        self.set_sensitive(enabled);
        imp.previous
            .set_sensitive(choice::previous(index, count).is_some());
        imp.next.set_sensitive(choice::next(index, count).is_some());
        let options = imp.options.borrow();
        let option = options.get(index);
        imp.values.set_visible(option.is_some());
        if option.is_some() {
            imp.values
                .stack()
                .set_visible_child_name(&index.to_string());
        }
        let value = option
            .map(|option| option.label.clone())
            .unwrap_or_default();
        let detail = option.and_then(|option| option.detail.clone());
        drop(options);
        self.set_subtitle(detail.as_deref().unwrap_or_default());
        let description = match &detail {
            Some(detail) => gettext("{value}, {detail}")
                .replace("{value}", &value)
                .replace("{detail}", detail),
            None => value.clone(),
        };
        self.update_property(&[
            gtk::accessible::Property::ValueText(&value),
            gtk::accessible::Property::Description(&description),
        ]);
        // The first state shown after binding is no change.
        let shown = (index < count).then_some(index);
        let before = imp.shown.replace(shown);
        let changed = before.is_some() && before != shown;
        if changed && !value.is_empty() && (imp.choosing.get() || self.has_focus()) {
            self.announce(&value, gtk::AccessibleAnnouncementPriority::Medium);
        }
    }

    /// Lets go of the action and the options.
    fn unbind(&self) {
        let imp = self.imp();
        if let Some(action) = imp.action.take() {
            for handler in imp.handlers.take() {
                action.disconnect(handler);
            }
        }
        let stack = imp.values.stack();
        while let Some(child) = stack.first_child() {
            stack.remove(&child);
        }
        imp.options.borrow_mut().clear();
    }
}

/// `widget`'s descendants, depth first.
fn descendants(widget: &gtk::Widget) -> impl Iterator<Item = gtk::Widget> {
    let mut stack: Vec<gtk::Widget> = widget.first_child().into_iter().collect();
    std::iter::from_fn(move || {
        let widget = stack.pop()?;
        stack.extend(widget.next_sibling());
        stack.extend(widget.first_child());
        Some(widget)
    })
}
