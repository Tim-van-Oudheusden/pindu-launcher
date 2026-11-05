//! Games added by hand: a folder the user granted, a program in it, a title and options.
//! Native games start through host spawn and record their own sessions; Windows games run
//! through umu and Proton. Art the user picks for any game, and art fetched from
//! SteamGridDB, is kept here too, in the data folder.

use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{
    ArtLayout, ArtOrigin, Artwork, Game, GameId, LaunchConfig, LaunchKind, Platform, Source,
    Timestamp,
};
use crate::pe::Icon;
use crate::proton::Defaults;
use crate::sources::sha256::sha256_hex;
use crate::store::{self, Store};

/// How deep below the game folder programs are looked for.
const EXECUTABLE_DEPTH: usize = 3;
/// How deep below a Windows game's folder its programs are looked for.
const WINDOWS_EXECUTABLE_DEPTH: usize = 4;
/// How deep below a prefix's top folders (such as `Program Files`) installed programs are
/// looked for: deep enough for a publisher folder above an engine's `Binaries/Win64`.
const PREFIX_PROGRAM_DEPTH: usize = 6;

/// The Add or Edit form of a game added by hand. The program and working folder may be
/// relative to the game folder. Paths are as the game's runner sees them: host paths for
/// native games (host spawn), paths inside the sandbox for Windows games (umu runs there).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualGame {
    pub title: String,
    pub folder: PathBuf,
    pub executable: String,
    /// Arguments as typed, split with shell-like quoting (no shell is run).
    pub arguments: String,
    /// Empty for the program's own folder.
    pub working_dir: String,
    /// Set for a Windows game, run through umu and Proton.
    pub windows: Option<WindowsOptions>,
}

/// How a Windows game added by hand runs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsOptions {
    /// The Proton build, by its internal name; `None` for the newest one at each start.
    pub proton_version: Option<String>,
    /// The game's Wine prefix; `None` for a new one of its own in the prefixes folder.
    pub prefix: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ManualError {
    NoTitle,
    NoExecutable,
    Arguments(String),
    Store(String),
    Io(String),
}

impl fmt::Display for ManualError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTitle => write!(f, "the game has no title"),
            Self::NoExecutable => write!(f, "the game has no program"),
            Self::Arguments(error) => write!(f, "the arguments are not valid: {error}"),
            Self::Store(error) | Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ManualError {}

impl From<store::StoreError> for ManualError {
    fn from(error: store::StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

impl From<std::io::Error> for ManualError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.to_string())
    }
}

/// Adds a game by hand and returns its new ID. A Windows game without a prefix gets a
/// new one of its own in the default prefixes folder, and starts with the default tools.
pub fn add_game(
    store: &Store,
    form: &ManualGame,
    defaults: &Defaults,
    now: Timestamp,
) -> Result<GameId, ManualError> {
    let mut config = launch_config(form)?;
    let id = GameId::new(Source::Manual, &new_native_id(&form.title));
    set_runner(&mut config, form, || defaults.prefixes.join(id.native_id()));
    if form.windows.is_some() {
        config.mangohud = defaults.mangohud;
        config.gamemode = defaults.gamemode;
    }
    let mut game = Game::new(id.clone(), form.title.trim(), LaunchKind::HostCommand, now);
    describe(&mut game, form);
    store.in_transaction(|store| {
        store.insert_game(&game)?;
        store.set_launch_config(&id, &config)
    })?;
    Ok(id)
}

/// Saves an edited game added by hand. Launch options set elsewhere (environment,
/// wrappers, GameMode, MangoHud) are kept, and so is a Windows game's prefix unless the
/// form names another; a game that becomes a Windows game gets one in `prefixes`.
pub fn update_game(
    store: &Store,
    id: &GameId,
    form: &ManualGame,
    prefixes: &Path,
) -> Result<(), ManualError> {
    let command = launch_config(form)?;
    store.in_transaction(|store| {
        let Some(mut game) = store.game(id)? else {
            return Err(store::StoreError::NotFound);
        };
        game.title = form.title.trim().to_owned();
        game.sort_title = game.title.clone();
        describe(&mut game, form);
        store.update_game(&game)?;
        let mut config = store.launch_config(id)?.unwrap_or_default();
        config.executable = command.executable;
        config.arguments = command.arguments;
        config.working_dir = command.working_dir;
        let kept = config.prefix_path.take();
        set_runner(&mut config, form, || {
            kept.unwrap_or_else(|| prefixes.join(id.native_id()))
        });
        store.set_launch_config(id, &config)
    })?;
    Ok(())
}

/// The form of a stored game added by hand, for editing.
pub fn read_game(store: &Store, id: &GameId) -> Result<Option<ManualGame>, ManualError> {
    let (Some(game), Some(config)) = (store.game(id)?, store.launch_config(id)?) else {
        return Ok(None);
    };
    let folder = game.install_path.unwrap_or_default();
    let relative = |path: &Path| {
        path.strip_prefix(&folder)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned()
    };
    let working_dir = match &config.working_dir {
        Some(dir) if Some(dir.as_path()) != config.executable.parent() => relative(dir),
        _ => String::new(),
    };
    let windows = (game.launch_kind == LaunchKind::Proton).then(|| WindowsOptions {
        proton_version: config.proton_version.clone(),
        prefix: config.prefix_path.clone(),
    });
    Ok(Some(ManualGame {
        title: game.title,
        executable: relative(&config.executable),
        arguments: join_arguments(&config.arguments),
        working_dir,
        folder,
        windows,
    }))
}

fn describe(game: &mut Game, form: &ManualGame) {
    game.installed = true;
    (game.launch_kind, game.platform) = match form.windows {
        Some(_) => (LaunchKind::Proton, Some(Platform::Windows)),
        None => (LaunchKind::HostCommand, Some(Platform::Linux)),
    };
    game.install_path = Some(form.folder.clone());
}

/// Sets the Proton build and prefix of a Windows game, `default_prefix` when the form
/// names none; native games have neither.
fn set_runner(
    config: &mut LaunchConfig,
    form: &ManualGame,
    default_prefix: impl FnOnce() -> PathBuf,
) {
    (config.proton_version, config.prefix_path) = match &form.windows {
        Some(windows) => (
            windows.proton_version.clone(),
            Some(windows.prefix.clone().unwrap_or_else(default_prefix)),
        ),
        None => (None, None),
    };
}

fn launch_config(form: &ManualGame) -> Result<LaunchConfig, ManualError> {
    if form.title.trim().is_empty() {
        return Err(ManualError::NoTitle);
    }
    if form.executable.trim().is_empty() {
        return Err(ManualError::NoExecutable);
    }
    let arguments = split_arguments(&form.arguments).map_err(ManualError::Arguments)?;
    let executable = form.folder.join(form.executable.trim());
    let working_dir = match form.working_dir.trim() {
        "" => executable.parent().map(Path::to_owned),
        dir => Some(form.folder.join(dir)),
    };
    Ok(LaunchConfig {
        executable,
        arguments,
        working_dir,
        ..LaunchConfig::default()
    })
}

/// A native ID no other game has: a hash of the title, the time and a counter.
fn new_native_id(title: &str) -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let count = COUNTER.fetch_add(1, Ordering::Relaxed);
    let seed = format!("{title}\n{nanos}\n{count}\n{}", std::process::id());
    sha256_hex(seed.as_bytes())[..16].to_owned()
}

/// A prefix folder in `prefixes` that no game has yet, for an installer run before the
/// game is added.
pub fn new_prefix(prefixes: &Path, title: &str) -> PathBuf {
    prefixes.join(new_native_id(title))
}

/// The programs in a game folder (files with an execute bit), at most three folders
/// deep, skipping hidden ones; relative to the folder, shallowest first, then by name.
pub fn find_executables(folder: &Path) -> Vec<PathBuf> {
    let mut found = find_files(folder, EXECUTABLE_DEPTH, &|_, metadata| {
        metadata.permissions().mode() & 0o111 != 0
    });
    found.sort_by(|a, b| (a.components().count(), a).cmp(&(b.components().count(), b)));
    found
}

/// The Windows programs in a game folder (`.exe` files), at most four folders deep, as
/// engines keep theirs in folders like `Binaries/Win64`; relative to the folder,
/// uninstallers last, then shallowest first, then by name.
pub fn find_windows_executables(folder: &Path) -> Vec<PathBuf> {
    let mut found = find_files(folder, WINDOWS_EXECUTABLE_DEPTH, &|path, _| is_exe(path));
    sort_windows_programs(&mut found);
    found
}

/// The Windows programs in a prefix's `drive_c`, skipping Wine's own `windows` folder:
/// what an installer may add to.
pub fn prefix_programs(prefix: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(prefix.join("drive_c")) else {
        return Vec::new();
    };
    let mut found: Vec<PathBuf> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter(|entry| !entry.file_name().eq_ignore_ascii_case("windows"))
        .flat_map(|entry| {
            let folder = entry.path();
            find_files(&folder, PREFIX_PROGRAM_DEPTH, &|path, _| is_exe(path))
                .into_iter()
                .map(move |program| folder.join(program))
        })
        .collect();
    found.sort();
    found
}

/// What an installer added to a prefix, from its programs before and after it ran: the
/// deepest folder the new programs share, and the programs relative to it, sorted as in
/// [`find_windows_executables`]. `None` when it added none.
pub fn installed_programs(
    before: &[PathBuf],
    after: &[PathBuf],
) -> Option<(PathBuf, Vec<PathBuf>)> {
    let added: Vec<&PathBuf> = after
        .iter()
        .filter(|program| !before.contains(program))
        .collect();
    let mut folder = added.first()?.parent()?.to_owned();
    while !added.iter().all(|program| program.starts_with(&folder)) {
        folder = folder.parent()?.to_owned();
    }
    let mut programs: Vec<PathBuf> = added
        .iter()
        .filter_map(|program| Some(program.strip_prefix(&folder).ok()?.to_owned()))
        .collect();
    sort_windows_programs(&mut programs);
    Some((folder, programs))
}

fn is_exe(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
}

fn sort_windows_programs(programs: &mut [PathBuf]) {
    let is_uninstaller = |path: &Path| {
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        name.starts_with("unins") || name.contains("uninstall")
    };
    programs.sort_by_cached_key(|path| {
        (
            is_uninstaller(path),
            path.components().count(),
            path.clone(),
        )
    });
}

/// The files below `folder` that `keep` accepts, at most `depth` levels deep (1 is the
/// folder itself), skipping hidden ones; relative to the folder, unsorted.
fn find_files(
    folder: &Path,
    depth: usize,
    keep: &dyn Fn(&Path, &std::fs::Metadata) -> bool,
) -> Vec<PathBuf> {
    fn walk(
        folder: &Path,
        base: &Path,
        level: usize,
        depth: usize,
        keep: &dyn Fn(&Path, &std::fs::Metadata) -> bool,
        out: &mut Vec<PathBuf>,
    ) {
        let Ok(entries) = std::fs::read_dir(folder) else {
            return;
        };
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() && level < depth {
                walk(&path, base, level + 1, depth, keep, out);
            } else if metadata.is_file()
                && keep(&path, &metadata)
                && let Ok(relative) = path.strip_prefix(base)
            {
                out.push(relative.to_owned());
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, 1, depth, keep, &mut found);
    found
}

/// Splits typed arguments into a list the way a POSIX shell would, without running one:
/// single quotes keep text as it is, double quotes allow `\"`, `\\`, `\$` and `` \` ``,
/// and a backslash outside quotes keeps the next character.
pub fn split_arguments(text: &str) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut word: Option<String> = None;
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        match c {
            ' ' | '\t' | '\n' => arguments.extend(word.take()),
            '\'' => {
                let word = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => word.push(c),
                        None => return Err("a single quote is not closed".into()),
                    }
                }
            }
            '"' => {
                let word = word.get_or_insert_default();
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(c @ ('"' | '\\' | '$' | '`')) => word.push(c),
                            Some(c) => word.extend(['\\', c]),
                            None => return Err("a double quote is not closed".into()),
                        },
                        Some(c) => word.push(c),
                        None => return Err("a double quote is not closed".into()),
                    }
                }
            }
            '\\' => match chars.next() {
                Some(c) => word.get_or_insert_default().push(c),
                None => return Err("the text ends with a backslash".into()),
            },
            c => word.get_or_insert_default().push(c),
        }
    }
    arguments.extend(word);
    Ok(arguments)
}

/// Joins arguments into text that [`split_arguments`] reads back the same, quoting only
/// where needed.
pub fn join_arguments(arguments: &[String]) -> String {
    let quote = |argument: &String| {
        let plain = !argument.is_empty()
            && argument
                .chars()
                .all(|c| c.is_alphanumeric() || "-_./=:,+@%".contains(c));
        if plain {
            argument.clone()
        } else {
            format!("'{}'", argument.replace('\'', r"'\''"))
        }
    };
    arguments.iter().map(quote).collect::<Vec<_>>().join(" ")
}

/// Stores art the user picked for a game, in `art_dir`, as the game's art in `layout`.
/// Replaces earlier user art in that layout.
pub fn set_user_art(
    store: &Store,
    art_dir: &Path,
    id: &GameId,
    layout: ArtLayout,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, ManualError> {
    store_art(
        store,
        art_dir,
        id,
        layout,
        ArtOrigin::User,
        extension,
        bytes,
    )
}

/// Removes the user's art of a game in `layout`, file and record.
pub fn remove_user_art(store: &Store, id: &GameId, layout: ArtLayout) -> Result<(), ManualError> {
    remove_art(store, id, layout, ArtOrigin::User)
}

/// Stores art fetched from SteamGridDB, beside user art in `art_dir` (not in a cache
/// that may be cleared). Replaces art fetched earlier in that layout.
pub fn set_steamgriddb_art(
    store: &Store,
    art_dir: &Path,
    id: &GameId,
    layout: ArtLayout,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, ManualError> {
    store_art(
        store,
        art_dir,
        id,
        layout,
        ArtOrigin::SteamGridDb,
        extension,
        bytes,
    )
}

/// Stores the icon a Windows game's program carries as the game's own icon, beside user
/// art in `art_dir`.
pub fn set_exe_icon(
    store: &Store,
    art_dir: &Path,
    id: &GameId,
    icon: &Icon,
) -> Result<PathBuf, ManualError> {
    store_art(
        store,
        art_dir,
        id,
        ArtLayout::Icon,
        ArtOrigin::Source,
        icon.extension,
        &icon.bytes,
    )
}

fn store_art(
    store: &Store,
    art_dir: &Path,
    id: &GameId,
    layout: ArtLayout,
    origin: ArtOrigin,
    extension: &str,
    bytes: &[u8],
) -> Result<PathBuf, ManualError> {
    let folder = art_dir.join(&sha256_hex(id.as_str().as_bytes())[..16]);
    let name = match origin {
        ArtOrigin::User => format!("{layout}.{extension}"),
        _ => format!("{layout}-{origin}.{extension}"),
    };
    let path = folder.join(name);
    std::fs::create_dir_all(&folder)?;
    remove_art(store, id, layout, origin)?;
    std::fs::write(&path, bytes)?;
    store.set_artwork(&Artwork {
        game_id: id.clone(),
        layout,
        origin,
        path: path.clone(),
    })?;
    Ok(path)
}

fn remove_art(
    store: &Store,
    id: &GameId,
    layout: ArtLayout,
    origin: ArtOrigin,
) -> Result<(), ManualError> {
    for art in store.artwork(id)? {
        if art.layout == layout && art.origin == origin {
            match std::fs::remove_file(&art.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
            store.delete_artwork(id, layout, origin)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::*;
    use crate::model::{ArtLayout, ArtOrigin, LaunchKind, Platform, Source};
    use crate::store::Store;

    const PREFIXES: &str = "/home/me/Games/pindu-launcher/prefixes";

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        (dir, store)
    }

    fn defaults() -> Defaults {
        Defaults {
            proton_version: None,
            prefixes: PREFIXES.into(),
            mangohud: false,
            gamemode: false,
        }
    }

    fn form() -> ManualGame {
        ManualGame {
            title: "Celeste".into(),
            folder: "/games/Celeste".into(),
            executable: "bin/Celeste".into(),
            arguments: "--fullscreen \"--save dir\"".into(),
            working_dir: String::new(),
            windows: None,
        }
    }

    #[test]
    fn arguments_split_like_a_shell_without_one() {
        assert_eq!(
            split_arguments(r#"-w 1920 "two words" 'single quoted' back\ slash"#),
            Ok(vec![
                "-w".into(),
                "1920".into(),
                "two words".into(),
                "single quoted".into(),
                "back slash".into(),
            ])
        );
        assert_eq!(split_arguments("  "), Ok(vec![]));
        assert!(split_arguments("\"unclosed").is_err());
    }

    #[test]
    fn joined_arguments_split_back_the_same() {
        let arguments = vec![
            "plain".to_owned(),
            "two words".to_owned(),
            "it's".to_owned(),
            "say \"hi\"".to_owned(),
            String::new(),
        ];

        assert_eq!(split_arguments(&join_arguments(&arguments)), Ok(arguments));
    }

    #[test]
    fn executables_are_listed_relative_to_the_folder() {
        let dir = tempfile::tempdir().unwrap();
        let touch = |path: &str, mode: u32| {
            let path = dir.path().join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        };
        touch("Celeste", 0o755);
        touch("lib64/libSDL2.so", 0o755);
        touch("zz-launcher.sh", 0o700);
        touch("Content/data.bin", 0o644);
        touch("tools/deep/er/still/hidden-by-depth", 0o755);
        touch(".hidden/run", 0o755);

        let found = find_executables(dir.path());

        // Programs at the top of the folder come first.
        assert_eq!(
            found,
            [
                Path::new("Celeste"),
                Path::new("zz-launcher.sh"),
                Path::new("lib64/libSDL2.so")
            ]
        );
    }

    fn touch_all(root: &Path, paths: &[&str]) {
        for path in paths {
            let path = root.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, "").unwrap();
        }
    }

    #[test]
    fn windows_programs_are_the_exe_files_with_uninstallers_last() {
        let dir = tempfile::tempdir().unwrap();
        touch_all(
            dir.path(),
            &[
                "unins000.exe",
                "Celeste.EXE",
                "Celeste.pdb",
                "Engine/Binaries/Win64/Celeste-Win64-Shipping.exe",
                "Engine/Binaries/Win64/too/deep.exe",
                ".backup/old.exe",
            ],
        );

        assert_eq!(
            find_windows_executables(dir.path()),
            [
                Path::new("Celeste.EXE"),
                Path::new("Engine/Binaries/Win64/Celeste-Win64-Shipping.exe"),
                Path::new("unins000.exe"),
            ]
        );
    }

    #[test]
    fn an_installer_leaves_its_programs_in_a_folder_they_share() {
        let prefix = tempfile::tempdir().unwrap();
        let drive = prefix.path().join("drive_c");
        touch_all(
            &drive,
            &[
                "windows/system32/notepad.exe",
                "Program Files/Internet Explorer/iexplore.exe",
            ],
        );
        let before = prefix_programs(prefix.path());
        assert_eq!(
            before,
            [drive.join("Program Files/Internet Explorer/iexplore.exe")]
        );
        touch_all(
            &drive,
            &[
                "Program Files (x86)/Celeste/unins000.exe",
                "Program Files (x86)/Celeste/tools/editor.exe",
                "Program Files (x86)/Celeste/Celeste.exe",
            ],
        );

        let installed = installed_programs(&before, &prefix_programs(prefix.path()));

        assert_eq!(
            installed,
            Some((
                drive.join("Program Files (x86)/Celeste"),
                vec![
                    PathBuf::from("Celeste.exe"),
                    PathBuf::from("tools/editor.exe"),
                    PathBuf::from("unins000.exe"),
                ]
            ))
        );
        assert_eq!(installed_programs(&before, &before), None);
    }

    #[test]
    fn adding_a_game_stores_it_with_its_command() {
        let (_dir, store) = temp_store();

        let id = add_game(&store, &form(), &defaults(), 100).unwrap();

        assert_eq!(id.source(), Source::Manual);
        let game = store.game(&id).unwrap().unwrap();
        assert_eq!(game.title, "Celeste");
        assert_eq!(game.launch_kind, LaunchKind::HostCommand);
        assert_eq!(game.platform, Some(Platform::Linux));
        assert!(game.installed);
        assert_eq!(
            game.install_path.as_deref(),
            Some(Path::new("/games/Celeste"))
        );
        assert_eq!(game.added_at, 100);
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!(config.executable, Path::new("/games/Celeste/bin/Celeste"));
        assert_eq!(config.arguments, ["--fullscreen", "--save dir"]);
        // Without a working folder, the game runs in its program's folder.
        assert_eq!(
            config.working_dir.as_deref(),
            Some(Path::new("/games/Celeste/bin"))
        );
    }

    fn windows_form() -> ManualGame {
        ManualGame {
            folder: "/run/user/1000/doc/1a2b3c/Celeste".into(),
            executable: "Celeste.exe".into(),
            windows: Some(WindowsOptions::default()),
            ..form()
        }
    }

    #[test]
    fn a_windows_game_runs_through_proton_in_a_prefix_of_its_own() {
        let (_dir, store) = temp_store();
        let form = ManualGame {
            windows: Some(WindowsOptions {
                proton_version: Some("GE-Proton10-34".into()),
                prefix: None,
            }),
            ..windows_form()
        };

        let id = add_game(&store, &form, &defaults(), 100).unwrap();

        let game = store.game(&id).unwrap().unwrap();
        assert_eq!(game.launch_kind, LaunchKind::Proton);
        assert_eq!(game.platform, Some(Platform::Windows));
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!(
            config.executable,
            Path::new("/run/user/1000/doc/1a2b3c/Celeste/Celeste.exe")
        );
        assert_eq!(config.proton_version.as_deref(), Some("GE-Proton10-34"));
        assert_eq!(
            config.prefix_path,
            Some(Path::new(PREFIXES).join(id.native_id()))
        );
    }

    #[test]
    fn a_new_windows_game_starts_with_the_default_tools_and_a_native_one_without() {
        let (_dir, store) = temp_store();
        let defaults = Defaults {
            mangohud: true,
            gamemode: true,
            ..defaults()
        };

        let windows = add_game(&store, &windows_form(), &defaults, 100).unwrap();
        let native = add_game(&store, &form(), &defaults, 100).unwrap();

        let windows = store.launch_config(&windows).unwrap().unwrap();
        assert!(windows.mangohud && windows.gamemode);
        // The default stays the default: no build of its own.
        assert_eq!(windows.proton_version, None);
        let native = store.launch_config(&native).unwrap().unwrap();
        assert!(!native.mangohud && !native.gamemode);
    }

    #[test]
    fn two_games_with_the_same_title_get_their_own_ids() {
        let (_dir, store) = temp_store();

        let first = add_game(&store, &form(), &defaults(), 100).unwrap();
        let second = add_game(&store, &form(), &defaults(), 100).unwrap();

        assert_ne!(first, second);
    }

    #[test]
    fn an_installer_gets_a_new_prefix_in_the_prefixes_folder() {
        let prefixes = Path::new(PREFIXES);

        let first = new_prefix(prefixes, "Celeste");
        let second = new_prefix(prefixes, "Celeste");

        assert_eq!(first.parent(), Some(prefixes));
        assert_eq!(second.parent(), Some(prefixes));
        assert_ne!(first, second);
    }

    #[test]
    fn a_game_needs_a_title_and_a_program() {
        let (_dir, store) = temp_store();

        let untitled = ManualGame {
            title: "  ".into(),
            ..form()
        };
        let no_program = ManualGame {
            executable: String::new(),
            ..form()
        };
        let broken = ManualGame {
            arguments: "\"oops".into(),
            ..form()
        };

        assert_eq!(
            add_game(&store, &untitled, &defaults(), 1),
            Err(ManualError::NoTitle)
        );
        assert_eq!(
            add_game(&store, &no_program, &defaults(), 1),
            Err(ManualError::NoExecutable)
        );
        assert!(matches!(
            add_game(&store, &broken, &defaults(), 1),
            Err(ManualError::Arguments(_))
        ));
        assert!(store.games().unwrap().is_empty());
    }

    #[test]
    fn editing_keeps_the_options_set_elsewhere() {
        let (_dir, store) = temp_store();
        let id = add_game(&store, &form(), &defaults(), 100).unwrap();
        let mut config = store.launch_config(&id).unwrap().unwrap();
        config.gamemode = true;
        store.set_launch_config(&id, &config).unwrap();

        let edited = ManualGame {
            title: "Celeste Classic".into(),
            working_dir: "/tmp".into(),
            ..form()
        };
        update_game(&store, &id, &edited, Path::new(PREFIXES)).unwrap();

        assert_eq!(store.game(&id).unwrap().unwrap().title, "Celeste Classic");
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!(config.working_dir.as_deref(), Some(Path::new("/tmp")));
        assert!(config.gamemode);
    }

    #[test]
    fn a_stored_game_reads_back_as_its_form() {
        let (_dir, store) = temp_store();
        let id = add_game(&store, &form(), &defaults(), 100).unwrap();

        let read = read_game(&store, &id).unwrap();

        assert_eq!(
            read,
            Some(ManualGame {
                // The same arguments, quoted the way they are joined.
                arguments: "--fullscreen '--save dir'".into(),
                ..form()
            })
        );
    }

    #[test]
    fn an_edited_windows_game_keeps_its_prefix_and_a_native_one_drops_proton() {
        let (_dir, store) = temp_store();
        let id = add_game(&store, &windows_form(), &defaults(), 100).unwrap();
        let prefix = Path::new(PREFIXES).join(id.native_id());

        let read = read_game(&store, &id).unwrap().unwrap();
        assert_eq!(
            read.windows,
            Some(WindowsOptions {
                proton_version: None,
                prefix: Some(prefix.clone()),
            })
        );

        // A new build, and no prefix named: the game keeps its own.
        let newer = ManualGame {
            windows: Some(WindowsOptions {
                proton_version: Some("GE-Proton11-3".into()),
                prefix: None,
            }),
            ..windows_form()
        };
        update_game(&store, &id, &newer, Path::new("/elsewhere")).unwrap();
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!(config.proton_version.as_deref(), Some("GE-Proton11-3"));
        assert_eq!(config.prefix_path, Some(prefix));

        update_game(&store, &id, &form(), Path::new(PREFIXES)).unwrap();
        let game = store.game(&id).unwrap().unwrap();
        assert_eq!(game.launch_kind, LaunchKind::HostCommand);
        assert_eq!(game.platform, Some(Platform::Linux));
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!((config.proton_version, config.prefix_path), (None, None));
    }

    #[test]
    fn user_art_lives_in_the_data_folder_and_wins_over_source_art() {
        let (dir, store) = temp_store();
        let id = add_game(&store, &form(), &defaults(), 100).unwrap();
        let art_dir = dir.path().join("art");

        let path = set_user_art(&store, &art_dir, &id, ArtLayout::Cover, "png", b"bytes").unwrap();

        assert!(path.starts_with(&art_dir));
        assert_eq!(std::fs::read(&path).unwrap(), b"bytes");
        let art = store.artwork(&id).unwrap();
        assert_eq!(art.len(), 1);
        assert_eq!(art[0].origin, ArtOrigin::User);
        assert_eq!(art[0].path, path);

        remove_user_art(&store, &id, ArtLayout::Cover).unwrap();

        assert!(store.artwork(&id).unwrap().is_empty());
        assert!(!path.exists());
    }

    #[test]
    fn steamgriddb_art_is_kept_beside_user_art_and_replaced_when_fetched_again() {
        let (dir, store) = temp_store();
        let id = add_game(&store, &form(), &defaults(), 100).unwrap();
        let art_dir = dir.path().join("art");
        let user = set_user_art(&store, &art_dir, &id, ArtLayout::Cover, "png", b"mine").unwrap();
        let first =
            set_steamgriddb_art(&store, &art_dir, &id, ArtLayout::Cover, "jpg", b"one").unwrap();

        let second =
            set_steamgriddb_art(&store, &art_dir, &id, ArtLayout::Cover, "png", b"two").unwrap();

        assert_eq!(std::fs::read(&second).unwrap(), b"two");
        assert!(!first.exists());
        assert_eq!(std::fs::read(&user).unwrap(), b"mine");
        let mut origins: Vec<_> = store
            .artwork(&id)
            .unwrap()
            .into_iter()
            .map(|art| (art.origin, art.path))
            .collect();
        origins.sort();
        assert_eq!(
            origins,
            [(ArtOrigin::SteamGridDb, second), (ArtOrigin::User, user)]
        );
    }

    #[test]
    fn the_programs_own_icon_is_kept_as_the_games_icon() {
        let (dir, store) = temp_store();
        let id = add_game(&store, &windows_form(), &defaults(), 100).unwrap();
        let icon = Icon {
            bytes: b"\x89PNG icon".to_vec(),
            extension: "png",
        };

        let path = set_exe_icon(&store, &dir.path().join("art"), &id, &icon).unwrap();

        assert_eq!(std::fs::read(&path).unwrap(), icon.bytes);
        assert_eq!(path.extension(), Some("png".as_ref()));
        let art = store.artwork(&id).unwrap();
        assert_eq!(
            (art[0].layout, art[0].origin, &art[0].path),
            (ArtLayout::Icon, ArtOrigin::Source, &path)
        );
    }
}
