//! The frame of a Home tile that holds text: as wide as the tile is meant to be, whatever
//! its text would like, and as tall, or taller when the text needs more at that width (a
//! larger Text Size). A box with a size request cannot do this: asked how wide it must be
//! for the row's height, a label that wraps answers with its whole text on one line, and
//! the tile grows sideways instead.

use std::cell::{Cell, RefCell};

use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct TileFrame {
        pub child: RefCell<Option<gtk::Widget>>,
        /// The tile's width and its height while its content needs no more, in pixels.
        pub size: Cell<(i32, i32)>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for TileFrame {
        const NAME: &'static str = "PinduTileFrame";
        type Type = super::TileFrame;
        type ParentType = gtk::Widget;
        type Interfaces = (gtk::Buildable,);
    }

    impl ObjectImpl for TileFrame {
        fn dispose(&self) {
            if let Some(child) = self.child.take() {
                child.unparent();
            }
        }
    }

    impl WidgetImpl for TileFrame {
        fn request_mode(&self) -> gtk::SizeRequestMode {
            gtk::SizeRequestMode::HeightForWidth
        }

        fn measure(&self, orientation: gtk::Orientation, _for_size: i32) -> (i32, i32, i32, i32) {
            let (width, height) = self.size.get();
            if orientation == gtk::Orientation::Horizontal {
                return (width, width, -1, -1);
            }
            let needed = self
                .child
                .borrow()
                .as_ref()
                .map_or(0, |child| child.measure(orientation, width).0);
            let height = height.max(needed);
            (height, height, -1, -1)
        }

        fn size_allocate(&self, width: i32, height: i32, baseline: i32) {
            let child = self.child.borrow();
            let Some(child) = child.as_ref() else {
                return;
            };
            if child.valign() != gtk::Align::Center {
                child.allocate(width, height, baseline, None);
                return;
            }
            // Content in the middle sits where a `GtkOverlay` puts it, which halves the
            // tile's height and the content's each on its own: the tiles held theirs in
            // one, and keep it to the pixel.
            let natural = child.measure(gtk::Orientation::Vertical, -1).1.min(height);
            let top = height / 2 - natural / 2;
            let offset = gtk::graphene::Point::new(0.0, top as f32);
            let moved = gtk::gsk::Transform::new().translate(&offset);
            child.allocate(width, natural, -1, Some(moved));
        }
    }

    // A template gives the frame its child as its one nested widget.
    impl BuildableImpl for TileFrame {
        fn add_child(&self, builder: &gtk::Builder, child: &glib::Object, type_: Option<&str>) {
            match child.downcast_ref::<gtk::Widget>() {
                Some(widget) => self.obj().set_child(widget),
                None => self.parent_add_child(builder, child, type_),
            }
        }
    }
}

glib::wrapper! {
    /// The frame of a Home tile that holds text, of a set width and at least a set height.
    pub struct TileFrame(ObjectSubclass<imp::TileFrame>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl TileFrame {
    /// A frame of `size` pixels around `child`, which fills it.
    pub fn new(child: &impl IsA<gtk::Widget>, size: (i32, i32)) -> Self {
        let frame: Self = glib::Object::new();
        frame.set_child(child);
        frame.set_size(size);
        frame
    }

    /// Puts `child` in the frame, in place of the one it holds.
    pub fn set_child(&self, child: &impl IsA<gtk::Widget>) {
        if let Some(previous) = self.imp().child.take() {
            previous.unparent();
        }
        child.set_parent(self);
        self.imp().child.replace(Some(child.clone().upcast()));
    }

    /// Makes the tile `width`×`height` pixels, and taller only where its content needs it.
    pub fn set_size(&self, size: (i32, i32)) {
        if self.imp().size.replace(size) != size {
            self.queue_resize();
        }
    }
}
