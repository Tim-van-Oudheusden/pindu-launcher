//! The file picker fullscreen mode draws itself, for Add Game with a controller: a
//! portal's file chooser runs in another process, which controller navigation cannot
//! drive. It starts at the places the launcher can read (the Games folder, the home
//! folder and the drives the system mounted under `/run/media/<user>`), and lists a
//! folder's subfolders and the files the step accepts, never more than the sandbox
//! already reaches.

use std::io;
use std::path::{Path, PathBuf};

/// The files a step of the picker accepts, besides the folders it always lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Accept {
    /// No files: the step picks a folder.
    Folders,
    /// Windows programs (`.exe`, in any case), such as a game's installer.
    WindowsPrograms,
}

impl Accept {
    /// Whether the step accepts the file named `name`.
    fn accepts(self, name: &str) -> bool {
        match self {
            Self::Folders => false,
            Self::WindowsPrograms => Path::new(name)
                .extension()
                .is_some_and(|extension| extension.eq_ignore_ascii_case("exe")),
        }
    }
}

/// What an entry of a listed folder is; folders sort first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EntryKind {
    /// A folder, which the picker opens.
    Folder,
    /// A file the step accepts, which the picker picks.
    File,
}

/// An entry of a listed folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Its name, as shown.
    pub name: String,
    /// Its full path.
    pub path: PathBuf,
    /// Whether it is a folder or a file.
    pub kind: EntryKind,
}

/// The subfolders of `folder`, then the files `accept` takes, each run sorted by name
/// regardless of case. Hidden entries, and those whose name is not UTF-8 or that cannot
/// be read, are left out; symbolic links count as what they point to.
pub fn list(folder: &Path, accept: Accept) -> io::Result<Vec<Entry>> {
    let mut entries: Vec<Entry> = std::fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            let metadata = std::fs::metadata(&path).ok()?;
            let kind = if metadata.is_dir() {
                EntryKind::Folder
            } else if metadata.is_file() && accept.accepts(&name) {
                EntryKind::File
            } else {
                return None;
            };
            Some(Entry { name, path, kind })
        })
        .collect();
    entries.sort_by_cached_key(|entry| (entry.kind, entry.name.to_lowercase()));
    Ok(entries)
}

/// What a place the picker starts from is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaceKind {
    /// `~/Games`, which the Flatpak can always read.
    Games,
    /// The home folder, as much of it as the launcher can read.
    Home,
    /// A drive the system mounted under [`removable_media`].
    Drive,
}

/// A place the picker starts from: the top of what it browses there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// What the place is.
    pub kind: PlaceKind,
    /// Its folder's name: a drive's label.
    pub name: String,
    /// Its folder.
    pub path: PathBuf,
}

/// The places the picker starts from that the launcher can read: the Games folder in
/// `home`, `home` itself, then each drive mounted in `media` by name. Mounting drives is
/// the system's job; a place the sandbox cannot reach is left out.
pub fn places(home: &Path, media: &Path) -> Vec<Place> {
    let readable = |path: &Path| std::fs::read_dir(path).is_ok();
    let place = |kind, path: PathBuf| Place {
        kind,
        name: path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
        path,
    };
    let mut found = Vec::new();
    let games = home.join("Games");
    if readable(&games) {
        found.push(place(PlaceKind::Games, games));
    }
    if readable(home) {
        found.push(place(PlaceKind::Home, home.to_owned()));
    }
    if let Ok(drives) = list(media, Accept::Folders) {
        found.extend(
            drives
                .into_iter()
                .filter(|drive| readable(&drive.path))
                .map(|drive| place(PlaceKind::Drive, drive.path)),
        );
    }
    found
}

/// Where the system mounts the removable drives of the user named `user`, as udisks2
/// does.
pub fn removable_media(user: &str) -> PathBuf {
    Path::new("/run/media").join(user)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn a_folder_lists_its_subfolders_then_the_files_the_step_accepts() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir(root.join("saves")).unwrap();
        fs::create_dir(root.join("Binaries")).unwrap();
        fs::create_dir(root.join(".cache")).unwrap();
        fs::write(root.join("setup.exe"), b"MZ").unwrap();
        fs::write(root.join("Data.EXE"), b"MZ").unwrap();
        fs::write(root.join("readme.txt"), b"").unwrap();
        fs::write(root.join(".hidden.exe"), b"MZ").unwrap();
        let names = |accept| {
            list(root, accept)
                .unwrap()
                .into_iter()
                .map(|entry| (entry.name, entry.kind))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(Accept::WindowsPrograms),
            [
                ("Binaries".to_owned(), EntryKind::Folder),
                ("saves".to_owned(), EntryKind::Folder),
                ("Data.EXE".to_owned(), EntryKind::File),
                ("setup.exe".to_owned(), EntryKind::File),
            ]
        );
        assert_eq!(
            names(Accept::Folders),
            [
                ("Binaries".to_owned(), EntryKind::Folder),
                ("saves".to_owned(), EntryKind::Folder),
            ]
        );
    }

    #[test]
    fn the_places_are_the_games_folder_the_home_folder_and_each_mounted_drive() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home/me");
        let media = dir.path().join("run/media/me");
        fs::create_dir_all(home.join("Games")).unwrap();
        fs::create_dir_all(media.join("USB STICK")).unwrap();
        fs::create_dir_all(media.join("Games Drive")).unwrap();
        fs::write(media.join("not-a-drive"), b"").unwrap();
        let found = places(&home, &media)
            .into_iter()
            .map(|place| (place.kind, place.name, place.path))
            .collect::<Vec<_>>();
        assert_eq!(
            found,
            [
                (PlaceKind::Games, "Games".to_owned(), home.join("Games")),
                (PlaceKind::Home, "me".to_owned(), home.clone()),
                (
                    PlaceKind::Drive,
                    "Games Drive".to_owned(),
                    media.join("Games Drive")
                ),
                (
                    PlaceKind::Drive,
                    "USB STICK".to_owned(),
                    media.join("USB STICK")
                ),
            ]
        );
    }

    #[test]
    fn a_place_the_launcher_cannot_read_is_left_out() {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("me");
        fs::create_dir(&home).unwrap();
        let found = places(&home, &dir.path().join("no-media"));
        assert_eq!(
            found.iter().map(|place| place.kind).collect::<Vec<_>>(),
            [PlaceKind::Home]
        );
        assert!(places(&dir.path().join("gone"), &dir.path().join("no-media")).is_empty());
    }

    #[test]
    fn removable_drives_are_mounted_under_run_media_and_the_user_name() {
        assert_eq!(removable_media("tim"), PathBuf::from("/run/media/tim"));
    }
}
