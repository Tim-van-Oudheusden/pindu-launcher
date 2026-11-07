//! Finds `umu-run`, which native installs need from umu-launcher (the Flatpak bundles it),
//! and follows umu's setup of the Steam Linux Runtime through its log lines, so a first
//! start shows its download instead of looking stuck. umu exits successfully even when the
//! setup fails, so its log is the only sign of how the setup went.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

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

/// The program umu-launcher installs, which every start through umu runs.
pub const RUN: &str = "umu-run";

/// Where `umu-run` is: in the first folder of `path`, a `PATH` value, where
/// `is_executable` finds it. Empty and relative entries, which name the current folder,
/// are passed over. `None` when umu-launcher is not installed.
pub fn find_run(path: &OsStr, is_executable: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter(|folder| folder.is_absolute())
        .map(|folder| folder.join(RUN))
        .find(|file| is_executable(file))
}

/// Whether `file` is a program that can run: a file, or a link to one, with an execute
/// bit set. The check [`find_run`] makes on this computer.
pub fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    std::fs::metadata(file)
        .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
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

    #[test]
    fn umu_run_is_found_in_the_first_path_folder_that_has_it() {
        let path = OsStr::new("/usr/local/bin:/home/me/.local/bin:/usr/bin");
        let has_umu = |file: &Path| {
            file == Path::new("/home/me/.local/bin/umu-run")
                || file == Path::new("/usr/bin/umu-run")
        };

        assert_eq!(
            find_run(path, has_umu),
            Some(PathBuf::from("/home/me/.local/bin/umu-run"))
        );
    }

    #[test]
    fn without_umu_run_in_an_absolute_path_folder_there_is_none() {
        // Empty and relative entries name the current folder, not an installed umu.
        let path = OsStr::new(":bin:/usr/bin");
        let in_current_folder =
            |file: &Path| file == Path::new("umu-run") || file == Path::new("bin/umu-run");

        assert_eq!(find_run(path, in_current_folder), None);
        assert_eq!(find_run(OsStr::new(""), |_| true), None);
    }

    #[test]
    fn only_a_file_with_an_execute_bit_counts_as_installed() {
        use std::os::unix::fs::PermissionsExt;

        let bin = tempfile::tempdir().unwrap();
        let file = bin.path().join(RUN);
        assert!(!is_executable(&file));

        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!is_executable(&file));

        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(is_executable(&file));
        assert!(!is_executable(bin.path()));
    }
}
