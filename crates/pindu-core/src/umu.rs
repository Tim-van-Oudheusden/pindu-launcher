//! Follows umu's setup of the Steam Linux Runtime through its log lines, so a first start
//! shows its download instead of looking stuck. umu exits successfully even when the
//! setup fails, so its log is the only sign of how the setup went.

use std::path::PathBuf;

/// A step of umu's runtime setup, in the order a first start goes through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeSetup {
    /// umu found no runtime and starts installing one.
    Started,
    /// The runtime archive is downloading.
    Downloading,
    /// The download checked out and is being unpacked.
    Unpacking,
    /// The unpacked runtime is being checked.
    Verifying,
    /// The runtime is ready and the game starts.
    Ready,
    /// There is no runtime and no internet connection to download one: the game cannot
    /// start.
    Offline,
}

/// The second line of umu's error when it has no runtime and cannot download one.
const OFFLINE: &str = "An internet connection is required to setup umu";

/// The setup step a line of umu's log reports, if any.
pub fn setup_event(line: &str) -> Option<RuntimeSetup> {
    if line == OFFLINE {
        return Some(RuntimeSetup::Offline);
    }
    let message = umu_info(line)?;
    Some(if message.starts_with("Setting up Unified Launcher") {
        RuntimeSetup::Started
    } else if message.starts_with("Downloading ") && message.ends_with("please wait...")
        || message.starts_with("Updating ") && message.ends_with("...")
        || message.starts_with("Found '") && message.ends_with("in cache, resuming...")
    {
        RuntimeSetup::Downloading
    } else if message.ends_with(": SHA256 is OK") {
        RuntimeSetup::Unpacking
    } else if message.starts_with("Verifying integrity of ") {
        RuntimeSetup::Verifying
    } else if message.starts_with("Using ") && message.ends_with(" (latest)")
        || message.starts_with("Running '")
    {
        RuntimeSetup::Ready
    } else {
        return None;
    })
}

/// The message of one of umu's own info lines: `INFO: …`, or `[umu.<module>:<line>] INFO: …`
/// with `UMU_LOG=debug`. Proton's lines, such as protonfixes', are not umu's.
fn umu_info(line: &str) -> Option<&str> {
    if let Some(message) = line.strip_prefix("INFO: ") {
        return Some(message);
    }
    let (_, message) = line.strip_prefix("[umu.")?.split_once("] INFO: ")?;
    Some(message)
}

/// How much of the runtime umu has downloaded: the size of its partial archive,
/// `SteamLinuxRuntime_*.parts`, in a temporary folder directly under one of `dirs` (the
/// temporary folder, then umu's cache when `/tmp` is too small). `None` before the
/// download starts.
pub fn downloaded_bytes(dirs: &[PathBuf]) -> Option<u64> {
    dirs.iter()
        .filter_map(|dir| std::fs::read_dir(dir).ok())
        .flatten()
        .flatten()
        .filter_map(|folder| std::fs::read_dir(folder.path()).ok())
        .flatten()
        .flatten()
        .find(|file| {
            let name = file.file_name();
            let name = name.to_string_lossy();
            name.starts_with("SteamLinuxRuntime_") && name.ends_with(".parts")
        })
        .and_then(|file| file.metadata().ok())
        .map(|metadata| metadata.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_first_start_moves_from_download_to_ready() {
        let log = [
            "INFO: umu-launcher version 1.4.4 (3.14.9 (main, Nov 10 2011, 15:00:00) [GCC 16.2.0])",
            "CRITICAL: steamrt4 validation failed",
            "CRITICAL: Could not find steamrt4_platform_* in '/home/me/.local/share/umu/steamrt4'",
            "INFO: Setting up Unified Launcher for Windows Games on Linux...",
            "INFO: Downloading steamrt4 (4.0.20261021.265997), please wait...",
            "INFO: SteamLinuxRuntime_4.tar.xz: SHA256 is OK",
            "INFO: Verifying integrity of steamrt4_platform_4.0.20261021.265997...",
            "INFO: steamrt4_platform_4.0.20261021.265997: mtree is OK",
            "INFO: Using steamrt4 (latest)",
            "INFO: Running 'GE-Proton11-3' using runtime 'steamrt4'",
            "ProtonFixes[74] INFO: Downloading upscaler file \"amdxcffx64.dll\"",
        ];

        let events: Vec<_> = log.iter().filter_map(|line| setup_event(line)).collect();

        assert_eq!(
            events,
            [
                RuntimeSetup::Started,
                RuntimeSetup::Downloading,
                RuntimeSetup::Unpacking,
                RuntimeSetup::Verifying,
                RuntimeSetup::Ready,
                RuntimeSetup::Ready,
            ]
        );
    }

    #[test]
    fn a_first_start_without_internet_reports_it() {
        let log = [
            "INFO: umu-launcher version 1.4.4 (3.14.9 (main, Nov 10 2011, 15:00:00) [GCC 16.2.0])",
            "CRITICAL: steamrt4 validation failed",
            "ERROR: umu has not been setup for the user",
            "An internet connection is required to setup umu",
            "Traceback (most recent call last):",
            "RuntimeError: umu has not been setup for the user",
            "An internet connection is required to setup umu",
        ];

        let events: Vec<_> = log.iter().filter_map(|line| setup_event(line)).collect();

        assert_eq!(events, [RuntimeSetup::Offline, RuntimeSetup::Offline]);
        assert_eq!(
            setup_event(
                "[umu.umu_runtime:657] INFO: Running 'GE-Proton11-3' using runtime 'steamrt4'"
            ),
            Some(RuntimeSetup::Ready)
        );
    }

    #[test]
    fn runtime_updates_and_resumed_downloads_are_downloads_too() {
        assert_eq!(
            setup_event("INFO: Updating steamrt4 to 4.0.20261104.270000..."),
            Some(RuntimeSetup::Downloading)
        );
        assert_eq!(
            setup_event(
                "INFO: Found 'SteamLinuxRuntime_4.tar.xz.4.0.20261021.265997.parts' in cache, resuming..."
            ),
            Some(RuntimeSetup::Downloading)
        );
        assert_eq!(setup_event("INFO: steamrt4 is up to date"), None);
    }

    #[test]
    fn the_download_so_far_is_the_size_of_umus_partial_runtime_file() {
        let tmp = tempfile::tempdir().unwrap();
        let cache = tempfile::tempdir().unwrap();
        let dirs = [tmp.path().to_owned(), cache.path().to_owned()];
        assert_eq!(downloaded_bytes(&dirs), None);

        let download = tmp.path().join("tmpdyvi0_fq");
        std::fs::create_dir(&download).unwrap();
        std::fs::write(
            download.join("SteamLinuxRuntime_4.tar.xz.4.0.1.parts"),
            [0; 1234],
        )
        .unwrap();
        let other = tmp.path().join("tmpother");
        std::fs::create_dir(&other).unwrap();
        std::fs::write(other.join("unrelated.parts"), [0; 99]).unwrap();

        assert_eq!(downloaded_bytes(&dirs), Some(1234));
    }
}
