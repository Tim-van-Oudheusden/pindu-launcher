//! Keeps the logs of a game's launches, one file per launch in the game's logs folder (see
//! `pindu_core::logs`). "Play with Logs" and "Start Safely" keep a full log, with what the
//! game, or umu, writes to its output. Proton writes its own log, with Wine's output, to a
//! folder of its own while the game runs; it is added to the end of the game's log once
//! umu exits. Any other launch of a game Pindu runs keeps a short log: the end of its
//! output, read through a pipe, written once it exits.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use gtk::glib;
use pindu_core::launch::{Command, Start};
use pindu_core::launch_outcome;
use pindu_core::logs::{self, KEEP, SHORT_LOG_BYTES, Tail};
use pindu_core::model::GameId;
use pindu_core::paths;

/// The log a launch keeps: a full one, or a short one.
#[derive(Debug)]
pub enum LaunchLog {
    /// The whole output, for "Play with Logs" and "Start Safely".
    Full(GameLog),
    /// The end of the output, for any other launch.
    Short(ShortLog),
}

impl LaunchLog {
    /// Starts the log `start` asks for: a full one when it keeps a log (see
    /// [`GameLog::create`]), else a short one. Blocks; run it on a worker thread.
    pub fn create(
        id: &GameId,
        title: &str,
        command: &mut Command,
        proton: bool,
        start: Start,
    ) -> io::Result<Self> {
        if start.keeps_log() {
            GameLog::create(id, title, command, proton, start).map(Self::Full)
        } else {
            ShortLog::create(id, title, command).map(Self::Short)
        }
    }

    /// The log's file name in the game's logs folder.
    pub fn name(&self) -> Option<String> {
        let path = match self {
            Self::Full(log) => &log.path,
            Self::Short(log) => &log.path,
        };
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }

    /// Whether the game's output comes to us through a pipe, as a short log reads it,
    /// rather than going to [`Self::output`].
    pub fn reads_pipe(&self) -> bool {
        matches!(self, Self::Short(_))
    }

    /// The file a full log has the game write its output to; none for a short log.
    pub fn output(&self) -> io::Result<Option<File>> {
        match self {
            Self::Full(log) => log.output().map(Some),
            Self::Short(_) => Ok(None),
        }
    }

    /// Adds a line the game, or umu, wrote.
    pub fn line(&mut self, line: &str) {
        match self {
            Self::Full(log) => log.line(line),
            Self::Short(log) => log.tail.push(line),
        }
    }

    /// Ends the log once the game has ended: a full log gets Proton's log, a short one
    /// the end of the output. Blocks; run it on a worker thread.
    pub fn finish(self) -> io::Result<()> {
        match self {
            Self::Full(log) => log.finish(),
            Self::Short(log) => log.finish(),
        }
    }
}

/// The short log of one launch: its header, and the end of its output once it exits.
#[derive(Debug)]
pub struct ShortLog {
    path: PathBuf,
    file: File,
    tail: Tail,
}

impl ShortLog {
    /// Starts a new short log for a game: makes its logs folder, writes the header and
    /// drops the short logs beyond the newest few. Blocks; run it on a worker thread.
    fn create(id: &GameId, title: &str, command: &Command) -> io::Result<Self> {
        let dir = logs::game_logs_dir(&data_home(), id);
        fs::create_dir_all(&dir)?;
        let (stamp, started) = start_stamps()?;
        let mut names = file_names(&dir)?;
        let name = logs::new_short_log_name(&stamp, &names);
        let path = dir.join(&name);
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(logs::header(title, &started, &command.argv).as_bytes())?;
        names.push(name);
        remove_stale(&dir, logs::stale_short_logs(&names, launch_outcome::KEEP));
        Ok(Self {
            path,
            file,
            tail: Tail::new(SHORT_LOG_BYTES),
        })
    }

    /// Writes the end of the output. Blocks; run it on a worker thread.
    fn finish(mut self) -> io::Result<()> {
        self.file.write_all(self.tail.text().as_bytes())
    }
}

/// The log of one launch, as it is written.
#[derive(Debug)]
pub struct GameLog {
    path: PathBuf,
    file: File,
    /// Where Proton writes its log, for Windows games.
    proton_dir: Option<PathBuf>,
}

impl GameLog {
    /// Starts a new log for a game: makes its logs folder, writes the header, which for a
    /// safe `start` says what it left out, and drops the logs beyond the newest `KEEP`. For
    /// a Windows game, `command` gets Proton logging to a folder of its own. Blocks; run it
    /// on a worker thread.
    pub fn create(
        id: &GameId,
        title: &str,
        command: &mut Command,
        proton: bool,
        start: Start,
    ) -> io::Result<Self> {
        let dir = logs::game_logs_dir(&data_home(), id);
        fs::create_dir_all(&dir)?;
        let (stamp, started) = start_stamps()?;
        let mut names = file_names(&dir)?;
        let name = logs::new_log_name(&stamp, &names);
        let path = dir.join(&name);
        // Appending, so the game's output and ours never overwrite each other.
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&path)?;
        let header = match start {
            Start::Safely => logs::safe_header(title, &started, &command.argv),
            Start::Play | Start::WithLogs => logs::header(title, &started, &command.argv),
        };
        file.write_all(header.as_bytes())?;
        // Proton's folder comes last, so a log that could not start leaves none behind.
        let proton_dir = if proton {
            let template = std::env::temp_dir().join("pindu-proton-log-XXXXXX");
            let proton_dir = glib::mkdtemp(template)
                .ok_or_else(|| io::Error::other("could not make Proton's log folder"))?;
            logs::log_proton(command, &proton_dir);
            Some(proton_dir)
        } else {
            None
        };
        names.push(name);
        remove_stale(&dir, logs::stale_logs(&names, KEEP));
        Ok(Self {
            path,
            file,
            proton_dir,
        })
    }

    /// The log file, for a game to write its output to.
    pub fn output(&self) -> io::Result<File> {
        self.file.try_clone()
    }

    /// Adds a line the game, or umu, wrote.
    pub fn line(&mut self, line: &str) {
        if let Err(error) = writeln!(self.file, "{line}") {
            glib::g_warning!("pindu", "Could not write to {:?}: {error}", self.path);
        }
    }

    /// Adds Proton's log, once the game has ended, and removes Proton's folder. Blocks;
    /// run it on a worker thread.
    pub fn finish(mut self) -> io::Result<()> {
        let Some(proton_dir) = self.proton_dir.take() else {
            return Ok(());
        };
        let mut proton_logs: Vec<String> = file_names(&proton_dir)?
            .into_iter()
            .filter(|name| name.ends_with(".log"))
            .collect();
        proton_logs.sort();
        if proton_logs.is_empty() {
            writeln!(self.file, "\n--- Proton wrote no log ---")?;
        }
        for name in proton_logs {
            writeln!(self.file, "\n--- Proton log ({name}) ---")?;
            io::copy(&mut File::open(proton_dir.join(&name))?, &mut self.file)?;
        }
        fs::remove_dir_all(&proton_dir)
    }
}

/// The local time now, as a log's name has it (`2026-11-05T14-03-22`) and as its header
/// says it (`2026-11-05 14:03:22`).
fn start_stamps() -> io::Result<(String, String)> {
    let now = glib::DateTime::now_local().map_err(io::Error::other)?;
    let stamp = now.format("%Y-%m-%dT%H-%M-%S").map_err(io::Error::other)?;
    let started = now.format("%Y-%m-%d %H:%M:%S").map_err(io::Error::other)?;
    Ok((stamp.into(), started.into()))
}

/// Removes the `stale` logs from `dir`, warning about any it could not.
fn remove_stale(dir: &Path, stale: Vec<String>) {
    for stale in stale {
        if let Err(error) = fs::remove_file(dir.join(&stale)) {
            glib::g_warning!("pindu", "Could not remove the old log {stale}: {error}");
        }
    }
}

/// What a toast says when "Play with Logs" starts a game without its log.
pub fn could_not_keep(title: &str) -> String {
    gettext("Could not keep a log of “{title}”").replace("{title}", title)
}

/// A game's newest log, if it has one. Blocks; run it on a worker thread.
pub fn latest(id: &GameId) -> Option<PathBuf> {
    let dir = logs::game_logs_dir(&data_home(), id);
    let names = file_names(&dir).ok()?;
    logs::latest_log(&names).map(|name| dir.join(name))
}

/// The log named `name` of one launch of the game with `id`, while it is kept. Blocks;
/// run it on a worker thread.
pub fn launch_log(id: &GameId, name: &str) -> Option<PathBuf> {
    logs::launch_log(&data_home(), id, name).filter(|path| path.is_file())
}

/// The names of the files in `dir`.
pub(crate) fn file_names(dir: &Path) -> io::Result<Vec<String>> {
    fs::read_dir(dir)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect()
}

/// `$XDG_DATA_HOME`, or its default under the home folder.
pub(crate) fn data_home() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    paths::data_home(
        xdg_data_home.as_deref(),
        &std::env::home_dir().unwrap_or_default(),
    )
}
