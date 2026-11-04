//! Resized copies of covers, one per tile size, under `$XDG_CACHE_HOME/pindu-launcher/art`.
//! Everything here can be rebuilt from the source art, so the folder is safe to delete.

use std::path::{Path, PathBuf};

use crate::paths::APP_DIR;
use crate::sources::sha256::sha256_hex;

/// `$XDG_CACHE_HOME/pindu-launcher/art`, falling back to `~/.cache` when unset or relative.
pub fn cache_dir(xdg_cache_home: Option<&Path>, home: &Path) -> PathBuf {
    let cache_home = match xdg_cache_home {
        Some(dir) if dir.is_absolute() => dir.to_owned(),
        _ => home.join(".cache"),
    };
    cache_home.join(APP_DIR).join("art")
}

/// The art cache for the current process environment.
pub fn default_cache_dir() -> PathBuf {
    let xdg_cache_home = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from);
    cache_dir(
        xdg_cache_home.as_deref(),
        &std::env::home_dir().unwrap_or_default(),
    )
}

/// Where the copy of `source` resized for a `width`×`height` tile lives. The source's
/// modification time is part of the name, so changed art gets a fresh copy.
pub fn resized_path(
    cache_dir: &Path,
    source: &Path,
    modified: i64,
    width: u32,
    height: u32,
) -> PathBuf {
    let key = format!("{}\n{modified}", source.display());
    cache_dir
        .join("covers")
        .join(format!("{width}x{height}"))
        .join(format!("{}.png", sha256_hex(key.as_bytes())))
}

/// The size to store `source_width`×`source_height` art at so it still covers a
/// `tile_width`×`tile_height` tile, or `None` when it is no bigger than that already.
pub fn resized_size(
    source_width: u32,
    source_height: u32,
    tile_width: u32,
    tile_height: u32,
) -> Option<(u32, u32)> {
    let scale = f64::max(
        f64::from(tile_width) / f64::from(source_width),
        f64::from(tile_height) / f64::from(source_height),
    );
    if scale >= 1.0 {
        return None;
    }
    let size = |length: u32| ((f64::from(length) * scale).round() as u32).max(1);
    Some((size(source_width), size(source_height)))
}

/// Shrinks 4-byte-per-pixel image data by averaging the source area behind each target
/// pixel. Premultiplied alpha averages correctly; `stride` is the source row length in
/// bytes.
pub fn downscale(
    pixels: &[u8],
    width: usize,
    height: usize,
    stride: usize,
    target_width: usize,
    target_height: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(target_width * target_height * 4);
    for ty in 0..target_height {
        let (y0, y1) = (
            ty * height / target_height,
            ((ty + 1) * height).div_ceil(target_height),
        );
        for tx in 0..target_width {
            let (x0, x1) = (
                tx * width / target_width,
                ((tx + 1) * width).div_ceil(target_width),
            );
            let mut sums = [0u64; 4];
            for y in y0..y1 {
                let row = &pixels[y * stride..];
                for pixel in row[x0 * 4..x1 * 4].as_chunks::<4>().0 {
                    for (sum, channel) in sums.iter_mut().zip(pixel) {
                        *sum += u64::from(*channel);
                    }
                }
            }
            let count = ((y1 - y0) * (x1 - x0)) as u64;
            out.extend(sums.iter().map(|sum| ((sum + count / 2) / count) as u8));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn the_cache_lives_in_xdg_cache_home() {
        assert_eq!(
            cache_dir(Some(Path::new("/cache")), Path::new("/home/me")),
            Path::new("/cache/pindu-launcher/art")
        );
        assert_eq!(
            cache_dir(None, Path::new("/home/me")),
            Path::new("/home/me/.cache/pindu-launcher/art")
        );
        assert_eq!(
            cache_dir(Some(Path::new("relative")), Path::new("/home/me")),
            Path::new("/home/me/.cache/pindu-launcher/art")
        );
    }

    #[test]
    fn each_tile_size_and_source_version_has_its_own_file() {
        let dir = Path::new("/cache/art");
        let source = Path::new("/steam/librarycache/570/library_600x900.jpg");

        let medium = resized_path(dir, source, 100, 400, 600);
        let large = resized_path(dir, source, 100, 500, 750);
        let changed = resized_path(dir, source, 200, 400, 600);

        assert!(medium.starts_with("/cache/art/covers/400x600"));
        assert_eq!(medium.extension().unwrap(), "png");
        assert_ne!(medium, large);
        assert_ne!(medium.file_name(), changed.file_name());
        assert_eq!(medium, resized_path(dir, source, 100, 400, 600));
    }

    #[test]
    fn big_art_is_scaled_to_just_cover_the_tile() {
        // 600×900 into a 400×600 tile: exactly the tile.
        assert_eq!(resized_size(600, 900, 400, 600), Some((400, 600)));
        // 3:4 art keeps its shape and covers the tile.
        assert_eq!(resized_size(900, 1200, 400, 600), Some((450, 600)));
        // Landscape art keeps its shape too.
        assert_eq!(resized_size(1920, 620, 400, 600), Some((1858, 600)));
    }

    #[test]
    fn art_no_bigger_than_the_tile_is_left_alone() {
        assert_eq!(resized_size(300, 450, 400, 600), None);
        assert_eq!(resized_size(460, 215, 400, 600), None);
    }

    #[test]
    fn downscaling_averages_each_area() {
        // A 2×2 image of four colours becomes one pixel of their average.
        let pixels = [
            255, 0, 0, 255, 0, 255, 0, 255, //
            0, 0, 255, 255, 255, 255, 255, 255,
        ];

        let small = downscale(&pixels, 2, 2, 8, 1, 1);

        assert_eq!(small, [128, 128, 128, 255]);
    }

    #[test]
    fn downscaling_keeps_a_flat_colour_flat_at_any_ratio() {
        let pixels: Vec<u8> = [10, 20, 30, 255].repeat(7 * 5);

        let small = downscale(&pixels, 7, 5, 7 * 4, 3, 2);

        assert_eq!(small, [10, 20, 30, 255].repeat(3 * 2));
    }

    #[test]
    fn downscaling_honours_the_row_stride() {
        // Each row carries 4 bytes of padding that must be skipped.
        let pixels = [
            1, 2, 3, 4, 1, 2, 3, 4, 99, 99, 99, 99, //
            1, 2, 3, 4, 1, 2, 3, 4, 99, 99, 99, 99,
        ];

        assert_eq!(downscale(&pixels, 2, 2, 12, 1, 1), [1, 2, 3, 4]);
    }
}
