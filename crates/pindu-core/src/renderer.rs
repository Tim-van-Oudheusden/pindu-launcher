//! The GTK renderer Pindu asks for. GTK picks its own almost everywhere; on a virtual
//! machine's virtio-gpu with 3D acceleration (virgl, with Venus for Vulkan) its Vulkan
//! renderer aborts the app when Venus fails to import a frame of Home's background video
//! (#359), so Pindu asks for GTK's GL renderer there. Plain reads of sysfs.

use std::ffi::OsStr;
use std::path::Path;

/// Where the kernel lists the DRM devices.
pub const SYSFS_DRM: &str = "/sys/class/drm";

/// The environment variable GTK reads its renderer from.
pub const VARIABLE: &str = "GSK_RENDERER";

/// The renderer to ask GTK for in [`VARIABLE`], or `None` to leave GTK's own choice.
/// `drm` is normally [`SYSFS_DRM`]; `chosen` is the variable's value when it is set, and a
/// renderer the user chose is left alone.
pub fn renderer(drm: &Path, chosen: Option<&OsStr>) -> Option<&'static str> {
    if chosen.is_some() {
        return None;
    }
    std::fs::read_dir(drm)
        .into_iter()
        .flatten()
        .flatten()
        .any(|node| is_virtio_gpu_3d(&node.path().join("device")))
        .then_some("gl")
}

/// Whether `device`, a DRM device's parent in sysfs, holds a virtio-gpu with 3D
/// acceleration: its `features` attribute, one character per feature bit from bit 0, has
/// `VIRTIO_GPU_F_VIRGL` (bit 0) set.
fn is_virtio_gpu_3d(device: &Path) -> bool {
    std::fs::read_dir(device)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("virtio"))
        .any(|virtio| {
            let read = |attribute: &str| std::fs::read_to_string(virtio.path().join(attribute));
            read("device").is_ok_and(|id| id.trim() == VIRTIO_ID_GPU)
                && read("features").is_ok_and(|features| features.starts_with('1'))
        })
}

/// The virtio device ID of a GPU, as sysfs writes it.
const VIRTIO_ID_GPU: &str = "0x0010";

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/drm")
            .join(name)
            .join("sys/class/drm")
    }

    #[test]
    fn a_virtio_gpu_with_3d_acceleration_gets_the_gl_renderer() {
        assert_eq!(renderer(&fixture("virtio-gpu-3d"), None), Some("gl"));
    }

    #[test]
    fn a_virtio_gpu_without_3d_acceleration_keeps_gtks_choice() {
        assert_eq!(renderer(&fixture("virtio-gpu-2d"), None), None);
    }

    #[test]
    fn a_renderer_the_user_chose_wins() {
        let chosen = OsStr::new("vulkan");
        assert_eq!(renderer(&fixture("virtio-gpu-3d"), Some(chosen)), None);
    }

    #[test]
    fn a_real_gpu_keeps_gtks_choice() {
        assert_eq!(renderer(&fixture("radeon"), None), None);
    }
}
