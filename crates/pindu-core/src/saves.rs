//! Save snapshots of the Windows games Pindu runs in a prefix it manages: a plain folder
//! copy of the prefix's user profiles, `drive_c/users/*`, without caches, runtimes,
//! media and credential stores (see [`EXCLUDED`]), taken when the game exits and before
//! a restore.
//!
//! A game's snapshots live in `~/Games/pindu-launcher/save-snapshots/<native ID>/`, one
//! folder each, named after the UTC time it was taken: `files/` mirrors the paths under
//! `drive_c/users/`, and `snapshot.json` (a [`Manifest`]) says what it holds. A snapshot
//! is written into a hidden `.partial` folder and renamed into place once complete, so a
//! failed write never touches the earlier ones. Symbolic links are never followed or
//! copied. Each new snapshot can also be copied, the same way, to a folder the user picked
//! (see [`copy_to`]).

use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value as Json, json};

use crate::model::{Game, GameId, LaunchConfig, Timestamp};
use crate::proton::{self, Defaults};

/// What a snapshot leaves out of the user profiles: temporary files, installed software
/// and runtimes, launcher and anti-cheat runtimes, credential stores, embedded browser
/// profiles, caches, crash dumps and media libraries, as the spike in #536 measured them.
pub const EXCLUDED: &[Exclusion] = &[
    Exclusion::Path("Temp"),
    Exclusion::Path("AppData/Local/Temp"),
    Exclusion::Path("AppData/Local/Programs"),
    Exclusion::Path("AppData/Local/dxvk"),
    Exclusion::Path("AppData/Roaming/wine_gecko"),
    Exclusion::Path("AppData/Local/D3DSCache"),
    Exclusion::Path("AppData/Local/NVIDIA"),
    Exclusion::Path("AppData/Local/Epic Games"),
    Exclusion::Path("AppData/Local/EpicGamesLauncher"),
    Exclusion::Path("AppData/Local/EpicOnlineServicesUIHelper"),
    Exclusion::Path("AppData/Roaming/EasyAntiCheat"),
    Exclusion::Path("AppData/Roaming/Microsoft"),
    Exclusion::Path("AppData/Local/Microsoft"),
    Exclusion::Path("Pictures"),
    Exclusion::Path("Videos"),
    Exclusion::Path("Music"),
    Exclusion::Path("Downloads"),
    Exclusion::Path("Documents/Pictures"),
    Exclusion::Path("Documents/Videos"),
    Exclusion::Path("Documents/Music"),
    Exclusion::Path("Documents/Downloads"),
    Exclusion::Folder("Local Storage"),
    Exclusion::Folder("Session Storage"),
    Exclusion::Folder("IndexedDB"),
    Exclusion::Folder("*cache*"),
    Exclusion::Folder("Crashes"),
    Exclusion::Folder("CrashDumps"),
    Exclusion::Folder("Crash Reports"),
    Exclusion::File("Cookies"),
    Exclusion::File("Cookies-journal"),
    Exclusion::File("Login Data"),
    Exclusion::File("Web Data"),
];

/// A rule that leaves part of the user profiles out of a snapshot. Names match whatever
/// their case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exclusion {
    /// The folder at this path below a user folder, as `AppData/Local/Temp`.
    Path(&'static str),
    /// Every folder with this name, at any depth; `*` stands for any text, as `*cache*`.
    Folder(&'static str),
    /// Every file with this name, at any depth.
    File(&'static str),
}

impl Exclusion {
    /// The rule as a snapshot's manifest records it and a list shows it: a path below a
    /// user folder, `**/<name>/` for a folder name and `**/<name>` for a file name.
    pub fn pattern(self) -> String {
        match self {
            Self::Path(path) => format!("{path}/"),
            Self::Folder(name) => format!("**/{name}/"),
            Self::File(name) => format!("**/{name}"),
        }
    }

    /// Whether the rule leaves out the folder at `path`, its names below a user folder.
    fn excludes_folder(self, path: &[String]) -> bool {
        match self {
            Self::Path(rule) => {
                rule.split('/').count() == path.len()
                    && rule
                        .split('/')
                        .zip(path)
                        .all(|(rule, name)| name.eq_ignore_ascii_case(rule))
            }
            Self::Folder(rule) => path.last().is_some_and(|name| matches(rule, name)),
            Self::File(_) => false,
        }
    }

    /// Whether the rule leaves out a file called `name`.
    fn excludes_file(self, name: &str) -> bool {
        matches!(self, Self::File(rule) if name.eq_ignore_ascii_case(rule))
    }
}

/// Whether `name` matches `pattern`, whatever their case, where `*` in the pattern stands
/// for any text.
fn matches(pattern: &str, name: &str) -> bool {
    let (pattern, name) = (pattern.to_lowercase(), name.to_lowercase());
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = name.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.ends_with(last)
}

/// The name of a snapshot taken at `taken_at`: its UTC time, as `2026-11-19T18-42-07Z`,
/// which sorts in the order the snapshots were taken.
pub fn snapshot_name(taken_at: Timestamp) -> String {
    let (days, seconds) = (taken_at.div_euclid(86_400), taken_at.rem_euclid(86_400));
    let (year, month, day) = civil_from_days(days);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}-{:02}-{:02}Z",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's algorithm).
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// The prefix's folder of user profiles, which a snapshot holds.
pub fn users_dir(prefix: &Path) -> PathBuf {
    prefix.join("drive_c").join("users")
}

/// One file a snapshot holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFile {
    /// Its path below `drive_c/users/`, parts joined with `/`, as
    /// `steamuser/Saved Games/CD Projekt Red/save.dat`.
    pub path: String,
    /// Its size in bytes.
    pub size: u64,
    /// When it was last written, in nanoseconds since 1970-01-01 UTC.
    pub mtime_ns: i64,
}

/// The files a snapshot of `prefix` would hold now, sorted by path: every file under the
/// real folders of `drive_c/users/`, minus what [`EXCLUDED`] leaves out. Symbolic links
/// are never followed, and files whose name is not UTF-8 are left out. A prefix without
/// user profiles has none.
pub fn collect(prefix: &Path) -> io::Result<Vec<SavedFile>> {
    let mut files = Vec::new();
    let users = match fs::read_dir(users_dir(prefix)) {
        Ok(users) => users,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(files),
        Err(error) => return Err(error),
    };
    for user in users {
        let user = user?;
        if !user.file_type()?.is_dir() {
            continue;
        }
        if let Some(name) = user.file_name().to_str() {
            walk(&user.path(), name, &mut Vec::new(), &mut files)?;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Adds the files in `dir`, at `path` below `drive_c/users/` and `below_user` below its
/// user folder, and in its folders, to `files`, minus what [`EXCLUDED`] leaves out.
fn walk(
    dir: &Path,
    path: &str,
    below_user: &mut Vec<String>,
    files: &mut Vec<SavedFile>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Neither the entry's type nor its metadata follow a symbolic link.
        let kind = entry.file_type()?;
        let entry_path = format!("{path}/{name}");
        if kind.is_dir() {
            below_user.push(name);
            if !EXCLUDED.iter().any(|rule| rule.excludes_folder(below_user)) {
                walk(&entry.path(), &entry_path, below_user, files)?;
            }
            below_user.pop();
        } else if kind.is_file() && !EXCLUDED.iter().any(|rule| rule.excludes_file(&name)) {
            let metadata = entry.metadata()?;
            files.push(SavedFile {
                path: entry_path,
                size: metadata.len(),
                mtime_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
            });
        }
    }
    Ok(())
}

/// Why a snapshot was taken, which decides how many of its kind are kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// The game exited.
    Exit,
    /// A restore was about to replace the saves.
    BeforeRestore,
}

impl Reason {
    /// The reason as `snapshot.json` records it: `exit` or `before-restore`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::BeforeRestore => "before-restore",
        }
    }

    fn from_str(text: &str) -> Option<Self> {
        [Self::Exit, Self::BeforeRestore]
            .into_iter()
            .find(|reason| reason.as_str() == text)
    }
}

/// What a snapshot holds, as its `snapshot.json` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The game whose saves it holds.
    pub game: GameId,
    /// The prefix it was taken from.
    pub prefix: PathBuf,
    /// When it was taken.
    pub taken_at: Timestamp,
    /// Why it was taken.
    pub reason: Reason,
    /// The user folders it holds, relative to the prefix, as `drive_c/users/steamuser/`.
    pub include: Vec<String>,
    /// What it left out, as [`Exclusion::pattern`] writes each rule.
    pub exclude: Vec<String>,
    /// Every file it holds, sorted by path.
    pub files: Vec<SavedFile>,
}

impl Manifest {
    /// The size of the files it holds, in bytes.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|file| file.size).sum()
    }
}

/// One snapshot of a game's saves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snapshot {
    /// Its folder's name, after the UTC time it was taken.
    pub name: String,
    /// Its folder.
    pub dir: PathBuf,
    /// What it holds.
    pub manifest: Manifest,
}

impl Snapshot {
    /// The folder holding its copies of the files, mirroring `drive_c/users/`.
    pub fn files_dir(&self) -> PathBuf {
        self.dir.join(FILES)
    }
}

/// What taking a snapshot did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// It wrote this new snapshot.
    Written(Snapshot),
    /// The saves are as this newest snapshot holds them, so it wrote none.
    Unchanged(Snapshot),
    /// The prefix has no files to save, so it wrote none.
    NothingToSave,
}

/// The folder in a game's snapshots folder holding a snapshot's copies of the files.
const FILES: &str = "files";

/// Takes a snapshot of the saves in `prefix`, the prefix of `game`, into `game_dir`, the
/// game's snapshots folder. It is written into a hidden `.partial` folder and renamed
/// into place once complete, so the earlier snapshots stay as they are whatever happens;
/// a failed write removes its partial folder, and one left by a crash is removed first.
/// Nothing is written when the saves are as the newest snapshot holds them or there are
/// none. Older snapshots are not pruned here: see [`prune`].
pub fn take(
    prefix: &Path,
    game_dir: &Path,
    game: &GameId,
    reason: Reason,
    now: Timestamp,
) -> io::Result<Taken> {
    remove_partials(game_dir)?;
    let files = collect(prefix)?;
    if files.is_empty() {
        return Ok(Taken::NothingToSave);
    }
    let snapshots = list(game_dir)?;
    if let Some(newest) = snapshots.first()
        && newest.manifest.files == files
    {
        return Ok(Taken::Unchanged(newest.clone()));
    }
    let stamp = snapshot_name(now);
    let name = std::iter::once(stamp.clone())
        .chain((2..).map(|n| format!("{stamp}-{n}")))
        .find(|name| !game_dir.join(name).exists())
        .expect("some name is free");
    let mut include: Vec<String> = files
        .iter()
        .filter_map(|file| file.path.split_once('/'))
        .map(|(user, _)| format!("drive_c/users/{user}/"))
        .collect();
    include.dedup();
    let manifest = Manifest {
        game: game.clone(),
        prefix: prefix.to_owned(),
        taken_at: now,
        reason,
        include,
        exclude: EXCLUDED.iter().map(|rule| rule.pattern()).collect(),
        files,
    };
    let dir = place(game_dir, &name, &manifest, &users_dir(prefix))?;
    Ok(Taken::Written(Snapshot {
        name,
        dir,
        manifest,
    }))
}

/// What copying a snapshot to the folder the user picked did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Copied {
    /// It wrote this copy.
    Written(Snapshot),
    /// The folder already holds a snapshot of that name, so it wrote none.
    AlreadyThere,
    /// The folder is gone, as when the drive it is on is not plugged in, so it wrote none.
    FolderMissing,
}

/// Copies `snapshot` into `folder`, the folder the user picked for copies, laid out as
/// the snapshots folder is: `<folder>/<native ID>/<snapshot name>/`. Like a new snapshot,
/// the copy is written into a hidden `.partial` folder and renamed into place once
/// complete; a failed write removes its partial folder, and one left by a crash is
/// removed first. A folder that is gone is not made again: the copy waits for it.
pub fn copy_to(snapshot: &Snapshot, folder: &Path) -> io::Result<Copied> {
    if !folder.is_dir() {
        return Ok(Copied::FolderMissing);
    }
    let game_dir = game_snapshots_dir(folder, &snapshot.manifest.game);
    if game_dir.join(&snapshot.name).exists() {
        return Ok(Copied::AlreadyThere);
    }
    remove_partials(&game_dir)?;
    let dir = place(
        &game_dir,
        &snapshot.name,
        &snapshot.manifest,
        &snapshot.files_dir(),
    )?;
    Ok(Copied::Written(Snapshot {
        name: snapshot.name.clone(),
        dir,
        manifest: snapshot.manifest.clone(),
    }))
}

/// Writes the snapshot `name` in `game_dir`, holding the files `manifest` lists copied
/// from `from`: into a hidden `.partial` folder, renamed into place once complete.
/// Returns its folder. A failed write removes its partial folder.
fn place(game_dir: &Path, name: &str, manifest: &Manifest, from: &Path) -> io::Result<PathBuf> {
    let partial = game_dir.join(format!(".{name}{PARTIAL}"));
    let written = write(&partial, manifest, from).and_then(|()| {
        let dir = game_dir.join(name);
        fs::rename(&partial, &dir)?;
        fs::File::open(game_dir)?.sync_all()?;
        Ok(dir)
    });
    if written.is_err() {
        // The partial folder is no snapshot whether or not it goes now; the next
        // snapshot clears it if it stays.
        let _ = fs::remove_dir_all(&partial);
    }
    written
}

/// The end of a partial snapshot's folder name.
const PARTIAL: &str = ".partial";

/// The file in a snapshot's folder that says what it holds.
const MANIFEST: &str = "snapshot.json";

/// The format of `snapshot.json`; a newer one is not read.
const FORMAT: u64 = 1;

/// Copies the files `manifest` lists from `from` into `partial`'s `files/`, keeping their
/// times, then writes its `snapshot.json`, everything flushed to disk.
fn write(partial: &Path, manifest: &Manifest, from: &Path) -> io::Result<()> {
    let files = partial.join(FILES);
    for file in &manifest.files {
        let target = files.join(&file.path);
        fs::create_dir_all(target.parent().expect("a file is in a folder"))?;
        // On btrfs and XFS the copy shares the file's blocks until either changes.
        fs::copy(from.join(&file.path), &target)?;
        let copy = fs::OpenOptions::new().write(true).open(&target)?;
        copy.set_modified(modified(file.mtime_ns))?;
        copy.sync_all()?;
    }
    let json = serde_json::to_string_pretty(&manifest_json(manifest))
        .expect("JSON values always serialize");
    let mut out = fs::File::create(partial.join(MANIFEST))?;
    io::Write::write_all(&mut out, json.as_bytes())?;
    out.sync_all()
}

/// The time `mtime_ns` nanoseconds after 1970-01-01 UTC.
fn modified(mtime_ns: i64) -> SystemTime {
    let since = Duration::from_nanos(mtime_ns.unsigned_abs());
    if mtime_ns >= 0 {
        UNIX_EPOCH + since
    } else {
        UNIX_EPOCH - since
    }
}

/// Removes the partial snapshots in `game_dir` that a crash left behind.
fn remove_partials(game_dir: &Path) -> io::Result<()> {
    for entry in read_dir_if_any(game_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with('.') && name.ends_with(PARTIAL) && entry.file_type()?.is_dir() {
            fs::remove_dir_all(entry.path())?;
        }
    }
    Ok(())
}

/// The entries of `dir`, none when it does not exist.
fn read_dir_if_any(dir: &Path) -> io::Result<impl Iterator<Item = io::Result<fs::DirEntry>>> {
    match fs::read_dir(dir) {
        Ok(entries) => Ok(Some(entries).into_iter().flatten()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None.into_iter().flatten()),
        Err(error) => Err(error),
    }
}

/// Removes a game's older snapshots in `game_dir`, its snapshots folder: all but the
/// newest [`KEEP_EXIT`] taken on exit and the newest [`KEEP_BEFORE_RESTORE`] taken before
/// a restore, so restores never push out the snapshots taken on exit. Returns the removed
/// ones, newest first.
pub fn prune(game_dir: &Path) -> io::Result<Vec<Snapshot>> {
    let (mut exit, mut before_restore) = (0, 0);
    let mut removed = Vec::new();
    for snapshot in list(game_dir)? {
        let (kept, keep) = match snapshot.manifest.reason {
            Reason::Exit => (&mut exit, KEEP_EXIT),
            Reason::BeforeRestore => (&mut before_restore, KEEP_BEFORE_RESTORE),
        };
        if *kept < keep {
            *kept += 1;
        } else {
            fs::remove_dir_all(&snapshot.dir)?;
            removed.push(snapshot);
        }
    }
    Ok(removed)
}

/// How many snapshots taken on exit a game keeps.
pub const KEEP_EXIT: usize = 5;

/// How many snapshots taken before a restore a game keeps.
pub const KEEP_BEFORE_RESTORE: usize = 2;

/// The folder in `root`, the snapshots folder, that holds a game's snapshots: named after
/// its native ID, as its prefix is.
pub fn game_snapshots_dir(root: &Path, game: &GameId) -> PathBuf {
    root.join(game.native_id())
}

/// The prefix `game` runs in when Pindu chose it, as `<prefixes folder>/<native ID>`:
/// the only prefixes snapshots cover. A prefix the user named elsewhere, which another
/// tool may own, is not managed.
pub fn managed_prefix(game: &Game, config: &LaunchConfig, defaults: &Defaults) -> Option<PathBuf> {
    let own = defaults.prefixes.join(game.id.native_id());
    let prefix = proton::prefix(game, config, defaults);
    (prefix == own).then_some(prefix)
}

/// A game's snapshots in `game_dir`, its snapshots folder, newest first. A folder without
/// a readable `snapshot.json` is no snapshot, and a game without a folder has none.
pub fn list(game_dir: &Path) -> io::Result<Vec<Snapshot>> {
    let mut snapshots = Vec::new();
    for entry in read_dir_if_any(game_dir)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if name.starts_with('.') || !entry.file_type()?.is_dir() {
            continue;
        }
        let dir = entry.path();
        let Some(manifest) = fs::read_to_string(dir.join(MANIFEST))
            .ok()
            .and_then(|text| manifest_from(&text))
        else {
            continue;
        };
        snapshots.push(Snapshot {
            name,
            dir,
            manifest,
        });
    }
    snapshots.sort_by(|a, b| {
        (b.manifest.taken_at, b.name.len(), &b.name).cmp(&(
            a.manifest.taken_at,
            a.name.len(),
            &a.name,
        ))
    });
    Ok(snapshots)
}

fn manifest_json(manifest: &Manifest) -> Json {
    json!({
        "format": FORMAT,
        "game": manifest.game.as_str(),
        "prefix": manifest.prefix.to_string_lossy(),
        "taken_at": manifest.taken_at,
        "reason": manifest.reason.as_str(),
        "include": manifest.include,
        "exclude": manifest.exclude,
        "files": manifest.files.iter().map(|file| json!({
            "path": file.path,
            "size": file.size,
            "mtime_ns": file.mtime_ns,
        })).collect::<Vec<_>>(),
    })
}

/// The manifest `text` holds, if it is one in a format this version reads.
fn manifest_from(text: &str) -> Option<Manifest> {
    let json: Json = serde_json::from_str(text).ok()?;
    if json["format"].as_u64()? > FORMAT {
        return None;
    }
    let strings = |value: &Json| -> Option<Vec<String>> {
        value
            .as_array()?
            .iter()
            .map(|item| item.as_str().map(str::to_owned))
            .collect()
    };
    Some(Manifest {
        game: json["game"].as_str()?.parse().ok()?,
        prefix: PathBuf::from(json["prefix"].as_str()?),
        taken_at: json["taken_at"].as_i64()?,
        reason: Reason::from_str(json["reason"].as_str()?)?,
        include: strings(&json["include"])?,
        exclude: strings(&json["exclude"])?,
        files: json["files"]
            .as_array()?
            .iter()
            .map(|file| {
                Some(SavedFile {
                    path: file["path"].as_str()?.to_owned(),
                    size: file["size"].as_u64()?,
                    mtime_ns: file["mtime_ns"].as_i64()?,
                })
            })
            .collect::<Option<_>>()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, LaunchKind, Source};

    use std::os::unix::fs::PermissionsExt;

    /// 2026-11-19 18:42:07 UTC.
    const NOW: Timestamp = 1_795_113_727;

    fn celeste() -> GameId {
        GameId::new(Source::Manual, "celeste-1a2b")
    }

    /// The snapshot `take` wrote.
    fn written(taken: Taken) -> Snapshot {
        match taken {
            Taken::Written(snapshot) => snapshot,
            other => panic!("expected a new snapshot, got {other:?}"),
        }
    }

    /// The names of the folders in `dir`, sorted.
    fn entries(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn a_snapshot_copies_the_saves_and_says_what_it_holds() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot =
            written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        assert_eq!(snapshot.name, "2026-11-19T18-42-07Z");
        assert_eq!(snapshot.dir, game_dir.join("2026-11-19T18-42-07Z"));
        assert_eq!(
            fs::read_to_string(
                snapshot
                    .files_dir()
                    .join("steamuser/Saved Games/CD Projekt Red/save.dat")
            )
            .unwrap(),
            "cp77"
        );
        assert!(!snapshot.files_dir().join("steamuser/Temp").exists());
        let manifest = &snapshot.manifest;
        assert_eq!(manifest.game, celeste());
        assert_eq!(manifest.prefix, prefix.path());
        assert_eq!(manifest.taken_at, NOW);
        assert_eq!(manifest.reason, Reason::Exit);
        assert_eq!(
            manifest.include,
            ["drive_c/users/Public/", "drive_c/users/steamuser/"]
        );
        assert!(
            manifest
                .exclude
                .contains(&"AppData/Roaming/Microsoft/".to_owned())
        );
        assert!(manifest.exclude.contains(&"**/*cache*/".to_owned()));
        assert_eq!(manifest.files, collect(prefix.path()).unwrap());
        assert_eq!(manifest.size(), 4 + 4 + 7 + 4 + 6 + 6 + 4);
        assert_eq!(list(&game_dir).unwrap(), [snapshot]);
        assert_eq!(entries(&game_dir), ["2026-11-19T18-42-07Z"]);
    }

    #[test]
    fn a_game_without_snapshots_lists_none() {
        let snapshots = tempfile::tempdir().unwrap();
        assert_eq!(list(&snapshots.path().join("celeste-1a2b")).unwrap(), []);
    }

    #[test]
    fn nothing_is_written_when_the_saves_are_as_the_newest_snapshot_holds_them() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        let again = take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW + 60).unwrap();
        assert_eq!(again, Taken::Unchanged(first.clone()));
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second = written(
            take(
                prefix.path(),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW + 120,
            )
            .unwrap(),
        );
        assert_eq!(list(&game_dir).unwrap(), [second, first]);
    }

    #[test]
    fn a_prefix_without_saves_gets_no_snapshot() {
        let (prefix, snapshots) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let taken = take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap();
        assert_eq!(taken, Taken::NothingToSave);
        assert_eq!(list(&game_dir).unwrap(), []);
    }

    #[test]
    fn snapshots_taken_in_the_same_second_get_names_of_their_own() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second =
            written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        assert_eq!(second.name, "2026-11-19T18-42-07Z-2");
        assert_eq!(list(&game_dir).unwrap(), [second, first]);
    }

    #[test]
    fn a_failed_write_leaves_the_earlier_snapshots_intact() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        let unreadable = users_dir(prefix.path()).join("steamuser/Saved Games/locked.sav");
        fs::write(&unreadable, "locked").unwrap();
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW + 60).is_err());
        assert_eq!(list(&game_dir).unwrap(), [first]);
        assert_eq!(entries(&game_dir), ["2026-11-19T18-42-07Z"]);
    }

    #[test]
    fn a_write_cut_short_is_cleared_before_the_next_snapshot() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        put(
            &game_dir,
            ".2026-11-19T18-00-00Z.partial/files/steamuser/half.sav",
            "x",
        );
        take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap();
        assert_eq!(entries(&game_dir), ["2026-11-19T18-42-07Z"]);
    }

    #[test]
    fn the_last_five_exit_and_the_last_two_safety_snapshots_are_kept() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let mut taken = Vec::new();
        for (minute, reason) in [
            Reason::Exit,
            Reason::BeforeRestore,
            Reason::Exit,
            Reason::Exit,
            Reason::BeforeRestore,
            Reason::Exit,
            Reason::Exit,
            Reason::BeforeRestore,
            Reason::Exit,
            Reason::Exit,
        ]
        .into_iter()
        .enumerate()
        {
            // A size of its own, as times written this fast may be the same.
            let turn = "x".repeat(minute + 1);
            put(
                &users_dir(prefix.path()),
                "steamuser/Saved Games/turn.sav",
                &turn,
            );
            let now = NOW + minute as i64 * 60;
            taken.push(written(
                take(prefix.path(), &game_dir, &celeste(), reason, now).unwrap(),
            ));
        }
        let names = |snapshots: Vec<Snapshot>| -> Vec<String> {
            snapshots
                .into_iter()
                .map(|snapshot| snapshot.name)
                .collect()
        };
        let at = |indices: &[usize]| -> Vec<String> {
            indices
                .iter()
                .map(|&index| taken[index].name.clone())
                .collect()
        };
        assert_eq!(names(prune(&game_dir).unwrap()), at(&[2, 1, 0]));
        assert_eq!(names(list(&game_dir).unwrap()), at(&[9, 8, 7, 6, 5, 4, 3]));
        assert_eq!(entries(&game_dir).len(), 7, "the pruned folders are gone");
        assert_eq!(names(prune(&game_dir).unwrap()), at(&[]));
    }

    #[test]
    fn a_game_s_snapshots_live_in_a_folder_named_after_its_native_id() {
        assert_eq!(
            game_snapshots_dir(
                Path::new("/home/me/Games/pindu-launcher/save-snapshots"),
                &celeste()
            ),
            Path::new("/home/me/Games/pindu-launcher/save-snapshots/celeste-1a2b")
        );
    }

    #[test]
    fn only_a_prefix_pindu_chose_is_managed() {
        let game = Game::new(celeste(), "Celeste", LaunchKind::Proton, 0);
        let defaults = Defaults {
            proton_version: None,
            prefixes: "/home/me/Games/pindu-launcher/prefixes".into(),
            mangohud: false,
            gamemode: false,
        };
        let own = Path::new("/home/me/Games/pindu-launcher/prefixes/celeste-1a2b");
        let by_default = LaunchConfig::default();
        assert_eq!(
            managed_prefix(&game, &by_default, &defaults).as_deref(),
            Some(own)
        );
        let named = LaunchConfig {
            prefix_path: Some(own.to_owned()),
            ..LaunchConfig::default()
        };
        assert_eq!(
            managed_prefix(&game, &named, &defaults).as_deref(),
            Some(own)
        );
        let lutris = LaunchConfig {
            prefix_path: Some("/home/me/Games/lutris/celeste".into()),
            ..LaunchConfig::default()
        };
        assert_eq!(managed_prefix(&game, &lutris, &defaults), None);
    }

    #[test]
    fn a_snapshot_is_copied_to_the_folder_the_user_picked() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot =
            written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        let copy = match copy_to(&snapshot, copies.path()).unwrap() {
            Copied::Written(copy) => copy,
            other => panic!("expected a new copy, got {other:?}"),
        };
        let copy_dir = copies.path().join("celeste-1a2b");
        assert_eq!(copy.dir, copy_dir.join("2026-11-19T18-42-07Z"));
        assert_eq!(copy.manifest, snapshot.manifest);
        assert_eq!(list(&copy_dir).unwrap(), std::slice::from_ref(&copy));
        assert_eq!(entries(&copy_dir), ["2026-11-19T18-42-07Z"]);
        let save = "steamuser/Saved Games/CD Projekt Red/save.dat";
        assert_eq!(
            fs::read_to_string(copy.files_dir().join(save)).unwrap(),
            "cp77"
        );
        assert_eq!(
            fs::metadata(copy.files_dir().join(save))
                .unwrap()
                .modified()
                .unwrap(),
            fs::metadata(snapshot.files_dir().join(save))
                .unwrap()
                .modified()
                .unwrap()
        );
    }

    #[test]
    fn a_folder_that_is_gone_is_reported_and_not_made_again() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let snapshot = written(
            take(
                prefix.path(),
                &snapshots.path().join("celeste-1a2b"),
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let gone = copies.path().join("Unplugged drive");
        assert_eq!(copy_to(&snapshot, &gone).unwrap(), Copied::FolderMissing);
        assert!(!gone.exists());
    }

    #[test]
    fn a_snapshot_already_in_the_folder_is_not_copied_again() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot =
            written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        assert!(matches!(
            copy_to(&snapshot, copies.path()).unwrap(),
            Copied::Written(_)
        ));
        assert_eq!(
            copy_to(&snapshot, copies.path()).unwrap(),
            Copied::AlreadyThere
        );
        // The snapshots folder itself already holds every snapshot.
        assert_eq!(
            copy_to(&snapshot, snapshots.path()).unwrap(),
            Copied::AlreadyThere
        );
    }

    #[test]
    fn a_failed_copy_leaves_no_partial_folder_and_the_earlier_copies_intact() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        copy_to(&first, copies.path()).unwrap();
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second =
            written(take(prefix.path(), &game_dir, &celeste(), Reason::Exit, NOW + 60).unwrap());
        let unreadable = second.files_dir().join("steamuser/Saved Games/new.sav");
        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        assert!(copy_to(&second, copies.path()).is_err());
        let copy_dir = copies.path().join("celeste-1a2b");
        assert_eq!(entries(&copy_dir), ["2026-11-19T18-42-07Z"]);
        assert_eq!(list(&copy_dir).unwrap()[0].manifest, first.manifest);
    }

    #[test]
    fn a_copy_cut_short_is_cleared_before_the_next_copy() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let snapshot = written(
            take(
                prefix.path(),
                &snapshots.path().join("celeste-1a2b"),
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let copy_dir = copies.path().join("celeste-1a2b");
        put(
            &copy_dir,
            ".2026-11-19T18-00-00Z.partial/files/steamuser/half.sav",
            "x",
        );
        copy_to(&snapshot, copies.path()).unwrap();
        assert_eq!(entries(&copy_dir), ["2026-11-19T18-42-07Z"]);
    }

    /// Writes `contents` to `path` below `root`, making its folders.
    fn put(root: &Path, path: &str, contents: &str) {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    fn paths(files: &[SavedFile]) -> Vec<&str> {
        files.iter().map(|file| file.path.as_str()).collect()
    }

    /// A Proton prefix's user profiles with saves, caches, runtimes and credential
    /// stores, as the spike in #536 found them.
    fn prefix() -> tempfile::TempDir {
        let prefix = tempfile::tempdir().unwrap();
        let users = users_dir(prefix.path());
        for (path, contents) in [
            ("steamuser/Saved Games/CD Projekt Red/save.dat", "cp77"),
            ("steamuser/Games/Age of Empires 2 DE/profile.dat", "aoe2"),
            ("steamuser/AppData/Roaming/Celeste/settings.xml", "celeste"),
            ("steamuser/Documents/My Games/Skyrim/Saves/1.ess", "skyrim"),
            ("Public/Documents/shared.ini", "public"),
            ("steamuser/Temp/a.tmp", "x"),
            ("steamuser/AppData/Local/Temp/b.tmp", "x"),
            ("steamuser/appdata/local/PROGRAMS/launcher.exe", "x"),
            ("steamuser/AppData/Local/dxvk/game.dxvk-cache", "x"),
            ("steamuser/AppData/Roaming/Microsoft/Protect/masterkey", "x"),
            ("steamuser/AppData/Local/Microsoft/Credentials/cred", "x"),
            ("steamuser/AppData/Local/EpicGamesLauncher/login.ini", "x"),
            ("steamuser/AppData/Roaming/EasyAntiCheat/eac.log", "x"),
            (
                "steamuser/AppData/LocalLow/VRChat/Cache-WindowsPlayer/blob",
                "x",
            ),
            ("steamuser/AppData/Local/Game/ShaderCache/s.bin", "x"),
            ("steamuser/AppData/Local/Game/Crash Reports/dump.dmp", "x"),
            ("steamuser/AppData/Local/Game/CrashDumps/dump.dmp", "x"),
            ("steamuser/AppData/Local/Game/Local Storage/leveldb", "x"),
            ("steamuser/AppData/Local/Game/Profile/Cookies", "x"),
            ("steamuser/AppData/Local/Game/Profile/Login Data", "x"),
            ("steamuser/AppData/Local/Game/Profile/Preferences", "kept"),
            ("steamuser/Pictures/VRChat/shot.png", "x"),
            ("steamuser/Documents/Videos/clip.mp4", "x"),
            ("steamuser/Downloads/patch.zip", "x"),
            // Only a top-level folder of that name is the media library.
            (
                "steamuser/Documents/My Games/Game/Music/playlist.cfg",
                "kept",
            ),
        ] {
            put(&users, path, contents);
        }
        // A file directly under `users/` is no user's.
        put(&users, "stray.txt", "x");
        prefix
    }

    #[test]
    fn a_snapshot_holds_the_user_profiles_without_caches_runtimes_media_or_credentials() {
        let prefix = prefix();
        let files = collect(prefix.path()).unwrap();
        assert_eq!(
            paths(&files),
            [
                "Public/Documents/shared.ini",
                "steamuser/AppData/Local/Game/Profile/Preferences",
                "steamuser/AppData/Roaming/Celeste/settings.xml",
                "steamuser/Documents/My Games/Game/Music/playlist.cfg",
                "steamuser/Documents/My Games/Skyrim/Saves/1.ess",
                "steamuser/Games/Age of Empires 2 DE/profile.dat",
                "steamuser/Saved Games/CD Projekt Red/save.dat",
            ]
        );
        assert_eq!(files[0].size, "public".len() as u64);
    }

    #[test]
    fn a_snapshot_never_follows_or_copies_a_symbolic_link() {
        let prefix = prefix();
        let outside = tempfile::tempdir().unwrap();
        put(outside.path(), "secret.txt", "x");
        let user = users_dir(prefix.path()).join("steamuser");
        std::os::unix::fs::symlink("./Documents", user.join("My Documents")).unwrap();
        std::os::unix::fs::symlink(outside.path(), user.join("Outside")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), user.join("secret.txt"))
            .unwrap();
        std::os::unix::fs::symlink(outside.path(), users_dir(prefix.path()).join("linked"))
            .unwrap();
        let files = collect(prefix.path()).unwrap();
        assert_eq!(files.len(), 7);
        assert!(files.iter().all(|file| !file.path.contains("secret")));
        assert!(files.iter().all(|file| !file.path.contains("My Documents")));
    }

    #[test]
    fn a_prefix_without_user_profiles_holds_nothing() {
        let prefix = tempfile::tempdir().unwrap();
        assert_eq!(collect(prefix.path()).unwrap(), []);
    }

    #[test]
    fn a_snapshot_is_named_after_its_utc_time() {
        assert_eq!(snapshot_name(0), "1970-01-01T00-00-00Z");
        assert_eq!(snapshot_name(1_795_113_727), "2026-11-19T18-42-07Z");
        assert_eq!(snapshot_name(1_709_251_199), "2024-02-29T23-59-59Z");
    }
}
