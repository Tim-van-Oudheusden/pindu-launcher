//! Draws a game's cover: art near 2:3 fills the box, other art fits on a blurred copy of
//! itself, and a game without a cover shows its icon the same way. Images are decoded by
//! glycin's sandboxed loaders.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib, graphene, gsk};
use pindu_core::art::{ICON_SCALE, Placement, Rect, crop, place};

use super::textures;

/// Blur radius of the background copy behind art that does not fill the cover.
const BLUR_RADIUS: f64 = 24.0;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct CoverArt {
        pub texture: RefCell<Option<gdk::Texture>>,
        /// Whether the texture is an icon standing in for a missing cover.
        pub is_icon: Cell<bool>,
        /// Whether art of any shape is cropped to fill the box.
        pub crop: Cell<bool>,
        /// Bumped on every new source, so a late load for an old source is dropped.
        pub generation: Cell<u64>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CoverArt {
        const NAME: &'static str = "PinduCoverArt";
        type Type = super::CoverArt;
        type ParentType = gtk::Widget;

        fn class_init(klass: &mut Self::Class) {
            klass.set_css_name("cover-art");
            klass.set_accessible_role(gtk::AccessibleRole::Img);
        }
    }

    impl ObjectImpl for CoverArt {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().set_overflow(gtk::Overflow::Hidden);
        }
    }

    impl WidgetImpl for CoverArt {
        fn snapshot(&self, snapshot: &gtk::Snapshot) {
            let Some(texture) = self.texture.borrow().clone() else {
                return;
            };
            let widget = self.obj();
            let (width, height) = (widget.width() as f32, widget.height() as f32);
            let (art_width, art_height) = (texture.width() as f32, texture.height() as f32);
            if self.crop.get() && !self.is_icon.get() {
                append(
                    snapshot,
                    &texture,
                    crop(art_width, art_height, width, height),
                );
                return;
            }
            let (background, foreground) = match place(art_width, art_height, width, height) {
                // An icon never fills: it sits, smaller, on a blurred copy of itself.
                Placement::Fill(rect) if !self.is_icon.get() => {
                    append(snapshot, &texture, rect);
                    return;
                }
                Placement::Fill(rect) => (rect, rect),
                Placement::FitOnBlur {
                    background,
                    foreground,
                } => (background, foreground),
            };
            let foreground = if self.is_icon.get() {
                let side = width * ICON_SCALE;
                Rect {
                    x: (width - side) / 2.0,
                    y: (height - side) / 2.0,
                    width: side,
                    height: side,
                }
            } else {
                foreground
            };
            snapshot.push_blur(BLUR_RADIUS);
            append(snapshot, &texture, background);
            snapshot.pop();
            append(snapshot, &texture, foreground);
        }
    }
}

fn append(snapshot: &gtk::Snapshot, texture: &gdk::Texture, rect: Rect) {
    let bounds = graphene::Rect::new(rect.x, rect.y, rect.width, rect.height);
    snapshot.append_scaled_texture(texture, gsk::ScalingFilter::Trilinear, &bounds);
}

glib::wrapper! {
    pub struct CoverArt(ObjectSubclass<imp::CoverArt>)
        @extends gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl CoverArt {
    pub fn new() -> Self {
        glib::Object::new()
    }

    /// Whether a cover or icon is drawn; without one, the tile shows its placeholder.
    pub fn has_art(&self) -> bool {
        self.imp().texture.borrow().is_some()
    }

    /// What a screen reader says for the art: the game's title.
    pub fn set_alternative_text(&self, text: &str) {
        self.update_property(&[gtk::accessible::Property::Label(text)]);
    }

    /// Crops art of any shape to fill the box, rather than fitting art that is not near
    /// 2:3 on a blurred copy; icons still sit on their blurred copy.
    pub fn set_crop(&self, crop: bool) {
        self.imp().crop.set(crop);
        self.queue_draw();
    }

    /// Shows `cover`, resized for a tile of `size` device pixels, or `icon` when there is
    /// no cover, loading it in the background. `on_loaded` runs once the art for this call
    /// is shown (or failed to load).
    pub fn show(
        &self,
        cover: Option<PathBuf>,
        icon: Option<PathBuf>,
        size: (u32, u32),
        on_loaded: impl FnOnce(&Self) + 'static,
    ) {
        let imp = self.imp();
        let generation = imp.generation.get() + 1;
        imp.generation.set(generation);
        imp.texture.replace(None);
        self.queue_draw();
        let (path, is_icon) = match (cover, icon) {
            (Some(cover), _) => (cover, false),
            (None, Some(icon)) => (icon, true),
            (None, None) => {
                on_loaded(self);
                return;
            }
        };
        imp.is_icon.set(is_icon);
        let (width, height) = size;
        let in_memory = if is_icon {
            textures::cached(&path)
        } else {
            textures::cached_cover(&path, width, height)
        };
        if let Some(texture) = in_memory {
            imp.texture.replace(Some(texture));
            on_loaded(self);
            return;
        }
        let art = self.downgrade();
        glib::spawn_future_local(async move {
            let texture = if is_icon {
                textures::load(&path).await
            } else {
                textures::load_cover(&path, width, height).await
            };
            let Some(art) = art.upgrade() else { return };
            let imp = art.imp();
            if imp.generation.get() != generation {
                return;
            }
            if let Ok(texture) = texture {
                imp.texture.replace(Some(texture));
            }
            art.queue_draw();
            on_loaded(&art);
        });
    }

    /// Forgets the shown art and any load in progress.
    pub fn clear(&self) {
        let imp = self.imp();
        imp.generation.set(imp.generation.get() + 1);
        imp.texture.replace(None);
        self.queue_draw();
    }
}

impl Default for CoverArt {
    fn default() -> Self {
        Self::new()
    }
}
