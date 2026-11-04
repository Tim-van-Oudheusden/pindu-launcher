//! Draws a game's cover: art near 2:3 fills the box, other art fits on a blurred copy of
//! itself, and a game without a cover shows its icon the same way. Images are decoded by
//! glycin's sandboxed loaders.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib, graphene, gsk};
use pindu_core::art::{Placement, Rect, place};

/// Blur radius of the background copy behind art that does not fill the cover.
const BLUR_RADIUS: f64 = 24.0;
/// Share of the cover's width an icon takes when it stands in for a cover.
const ICON_SCALE: f32 = 0.5;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct CoverArt {
        pub texture: RefCell<Option<gdk::Texture>>,
        /// Whether the texture is an icon standing in for a missing cover.
        pub is_icon: Cell<bool>,
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

    /// Shows `cover`, or `icon` when there is no cover, loading it in the background.
    /// `on_loaded` runs once the art for this call is shown (or failed to load).
    pub fn show(
        &self,
        cover: Option<PathBuf>,
        icon: Option<PathBuf>,
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
        if let Some(texture) = cached_texture(&path) {
            imp.texture.replace(Some(texture));
            on_loaded(self);
            return;
        }
        let art = self.downgrade();
        glib::spawn_future_local(async move {
            let texture = load_texture(&path).await;
            let Some(art) = art.upgrade() else { return };
            let imp = art.imp();
            if imp.generation.get() != generation {
                return;
            }
            match texture {
                Ok(texture) => {
                    cache_texture(path, texture.clone());
                    imp.texture.replace(Some(texture));
                }
                Err(error) => glib::g_debug!("pindu", "Could not load {}: {error}", path.display()),
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

/// Decodes an image file in glycin's sandboxed loaders.
async fn load_texture(path: &Path) -> Result<gdk::Texture, String> {
    let loader = glycin::Loader::new(gio::File::for_path(path));
    let mut image = loader.load().await.map_err(|error| error.to_string())?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| error.to_string())?;
    Ok(frame.texture())
}

/// Decoded art kept in memory, so tiles that rebind or scroll back draw at once.
const TEXTURE_CACHE_SIZE: usize = 256;

thread_local! {
    static TEXTURES: RefCell<(HashMap<PathBuf, gdk::Texture>, VecDeque<PathBuf>)> =
        RefCell::default();
}

fn cached_texture(path: &Path) -> Option<gdk::Texture> {
    TEXTURES.with_borrow(|(textures, _)| textures.get(path).cloned())
}

/// Keeps a texture, dropping the oldest once the cache is full.
fn cache_texture(path: PathBuf, texture: gdk::Texture) {
    TEXTURES.with_borrow_mut(|(textures, order)| {
        if textures.insert(path.clone(), texture).is_none() {
            order.push_back(path);
        }
        while order.len() > TEXTURE_CACHE_SIZE {
            if let Some(oldest) = order.pop_front() {
                textures.remove(&oldest);
            }
        }
    });
}
