//! Where the app keeps its own files.

use std::path::{Component, Path, PathBuf};

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

/// `$XDG_DATA_HOME/pindu-launcher/art`: art the user picked, which unlike the art cache
/// cannot be rebuilt.
pub fn default_user_art_dir() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::home_dir().unwrap_or_default();
    data_home(xdg_data_home.as_deref(), &home)
        .join(APP_DIR)
        .join("art")
}

/// The library path for the current process environment.
pub fn default_library_db_path() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::home_dir().unwrap_or_default();
    library_db_path(xdg_data_home.as_deref(), &home)
}

/// `$XDG_DATA_HOME/pindu-launcher/compatibilitytools.d`: our own Proton builds.
pub fn default_tools_dir() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    let home = std::env::home_dir().unwrap_or_default();
    data_home(xdg_data_home.as_deref(), &home)
        .join(APP_DIR)
        .join("compatibilitytools.d")
}

/// `~/Games/pindu-launcher/prefixes`: the Wine prefixes of Windows games without one of
/// their own, in the games folder the sandbox may write to.
pub fn default_prefixes_dir() -> PathBuf {
    std::env::home_dir()
        .unwrap_or_default()
        .join("Games")
        .join(APP_DIR)
        .join("prefixes")
}

/// Splits a document portal path, `/run/user/<uid>/doc/<id>/<name>/<rest…>`, into the
/// document ID and the parts below the document itself; `None` for any other path.
pub fn document_portal_parts(path: &Path) -> Option<(String, Vec<PathBuf>)> {
    let mut parts = path.components();
    let mut next = || match parts.next()? {
        Component::RootDir => Some(String::new()),
        Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
        _ => None,
    };
    let (root, run, user, _uid, doc) = (next()?, next()?, next()?, next()?, next()?);
    if !root.is_empty() || run != "run" || user != "user" || doc != "doc" {
        return None;
    }
    let id = next()?;
    // The document's own name, which its host path already ends with.
    next()?;
    let rest = std::iter::from_fn(next).map(PathBuf::from).collect();
    Some((id, rest))
}

/// The root of the document a path lies below, `/run/user/<uid>/doc/<id>`; `None` for the
/// root itself and for any other path.
pub fn document_root(path: &Path) -> Option<PathBuf> {
    document_portal_parts(path)?;
    Some(path.components().take(6).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_portal_paths_split_into_id_and_rest() {
        assert_eq!(
            document_portal_parts(Path::new("/run/user/1000/doc/a1b2c3/Celeste")),
            Some(("a1b2c3".to_owned(), vec![]))
        );
        assert_eq!(
            document_portal_parts(Path::new("/run/user/1000/doc/a1b2c3/Celeste/bin/run")),
            Some(("a1b2c3".to_owned(), vec!["bin".into(), "run".into()]))
        );
        assert_eq!(
            document_portal_parts(Path::new("/home/me/Games/Celeste")),
            None
        );
        assert_eq!(
            document_portal_parts(Path::new("/run/user/1000/doc/a1b2c3")),
            None
        );
    }

    #[test]
    fn paths_below_a_document_have_its_root() {
        assert_eq!(
            document_root(Path::new("/run/user/1000/doc/a1b2c3/Celeste/bin")),
            Some("/run/user/1000/doc/a1b2c3".into())
        );
        assert_eq!(
            document_root(Path::new("/run/user/1000/doc/a1b2c3/Celeste")),
            Some("/run/user/1000/doc/a1b2c3".into())
        );
        assert_eq!(document_root(Path::new("/run/user/1000/doc/a1b2c3")), None);
        assert_eq!(document_root(Path::new("/home/me/Games/Celeste")), None);
    }

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
