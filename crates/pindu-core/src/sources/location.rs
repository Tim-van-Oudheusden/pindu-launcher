//! Finds where each launcher keeps its data on this machine.
//!
//! Every source has ordered candidate folders, native first and Flatpak second, and the
//! sub-paths a folder must contain to count. The first candidate with every sub-path wins;
//! a user override replaces the candidates entirely.

use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

use crate::model::{Source, SourceState};
use crate::paths::data_home;
use crate::store::{self, Store};

/// The host's home, XDG and system folders, as seen from inside or outside the sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostDirs {
    pub home: PathBuf,
    pub data_home: PathBuf,
    pub config_home: PathBuf,
    /// The host's `/usr`: `/run/host/usr` inside Flatpak (`host-os:ro`).
    pub host_usr: PathBuf,
    /// The system Flatpak installation, whose `exports` the sandbox can read.
    pub system_flatpak: PathBuf,
}

impl HostDirs {
    /// Host folders for the current process.
    pub fn from_env() -> Self {
        let home = std::env::home_dir().unwrap_or_default();
        let in_flatpak = Path::new("/.flatpak-info").exists();
        Self::from_vars(&home, in_flatpak, |name| std::env::var_os(name))
    }

    /// Inside Flatpak, `XDG_*_HOME` point into the app's own folder, so only the
    /// `HOST_XDG_*_HOME` copies Flatpak makes of the host's values count; without
    /// them the XDG defaults under the home folder apply.
    pub fn from_vars(
        home: &Path,
        in_flatpak: bool,
        vars: impl Fn(&str) -> Option<OsString>,
    ) -> Self {
        let host_dir = |name: &str| {
            let value = vars(&format!("HOST_{name}"))
                .or_else(|| (!in_flatpak).then(|| vars(name)).flatten());
            value.map(PathBuf::from)
        };
        let config_home = match host_dir("XDG_CONFIG_HOME") {
            Some(dir) if dir.is_absolute() => dir,
            _ => home.join(".config"),
        };
        Self {
            home: home.to_owned(),
            data_home: data_home(host_dir("XDG_DATA_HOME").as_deref(), home),
            config_home,
            host_usr: PathBuf::from(if in_flatpak { "/run/host/usr" } else { "/usr" }),
            system_flatpak: PathBuf::from("/var/lib/flatpak"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResolveError {
    /// No candidate folder has every required sub-path: the launcher is not installed,
    /// or the sandbox cannot see it.
    NotFound,
    /// The user's override folder lacks a required sub-path.
    OverrideIncomplete { path: PathBuf, missing: PathBuf },
}

impl fmt::Display for ResolveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotFound => f.write_str("no data folder found"),
            Self::OverrideIncomplete { path, missing } => write!(
                f,
                "the chosen folder {} has no {}",
                path.display(),
                missing.display()
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Where one source's data can live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    candidates: Vec<PathBuf>,
    required: &'static [&'static str],
}

impl Location {
    /// The candidates of a source, or `None` for sources without one data folder.
    pub fn of(source: Source, dirs: &HostDirs) -> Option<Self> {
        let home = &dirs.home;
        let flatpak = |app_id: &str, rest: &str| home.join(".var/app").join(app_id).join(rest);
        let heroic = || {
            vec![
                dirs.config_home.join("heroic"),
                flatpak("com.heroicgameslauncher.hgl", "config/heroic"),
            ]
        };
        let (candidates, required): (Vec<PathBuf>, &'static [&'static str]) = match source {
            Source::Steam => (
                vec![
                    home.join(".steam/steam"),
                    dirs.data_home.join("Steam"),
                    flatpak("com.valvesoftware.Steam", "data/Steam"),
                ],
                &["steamapps/libraryfolders.vdf"],
            ),
            Source::HeroicEpic => (heroic(), &["store_cache/legendary_library.json"]),
            Source::HeroicGog => (heroic(), &["store_cache/gog_library.json"]),
            Source::HeroicAmazon => (heroic(), &["store_cache/nile_library.json"]),
            Source::HeroicSideload => (heroic(), &["sideload_apps/library.json"]),
            Source::Lutris => (
                vec![
                    dirs.data_home.join("lutris"),
                    flatpak("net.lutris.Lutris", "data/lutris"),
                ],
                &["pga.db"],
            ),
            Source::Bottles => (
                vec![
                    dirs.data_home.join("bottles"),
                    flatpak("com.usebottles.bottles", "data/bottles"),
                ],
                &["library.yml"],
            ),
            Source::Faugus => (
                vec![
                    dirs.data_home.join("faugus-launcher"),
                    flatpak("io.github.Faugus.faugus-launcher", "data/faugus-launcher"),
                ],
                &["games.json"],
            ),
            Source::Desktop | Source::Manual => return None,
        };
        Some(Self {
            candidates,
            required,
        })
    }

    /// Candidate folders in the order they are tried.
    pub fn candidates(&self) -> &[PathBuf] {
        &self.candidates
    }

    /// The folder to read: the override when given, else the first complete candidate.
    pub fn resolve(&self, user_override: Option<&Path>) -> Result<PathBuf, ResolveError> {
        if let Some(path) = user_override {
            return match self.first_missing(path) {
                None => Ok(path.to_owned()),
                Some(missing) => Err(ResolveError::OverrideIncomplete {
                    path: path.to_owned(),
                    missing: missing.into(),
                }),
            };
        }
        self.candidates
            .iter()
            .find(|candidate| self.first_missing(candidate).is_none())
            .cloned()
            .ok_or(ResolveError::NotFound)
    }

    fn first_missing(&self, root: &Path) -> Option<&'static str> {
        self.required
            .iter()
            .copied()
            .find(|sub_path| !root.join(sub_path).exists())
    }
}

/// Resolves a source's folder, honouring and keeping the user override stored in its
/// [`SourceState`], and records the result as the state's resolved location.
pub fn resolve_and_record(
    store: &Store,
    source: Source,
    dirs: &HostDirs,
) -> store::Result<Result<PathBuf, ResolveError>> {
    let mut state = store
        .source_state(source)?
        .unwrap_or_else(|| SourceState::new(source));
    let resolved = match Location::of(source, dirs) {
        Some(location) => location.resolve(state.user_override.as_deref()),
        None => Err(ResolveError::NotFound),
    };
    state.resolved_location = resolved.as_ref().ok().cloned();
    store.set_source_state(&state)?;
    Ok(resolved)
}

/// Stores (or with `None` clears) the folder the user picked for a source.
pub fn set_user_override(store: &Store, source: Source, path: Option<&Path>) -> store::Result<()> {
    let mut state = store
        .source_state(source)?
        .unwrap_or_else(|| SourceState::new(source));
    state.user_override = path.map(Path::to_owned);
    store.set_source_state(&state)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(home: &Path) -> HostDirs {
        HostDirs {
            home: home.to_owned(),
            data_home: home.join(".local/share"),
            config_home: home.join(".config"),
            host_usr: home.join("usr"),
            system_flatpak: home.join("var/lib/flatpak"),
        }
    }

    fn touch(path: &Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn the_first_candidate_with_every_sub_path_wins() {
        let home = tempfile::tempdir().unwrap();
        let native = home.path().join(".local/share/Steam");
        let flatpak = home
            .path()
            .join(".var/app/com.valvesoftware.Steam/data/Steam");
        touch(&native.join("steamapps/libraryfolders.vdf"));
        touch(&flatpak.join("steamapps/libraryfolders.vdf"));

        let location = Location::of(Source::Steam, &dirs(home.path())).unwrap();

        assert_eq!(location.resolve(None), Ok(native));
    }

    #[test]
    fn a_candidate_missing_a_sub_path_is_skipped() {
        let home = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(home.path().join(".local/share/Steam")).unwrap();
        let flatpak = home
            .path()
            .join(".var/app/com.valvesoftware.Steam/data/Steam");
        touch(&flatpak.join("steamapps/libraryfolders.vdf"));

        let location = Location::of(Source::Steam, &dirs(home.path())).unwrap();

        assert_eq!(location.resolve(None), Ok(flatpak));
    }

    #[test]
    fn native_candidates_come_before_flatpak_ones() {
        let home = Path::new("/home/me");

        let location = Location::of(Source::Steam, &dirs(home)).unwrap();

        assert_eq!(
            location.candidates(),
            [
                home.join(".steam/steam"),
                home.join(".local/share/Steam"),
                home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
            ]
        );
    }

    #[test]
    fn nothing_found_is_reported_as_not_installed() {
        let home = tempfile::tempdir().unwrap();

        let location = Location::of(Source::Lutris, &dirs(home.path())).unwrap();

        assert_eq!(location.resolve(None), Err(ResolveError::NotFound));
    }

    #[test]
    fn a_valid_override_wins_over_every_candidate() {
        let home = tempfile::tempdir().unwrap();
        touch(
            &home
                .path()
                .join(".config/heroic/store_cache/gog_library.json"),
        );
        let custom = home.path().join("elsewhere/heroic");
        touch(&custom.join("store_cache/gog_library.json"));

        let location = Location::of(Source::HeroicGog, &dirs(home.path())).unwrap();

        assert_eq!(location.resolve(Some(&custom)), Ok(custom));
    }

    #[test]
    fn an_override_missing_a_sub_path_is_an_error_not_a_fallback() {
        let home = tempfile::tempdir().unwrap();
        touch(
            &home
                .path()
                .join(".config/heroic/store_cache/gog_library.json"),
        );
        let custom = home.path().join("elsewhere");
        std::fs::create_dir_all(&custom).unwrap();

        let location = Location::of(Source::HeroicGog, &dirs(home.path())).unwrap();

        assert_eq!(
            location.resolve(Some(&custom)),
            Err(ResolveError::OverrideIncomplete {
                path: custom,
                missing: PathBuf::from("store_cache/gog_library.json"),
            })
        );
    }

    #[test]
    fn each_heroic_store_requires_its_own_library_file() {
        let home = tempfile::tempdir().unwrap();
        let root = home
            .path()
            .join(".var/app/com.heroicgameslauncher.hgl/config/heroic");
        touch(&root.join("store_cache/legendary_library.json"));
        let dirs = dirs(home.path());

        let resolve = |source| Location::of(source, &dirs).unwrap().resolve(None);

        assert_eq!(resolve(Source::HeroicEpic), Ok(root));
        assert_eq!(resolve(Source::HeroicGog), Err(ResolveError::NotFound));
        assert_eq!(resolve(Source::HeroicAmazon), Err(ResolveError::NotFound));
    }

    #[test]
    fn sources_without_a_data_folder_have_no_location() {
        let dirs = dirs(Path::new("/home/me"));

        assert!(Location::of(Source::Desktop, &dirs).is_none());
        assert!(Location::of(Source::Manual, &dirs).is_none());
    }

    #[test]
    fn host_dirs_prefer_the_hosts_xdg_variables_inside_flatpak() {
        let vars = |name: &str| match name {
            "HOST_XDG_DATA_HOME" => Some(OsString::from("/host/data")),
            "XDG_DATA_HOME" => Some(OsString::from("/home/me/.var/app/x/data")),
            "XDG_CONFIG_HOME" => Some(OsString::from("/home/me/.var/app/x/config")),
            _ => None,
        };

        let dirs = HostDirs::from_vars(Path::new("/home/me"), true, vars);

        assert_eq!(dirs.data_home, Path::new("/host/data"));
        assert_eq!(dirs.config_home, Path::new("/home/me/.config"));
        assert_eq!(dirs.host_usr, Path::new("/run/host/usr"));
        assert_eq!(dirs.system_flatpak, Path::new("/var/lib/flatpak"));
    }

    #[test]
    fn host_dirs_use_the_xdg_variables_outside_flatpak() {
        let vars = |name: &str| match name {
            "XDG_DATA_HOME" => Some(OsString::from("/data")),
            "XDG_CONFIG_HOME" => Some(OsString::from("relative/is/ignored")),
            _ => None,
        };

        let dirs = HostDirs::from_vars(Path::new("/home/me"), false, vars);

        assert_eq!(dirs.data_home, Path::new("/data"));
        assert_eq!(dirs.config_home, Path::new("/home/me/.config"));
        assert_eq!(dirs.host_usr, Path::new("/usr"));
        assert_eq!(dirs.system_flatpak, Path::new("/var/lib/flatpak"));
    }

    #[test]
    fn resolving_records_the_location_in_source_state() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/lutris");
        touch(&root.join("pga.db"));
        let store = Store::open(&home.path().join("library.db")).unwrap();

        let resolved = resolve_and_record(&store, Source::Lutris, &dirs(home.path())).unwrap();

        assert_eq!(resolved, Ok(root.clone()));
        let state = store.source_state(Source::Lutris).unwrap().unwrap();
        assert_eq!(state.resolved_location, Some(root));
    }

    #[test]
    fn the_stored_override_is_used_and_kept() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(".local/share/lutris/pga.db"));
        let custom = home.path().join("custom");
        touch(&custom.join("pga.db"));
        let store = Store::open(&home.path().join("library.db")).unwrap();
        set_user_override(&store, Source::Lutris, Some(&custom)).unwrap();

        let resolved = resolve_and_record(&store, Source::Lutris, &dirs(home.path())).unwrap();

        assert_eq!(resolved, Ok(custom.clone()));
        let state = store.source_state(Source::Lutris).unwrap().unwrap();
        assert_eq!(state.user_override, Some(custom.clone()));
        assert_eq!(state.resolved_location, Some(custom));
    }

    #[test]
    fn a_failed_resolution_clears_the_recorded_location() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/lutris");
        touch(&root.join("pga.db"));
        let store = Store::open(&home.path().join("library.db")).unwrap();
        resolve_and_record(&store, Source::Lutris, &dirs(home.path()))
            .unwrap()
            .unwrap();
        std::fs::remove_dir_all(&root).unwrap();

        let resolved = resolve_and_record(&store, Source::Lutris, &dirs(home.path())).unwrap();

        assert_eq!(resolved, Err(ResolveError::NotFound));
        let state = store.source_state(Source::Lutris).unwrap().unwrap();
        assert_eq!(state.resolved_location, None);
    }

    #[test]
    fn clearing_the_override_goes_back_to_the_candidates() {
        let home = tempfile::tempdir().unwrap();
        let root = home.path().join(".local/share/lutris");
        touch(&root.join("pga.db"));
        let store = Store::open(&home.path().join("library.db")).unwrap();
        set_user_override(&store, Source::Lutris, Some(Path::new("/nowhere"))).unwrap();

        set_user_override(&store, Source::Lutris, None).unwrap();

        let resolved = resolve_and_record(&store, Source::Lutris, &dirs(home.path())).unwrap();
        assert_eq!(resolved, Ok(root));
    }
}
