//! Installing an external importer from a `.tar.gz` the user chose, replacing one, and
//! removing one.
//!
//! The archive is listed before anything is unpacked: only folders and plain files, no
//! absolute path and no `..`, and the limits on entries and unpacked size. It is then
//! unpacked into a work folder beside the importers folder, on the same filesystem, checked
//! again as a tree, and its manifest is read. Only once the user confirms is the checked
//! folder moved into the importers folder, in one rename. The work folder is deleted
//! however the install ends.

use std::path::{Component, Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use super::manifest::{MANIFEST, Manifest, is_helper_id, read_manifest};
use super::runner::size;
use crate::model::{ExternalImporterState, Source, Timestamp};
use crate::sources::Snapshot;
use crate::sources::sync::apply_snapshot_within;
use crate::store::Store;

/// How large an archive, and what it unpacks to, may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstallLimits {
    /// The largest archive, in bytes.
    pub archive_bytes: u64,
    /// The most it may unpack to, in bytes.
    pub unpacked_bytes: u64,
    /// The most files and folders it may hold.
    pub entries: usize,
}

impl InstallLimits {
    /// The limits the maintainer set: a 16 MiB archive, unpacking to 64 MiB and 1,000
    /// entries.
    pub const DEFAULT: Self = Self {
        archive_bytes: 16_777_216,
        unpacked_bytes: 67_108_864,
        entries: 1000,
    };
}

/// A helper unpacked and checked, waiting for the user to confirm. Dropping it deletes
/// its work folder.
#[derive(Debug)]
pub struct Prepared {
    /// The work folder, deleted on drop.
    work: WorkFolder,
    /// The checked helper folder inside the work folder.
    folder: PathBuf,
    /// The helper's manifest.
    pub manifest: Manifest,
    /// The manifest of the installed helper this one replaces; `Some(None)` when that
    /// one's manifest cannot be read, `None` when nothing is replaced.
    pub replaces: Option<Option<Manifest>>,
}

impl Prepared {
    /// Whether a helper this one replaces, if it is on, stays on: when this one runs where
    /// it ran, reads what it read and launches through the same schemes.
    pub fn stays_on(&self) -> bool {
        self.replaces.as_ref().is_some_and(|previous| {
            previous.as_ref().is_some_and(|previous| {
                previous.runs == self.manifest.runs
                    && previous.reads == self.manifest.reads
                    && previous.uri_schemes == self.manifest.uri_schemes
            })
        })
    }
}

/// A work folder beside the importers folder, deleted when dropped.
#[derive(Debug)]
struct WorkFolder(PathBuf);

impl WorkFolder {
    /// A new, empty work folder in `parent`.
    fn new(parent: &Path) -> Result<Self, String> {
        static COUNT: AtomicU64 = AtomicU64::new(0);
        let name = format!(
            ".importer-install-{}-{}",
            std::process::id(),
            COUNT.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(name);
        std::fs::create_dir_all(&path)
            .map_err(|error| format!("Pindu cannot unpack it: {error}"))?;
        Ok(Self(path))
    }
}

impl Drop for WorkFolder {
    fn drop(&mut self) {
        // Nothing else can be done about a folder that cannot be deleted.
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Unpacks and checks the helper in `archive`, to install into `importers`; refused, with
/// the reason for the user, when it breaks a rule or has the ID of a helper in
/// `bundled_ids`, which come with Pindu.
pub fn prepare(
    archive: &Path,
    importers: &Path,
    bundled_ids: &[String],
    limits: InstallLimits,
) -> Result<Prepared, String> {
    let length = std::fs::metadata(archive)
        .map_err(|error| format!("the archive cannot be read: {error}"))?
        .len();
    if length > limits.archive_bytes {
        return Err(format!(
            "the archive is larger than {}",
            size(limits.archive_bytes)
        ));
    }
    check_listing(archive, limits)?;
    let parent = importers
        .parent()
        .ok_or("the importers folder has no parent folder")?;
    let work = WorkFolder::new(parent)?;
    let unpacked = work.0.join("unpacked");
    std::fs::create_dir(&unpacked).map_err(|error| format!("Pindu cannot unpack it: {error}"))?;
    tar(&[
        "--extract".as_ref(),
        "--gzip".as_ref(),
        "--no-same-owner".as_ref(),
        "--file".as_ref(),
        archive.as_os_str(),
        "--directory".as_ref(),
        unpacked.as_os_str(),
    ])?;
    let mut entries = 0;
    let mut bytes = 0;
    check_tree(&unpacked, &unpacked, limits, &mut entries, &mut bytes)?;
    let folder = helper_folder(&unpacked)?;
    let manifest = read_manifest(&folder)?;
    if bundled_ids.contains(&manifest.id) {
        return Err(format!(
            "a helper that comes with Pindu has its ID “{}”",
            manifest.id
        ));
    }
    let installed = importers.join(&manifest.id);
    let replaces = installed.is_dir().then(|| read_manifest(&installed).ok());
    Ok(Prepared {
        work,
        folder,
        manifest,
        replaces,
    })
}

/// Runs `tar` with `arguments` in the C locale; its output, or why the archive cannot be
/// read.
fn tar(arguments: &[&std::ffi::OsStr]) -> Result<String, String> {
    let output = Command::new("tar")
        .env("LC_ALL", "C")
        .args(arguments)
        .output()
        .map_err(|error| format!("the archive cannot be read: {error}"))?;
    if !output.status.success() {
        let message = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "the archive cannot be read: {}",
            message.lines().last().unwrap_or("").trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Checks the archive's listing before anything is unpacked: only folders and plain files,
/// no absolute path and no `..`, and the limits on entries and unpacked size.
fn check_listing(archive: &Path, limits: InstallLimits) -> Result<(), String> {
    let list = |verbose: bool| {
        let mut arguments: Vec<&std::ffi::OsStr> = vec![
            "--list".as_ref(),
            "--gzip".as_ref(),
            "--absolute-names".as_ref(),
            "--numeric-owner".as_ref(),
            "--quoting-style=escape".as_ref(),
        ];
        if verbose {
            arguments.push("--verbose".as_ref());
        }
        arguments.extend(["--file".as_ref(), archive.as_os_str()]);
        tar(&arguments)
    };
    let names = list(false)?;
    let details = list(true)?;
    let names: Vec<&str> = names.lines().collect();
    if names.len() > limits.entries {
        return Err(format!(
            "the archive holds more than {} files and folders",
            limits.entries
        ));
    }
    let mut bytes: u64 = 0;
    for (name, detail) in names.iter().zip(details.lines()) {
        let path = Path::new(name);
        if path.is_absolute() {
            return Err(format!("“{name}” in the archive is an absolute path"));
        }
        if path
            .components()
            .any(|component| component == Component::ParentDir)
        {
            return Err(format!("“{name}” in the archive leads out of its folder"));
        }
        match detail.chars().next() {
            Some('-' | 'd') => {}
            Some('l' | 'h') => return Err(format!("“{name}” in the archive is a link")),
            _ => return Err(format!("“{name}” in the archive is a special file")),
        }
        let length = detail
            .split_whitespace()
            .nth(2)
            .and_then(|length| length.parse::<u64>().ok())
            .ok_or_else(|| format!("the size of “{name}” in the archive cannot be read"))?;
        bytes = bytes.saturating_add(length);
    }
    if bytes > limits.unpacked_bytes {
        return Err(format!(
            "the archive unpacks to more than {}",
            size(limits.unpacked_bytes)
        ));
    }
    Ok(())
}

/// Checks the unpacked tree below `root` again, as it is on disk: only folders and plain
/// files, within the limits. `entries` and `bytes` count what was seen so far.
fn check_tree(
    root: &Path,
    dir: &Path,
    limits: InstallLimits,
    entries: &mut usize,
    bytes: &mut u64,
) -> Result<(), String> {
    let read = |error: std::io::Error| format!("the unpacked helper cannot be read: {error}");
    for entry in std::fs::read_dir(dir).map_err(read)? {
        let entry = entry.map_err(read)?;
        let metadata = entry.path().symlink_metadata().map_err(read)?;
        let name = entry
            .path()
            .strip_prefix(root)
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        *entries += 1;
        if *entries > limits.entries {
            return Err(format!(
                "the archive holds more than {} files and folders",
                limits.entries
            ));
        }
        if metadata.is_symlink() {
            return Err(format!("“{name}” in the archive is a link"));
        } else if metadata.is_dir() {
            check_tree(root, &entry.path(), limits, entries, bytes)?;
        } else if metadata.is_file() {
            *bytes = bytes.saturating_add(metadata.len());
            if *bytes > limits.unpacked_bytes {
                return Err(format!(
                    "the archive unpacks to more than {}",
                    size(limits.unpacked_bytes)
                ));
            }
        } else {
            return Err(format!("“{name}” in the archive is a special file"));
        }
    }
    Ok(())
}

/// The helper's folder in the unpacked tree: the tree itself when it holds the manifest,
/// else its one folder that does.
fn helper_folder(unpacked: &Path) -> Result<PathBuf, String> {
    if unpacked.join(MANIFEST).is_file() {
        return Ok(unpacked.to_owned());
    }
    let entries: Vec<PathBuf> = std::fs::read_dir(unpacked)
        .map_err(|error| format!("the unpacked helper cannot be read: {error}"))?
        .flatten()
        .map(|entry| entry.path())
        .collect();
    match entries.as_slice() {
        [folder] if folder.join(MANIFEST).is_file() => Ok(folder.clone()),
        _ => Err(format!(
            "the archive holds neither {MANIFEST} nor one folder with it"
        )),
    }
}

/// Installs a confirmed helper into `importers`: in one rename, replacing the installed
/// helper with its ID. A new helper is off until the user turns it on; a replaced one stays
/// on only when it runs where it ran, reads what it read and launches through the same
/// schemes. Its games keep their IDs.
pub fn install(prepared: Prepared, importers: &Path, store: &Store) -> Result<(), String> {
    let failed = |error: std::io::Error| format!("Pindu cannot install it: {error}");
    std::fs::create_dir_all(importers).map_err(failed)?;
    let id = &prepared.manifest.id;
    let target = importers.join(id);
    let previous = prepared.work.0.join("previous");
    let replacing = target.exists();
    if replacing {
        std::fs::rename(&target, &previous).map_err(failed)?;
    }
    if let Err(error) = std::fs::rename(&prepared.folder, &target) {
        if replacing {
            // Put the installed helper back; the install failed either way.
            let _ = std::fs::rename(&previous, &target);
        }
        return Err(failed(error));
    }
    let stored = |error| format!("Pindu cannot record it: {error}");
    match &prepared.replaces {
        Some(_) => {
            if !prepared.stays_on()
                && let Some(mut state) = store.external_importer(id).map_err(stored)?
            {
                state.enabled = false;
                store.set_external_importer(&state).map_err(stored)?;
            }
        }
        None => store
            .set_external_importer(&ExternalImporterState::new(id))
            .map_err(stored)?,
    }
    Ok(())
}

/// Removes the installed helper `helper_id` from `importers`: its folder and its state.
/// Its games get `missing_since`, as the games of a source no longer on the machine do,
/// and keep their data.
pub fn remove(
    store: &Store,
    importers: &Path,
    helper_id: &str,
    now: Timestamp,
) -> Result<(), String> {
    let folder = importers.join(helper_id);
    if !is_helper_id(helper_id) || !folder.is_dir() {
        return Err(format!("“{helper_id}” is not installed"));
    }
    std::fs::remove_dir_all(&folder).map_err(|error| format!("Pindu cannot remove it: {error}"))?;
    let stored = |error| format!("Pindu cannot record it: {error}");
    store.delete_external_importer(helper_id).map_err(stored)?;
    apply_snapshot_within(
        store,
        &Snapshot::new(Source::External),
        now,
        &format!("{helper_id}/"),
    )
    .map_err(stored)?;
    Ok(())
}

#[cfg(test)]
mod tests;
