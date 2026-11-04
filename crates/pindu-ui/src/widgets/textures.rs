//! Game art decoded by glycin's sandboxed loaders, with the last few hundred images kept
//! in memory so tiles that rebind or scroll back, and pages that reopen, draw at once.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use gtk::{gdk, gio, glib};

const CACHE_SIZE: usize = 256;

thread_local! {
    static TEXTURES: RefCell<(HashMap<PathBuf, gdk::Texture>, VecDeque<PathBuf>)> =
        RefCell::default();
}

/// The decoded image, if it is in memory.
pub fn cached(path: &Path) -> Option<gdk::Texture> {
    TEXTURES.with_borrow(|(textures, _)| textures.get(path).cloned())
}

/// Decodes an image file, or returns it from memory.
pub async fn load(path: &Path) -> Result<gdk::Texture, String> {
    if let Some(texture) = cached(path) {
        return Ok(texture);
    }
    let decoded = decode(path).await;
    match &decoded {
        Ok(texture) => remember(path.to_owned(), texture.clone()),
        Err(error) => glib::g_debug!("pindu", "Could not load {}: {error}", path.display()),
    }
    decoded
}

async fn decode(path: &Path) -> Result<gdk::Texture, String> {
    let loader = glycin::Loader::new(gio::File::for_path(path));
    let mut image = loader.load().await.map_err(|error| error.to_string())?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| error.to_string())?;
    Ok(frame.texture())
}

/// Keeps a texture, dropping the oldest once the cache is full.
fn remember(path: PathBuf, texture: gdk::Texture) {
    TEXTURES.with_borrow_mut(|(textures, order)| {
        if textures.insert(path.clone(), texture).is_none() {
            order.push_back(path);
        }
        while order.len() > CACHE_SIZE {
            if let Some(oldest) = order.pop_front() {
                textures.remove(&oldest);
            }
        }
    });
}
