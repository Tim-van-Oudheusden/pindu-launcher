//! Game art decoded by glycin's sandboxed loaders, with the last few hundred images kept
//! in memory so tiles that rebind or scroll back, and pages that reopen, draw at once.
//! Covers are also stored resized per tile size in the art cache, so a big library does
//! not decode full-size art on every start.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gtk::prelude::*;
use gtk::{gdk, gio, glib};
use pindu_core::art_cache::{default_cache_dir, downscale, resized_path, resized_size};

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
    first_frame(glycin::Loader::new(gio::File::for_path(path))).await
}

/// Decodes an image held in memory, such as a program's icon; not kept.
pub async fn decode_bytes(bytes: &[u8]) -> Result<gdk::Texture, String> {
    first_frame(glycin::Loader::new_bytes(glib::Bytes::from(bytes))).await
}

async fn first_frame(loader: glycin::Loader) -> Result<gdk::Texture, String> {
    let mut image = loader.load().await.map_err(|error| error.to_string())?;
    let frame = image
        .next_frame()
        .await
        .map_err(|error| error.to_string())?;
    Ok(frame.texture())
}

/// The resized cover, if it is in memory.
pub fn cached_cover(path: &Path, width: u32, height: u32) -> Option<gdk::Texture> {
    cached(&cover_key(path, width, height))
}

fn cover_key(path: &Path, width: u32, height: u32) -> PathBuf {
    PathBuf::from(format!("{}@{width}x{height}", path.display()))
}

/// The cover at `path`, shrunk to cover a `width`×`height` tile: from memory, from the
/// art cache, or decoded and then stored resized in the art cache.
pub async fn load_cover(path: &Path, width: u32, height: u32) -> Result<gdk::Texture, String> {
    let key = cover_key(path, width, height);
    if let Some(texture) = cached(&key) {
        return Ok(texture);
    }
    let source = path.to_owned();
    let cached_file = gio::spawn_blocking(move || {
        let modified = std::fs::metadata(&source)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |elapsed| elapsed.as_secs() as i64);
        let file = resized_path(&default_cache_dir(), &source, modified, width, height);
        // Our own resized PNG, so GTK's loader is enough; anything wrong means a rebuild.
        let texture = gdk::Texture::from_filename(&file).ok();
        (file, texture)
    })
    .await
    .map_err(|_| "the art cache lookup stopped".to_owned())?;
    let texture = match cached_file {
        (_, Some(texture)) => texture,
        (file, None) => {
            let full = decode(path).await.inspect_err(|error| {
                glib::g_debug!("pindu", "Could not load {}: {error}", path.display());
            })?;
            shrink_and_store(full, width, height, file).await
        }
    };
    remember(key, texture.clone());
    Ok(texture)
}

/// Shrinks a texture bigger than the tile and writes it to the art cache, off the main
/// thread; a texture no bigger than the tile is returned as it is.
async fn shrink_and_store(
    full: gdk::Texture,
    width: u32,
    height: u32,
    file: PathBuf,
) -> gdk::Texture {
    let (full_width, full_height) = (full.width() as u32, full.height() as u32);
    let Some((target_width, target_height)) = resized_size(full_width, full_height, width, height)
    else {
        return full;
    };
    let mut downloader = gdk::TextureDownloader::new(&full);
    downloader.set_format(gdk::MemoryFormat::R8g8b8a8Premultiplied);
    let (pixels, stride) = downloader.download_bytes();
    let shrunk = gio::spawn_blocking(move || {
        let small = downscale(
            &pixels,
            full_width as usize,
            full_height as usize,
            stride,
            target_width as usize,
            target_height as usize,
        );
        let texture = gdk::MemoryTexture::new(
            target_width as i32,
            target_height as i32,
            gdk::MemoryFormat::R8g8b8a8Premultiplied,
            &glib::Bytes::from_owned(small),
            target_width as usize * 4,
        );
        if let Some(dir) = file.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
                .map_err(|error| error.to_string())
                .and_then(|()| {
                    texture
                        .save_to_png(&file)
                        .map_err(|error| error.to_string())
                })
        {
            glib::g_debug!("pindu", "Could not store {}: {error}", file.display());
        }
        texture.upcast::<gdk::Texture>()
    })
    .await;
    shrunk.unwrap_or(full)
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
