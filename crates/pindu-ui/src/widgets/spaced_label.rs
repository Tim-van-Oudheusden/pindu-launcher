//! A label that ellipsizes and can sit beside other widgets under a design that spaces its
//! letters (`letter-spacing`). GTK asks for such a label a little less room than laying it
//! out takes, by the spacing after the last letter; given exactly that, the label loses
//! its last word to an ellipsis. This widget asks for the label's width plus that spacing.

use gtk::glib;
use gtk::pango;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Debug)]
    pub struct SpacedLabel {
        pub label: gtk::Label,
    }

    impl Default for SpacedLabel {
        fn default() -> Self {
            Self {
                label: gtk::Label::builder()
                    .xalign(0.0)
                    .ellipsize(pango::EllipsizeMode::End)
                    .build(),
            }
        }
    }

    #[glib::object_subclass]
    impl ObjectSubclass for SpacedLabel {
        const NAME: &'static str = "PinduSpacedLabel";
        type Type = super::SpacedLabel;
        type ParentType = gtk::Widget;
    }

    impl ObjectImpl for SpacedLabel {
        fn constructed(&self) {
            self.parent_constructed();
            self.label.set_parent(&*self.obj());
        }

        fn dispose(&self) {
            self.label.unparent();
        }
    }

    impl WidgetImpl for SpacedLabel {
        fn measure(&self, orientation: gtk::Orientation, for_size: i32) -> (i32, i32, i32, i32) {
            let (minimum, natural, minimum_baseline, natural_baseline) =
                self.label.measure(orientation, for_size);
            if orientation == gtk::Orientation::Horizontal {
                (minimum, natural + self.letter_spacing(), -1, -1)
            } else {
                (minimum, natural, minimum_baseline, natural_baseline)
            }
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            self.label.allocate(width, height, baseline, None);
        }
    }

    impl SpacedLabel {
        /// The widest spacing between the label's letters, in whole pixels: what the
        /// design's `letter-spacing` gives it, or none.
        fn letter_spacing(&self) -> i32 {
            let spacing = self
                .label
                .layout()
                .attributes()
                .into_iter()
                .flat_map(|attributes| attributes.attributes())
                .filter(|attribute| attribute.type_() == pango::AttrType::LetterSpacing)
                .filter_map(|attribute| attribute.downcast::<pango::AttrInt>().ok())
                .map(|attribute| attribute.value())
                .max()
                .unwrap_or(0)
                .max(0);
            (spacing + pango::SCALE - 1) / pango::SCALE
        }
    }
}

glib::wrapper! {
    /// A label that ellipsizes, with room for the letter spacing a design gives it.
    pub struct SpacedLabel(ObjectSubclass<imp::SpacedLabel>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl SpacedLabel {
    /// A label showing `text`, left-aligned, with the style `classes`.
    pub fn new(text: &str, classes: &[&str]) -> Self {
        let spaced: Self = glib::Object::new();
        let label = &spaced.imp().label;
        label.set_label(text);
        label.set_css_classes(classes);
        spaced
    }

    /// The label inside, which carries the text and the style classes.
    pub fn label(&self) -> &gtk::Label {
        &self.imp().label
    }
}
