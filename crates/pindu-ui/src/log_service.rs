//! Keeps the logs of "Play with Logs": one file per launch in the game's logs folder
//! (see `pindu_core::logs`), with what the game, or umu, writes to its output. Proton
//! writes its own log, with Wine's output, to a folder of its own while the game runs;
//! it is added to the end of the game's log once umu exits.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use gtk::glib;
use pindu_core::launch::Command;
use pindu_core::logs::{self, KEEP};
use pindu_core::model::GameId;
use pindu_core::paths;

/// The log of one launch, as it is written.
#[derive(Debug)]
pub struct GameLog {
    path: PathBuf,
    file: File,
    /// Where Proton writes its log, for Windows games.
    proton_dir: Option<PathBuf>,
}

impl GameLog {
    /// Starts a new log for a game: makes its logs folder, writes the header and drops
    /// the logs beyond the newest `KEEP`. For a Windows game, `command` gets Proton
    /// logging to a folder of its own. Blocks; run it on a worker thread.
    pub fn create(
        id: &GameId,
        title: &str,
        command: &mut Command,
        proton: bool,
    ) -> io::Result<Self> {
        let dir = logs::game_logs_dir(&data_home(), id);
        fs::create_dir_all(&dir)?;
        let now = glib::DateTime::now_local().map_err(io::Error::other)?;
        let stamp = now.format("%Y-%m-%dT%H-%M-%S").map_err(io::Error::other)?;
        let started = now.format("%Y-%m-%d %H:%M:%S").map_err(io::Error::other)?;
        let mut names = file_names(&dir)?;
        let name = logs::new_log_name(&stamp, &names);
        let path = dir.join(&name);
        // Appending, so the game's output and ours never overwrite each other.
        let mut file = OpenOptions::new()
            .append(true)
            .create_new(true)
            .open(&path)?;
        file.write_all(logs::header(title, &started, &command.argv).as_bytes())?;
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
        for stale in logs::stale_logs(&names, KEEP) {
            if let Err(error) = fs::remove_file(dir.join(&stale)) {
                glib::g_warning!("pindu", "Could not remove the old log {stale}: {error}");
            }
        }
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

fn file_names(dir: &Path) -> io::Result<Vec<String>> {
    fs::read_dir(dir)?
        .map(|entry| Ok(entry?.file_name().to_string_lossy().into_owned()))
        .collect()
}

fn data_home() -> PathBuf {
    let xdg_data_home = std::env::var_os("XDG_DATA_HOME").map(PathBuf::from);
    paths::data_home(
        xdg_data_home.as_deref(),
        &std::env::home_dir().unwrap_or_default(),
    )
}
