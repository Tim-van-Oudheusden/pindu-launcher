//! Steam's first start: a Steam client that has never run downloads its own update,
//! unpacks and installs it, and only then shows its sign-in window, which takes minutes.
//! Its updater writes each step to `logs/bootstrap_log.txt` in the Steam folder, so the
//! launcher follows that log, as [`crate::umu`] follows umu's runtime setup, to show what
//! Steam is doing on its Setting Up Steam screen.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::add_store::StoreApp;
use crate::launch::StoreClient;
use crate::sources::location::HostDirs;

/// How long the screen waits for the first line of Steam's log before it says that Steam
/// did not set itself up: 10 minutes, as long as the screen waits where it cannot read the
/// log at all.
pub const SETUP_START_TIMEOUT: Duration = Duration::from_secs(600);

/// How long the screen waits after the last line in Steam's log before it says that
/// Steam stopped: 5 minutes. A download that still moves never runs out.
pub const SETUP_STALL_TIMEOUT: Duration = Duration::from_secs(300);

/// A step of Steam's set-up, in the order a first start goes through them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Steam is starting; nothing in its log yet.
    Starting,
    /// Steam checks its files and looks for its update.
    Checking,
    /// Steam downloads its update: `done` of `total` kilobytes, `total` 0 before Steam
    /// says how large it is.
    Downloading {
        /// The kilobytes downloaded.
        done: u64,
        /// The kilobytes of the whole update, 0 while unknown.
        total: u64,
    },
    /// Steam unpacks the update it downloaded.
    Unpacking,
    /// Steam installs the update it unpacked.
    Installing,
    /// The update is in place and Steam starts its client, which shows the sign-in.
    Launching,
}

/// Why Steam's set-up stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    /// Steam reached none of its download servers: the computer is offline.
    Offline,
    /// A download failed.
    Download,
}

/// What a line of Steam's bootstrap log says about the set-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogEvent {
    /// The set-up reached a step.
    Step(Step),
    /// Steam found its client up to date: it has been set up before.
    UpToDate,
    /// The set-up failed.
    Failed(Failure),
    /// Another line, which still shows Steam at work.
    Other,
}

/// What a line of Steam's bootstrap log, `[2026-04-17 15:24:48] Extracting package...`,
/// says; `None` for the blank lines between Steam's starts.
pub fn log_event(line: &str) -> Option<LogEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    // Each line starts with its time in brackets.
    let message = match line
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("] "))
    {
        Some((_, message)) => message,
        None => line,
    };
    Some(
        if message.starts_with("Verifying installation")
            || message.starts_with("Checking for available update")
        {
            LogEvent::Step(Step::Checking)
        } else if let Some(sizes) = message.strip_prefix("Downloading update (") {
            LogEvent::Step(download_step(sizes))
        } else if message.starts_with("Extracting package") {
            LogEvent::Step(Step::Unpacking)
        } else if message.starts_with("Installing update") {
            LogEvent::Step(Step::Installing)
        } else if message.starts_with("Update complete") {
            LogEvent::Step(Step::Launching)
        } else if message.starts_with("Nothing to do") {
            LogEvent::UpToDate
        } else if let Some(error) = message.strip_prefix("Error: ") {
            // HTTP error 0: no server answered at all.
            LogEvent::Failed(if error.ends_with("http error 0") {
                Failure::Offline
            } else {
                Failure::Download
            })
        } else {
            LogEvent::Other
        },
    )
}

/// The step of `47,930 of 414,760 KB)...`, what follows `Downloading update (`.
fn download_step(sizes: &str) -> Step {
    let kilobytes = |text: &str| -> u64 {
        let digits: String = text.chars().filter(char::is_ascii_digit).collect();
        digits.parse().unwrap_or(0)
    };
    let sizes = sizes.split_once(" KB").map_or(sizes, |(sizes, _)| sizes);
    match sizes.split_once(" of ") {
        Some((done, total)) => Step::Downloading {
            done: kilobytes(done),
            total: kilobytes(total),
        },
        None => Step::Downloading { done: 0, total: 0 },
    }
}

/// Where the Setting Up Steam screen is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Steam sets itself up, at this step.
    Working(Step),
    /// Steam is ready: its sign-in window shows, so the screen goes.
    Ready,
    /// Steam's set-up failed; the screen offers Try Again.
    Failed(Failure),
    /// Nothing moved for too long; the screen offers Try Again.
    TimedOut,
    /// The user gave up on the set-up and is back where they were.
    Cancelled,
}

/// Steam's first start, as the Setting Up Steam screen follows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SteamSetup {
    state: State,
    /// When Steam was last seen at work: started, or a line in its log.
    active: Instant,
    /// Whether Steam's log has said anything yet.
    logged: bool,
    /// Whether the launcher sees Steam's windows, so its sign-in window ends the screen.
    follows_windows: bool,
}

impl SteamSetup {
    /// The set-up of a Steam that was started at `now`. Where the launcher `follows_windows`
    /// (a `--steam` gamescope), the screen stays until Steam's sign-in window shows (see
    /// [`Self::sign_in_shown`]); elsewhere it goes once Steam's log says that its client
    /// starts, and Steam's windows are the desktop's to show.
    pub fn new(now: Instant, follows_windows: bool) -> Self {
        Self {
            state: State::Working(Step::Starting),
            active: now,
            logged: false,
            follows_windows,
        }
    }

    /// Where the set-up is.
    pub fn state(&self) -> State {
        self.state
    }

    /// When the set-up times out unless Steam is seen at work first, so the launcher knows
    /// when to look again; `None` once it no longer waits.
    pub fn deadline(&self) -> Option<Instant> {
        let wait = if self.logged {
            SETUP_STALL_TIMEOUT
        } else {
            SETUP_START_TIMEOUT
        };
        matches!(self.state, State::Working(_)).then_some(self.active + wait)
    }

    /// Looks at the set-up at `now`: it has timed out once nothing moved until its
    /// [`deadline`](Self::deadline).
    pub fn check(&mut self, now: Instant) {
        if self.deadline().is_some_and(|deadline| now >= deadline) {
            self.state = State::TimedOut;
        }
    }

    /// Follows a line Steam wrote to its bootstrap log at `now`.
    pub fn log_line(&mut self, line: &str, now: Instant) {
        let Some(event) = log_event(line) else {
            return;
        };
        let step = match (self.state, event) {
            (State::Working(step), _) => step,
            // Steam that moves on after a failure, or after the wait ran out, is back at work.
            (State::Failed(_) | State::TimedOut, LogEvent::Step(_)) => Step::Starting,
            _ => return,
        };
        self.active = now;
        self.logged = true;
        self.state = match event {
            // Where the launcher cannot see Steam's windows, its client starting ends the
            // set-up: its sign-in window shows next, as the desktop places it.
            LogEvent::Step(Step::Launching) | LogEvent::UpToDate if !self.follows_windows => {
                State::Ready
            }
            // Once its client starts, Steam checks its files again: still starting.
            LogEvent::Step(_) if step == Step::Launching => State::Working(Step::Launching),
            LogEvent::Step(next) => State::Working(next),
            // The background checks of a client that runs are not the set-up.
            LogEvent::Failed(failure) if step != Step::Launching => State::Failed(failure),
            LogEvent::UpToDate | LogEvent::Failed(_) | LogEvent::Other => State::Working(step),
        };
    }

    /// How far the download is, from 0 to 1, while Steam downloads an update of a known
    /// size.
    pub fn progress(&self) -> Option<f64> {
        match self.state {
            State::Working(Step::Downloading { done, total }) if total > 0 => {
                Some((done.min(total) as f64) / (total as f64))
            }
            _ => None,
        }
    }

    /// Starts the wait over at `now`, after Try Again started Steam again.
    pub fn try_again(&mut self, now: Instant) {
        *self = Self::new(now, self.follows_windows);
    }

    /// Steam's sign-in window shows: the set-up is over and the screen goes, even after it
    /// said that the set-up failed or stopped.
    pub fn sign_in_shown(&mut self) {
        if self.state != State::Cancelled {
            self.state = State::Ready;
        }
    }

    /// The user gave up on the set-up with Cancel. Steam goes on setting itself up in the
    /// background; nothing the launcher sees of it brings the screen back.
    pub fn cancel(&mut self) {
        if self.state != State::Ready {
            self.state = State::Cancelled;
        }
    }
}

/// Where `client` writes its bootstrap log, most likely first: the host's Steam in
/// `~/.local/share/Steam` (`~/.steam/steam` links there), the Flatpak in its own data.
pub fn bootstrap_logs(dirs: &HostDirs, client: StoreClient) -> Vec<PathBuf> {
    let folders = match client {
        StoreClient::Host => vec![dirs.data_home.join("Steam"), dirs.home.join(".steam/steam")],
        StoreClient::Flatpak => {
            let data = dirs.home.join(".var/app").join(StoreApp::Steam.app_id());
            vec![data.join(".local/share/Steam"), data.join("data/Steam")]
        }
    };
    folders
        .into_iter()
        .map(|folder| folder.join("logs/bootstrap_log.txt"))
        .collect()
}

/// Steam's bootstrap log as the screen follows it: the lines Steam writes from the moment
/// it was started on, in the first of its places that has one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootstrapLog {
    /// Each place the log can be, with how much of it was read.
    files: Vec<(PathBuf, u64)>,
}

impl BootstrapLog {
    /// Follows the log in the first of `paths` that has one, from where each ends now: the
    /// lines of Steam's earlier starts are not this set-up's.
    pub fn open(paths: Vec<PathBuf>) -> Self {
        let files = paths
            .into_iter()
            .map(|path| {
                let end = std::fs::metadata(&path).map_or(0, |metadata| metadata.len());
                (path, end)
            })
            .collect();
        Self { files }
    }

    /// The whole lines Steam wrote since the last call; a line it is still writing waits
    /// for the next.
    pub fn new_lines(&mut self) -> Vec<String> {
        let Some((path, read)) = self.files.iter_mut().find(|(path, _)| path.is_file()) else {
            return Vec::new();
        };
        let Ok(mut file) = File::open(&*path) else {
            return Vec::new();
        };
        // A log that shrank was started over.
        if file.metadata().is_ok_and(|metadata| metadata.len() < *read) {
            *read = 0;
        }
        let mut text = Vec::new();
        if file.seek(SeekFrom::Start(*read)).is_err() || file.read_to_end(&mut text).is_err() {
            return Vec::new();
        }
        let Some(end) = text.iter().rposition(|&byte| byte == b'\n') else {
            return Vec::new();
        };
        *read += end as u64 + 1;
        String::from_utf8_lossy(&text[..end])
            .split('\n')
            .map(|line| line.trim_end_matches('\r').to_owned())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs(home: &str) -> HostDirs {
        HostDirs::from_vars(std::path::Path::new(home), false, |_| None)
    }

    #[test]
    fn steams_bootstrap_log_is_in_its_folder() {
        assert_eq!(
            bootstrap_logs(&dirs("/home/me"), StoreClient::Host),
            [
                PathBuf::from("/home/me/.local/share/Steam/logs/bootstrap_log.txt"),
                PathBuf::from("/home/me/.steam/steam/logs/bootstrap_log.txt"),
            ]
        );
        assert_eq!(
            bootstrap_logs(&dirs("/home/me"), StoreClient::Flatpak),
            [
                PathBuf::from(
                    "/home/me/.var/app/com.valvesoftware.Steam/.local/share/Steam/logs/bootstrap_log.txt"
                ),
                PathBuf::from(
                    "/home/me/.var/app/com.valvesoftware.Steam/data/Steam/logs/bootstrap_log.txt"
                ),
            ]
        );
    }

    #[test]
    fn the_log_follows_what_steam_writes_from_its_start_on() {
        use std::io::Write;

        let home = tempfile::tempdir().unwrap();
        let old = home.path().join("old/bootstrap_log.txt");
        let new = home.path().join("new/bootstrap_log.txt");
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "[2026-04-17 15:24:48] Nothing to do\n").unwrap();
        let mut log = BootstrapLog::open(vec![new.clone(), old.clone()]);
        assert!(log.new_lines().is_empty());
        let mut file = std::fs::OpenOptions::new().append(true).open(&old).unwrap();
        file.write_all(
            b"[2026-04-17 15:24:49] Verifying installation...\n[2026-04-17 15:24:49] Verif",
        )
        .unwrap();
        assert_eq!(
            log.new_lines(),
            ["[2026-04-17 15:24:49] Verifying installation..."]
        );
        file.write_all(b"ication complete\n").unwrap();
        assert_eq!(
            log.new_lines(),
            ["[2026-04-17 15:24:49] Verification complete"]
        );
        // A log that appears where none was is read from its start, and comes first.
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::write(
            &new,
            "\n[2026-04-17 15:25:06] Update complete, launching...\n",
        )
        .unwrap();
        assert_eq!(
            log.new_lines(),
            ["", "[2026-04-17 15:25:06] Update complete, launching..."]
        );
    }

    #[test]
    fn the_screen_waits_for_steams_log_then_times_out() {
        let start = Instant::now();
        let mut setup = SteamSetup::new(start, true);
        assert_eq!(setup.state(), State::Working(Step::Starting));
        setup.check(start + SETUP_START_TIMEOUT - Duration::from_secs(1));
        assert_eq!(setup.state(), State::Working(Step::Starting));
        setup.check(start + SETUP_START_TIMEOUT);
        assert_eq!(setup.state(), State::TimedOut);
    }

    /// A line of Steam's log downloading `done` of 414,760 KB.
    fn downloading(done: &str) -> String {
        format!("[2026-04-17 15:24:48] Downloading update ({done} of 414,760 KB)...")
    }

    #[test]
    fn the_screen_shows_the_download_and_waits_while_it_moves() {
        let start = Instant::now();
        let at = |minutes| start + Duration::from_secs(minutes * 60);
        let mut setup = SteamSetup::new(start, true);
        setup.log_line("[2026-04-17 15:24:48] Verifying installation...", at(1));
        assert_eq!(setup.state(), State::Working(Step::Checking));
        assert_eq!(setup.progress(), None);
        // A slow download, a line a minute, for well over ten minutes.
        for minute in 2_u64..=15 {
            setup.log_line(&downloading(&format!("{},000", minute * 20)), at(minute));
            setup.check(at(minute));
        }
        assert_eq!(
            setup.state(),
            State::Working(Step::Downloading {
                done: 300_000,
                total: 414_760
            })
        );
        let progress = setup.progress().unwrap();
        assert!(
            (progress - 300_000.0 / 414_760.0).abs() < 1e-9,
            "{progress}"
        );
        // Then nothing moves for five minutes.
        setup.check(at(15) + SETUP_STALL_TIMEOUT - Duration::from_secs(1));
        assert!(matches!(setup.state(), State::Working(_)));
        setup.check(at(15) + SETUP_STALL_TIMEOUT);
        assert_eq!(setup.state(), State::TimedOut);
    }

    #[test]
    fn a_failed_set_up_says_why_until_the_user_tries_again() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut setup = SteamSetup::new(start, true);
        setup.log_line("[2026-04-22 08:52:42] Verifying installation...", at(1));
        setup.log_line(
            "[2026-04-22 08:52:42] Error: Download failed: http error 0",
            at(2),
        );
        setup.log_line(
            "[2026-04-22 08:52:42] Saving metrics to disk (/home/me/.local/share/Steam/package/steam_client_metrics.bin)",
            at(2),
        );
        assert_eq!(setup.state(), State::Failed(Failure::Offline));
        // A failure does not time out: the screen waits for the user.
        assert_eq!(setup.deadline(), None);
        setup.try_again(at(600));
        assert_eq!(setup.state(), State::Working(Step::Starting));
        assert_eq!(setup.deadline(), Some(at(600) + SETUP_START_TIMEOUT));
    }

    #[test]
    fn steam_that_moves_on_after_a_failure_is_back_at_work() {
        let start = Instant::now();
        let mut setup = SteamSetup::new(start, true);
        setup.log_line(
            "[2026-04-22 08:52:42] Error: Download failed: http error 503",
            start,
        );
        assert_eq!(setup.state(), State::Failed(Failure::Download));
        setup.log_line(&downloading("1,000"), start);
        assert_eq!(
            setup.state(),
            State::Working(Step::Downloading {
                done: 1_000,
                total: 414_760
            })
        );
    }

    #[test]
    fn in_a_steam_gamescope_the_screen_stays_until_the_sign_in_shows() {
        let start = Instant::now();
        let at = |seconds| start + Duration::from_secs(seconds);
        let mut setup = SteamSetup::new(start, true);
        setup.log_line(
            "[2026-04-17 15:25:06] Update complete, launching...",
            at(60),
        );
        // The client starts, checks its files again, and is quiet until its window shows.
        setup.log_line("[2026-04-17 15:25:08] Verifying installation...", at(62));
        assert_eq!(setup.state(), State::Working(Step::Launching));
        setup.sign_in_shown();
        assert_eq!(setup.state(), State::Ready);
        assert_eq!(setup.deadline(), None);
    }

    #[test]
    fn elsewhere_the_screen_goes_once_steams_client_starts() {
        let start = Instant::now();
        let mut setup = SteamSetup::new(start, false);
        setup.log_line(&downloading("1,000"), start);
        setup.log_line("[2026-04-17 15:25:06] Update complete, launching...", start);
        assert_eq!(setup.state(), State::Ready);
        // A Steam that has run before is up to date.
        let mut setup = SteamSetup::new(start, false);
        setup.log_line(
            "[2026-04-18 12:06:02] Checking for update on startup",
            start,
        );
        setup.log_line("[2026-04-18 12:06:02] Nothing to do", start);
        assert_eq!(setup.state(), State::Ready);
        // In a `--steam` gamescope its window decides.
        let mut setup = SteamSetup::new(start, true);
        setup.log_line("[2026-04-18 12:06:02] Nothing to do", start);
        assert!(matches!(setup.state(), State::Working(_)));
    }

    #[test]
    fn cancel_ends_the_wait_and_nothing_brings_it_back() {
        let start = Instant::now();
        let mut setup = SteamSetup::new(start, true);
        setup.log_line(&downloading("1,000"), start);
        setup.cancel();
        assert_eq!(setup.state(), State::Cancelled);
        assert_eq!(setup.deadline(), None);
        setup.log_line(&downloading("2,000"), start);
        setup.sign_in_shown();
        assert_eq!(setup.state(), State::Cancelled);
    }

    #[test]
    fn a_log_steam_started_over_is_read_from_its_start() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("bootstrap_log.txt");
        std::fs::write(&path, "[2026-04-17 15:24:48] Nothing to do\n".repeat(4)).unwrap();
        let mut log = BootstrapLog::open(vec![path.clone()]);
        std::fs::write(&path, "[2026-04-17 15:24:49] Extracting package...\n").unwrap();
        assert_eq!(
            log.new_lines(),
            ["[2026-04-17 15:24:49] Extracting package..."]
        );
    }

    #[test]
    fn a_first_start_moves_from_checking_to_launching() {
        // From a real first start's bootstrap_log.txt, with the package lines left out.
        let log = [
            "",
            "[2026-04-17 15:24:48] Startup - updater built Sep 12 2025 03:55:50",
            "[2026-04-17 15:24:48] Verifying installation...",
            "[2026-04-17 15:24:48] Unable to read and verify install manifest /home/me/.local/share/Steam/package/steam_client_ubuntu12.installed",
            "[2026-04-17 15:24:48] Verification complete",
            "[2026-04-17 15:24:48] Downloading Update...",
            "[2026-04-17 15:24:48] Checking for available update...",
            "[2026-04-17 15:24:48] Downloading update (47,930 of 414,760 KB)...",
            "[2026-04-17 15:24:53] Downloading update (414,760 of 414,760 KB)...",
            "[2026-04-17 15:24:53] Download Complete.",
            "[2026-04-17 15:24:53] Extracting package...",
            "[2026-04-17 15:25:00] Installing update...",
            "[2026-04-17 15:25:06] Cleaning up...",
            "[2026-04-17 15:25:06] Update complete, launching...",
        ];

        let events: Vec<_> = log.iter().filter_map(|line| log_event(line)).collect();

        assert_eq!(
            events,
            [
                LogEvent::Other,
                LogEvent::Step(Step::Checking),
                LogEvent::Other,
                LogEvent::Other,
                LogEvent::Other,
                LogEvent::Step(Step::Checking),
                LogEvent::Step(Step::Downloading {
                    done: 47_930,
                    total: 414_760
                }),
                LogEvent::Step(Step::Downloading {
                    done: 414_760,
                    total: 414_760
                }),
                LogEvent::Other,
                LogEvent::Step(Step::Unpacking),
                LogEvent::Step(Step::Installing),
                LogEvent::Other,
                LogEvent::Step(Step::Launching),
            ]
        );
    }

    #[test]
    fn a_start_without_internet_fails_offline() {
        // From a real start with the network down.
        let log = [
            "[2026-04-22 08:52:42] Download failed: http error 0 (client-update.steamstatic.com/steam_client_ubuntu12)",
            "[2026-04-22 08:52:42] DownloadManifest - exhausted list of download hosts",
            "[2026-04-22 08:52:42] Failed to load manifest",
            "[2026-04-22 08:52:42] Error: Download failed: http error 0",
        ];

        let events: Vec<_> = log.iter().filter_map(|line| log_event(line)).collect();

        assert_eq!(
            events,
            [
                LogEvent::Other,
                LogEvent::Other,
                LogEvent::Other,
                LogEvent::Failed(Failure::Offline),
            ]
        );
        assert_eq!(
            log_event("[2026-04-22 08:52:42] Error: Download failed: http error 503"),
            Some(LogEvent::Failed(Failure::Download))
        );
    }

    #[test]
    fn a_steam_that_has_run_before_is_up_to_date() {
        assert_eq!(
            log_event("[2026-04-18 12:06:02] Nothing to do"),
            Some(LogEvent::UpToDate)
        );
    }

    #[test]
    fn lines_without_a_time_or_sizes_still_count() {
        assert_eq!(
            log_event("Extracting package..."),
            Some(LogEvent::Step(Step::Unpacking))
        );
        let no_sizes = "[2026-04-17 15:24:48] Downloading update (starting)...";
        assert_eq!(
            log_event(no_sizes),
            Some(LogEvent::Step(Step::Downloading { done: 0, total: 0 }))
        );
        let mut setup = SteamSetup::new(Instant::now(), true);
        setup.log_line(no_sizes, Instant::now());
        assert_eq!(setup.progress(), None);
    }

    #[test]
    fn a_failed_check_of_the_started_client_is_not_the_set_ups() {
        let start = Instant::now();
        let mut setup = SteamSetup::new(start, true);
        setup.log_line("[2026-04-17 15:25:06] Update complete, launching...", start);
        setup.log_line(
            "[2026-04-17 15:27:09] Error: Download failed: http error 0",
            start,
        );
        assert_eq!(setup.state(), State::Working(Step::Launching));
    }

    #[test]
    fn cancel_after_the_sign_in_showed_changes_nothing() {
        let mut setup = SteamSetup::new(Instant::now(), true);
        setup.sign_in_shown();
        setup.cancel();
        assert_eq!(setup.state(), State::Ready);
    }

    #[test]
    fn a_line_steam_is_still_writing_waits() {
        let home = tempfile::tempdir().unwrap();
        let path = home.path().join("bootstrap_log.txt");
        let mut log = BootstrapLog::open(vec![path.clone()]);
        std::fs::write(&path, "[2026-04-17 15:24:49] Extracting pack").unwrap();
        assert!(log.new_lines().is_empty());
        std::fs::write(&path, "[2026-04-17 15:24:49] Extracting package...\r\n").unwrap();
        assert_eq!(
            log.new_lines(),
            ["[2026-04-17 15:24:49] Extracting package..."]
        );
    }
}
