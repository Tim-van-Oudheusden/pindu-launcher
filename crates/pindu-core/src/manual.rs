//! Games added by hand: a folder the user granted, a program in it, a title and options.
//! They start through host spawn and record their own sessions. Art the user picks for
//! any game is kept here too, in the data folder.

use std::fmt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::model::{
    ArtLayout, ArtOrigin, Artwork, Game, GameId, LaunchConfig, LaunchKind, Platform, Source,
    Timestamp,
};
use crate::sources::sha256::sha256_hex;
use crate::store::{self, Store};

/// How deep below the game folder programs are looked for.
const EXECUTABLE_DEPTH: usize = 3;

/// The Add or Edit form of a game added by hand. Paths are host paths; the program and
/// working folder may be relative to the game folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualGame {
    pub title: String,
    pub folder: PathBuf,
    pub executable: String,
    /// Arguments as typed, split with shell-like quoting (no shell is run).
    pub arguments: String,
    /// Empty for the program's own folder.
    pub working_dir: String,
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

/// Adds a game by hand and returns its new ID.
pub fn add_game(store: &Store, form: &ManualGame, now: Timestamp) -> Result<GameId, ManualError> {
    let config = launch_config(form)?;
    let id = GameId::new(Source::Manual, &new_native_id(&form.title));
    let mut game = Game::new(id.clone(), form.title.trim(), LaunchKind::HostCommand, now);
    describe(&mut game, form);
    store.in_transaction(|store| {
        store.insert_game(&game)?;
        store.set_launch_config(&id, &config)
    })?;
    Ok(id)
}

/// Saves an edited game added by hand. Launch options set elsewhere (environment,
/// wrappers, GameMode, MangoHud) are kept.
pub fn update_game(store: &Store, id: &GameId, form: &ManualGame) -> Result<(), ManualError> {
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
    Ok(Some(ManualGame {
        title: game.title,
        executable: relative(&config.executable),
        arguments: join_arguments(&config.arguments),
        working_dir,
        folder,
    }))
}

fn describe(game: &mut Game, form: &ManualGame) {
    game.installed = true;
    game.platform = Some(Platform::Linux);
    game.install_path = Some(form.folder.clone());
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

/// The programs in a game folder (files with an execute bit), at most three folders
/// deep, skipping hidden ones; relative to the folder, shallowest first, then by name.
pub fn find_executables(folder: &Path) -> Vec<PathBuf> {
    fn walk(folder: &Path, base: &Path, depth: usize, out: &mut Vec<PathBuf>) {
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
            if metadata.is_dir() && depth < EXECUTABLE_DEPTH {
                walk(&path, base, depth + 1, out);
            } else if metadata.is_file()
                && metadata.permissions().mode() & 0o111 != 0
                && let Ok(relative) = path.strip_prefix(base)
            {
                out.push(relative.to_owned());
            }
        }
    }
    let mut found = Vec::new();
    walk(folder, folder, 1, &mut found);
    found.sort_by(|a, b| (a.components().count(), a).cmp(&(b.components().count(), b)));
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
    let folder = art_dir.join(&sha256_hex(id.as_str().as_bytes())[..16]);
    let path = folder.join(format!("{layout}.{extension}"));
    std::fs::create_dir_all(&folder)?;
    remove_user_art(store, id, layout)?;
    std::fs::write(&path, bytes)?;
    store.set_artwork(&Artwork {
        game_id: id.clone(),
        layout,
        origin: ArtOrigin::User,
        path: path.clone(),
    })?;
    Ok(path)
}

/// Removes the user's art of a game in `layout`, file and record.
pub fn remove_user_art(store: &Store, id: &GameId, layout: ArtLayout) -> Result<(), ManualError> {
    for art in store.artwork(id)? {
        if art.layout == layout && art.origin == ArtOrigin::User {
            match std::fs::remove_file(&art.path) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    return Err(error.into());
                }
                _ => {}
            }
            store.delete_artwork(id, layout, ArtOrigin::User)?;
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

    fn temp_store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        (dir, store)
    }

    fn form() -> ManualGame {
        ManualGame {
            title: "Celeste".into(),
            folder: "/games/Celeste".into(),
            executable: "bin/Celeste".into(),
            arguments: "--fullscreen \"--save dir\"".into(),
            working_dir: String::new(),
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

    #[test]
    fn adding_a_game_stores_it_with_its_command() {
        let (_dir, store) = temp_store();

        let id = add_game(&store, &form(), 100).unwrap();

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

    #[test]
    fn two_games_with_the_same_title_get_their_own_ids() {
        let (_dir, store) = temp_store();

        let first = add_game(&store, &form(), 100).unwrap();
        let second = add_game(&store, &form(), 100).unwrap();

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

        assert_eq!(add_game(&store, &untitled, 1), Err(ManualError::NoTitle));
        assert_eq!(
            add_game(&store, &no_program, 1),
            Err(ManualError::NoExecutable)
        );
        assert!(matches!(
            add_game(&store, &broken, 1),
            Err(ManualError::Arguments(_))
        ));
        assert!(store.games().unwrap().is_empty());
    }

    #[test]
    fn editing_keeps_the_options_set_elsewhere() {
        let (_dir, store) = temp_store();
        let id = add_game(&store, &form(), 100).unwrap();
        let mut config = store.launch_config(&id).unwrap().unwrap();
        config.gamemode = true;
        store.set_launch_config(&id, &config).unwrap();

        let edited = ManualGame {
            title: "Celeste Classic".into(),
            working_dir: "/tmp".into(),
            ..form()
        };
        update_game(&store, &id, &edited).unwrap();

        assert_eq!(store.game(&id).unwrap().unwrap().title, "Celeste Classic");
        let config = store.launch_config(&id).unwrap().unwrap();
        assert_eq!(config.working_dir.as_deref(), Some(Path::new("/tmp")));
        assert!(config.gamemode);
    }

    #[test]
    fn a_stored_game_reads_back_as_its_form() {
        let (_dir, store) = temp_store();
        let id = add_game(&store, &form(), 100).unwrap();

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
    fn user_art_lives_in_the_data_folder_and_wins_over_source_art() {
        let (dir, store) = temp_store();
        let id = add_game(&store, &form(), 100).unwrap();
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
}
