//! Save snapshots of the games Pindu knows: a plain folder copy of the user profiles,
//! `drive_c/users/*`, of the prefix Pindu manages for a Windows game, without caches,
//! runtimes, media and credential stores (see [`EXCLUDED`]), and of the save folders the
//! user added for any game (see [`Roots`]), taken when the game exits, by hand and before
//! a restore.
//!
//! A game's snapshots live in `~/Games/pindu-launcher/save-snapshots/<native ID>/`, one
//! folder each, named after the UTC time it was taken: `files/` mirrors the paths under
//! `drive_c/users/`, `folders/<index>/` each save folder, and `snapshot.json` (a
//! [`Manifest`]) says what it holds. A snapshot is written into a hidden `.partial` folder
//! and renamed into place once complete, so a failed write never touches the earlier
//! ones. Symbolic links are never followed or copied. Each new snapshot can also be
//! copied, the same way, to a folder the user picked (see [`copy_to`]). [`restore`] puts a
//! snapshot back after a safety snapshot, never while the game runs, and writes only
//! inside the user folders of the prefix's `drive_c/users/` and the save folders the user
//! added.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde_json::{Value as Json, json};

use crate::model::{Game, GameId, LaunchConfig, LaunchKind, Timestamp};
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
    /// Its path below `drive_c/users/`, or below its save folder, parts joined with `/`,
    /// as `steamuser/Saved Games/CD Projekt Red/save.dat`.
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
            walk(
                &user.path(),
                name,
                &mut Vec::new(),
                Within::Prefix,
                &mut files,
            )?;
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The files a snapshot of `folder`, a save folder the user added, would hold now, with
/// their paths below it, sorted by path: every file in it and its folders, minus what the
/// rules of [`EXCLUDED`] for names at any depth leave out; those for paths below a user
/// folder are the prefix's alone. Symbolic links below it are never followed, and files
/// whose name is not UTF-8 are left out.
pub fn collect_folder(folder: &Path) -> io::Result<Vec<SavedFile>> {
    let mut files = Vec::new();
    walk(folder, "", &mut Vec::new(), Within::Folder, &mut files)?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// Where a walk through saves is, which decides the rules of [`EXCLUDED`] that apply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Within {
    /// The prefix's `drive_c/users/`, where every rule applies.
    Prefix,
    /// A save folder the user added, where only the rules for names at any depth apply.
    Folder,
}

impl Within {
    /// The rules of [`EXCLUDED`] that apply here.
    fn rules(self) -> impl Iterator<Item = Exclusion> {
        EXCLUDED
            .iter()
            .copied()
            .filter(move |rule| self == Self::Prefix || !matches!(rule, Exclusion::Path(_)))
    }
}

/// Adds the files in `dir`, at `path` below `drive_c/users/` or the save folder (empty
/// for the save folder itself) and `below_user` below its user folder or the save folder,
/// and in its folders, to `files`, minus what the rules that apply `within` leave out.
fn walk(
    dir: &Path,
    path: &str,
    below_user: &mut Vec<String>,
    within: Within,
    files: &mut Vec<SavedFile>,
) -> io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // Neither the entry's type nor its metadata follow a symbolic link.
        let kind = entry.file_type()?;
        let entry_path = if path.is_empty() {
            name.clone()
        } else {
            format!("{path}/{name}")
        };
        if kind.is_dir() {
            below_user.push(name);
            if !within.rules().any(|rule| rule.excludes_folder(below_user)) {
                walk(&entry.path(), &entry_path, below_user, within, files)?;
            }
            below_user.pop();
        } else if kind.is_file() && !within.rules().any(|rule| rule.excludes_file(&name)) {
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
    /// The user took it by hand. It is kept as one taken on exit is.
    Manual,
    /// A restore was about to replace the saves.
    BeforeRestore,
}

impl Reason {
    /// The reason as `snapshot.json` records it: `exit`, `manual` or `before-restore`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exit => "exit",
            Self::Manual => "manual",
            Self::BeforeRestore => "before-restore",
        }
    }

    fn from_str(text: &str) -> Option<Self> {
        [Self::Exit, Self::Manual, Self::BeforeRestore]
            .into_iter()
            .find(|reason| reason.as_str() == text)
    }
}

/// One save folder the user added, as a snapshot holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SavedFolder {
    /// The folder, as the app reaches it: in the Flatpak, the document portal's path of a
    /// folder picked through the file chooser.
    pub path: PathBuf,
    /// Every file it holds, with its path below the folder, sorted by path.
    pub files: Vec<SavedFile>,
}

/// What a snapshot holds, as its `snapshot.json` records it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The game whose saves it holds.
    pub game: GameId,
    /// The prefix it was taken from; `None` for a game whose saves are in save folders
    /// alone.
    pub prefix: Option<PathBuf>,
    /// When it was taken.
    pub taken_at: Timestamp,
    /// Why it was taken.
    pub reason: Reason,
    /// The user folders it holds, relative to the prefix, as `drive_c/users/steamuser/`.
    pub include: Vec<String>,
    /// What it left out, as [`Exclusion::pattern`] writes each rule.
    pub exclude: Vec<String>,
    /// Every file it holds from the prefix, sorted by path.
    pub files: Vec<SavedFile>,
    /// The save folders it holds, in the order they were added; `folders/<index>/` in the
    /// snapshot's folder holds the files of each.
    pub folders: Vec<SavedFolder>,
}

impl Manifest {
    /// The size of the files it holds, in bytes.
    pub fn size(&self) -> u64 {
        self.files
            .iter()
            .chain(self.folders.iter().flat_map(|folder| &folder.files))
            .map(|file| file.size)
            .sum()
    }

    /// How many files it holds.
    pub fn file_count(&self) -> usize {
        self.files.len()
            + self
                .folders
                .iter()
                .map(|folder| folder.files.len())
                .sum::<usize>()
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

    /// The folder holding its copies of the files of the save folder at `index` in its
    /// manifest.
    pub fn folder_files_dir(&self, index: usize) -> PathBuf {
        self.dir.join(FOLDERS).join(index.to_string())
    }
}

/// What taking a snapshot did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Taken {
    /// It wrote this new snapshot.
    Written(Snapshot),
    /// The saves are as this newest snapshot holds them, so it wrote none.
    Unchanged(Snapshot),
    /// The prefix and the save folders have no files to save, so it wrote none.
    NothingToSave,
}

/// The folder in a snapshot's folder holding its copies of the prefix's files.
const FILES: &str = "files";

/// The folder in a snapshot's folder holding its copies of the save folders, one numbered
/// folder each.
const FOLDERS: &str = "folders";

/// Where a game's saves are: the prefix Pindu manages for it, if any, and the save folders
/// the user added for it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Roots<'a> {
    /// The prefix Pindu manages for the game (see [`managed_prefix`]), whose user folders a
    /// snapshot holds.
    pub prefix: Option<&'a Path>,
    /// The save folders the user added for the game, as the app reaches them.
    pub folders: &'a [PathBuf],
}

impl<'a> Roots<'a> {
    /// The saves in `prefix` alone.
    pub fn prefix(prefix: &'a Path) -> Self {
        Self {
            prefix: Some(prefix),
            folders: &[],
        }
    }
}

/// Takes a snapshot of the saves of `game` in `roots` into `game_dir`, the game's
/// snapshots folder. A save folder the app cannot reach now (see [`reach`]) is left out.
/// It is written into a hidden `.partial` folder and renamed into place once complete, so
/// the earlier snapshots stay as they are whatever happens; a failed write removes its
/// partial folder, and one left by a crash is removed first. Nothing is written when the
/// saves are as the newest snapshot holds them or there are none. Older snapshots are not
/// pruned here: see [`prune`].
pub fn take(
    roots: Roots<'_>,
    game_dir: &Path,
    game: &GameId,
    reason: Reason,
    now: Timestamp,
) -> io::Result<Taken> {
    remove_partials(game_dir)?;
    let files = match roots.prefix {
        Some(prefix) => collect(prefix)?,
        None => Vec::new(),
    };
    let folders = roots
        .folders
        .iter()
        .filter(|folder| reach(folder) == Reach::Reachable)
        .map(|folder| {
            Ok(SavedFolder {
                path: folder.clone(),
                files: collect_folder(folder)?,
            })
        })
        .collect::<io::Result<Vec<_>>>()?;
    if files.is_empty() && folders.iter().all(|folder| folder.files.is_empty()) {
        return Ok(Taken::NothingToSave);
    }
    let snapshots = list(game_dir)?;
    if let Some(newest) = snapshots.first()
        && newest.manifest.files == files
        && newest.manifest.folders == folders
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
        prefix: roots.prefix.map(Path::to_owned),
        taken_at: now,
        reason,
        include,
        exclude: EXCLUDED.iter().map(|rule| rule.pattern()).collect(),
        files,
        folders,
    };
    let dir = place(game_dir, &name, &manifest, Origin::Saves)?;
    Ok(Taken::Written(Snapshot {
        name,
        dir,
        manifest,
    }))
}

/// What copying a snapshot to the folder the user picked did.
#[derive(Debug, Clone, PartialEq, Eq)]
#[expect(
    clippy::large_enum_variant,
    reason = "one is made per snapshot copied, so boxing the snapshot saves nothing"
)]
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
        Origin::Snapshot(snapshot),
    )?;
    Ok(Copied::Written(Snapshot {
        name: snapshot.name.clone(),
        dir,
        manifest: snapshot.manifest.clone(),
    }))
}

/// Where the files of a snapshot being written come from.
#[derive(Debug, Clone, Copy)]
enum Origin<'a> {
    /// The saves themselves: the prefix and save folders its manifest names.
    Saves,
    /// Another snapshot, being copied.
    Snapshot(&'a Snapshot),
}

impl Origin<'_> {
    /// The folder the prefix's files of `manifest` come from.
    fn files(self, manifest: &Manifest) -> PathBuf {
        match self {
            Self::Saves => manifest
                .prefix
                .as_deref()
                .map(users_dir)
                .unwrap_or_default(),
            Self::Snapshot(snapshot) => snapshot.files_dir(),
        }
    }

    /// The folder the files of the save folder at `index` in `manifest` come from.
    fn folder(self, manifest: &Manifest, index: usize) -> PathBuf {
        match self {
            Self::Saves => manifest.folders[index].path.clone(),
            Self::Snapshot(snapshot) => snapshot.folder_files_dir(index),
        }
    }
}

/// Writes the snapshot `name` in `game_dir`, holding the files `manifest` lists copied
/// from `from`: into a hidden `.partial` folder, renamed into place once complete.
/// Returns its folder. A failed write removes its partial folder.
fn place(
    game_dir: &Path,
    name: &str,
    manifest: &Manifest,
    from: Origin<'_>,
) -> io::Result<PathBuf> {
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

/// The format of `snapshot.json`; a newer one is not read. Format 2 added save folders,
/// snapshots without a prefix and the reason `manual`.
const FORMAT: u64 = 2;

/// Copies the files `manifest` lists from `from` into `partial`'s `files/` and
/// `folders/<index>/`, keeping their times, then writes its `snapshot.json`, everything
/// flushed to disk.
fn write(partial: &Path, manifest: &Manifest, from: Origin<'_>) -> io::Result<()> {
    copy_files(&from.files(manifest), &partial.join(FILES), &manifest.files)?;
    for (index, folder) in manifest.folders.iter().enumerate() {
        copy_files(
            &from.folder(manifest, index),
            &partial.join(FOLDERS).join(index.to_string()),
            &folder.files,
        )?;
    }
    let json = serde_json::to_string_pretty(&manifest_json(manifest))
        .expect("JSON values always serialize");
    let mut out = fs::File::create(partial.join(MANIFEST))?;
    io::Write::write_all(&mut out, json.as_bytes())?;
    out.sync_all()
}

/// Copies `files` from `from` into `to`, keeping their times, each flushed to disk. The
/// folder is made even for no files, so a snapshot of an empty save folder holds it.
fn copy_files(from: &Path, to: &Path, files: &[SavedFile]) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for file in files {
        let target = to.join(&file.path);
        fs::create_dir_all(target.parent().expect("a file is in a folder"))?;
        // On btrfs and XFS the copy shares the file's blocks until either changes.
        fs::copy(from.join(&file.path), &target)?;
        let copy = fs::OpenOptions::new().write(true).open(&target)?;
        copy.set_modified(modified(file.mtime_ns))?;
        copy.sync_all()?;
    }
    Ok(())
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
/// newest [`KEEP_EXIT`] taken on exit or by hand and the newest [`KEEP_BEFORE_RESTORE`]
/// taken before a restore, so restores never push out the snapshots taken on exit.
/// Returns the removed ones, newest first.
pub fn prune(game_dir: &Path) -> io::Result<Vec<Snapshot>> {
    let (mut exit, mut before_restore) = (0, 0);
    let mut removed = Vec::new();
    for snapshot in list(game_dir)? {
        let (kept, keep) = match snapshot.manifest.reason {
            Reason::Exit | Reason::Manual => (&mut exit, KEEP_EXIT),
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

/// How many snapshots taken on exit or by hand a game keeps.
pub const KEEP_EXIT: usize = 5;

/// How many snapshots taken before a restore a game keeps.
pub const KEEP_BEFORE_RESTORE: usize = 2;

/// Whether a game has save snapshots to list without save folders of its own: a Windows
/// game we run through Proton. Only those in a prefix Pindu manages get any (see
/// [`managed_prefix`]). Any game the user added save folders for has them too.
pub fn offers_snapshots(game: &Game) -> bool {
    game.launch_kind == LaunchKind::Proton
}

/// The folder in `root`, the snapshots folder, that holds a game's snapshots: named after
/// its native ID, as its prefix is.
pub fn game_snapshots_dir(root: &Path, game: &GameId) -> PathBuf {
    root.join(game.native_id())
}

/// Whether the app can reach a save folder now, and why not.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    /// It can read the folder.
    Reachable,
    /// The folder is not there: removed, moved, on a drive that is not plugged in, or, in
    /// the Flatpak, no longer shared with the app through the document portal.
    Gone,
    /// The path is no folder.
    NotAFolder,
    /// The app may not read it: the folder's permissions, or a sandbox that does not reach
    /// it.
    Denied,
}

/// Whether the app can reach `folder`, a save folder, now.
pub fn reach(folder: &Path) -> Reach {
    match fs::read_dir(folder) {
        Ok(_) => Reach::Reachable,
        Err(error) => match error.kind() {
            io::ErrorKind::NotFound => Reach::Gone,
            io::ErrorKind::NotADirectory => Reach::NotAFolder,
            _ => Reach::Denied,
        },
    }
}

/// Why a folder cannot be added as a save folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewFolderError {
    /// It is one of the game's save folders already.
    AlreadyAdded,
    /// Snapshots, or copies of them, are kept in it or it is in them: a snapshot would
    /// hold the snapshots, and a restore write into them.
    HoldsSnapshots,
}

/// Whether `folder` may be added to `added`, a game's save folders: not one of them
/// already, and neither in nor around one of `snapshots`, the folders snapshots and their
/// copies are kept in.
pub fn check_new_folder(
    folder: &Path,
    added: &[PathBuf],
    snapshots: &[&Path],
) -> Result<(), NewFolderError> {
    if added.iter().any(|added| added == folder) {
        return Err(NewFolderError::AlreadyAdded);
    }
    if snapshots
        .iter()
        .any(|snapshots| snapshots.starts_with(folder) || folder.starts_with(snapshots))
    {
        return Err(NewFolderError::HoldsSnapshots);
    }
    Ok(())
}

/// The prefix `game` runs in when Pindu chose it, as `<prefixes folder>/<native ID>`:
/// the only prefixes snapshots cover. A prefix the user named elsewhere, which another
/// tool may own, is not managed.
pub fn managed_prefix(game: &Game, config: &LaunchConfig, defaults: &Defaults) -> Option<PathBuf> {
    let own = defaults.prefixes.join(game.id.native_id());
    let prefix = proton::prefix(game, config, defaults);
    (prefix == own).then_some(prefix)
}

/// Why a restore did not happen.
#[derive(Debug)]
pub enum RestoreError {
    /// The game runs, and would write its saves over the restored ones.
    Running,
    /// The snapshot is another game's.
    NotThisGame,
    /// The prefix has no folder of user profiles to restore into: it is gone, not set up
    /// yet, no longer one Pindu manages, or its `drive_c` or `users` is a symbolic link.
    NoPrefix,
    /// The snapshot holds this save folder, which is not one of the game's save folders
    /// now.
    NotASaveFolder(PathBuf),
    /// The snapshot holds this save folder, which the app cannot reach now (see
    /// [`reach`]).
    FolderUnreachable(PathBuf, Reach),
    /// The snapshot holds this path, which a restore may not write: one outside the
    /// user folders of `drive_c/users/` or the save folder it is in, in what snapshots
    /// leave out, or through a symbolic link, in the prefix, the save folder or the
    /// snapshot. A path in a save folder is given in full.
    Refused(String),
    /// Reading or writing failed.
    Io(io::Error),
}

impl From<io::Error> for RestoreError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

/// Restores `snapshot`, one of `game`'s, into `roots`, the game's managed prefix and save
/// folders: the files a snapshot holds become as the snapshot holds them, each written
/// beside its place and renamed over it, with its recorded time, and those it does not
/// hold are removed. What snapshots leave out, everything outside the user folders of
/// `drive_c/users/` and the save folders the snapshot holds, and the save folders it does
/// not hold, stay as they are.
///
/// It is refused while the game runs (`running`), for another game's snapshot, for one
/// that holds the prefix when `roots` has none, for one that holds a folder that is not
/// among the save folders of `roots` or that the app cannot reach, and for one that holds
/// a path it may not write, before anything is written. Otherwise it first takes a safety
/// snapshot of the saves in `roots` into `game_dir`, the game's snapshots folder (see
/// [`take`] and [`Reason::BeforeRestore`]), and returns what that did; prune the older
/// ones after the restore (see [`prune`]), so the snapshot being restored is never
/// removed before it is put back.
pub fn restore(
    roots: Roots<'_>,
    game_dir: &Path,
    game: &GameId,
    snapshot: &Snapshot,
    running: bool,
    now: Timestamp,
) -> Result<Taken, RestoreError> {
    if running {
        return Err(RestoreError::Running);
    }
    let manifest = &snapshot.manifest;
    if manifest.game != *game {
        return Err(RestoreError::NotThisGame);
    }
    if let Some(file) = manifest
        .files
        .iter()
        .find(|file| !restorable(&file.path, Within::Prefix))
    {
        return Err(RestoreError::Refused(file.path.clone()));
    }
    for (index, folder) in manifest.folders.iter().enumerate() {
        if let Some(file) = folder
            .files
            .iter()
            .find(|file| !restorable(&file.path, Within::Folder))
        {
            return Err(RestoreError::Refused(in_folder(folder, file)));
        }
        if !roots.folders.contains(&folder.path)
            || manifest.folders[..index]
                .iter()
                .any(|earlier| earlier.path == folder.path)
        {
            return Err(RestoreError::NotASaveFolder(folder.path.clone()));
        }
        match reach(&folder.path) {
            Reach::Reachable => {}
            unreachable => {
                return Err(RestoreError::FolderUnreachable(
                    folder.path.clone(),
                    unreachable,
                ));
            }
        }
    }
    let prefix = if manifest.prefix.is_some() || !manifest.files.is_empty() {
        let prefix = roots.prefix.ok_or(RestoreError::NoPrefix)?;
        for dir in [prefix.join("drive_c"), users_dir(prefix)] {
            if !is_real_dir(&dir)? {
                return Err(RestoreError::NoPrefix);
            }
        }
        Some(prefix)
    } else {
        None
    };
    let source = snapshot.files_dir();
    if let Some(prefix) = prefix {
        let users = users_dir(prefix);
        for file in &manifest.files {
            if !fs::symlink_metadata(source.join(&file.path))?.is_file()
                || !writable(&users, &file.path)?
            {
                return Err(RestoreError::Refused(file.path.clone()));
            }
        }
    }
    for (index, folder) in manifest.folders.iter().enumerate() {
        let source = snapshot.folder_files_dir(index);
        for file in &folder.files {
            if !fs::symlink_metadata(source.join(&file.path))?.is_file()
                || !writable(&folder.path, &file.path)?
            {
                return Err(RestoreError::Refused(in_folder(folder, file)));
            }
        }
    }
    let safety = take(roots, game_dir, game, Reason::BeforeRestore, now)?;
    if let Some(prefix) = prefix {
        put_all(&source, &users_dir(prefix), &manifest.files, || {
            collect(prefix)
        })?;
    }
    for (index, folder) in manifest.folders.iter().enumerate() {
        put_all(
            &snapshot.folder_files_dir(index),
            &folder.path,
            &folder.files,
            || collect_folder(&folder.path),
        )?;
    }
    Ok(safety)
}

/// The full path of `file` in the save folder `folder`, as a refused restore names it.
fn in_folder(folder: &SavedFolder, file: &SavedFile) -> String {
    format!("{}/{}", folder.path.display(), file.path)
}

/// Makes the files in `root` as `files` hold them: copies each from `source` (see
/// [`put_back`]), then removes those that `present` lists now and `files` does not hold.
fn put_all(
    source: &Path,
    root: &Path,
    files: &[SavedFile],
    present: impl FnOnce() -> io::Result<Vec<SavedFile>>,
) -> io::Result<()> {
    for file in files {
        put_back(source, root, file)?;
    }
    let kept: HashSet<&str> = files.iter().map(|file| file.path.as_str()).collect();
    for file in present()? {
        if !kept.contains(file.path.as_str()) {
            fs::remove_file(root.join(&file.path))?;
        }
    }
    Ok(())
}

/// Whether a restore may write the file at `path` below `drive_c/users/`, `within` the
/// prefix, or below its save folder: in the prefix one in a user folder, with no step out
/// of it, outside what the rules of [`EXCLUDED`] that apply there leave out.
fn restorable(path: &str, within: Within) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    if parts
        .iter()
        .any(|part| part.is_empty() || *part == "." || *part == ".." || part.contains('\0'))
    {
        return false;
    }
    let Some((name, folders)) = parts.split_last() else {
        return false;
    };
    let folders = match (within, folders) {
        (Within::Prefix, [_user, folders @ ..]) => folders,
        (Within::Prefix, []) => return false,
        (Within::Folder, folders) => folders,
    };
    let mut below_user = Vec::new();
    for folder in folders {
        below_user.push((*folder).to_owned());
        if within.rules().any(|rule| rule.excludes_folder(&below_user)) {
            return false;
        }
    }
    !within.rules().any(|rule| rule.excludes_file(name))
}

/// Whether `path` is a folder itself, not a symbolic link to one; `false` when it is
/// missing.
fn is_real_dir(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Whether a restore can write the file at `path` in `root`, the prefix's
/// `drive_c/users/` or a save folder, without going through a symbolic link: each folder
/// on its way is a real folder or missing, as is everything below a missing one.
fn writable(root: &Path, path: &str) -> io::Result<bool> {
    let mut dir = root.to_owned();
    for part in folders_of(path) {
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Ok(false),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

/// The names of the folders on the way to the file at `path`, below the folder it is in.
fn folders_of(path: &str) -> impl Iterator<Item = &str> {
    path.rsplit_once('/')
        .map(|(folders, _)| folders.split('/'))
        .into_iter()
        .flatten()
}

/// The end of the name of a file being restored, beside the file it replaces.
const RESTORING: &str = ".pindu-restore";

/// Copies `file` from `source`, a snapshot's `files/` or `folders/<index>/`, to its place
/// in `root`, the prefix's `drive_c/users/` or the save folder, with its recorded time:
/// into a hidden file beside it first, flushed to disk, then renamed over it, so a failed
/// copy leaves the file as it was. The folders on its way are made where missing; one that
/// is a symbolic link stops it.
fn put_back(source: &Path, root: &Path, file: &SavedFile) -> io::Result<()> {
    let name = file
        .path
        .rsplit_once('/')
        .map_or(file.path.as_str(), |(_, name)| name);
    let mut dir = root.to_owned();
    for part in folders_of(&file.path) {
        dir.push(part);
        match fs::symlink_metadata(&dir) {
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => return Err(not_a_folder(&dir)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => fs::create_dir(&dir)?,
            Err(error) => return Err(error),
        }
    }
    let temporary = dir.join(format!(".{name}{RESTORING}"));
    let copied = (|| {
        fs::copy(source.join(&file.path), &temporary)?;
        let copy = fs::OpenOptions::new().write(true).open(&temporary)?;
        copy.set_modified(modified(file.mtime_ns))?;
        copy.sync_all()?;
        fs::rename(&temporary, dir.join(name))
    })();
    if copied.is_err() {
        // The hidden copy is no save whether or not it goes now.
        let _ = fs::remove_file(&temporary);
    }
    copied
}

/// The error for `path`, which a restore needs to be a real folder.
fn not_a_folder(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotADirectory,
        format!("{} is not a folder", path.display()),
    )
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
        "prefix": manifest.prefix.as_ref().map(|prefix| prefix.to_string_lossy()),
        "taken_at": manifest.taken_at,
        "reason": manifest.reason.as_str(),
        "include": manifest.include,
        "exclude": manifest.exclude,
        "files": files_json(&manifest.files),
        "folders": manifest.folders.iter().map(|folder| json!({
            "path": folder.path.to_string_lossy(),
            "files": files_json(&folder.files),
        })).collect::<Vec<_>>(),
    })
}

fn files_json(files: &[SavedFile]) -> Json {
    files
        .iter()
        .map(|file| {
            json!({
                "path": file.path,
                "size": file.size,
                "mtime_ns": file.mtime_ns,
            })
        })
        .collect()
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
    let prefix = match &json["prefix"] {
        Json::Null => None,
        prefix => Some(PathBuf::from(prefix.as_str()?)),
    };
    // Format 1 had no save folders.
    let folders = match &json["folders"] {
        Json::Null => Vec::new(),
        folders => folders
            .as_array()?
            .iter()
            .map(|folder| {
                Some(SavedFolder {
                    path: PathBuf::from(folder["path"].as_str()?),
                    files: files_from(&folder["files"])?,
                })
            })
            .collect::<Option<_>>()?,
    };
    Some(Manifest {
        game: json["game"].as_str()?.parse().ok()?,
        prefix,
        taken_at: json["taken_at"].as_i64()?,
        reason: Reason::from_str(json["reason"].as_str()?)?,
        include: strings(&json["include"])?,
        exclude: strings(&json["exclude"])?,
        files: files_from(&json["files"])?,
        folders,
    })
}

/// The files `json` lists, if it is a list of them.
fn files_from(json: &Json) -> Option<Vec<SavedFile>> {
    json.as_array()?
        .iter()
        .map(|file| {
            Some(SavedFile {
                path: file["path"].as_str()?.to_owned(),
                size: file["size"].as_u64()?,
                mtime_ns: file["mtime_ns"].as_i64()?,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Game, LaunchKind, Source};

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

    /// Puts a save under `users` whose path fits there but is too long for Linux once it
    /// is copied into a snapshot of `game_dir`, so the copy fails for any user, root too.
    fn put_too_long_to_copy(users: &Path, game_dir: &Path) {
        // Linux refuses a path of PATH_MAX (4096) bytes or more.
        const PATH_MAX: usize = 4096;
        let source = users.join("steamuser").as_os_str().len() + 1;
        let partial = game_dir
            .join(".2026-11-19T18-43-07Z.partial/files/steamuser")
            .as_os_str()
            .len()
            + 1;
        assert!(
            partial >= source + 8,
            "the snapshot's path must be the longer"
        );
        // The source path ends 8 bytes short of the limit; its copy runs past it.
        let mut left = PATH_MAX - 8 - source;
        let mut parts = Vec::new();
        while left > 201 {
            parts.push("a".repeat(200));
            left -= 201;
        }
        parts.push("b".repeat(left));
        let file = users.join("steamuser").join(parts.join("/"));
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "too long").unwrap();
    }

    #[test]
    fn a_snapshot_copies_the_saves_and_says_what_it_holds() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
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
        assert_eq!(manifest.prefix.as_deref(), Some(prefix.path()));
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
        let first = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let again = take(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            Reason::Exit,
            NOW + 60,
        )
        .unwrap();
        assert_eq!(again, Taken::Unchanged(first.clone()));
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second = written(
            take(
                Roots::prefix(prefix.path()),
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
        let taken = take(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            Reason::Exit,
            NOW,
        )
        .unwrap();
        assert_eq!(taken, Taken::NothingToSave);
        assert_eq!(list(&game_dir).unwrap(), []);
    }

    #[test]
    fn snapshots_taken_in_the_same_second_get_names_of_their_own() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        assert_eq!(second.name, "2026-11-19T18-42-07Z-2");
        assert_eq!(list(&game_dir).unwrap(), [second, first]);
    }

    #[test]
    fn a_failed_write_leaves_the_earlier_snapshots_intact() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let first = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        // A save whose path fits in the prefix but not in the snapshot's partial folder:
        // copying it fails for any user, root included, as CI's tests run.
        put_too_long_to_copy(&users_dir(prefix.path()), &game_dir);
        assert!(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW + 60
            )
            .is_err()
        );
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
        take(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            Reason::Exit,
            NOW,
        )
        .unwrap();
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
                take(
                    Roots::prefix(prefix.path()),
                    &game_dir,
                    &celeste(),
                    reason,
                    now,
                )
                .unwrap(),
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
    fn a_restore_puts_the_snapshot_s_files_back_and_removes_newer_ones() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let users = users_dir(prefix.path());
        put(
            &users,
            "steamuser/Saved Games/CD Projekt Red/save.dat",
            "later",
        );
        put(
            &users,
            "steamuser/Saved Games/CD Projekt Red/new.dat",
            "new",
        );
        fs::remove_file(users.join("Public/Documents/shared.ini")).unwrap();
        restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        )
        .unwrap();
        assert_eq!(collect(prefix.path()).unwrap(), snapshot.manifest.files);
        assert_eq!(
            fs::read_to_string(users.join("steamuser/Saved Games/CD Projekt Red/save.dat"))
                .unwrap(),
            "cp77"
        );
        // What a snapshot leaves out stays as it is.
        assert!(users.join("steamuser/Temp/a.tmp").exists());
        assert!(
            users
                .join("steamuser/AppData/Roaming/Microsoft/Protect/masterkey")
                .exists()
        );
    }

    #[test]
    fn a_restore_first_takes_a_safety_snapshot_of_the_saves_it_replaces() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let users = users_dir(prefix.path());
        put(
            &users,
            "steamuser/Saved Games/CD Projekt Red/save.dat",
            "later",
        );
        let replaced = collect(prefix.path()).unwrap();
        let safety = written(
            restore(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                &snapshot,
                false,
                NOW + 60,
            )
            .unwrap(),
        );
        assert_eq!(safety.manifest.reason, Reason::BeforeRestore);
        assert_eq!(safety.manifest.taken_at, NOW + 60);
        assert_eq!(safety.manifest.files, replaced);
        assert_eq!(
            fs::read_to_string(
                safety
                    .files_dir()
                    .join("steamuser/Saved Games/CD Projekt Red/save.dat")
            )
            .unwrap(),
            "later"
        );
        assert_eq!(list(&game_dir).unwrap(), [safety, snapshot]);
    }

    #[test]
    fn a_restore_is_refused_while_the_game_runs() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let users = users_dir(prefix.path());
        put(
            &users,
            "steamuser/Saved Games/CD Projekt Red/save.dat",
            "later",
        );
        let now = collect(prefix.path()).unwrap();
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            true,
            NOW + 60,
        );
        assert!(matches!(refused, Err(RestoreError::Running)), "{refused:?}");
        assert_eq!(collect(prefix.path()).unwrap(), now);
        assert_eq!(list(&game_dir).unwrap(), [snapshot], "no safety snapshot");
    }

    #[test]
    fn a_restore_never_writes_outside_the_user_profiles_or_into_what_snapshots_leave_out() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let before = collect(prefix.path()).unwrap();
        for path in [
            "../user.reg",
            "steamuser/../../user.reg",
            "/etc/passwd",
            "steamuser//save.dat",
            "steamuser/./save.dat",
            "stray.txt",
            "steamuser/Temp/a.tmp",
            "steamuser/AppData/Roaming/Microsoft/Protect/masterkey",
            "steamuser/AppData/Local/Game/ShaderCache/s.bin",
            "steamuser/AppData/Local/Game/Profile/Cookies",
        ] {
            let mut tampered = snapshot.clone();
            tampered.manifest.files.push(SavedFile {
                path: path.to_owned(),
                size: 1,
                mtime_ns: 0,
            });
            let refused = restore(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                &tampered,
                false,
                NOW + 60,
            );
            assert!(
                matches!(&refused, Err(RestoreError::Refused(refused)) if refused == path),
                "{path}: {refused:?}"
            );
        }
        let other = GameId::new(Source::Manual, "other-3c4d");
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &other,
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(refused, Err(RestoreError::NotThisGame)),
            "{refused:?}"
        );
        assert_eq!(collect(prefix.path()).unwrap(), before);
        assert_eq!(list(&game_dir).unwrap(), [snapshot], "no safety snapshot");
    }

    #[test]
    fn a_restore_never_writes_through_a_symbolic_link() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let users = users_dir(prefix.path());
        let outside = tempfile::tempdir().unwrap();
        fs::remove_dir_all(users.join("steamuser/Saved Games")).unwrap();
        std::os::unix::fs::symlink(outside.path(), users.join("steamuser/Saved Games")).unwrap();
        let before = collect(prefix.path()).unwrap();
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(&refused, Err(RestoreError::Refused(path))
                if path == "steamuser/Saved Games/CD Projekt Red/save.dat"),
            "{refused:?}"
        );
        assert_eq!(entries(outside.path()), Vec::<String>::new());
        assert_eq!(collect(prefix.path()).unwrap(), before);
        assert_eq!(list(&game_dir).unwrap(), [snapshot], "no safety snapshot");
    }

    #[test]
    fn a_restore_never_copies_a_symbolic_link_in_the_snapshot() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let outside = tempfile::tempdir().unwrap();
        put(outside.path(), "secret.txt", "secret");
        let copy = snapshot
            .files_dir()
            .join("steamuser/Saved Games/CD Projekt Red/save.dat");
        fs::remove_file(&copy).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.txt"), &copy).unwrap();
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(&refused, Err(RestoreError::Refused(path))
                if path == "steamuser/Saved Games/CD Projekt Red/save.dat"),
            "{refused:?}"
        );
    }

    #[test]
    fn a_prefix_without_user_profiles_is_not_restored_into() {
        let (prefix, snapshots) = (prefix(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let moved = tempfile::tempdir().unwrap();
        fs::rename(prefix.path().join("drive_c"), moved.path().join("drive_c")).unwrap();
        std::os::unix::fs::symlink(moved.path().join("drive_c"), prefix.path().join("drive_c"))
            .unwrap();
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(refused, Err(RestoreError::NoPrefix)),
            "{refused:?}"
        );
        fs::remove_file(prefix.path().join("drive_c")).unwrap();
        let refused = restore(
            Roots::prefix(prefix.path()),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(refused, Err(RestoreError::NoPrefix)),
            "{refused:?}"
        );
        assert!(!prefix.path().join("drive_c").exists());
    }

    #[test]
    fn a_snapshot_is_copied_to_the_folder_the_user_picked() {
        let (prefix, snapshots, copies) = (
            prefix(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let game_dir = snapshots.path().join("celeste-1a2b");
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
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
                Roots::prefix(prefix.path()),
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
        let snapshot = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
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
        let first = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        copy_to(&first, copies.path()).unwrap();
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let second = written(
            take(
                Roots::prefix(prefix.path()),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW + 60,
            )
            .unwrap(),
        );
        // A file the snapshot lists but no longer holds: the copy fails for any user, root
        // included, as CI's tests run.
        fs::remove_file(second.files_dir().join("steamuser/Saved Games/new.sav")).unwrap();
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
                Roots::prefix(prefix.path()),
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

    /// A save folder the user added, as a game in its install folder keeps one: saves,
    /// a cache and cookies.
    fn save_folder() -> tempfile::TempDir {
        let folder = tempfile::tempdir().unwrap();
        for (path, contents) in [
            ("slot1.sav", "slot one"),
            ("Profiles/me/profile.dat", "me"),
            ("Temp/kept.tmp", "kept"),
            ("ShaderCache/s.bin", "x"),
            ("Profiles/me/Cookies", "x"),
        ] {
            put(folder.path(), path, contents);
        }
        folder
    }

    /// The roots of a game with `prefix` and the save `folders`.
    fn roots<'a>(prefix: Option<&'a Path>, folders: &'a [PathBuf]) -> Roots<'a> {
        Roots { prefix, folders }
    }

    /// `path` below the save folder `folder`, as a refused restore names it.
    fn in_folder(folder: &Path, path: &str) -> String {
        format!("{}/{path}", folder.display())
    }

    #[test]
    fn a_save_folder_holds_its_files_without_caches_cookies_or_symbolic_links() {
        let (folder, outside) = (save_folder(), tempfile::tempdir().unwrap());
        put(outside.path(), "secret.txt", "x");
        std::os::unix::fs::symlink(outside.path(), folder.path().join("Outside")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("secret.txt"),
            folder.path().join("secret.txt"),
        )
        .unwrap();
        let files = collect_folder(folder.path()).unwrap();
        // Only the rules for names at any depth apply: `Temp` is a prefix's own.
        assert_eq!(
            paths(&files),
            ["Profiles/me/profile.dat", "Temp/kept.tmp", "slot1.sav"]
        );
        assert_eq!(files[2].size, "slot one".len() as u64);
    }

    #[test]
    fn a_snapshot_holds_the_save_folders_too() {
        let (prefix, folder, snapshots) = (prefix(), save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let snapshot = written(
            take(
                roots(Some(prefix.path()), &folders),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let manifest = &snapshot.manifest;
        assert_eq!(manifest.files, collect(prefix.path()).unwrap());
        assert_eq!(
            manifest.folders,
            [SavedFolder {
                path: folder.path().to_owned(),
                files: collect_folder(folder.path()).unwrap(),
            }]
        );
        assert_eq!(manifest.file_count(), 7 + 3);
        assert_eq!(
            manifest.size(),
            4 + 4 + 7 + 4 + 6 + 6 + 4 + 8 + 2 + 4,
            "{manifest:?}"
        );
        assert_eq!(
            fs::read_to_string(snapshot.folder_files_dir(0).join("Profiles/me/profile.dat"))
                .unwrap(),
            "me"
        );
        assert!(!snapshot.folder_files_dir(0).join("ShaderCache").exists());
        assert_eq!(list(&game_dir).unwrap(), [snapshot]);
    }

    #[test]
    fn a_game_without_a_prefix_gets_snapshots_of_its_save_folders() {
        let (folder, snapshots) = (save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let first = written(
            take(
                roots(None, &folders),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        assert_eq!(first.manifest.prefix, None);
        assert_eq!(first.manifest.files, []);
        assert_eq!(first.manifest.include, Vec::<String>::new());
        assert_eq!(list(&game_dir).unwrap(), std::slice::from_ref(&first));
        let again = take(
            roots(None, &folders),
            &game_dir,
            &celeste(),
            Reason::Exit,
            NOW + 60,
        )
        .unwrap();
        assert_eq!(again, Taken::Unchanged(first.clone()));
        put(folder.path(), "slot2.sav", "slot two");
        let second = written(
            take(
                roots(None, &folders),
                &game_dir,
                &celeste(),
                Reason::Manual,
                NOW + 120,
            )
            .unwrap(),
        );
        assert_eq!(second.manifest.reason, Reason::Manual);
        assert_eq!(list(&game_dir).unwrap(), [second, first]);
        let empty = tempfile::tempdir().unwrap();
        let nothing = take(
            roots(None, &[empty.path().to_owned()]),
            &snapshots.path().join("other-3c4d"),
            &GameId::new(Source::Manual, "other-3c4d"),
            Reason::Exit,
            NOW,
        )
        .unwrap();
        assert_eq!(nothing, Taken::NothingToSave);
    }

    #[test]
    fn a_save_folder_the_app_cannot_reach_is_explained_and_left_out() {
        let (folder, other, snapshots) = (
            save_folder(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let gone = other.path().join("Unplugged drive");
        let file = other.path().join("a file");
        fs::write(&file, "x").unwrap();
        // A folder no user can open, root included as CI's tests run: a link to itself.
        let denied = other.path().join("Locked");
        std::os::unix::fs::symlink(&denied, &denied).unwrap();
        assert_eq!(reach(folder.path()), Reach::Reachable);
        assert_eq!(reach(&gone), Reach::Gone);
        assert_eq!(reach(&file), Reach::NotAFolder);
        assert_eq!(reach(&denied), Reach::Denied);
        let folders = [gone, file, denied.clone(), folder.path().to_owned()];
        let snapshot = written(
            take(
                roots(None, &folders),
                &snapshots.path().join("celeste-1a2b"),
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let held: Vec<&Path> = snapshot
            .manifest
            .folders
            .iter()
            .map(|folder| folder.path.as_path())
            .collect();
        assert_eq!(held, [folder.path()]);
    }

    #[test]
    fn a_snapshot_taken_by_hand_is_kept_as_one_taken_on_exit() {
        let (folder, snapshots) = (save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let mut taken = Vec::new();
        for (minute, reason) in [
            Reason::Exit,
            Reason::Manual,
            Reason::Exit,
            Reason::Manual,
            Reason::Exit,
            Reason::Manual,
        ]
        .into_iter()
        .enumerate()
        {
            put(folder.path(), "turn.sav", &"x".repeat(minute + 1));
            let now = NOW + minute as i64 * 60;
            taken.push(written(
                take(roots(None, &folders), &game_dir, &celeste(), reason, now).unwrap(),
            ));
        }
        let removed: Vec<String> = prune(&game_dir)
            .unwrap()
            .into_iter()
            .map(|snapshot| snapshot.name)
            .collect();
        assert_eq!(removed, [taken[0].name.clone()]);
    }

    #[test]
    fn a_copy_holds_the_save_folders_too() {
        let (folder, snapshots, copies) = (
            save_folder(),
            tempfile::tempdir().unwrap(),
            tempfile::tempdir().unwrap(),
        );
        let folders = [folder.path().to_owned()];
        let snapshot = written(
            take(
                roots(None, &folders),
                &snapshots.path().join("celeste-1a2b"),
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let copy = match copy_to(&snapshot, copies.path()).unwrap() {
            Copied::Written(copy) => copy,
            other => panic!("expected a new copy, got {other:?}"),
        };
        assert_eq!(copy.manifest, snapshot.manifest);
        assert_eq!(
            fs::read_to_string(copy.folder_files_dir(0).join("slot1.sav")).unwrap(),
            "slot one"
        );
    }

    #[test]
    fn a_snapshot_from_before_save_folders_is_still_read() {
        let snapshots = tempfile::tempdir().unwrap();
        let game_dir = snapshots.path().join("celeste-1a2b");
        put(
            &game_dir,
            "2026-11-19T18-42-07Z/snapshot.json",
            r#"{"format": 1, "game": "manual:celeste-1a2b", "prefix": "/home/me/prefix",
                "taken_at": 1795113727, "reason": "exit",
                "include": ["drive_c/users/steamuser/"], "exclude": ["Temp/"],
                "files": [{"path": "steamuser/a.sav", "size": 1, "mtime_ns": 0}]}"#,
        );
        let manifest = &list(&game_dir).unwrap()[0].manifest;
        assert_eq!(
            manifest.prefix.as_deref(),
            Some(Path::new("/home/me/prefix"))
        );
        assert_eq!(manifest.folders, []);
        assert_eq!(paths(&manifest.files), ["steamuser/a.sav"]);
    }

    #[test]
    fn a_restore_puts_the_save_folders_back_and_removes_newer_files() {
        let (prefix, folder, snapshots) = (prefix(), save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let both = roots(Some(prefix.path()), &folders);
        let snapshot = written(take(both, &game_dir, &celeste(), Reason::Exit, NOW).unwrap());
        put(folder.path(), "slot1.sav", "later");
        put(folder.path(), "slot2.sav", "new");
        fs::remove_file(folder.path().join("Profiles/me/profile.dat")).unwrap();
        put(
            &users_dir(prefix.path()),
            "steamuser/Saved Games/new.sav",
            "new",
        );
        let safety =
            written(restore(both, &game_dir, &celeste(), &snapshot, false, NOW + 60).unwrap());
        assert_eq!(
            safety.manifest.folders[0].files,
            collect_folder(safety.folder_files_dir(0).as_path()).unwrap()
        );
        assert_eq!(
            collect_folder(folder.path()).unwrap(),
            snapshot.manifest.folders[0].files
        );
        assert_eq!(
            fs::read_to_string(folder.path().join("slot1.sav")).unwrap(),
            "slot one"
        );
        assert_eq!(collect(prefix.path()).unwrap(), snapshot.manifest.files);
        // What a snapshot leaves out stays as it is.
        assert!(folder.path().join("ShaderCache/s.bin").exists());
        assert!(folder.path().join("Profiles/me/Cookies").exists());
    }

    #[test]
    fn a_restore_writes_only_inside_the_save_folders_it_holds() {
        let (folder, snapshots) = (save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let snapshot = written(
            take(
                roots(None, &folders),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let before = collect_folder(folder.path()).unwrap();
        for path in [
            "../outside.sav",
            "Profiles/../../outside.sav",
            "/etc/passwd",
            "Profiles//profile.dat",
            "./slot1.sav",
            "ShaderCache/s.bin",
            "Profiles/me/Cookies",
        ] {
            let mut tampered = snapshot.clone();
            tampered.manifest.folders[0].files.push(SavedFile {
                path: path.to_owned(),
                size: 1,
                mtime_ns: 0,
            });
            let refused = restore(
                roots(None, &folders),
                &game_dir,
                &celeste(),
                &tampered,
                false,
                NOW + 60,
            );
            assert!(
                matches!(&refused, Err(RestoreError::Refused(refused))
                    if *refused == in_folder(folder.path(), path)),
                "{path}: {refused:?}"
            );
        }
        // A folder that is no longer one of the game's save folders is not written.
        let refused = restore(
            roots(None, &[]),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(&refused, Err(RestoreError::NotASaveFolder(path)) if path == folder.path()),
            "{refused:?}"
        );
        // Nor is one the app cannot reach, which a restore would make again.
        let gone = snapshots.path().join("Unplugged drive");
        let mut elsewhere = snapshot.clone();
        elsewhere.manifest.folders[0].path = gone.clone();
        let refused = restore(
            roots(None, std::slice::from_ref(&gone)),
            &game_dir,
            &celeste(),
            &elsewhere,
            false,
            NOW + 60,
        );
        assert!(
            matches!(&refused, Err(RestoreError::FolderUnreachable(path, Reach::Gone))
                if *path == gone),
            "{refused:?}"
        );
        assert!(!gone.exists());
        // A snapshot with the prefix needs it.
        let mut with_prefix = snapshot.clone();
        with_prefix.manifest.prefix = Some("/home/me/prefix".into());
        let refused = restore(
            roots(None, &folders),
            &game_dir,
            &celeste(),
            &with_prefix,
            false,
            NOW + 60,
        );
        assert!(
            matches!(refused, Err(RestoreError::NoPrefix)),
            "{refused:?}"
        );
        assert_eq!(collect_folder(folder.path()).unwrap(), before);
        assert_eq!(list(&game_dir).unwrap(), [snapshot], "no safety snapshot");
    }

    #[test]
    fn a_restore_never_writes_through_a_symbolic_link_in_a_save_folder() {
        let (folder, snapshots) = (save_folder(), tempfile::tempdir().unwrap());
        let game_dir = snapshots.path().join("celeste-1a2b");
        let folders = [folder.path().to_owned()];
        let snapshot = written(
            take(
                roots(None, &folders),
                &game_dir,
                &celeste(),
                Reason::Exit,
                NOW,
            )
            .unwrap(),
        );
        let outside = tempfile::tempdir().unwrap();
        fs::remove_dir_all(folder.path().join("Profiles")).unwrap();
        std::os::unix::fs::symlink(outside.path(), folder.path().join("Profiles")).unwrap();
        let refused = restore(
            roots(None, &folders),
            &game_dir,
            &celeste(),
            &snapshot,
            false,
            NOW + 60,
        );
        assert!(
            matches!(&refused, Err(RestoreError::Refused(path))
                if *path == in_folder(folder.path(), "Profiles/me/profile.dat")),
            "{refused:?}"
        );
        assert_eq!(entries(outside.path()), Vec::<String>::new());
        assert_eq!(list(&game_dir).unwrap(), [snapshot], "no safety snapshot");
    }

    #[test]
    fn a_save_folder_is_added_once_and_never_where_snapshots_are_kept() {
        let snapshots = Path::new("/home/me/Games/pindu-launcher/save-snapshots");
        let copies = Path::new("/run/media/me/Backup/Saves");
        let added = [PathBuf::from("/home/me/.local/share/Celeste/Saves")];
        let check =
            |folder: &str| check_new_folder(Path::new(folder), &added, &[snapshots, copies]);
        assert_eq!(
            check("/home/me/.local/share/Celeste/Saves"),
            Err(NewFolderError::AlreadyAdded)
        );
        for folder in [
            "/home/me/Games",
            "/home/me",
            "/",
            "/home/me/Games/pindu-launcher/save-snapshots",
            "/home/me/Games/pindu-launcher/save-snapshots/celeste-1a2b",
            "/run/media/me/Backup",
            "/run/media/me/Backup/Saves/celeste-1a2b",
        ] {
            assert_eq!(
                check(folder),
                Err(NewFolderError::HoldsSnapshots),
                "{folder}"
            );
        }
        for folder in [
            "/home/me/.local/share/Celeste",
            "/home/me/Games/pindu-launcher/prefixes/celeste-1a2b/drive_c/Celeste/Saves",
            "/home/me/Games/pindu-launcher/save-snapshots-old",
        ] {
            assert_eq!(check(folder), Ok(()), "{folder}");
        }
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
