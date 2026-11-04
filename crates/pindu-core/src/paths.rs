//! Where the app keeps its own files.

use std::path::{Path, PathBuf};

/// Folder name under the XDG base directories.
pub const APP_DIR: &str = "pindu-launcher";

/// `$XDG_DATA_HOME`, falling back to `~/.local/share` when unset or not absolute, as the
/// XDG Base Directory spec requires.
pub fn data_home(xdg_data_home: Option<&Path>, home: &Path) -> PathBuf {
    match xdg_data_home {
        Some(dir) if dir.is_absolute() => dir.to_owned(),
        _ => home.join(".local/share"),
    }
}

/// `$XDG_DATA_HOME/pindu-launcher/library.db`.
pub fn library_db_path(xdg_data_home: Option<&Path>, home: &Path) -> PathBuf {
    data_home(xdg_data_home, home)
        .join(APP_DIR)
        .join("library.db")
}

/// The library path for the current process environment.
pub fn default_library_db_path() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::home_dir().unwrap_or_default();
    library_db_path(xdg_data_home.as_deref(), &home)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_library_lives_in_xdg_data_home() {
        let path = library_db_path(Some(Path::new("/data")), Path::new("/home/me"));

        assert_eq!(path, Path::new("/data/pindu-launcher/library.db"));
    }

    #[test]
    fn without_xdg_data_home_the_library_lives_in_local_share() {
        let path = library_db_path(None, Path::new("/home/me"));

        assert_eq!(
            path,
            Path::new("/home/me/.local/share/pindu-launcher/library.db")
        );
    }

    #[test]
    fn a_relative_xdg_data_home_is_ignored() {
        let path = library_db_path(Some(Path::new("data")), Path::new("/home/me"));

        assert_eq!(
            path,
            Path::new("/home/me/.local/share/pindu-launcher/library.db")
        );
    }
}
