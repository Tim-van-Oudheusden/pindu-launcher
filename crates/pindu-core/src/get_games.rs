//! Get Games' installs and removals (#886, `docs/get-games.md` section 7): Add a Store's
//! route generalised to any app the catalogue offers, plus uninstall and the catalogue's
//! refresh. Jobs run one at a time through the host's `flatpak`, the rest queued; an
//! install first asks Flathub for its sizes and checks the free space.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::Path;
use std::rc::{Rc, Weak};
use std::time::{Duration, SystemTime};

use crate::add_store::{
    Done, FLATHUB, FLATHUB_REPO, Installation, Line, ProgressReader, StoreApp, error_message,
    network_failure,
};
use crate::catalogue::Offer;
use crate::model::{GameId, Source};

/// The commands that install the app `id` into `installation`, in order, each an argv
/// list: the user installation gets Flathub first, unless it has it already. They pass
/// `-y` but not `--noninteractive`, which would print no progress (#884).
pub fn install_commands(installation: Installation, id: &str) -> Vec<Vec<String>> {
    let install = owned(&["flatpak", "install", flag(installation), "-y", FLATHUB, id]);
    match installation {
        Installation::System => vec![install],
        Installation::User => vec![
            owned(&[
                "flatpak",
                "remote-add",
                "--user",
                "--if-not-exists",
                FLATHUB,
                FLATHUB_REPO,
            ]),
            install,
        ],
    }
}

/// The command that removes the app `id` from `installation`, with its data in the
/// user's home when `delete_data`. Runtimes it leaves unused are not the launcher's to
/// remove, though `flatpak` takes those it pulled in itself.
pub fn uninstall_command(installation: Installation, id: &str, delete_data: bool) -> Vec<String> {
    let mut argv = owned(&[
        "flatpak",
        "uninstall",
        flag(installation),
        "--noninteractive",
        "-y",
    ]);
    if delete_data {
        argv.push("--delete-data".to_owned());
    }
    argv.push(id.to_owned());
    argv
}

/// The command that asks Flathub about the app `id`, for its [`sizes`].
pub fn remote_info_command(installation: Installation, id: &str) -> Vec<String> {
    owned(&["flatpak", "remote-info", flag(installation), FLATHUB, id])
}

/// The command that downloads Flathub's catalogue into `installation` again. It prints
/// nothing and succeeds even offline, so only the catalogue's age tells.
pub fn refresh_command(installation: Installation) -> Vec<String> {
    owned(&[
        "flatpak",
        "update",
        "--appstream",
        flag(installation),
        "--noninteractive",
    ])
}

/// `flatpak`'s option for `installation`.
pub(crate) fn flag(installation: Installation) -> &'static str {
    match installation {
        Installation::System => "--system",
        Installation::User => "--user",
    }
}

fn owned(words: &[&str]) -> Vec<String> {
    words.iter().map(|word| (*word).to_owned()).collect()
}

/// How old a catalogue may grow before Get Games refreshes it.
pub const CATALOGUE_AGE: Duration = Duration::from_secs(86_400);

/// Whether a catalogue last changed at `modified` (`None`: there is none) wants
/// refreshing at `now`: it is missing or older than [`CATALOGUE_AGE`].
pub fn needs_refresh(modified: Option<SystemTime>, now: SystemTime) -> bool {
    modified.is_none_or(|modified| {
        now.duration_since(modified)
            .is_ok_and(|age| age > CATALOGUE_AGE)
    })
}

/// What the catalogue screen can show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogueStatus {
    /// There is a catalogue to browse.
    Ready,
    /// There is none yet, and one is being downloaded.
    Waiting,
    /// There is none, and the machine is offline.
    Offline,
    /// There is none, and downloading one did not bring it.
    Unavailable,
}

/// What the catalogue screen shows: `has_catalogue` tells whether there is one, `refreshing`
/// whether a refresh is queued or runs, `online` whether the machine has a network.
pub fn catalogue_status(has_catalogue: bool, refreshing: bool, online: bool) -> CatalogueStatus {
    if has_catalogue {
        CatalogueStatus::Ready
    } else if refreshing {
        CatalogueStatus::Waiting
    } else if online {
        CatalogueStatus::Unavailable
    } else {
        CatalogueStatus::Offline
    }
}

/// An app's download and installed sizes, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Sizes {
    /// What installing it downloads, without a runtime it may need.
    pub download: u64,
    /// What it takes once installed.
    pub installed: u64,
}

/// The sizes [`remote_info_command`] printed in the C locale; `None` when it did not.
pub fn sizes(remote_info: &str) -> Option<Sizes> {
    let size = |label: &str| {
        remote_info.lines().find_map(|line| {
            let (name, value) = line.split_once(':')?;
            (name.trim() == label).then(|| bytes(value.trim()))?
        })
    };
    Some(Sizes {
        download: size("Download Size")?,
        installed: size("Installed Size")?,
    })
}

/// The bytes in a size as GLib writes it in the C locale (`g_format_size`): `900 bytes`,
/// `408.7 kB`, `1.2 GB`, in powers of 1000, a no-break space before the unit.
fn bytes(size: &str) -> Option<u64> {
    let (number, unit) = size.split_once(char::is_whitespace)?;
    let scale: u64 = match unit {
        "byte" | "bytes" => 1,
        "kB" => 1_000,
        "MB" => 1_000_000,
        "GB" => 1_000_000_000,
        "TB" => 1_000_000_000_000,
        _ => return None,
    };
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let digits = |text: &str| !text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit());
    if !digits(whole) || !(fraction.is_empty() || digits(fraction)) {
        return None;
    }
    let tenths = 10_u64.checked_pow(u32::try_from(fraction.len()).ok()?)?;
    let fraction = if fraction.is_empty() {
        0
    } else {
        fraction.parse::<u64>().ok()?
    };
    whole
        .parse::<u64>()
        .ok()?
        .checked_mul(scale)?
        .checked_add(fraction.checked_mul(scale)? / tenths)
}

/// The room Flatpak keeps free in an installation (`min-free-space-size` in its
/// repository's configuration): an install that would leave less fails.
pub const RESERVE: u64 = 500_000_000;

/// The free space an app of `sizes` needs to install: its installed size and Flatpak's
/// [`RESERVE`].
pub fn space_needed(sizes: Sizes) -> u64 {
    sizes.installed.saturating_add(RESERVE)
}

/// Whether `id` is a Flatpak app ID: three or more parts split by dots, each of letters,
/// digits, `_` and `-`, none empty or starting with `-`, the first not starting with a
/// digit, 255 characters at most. Nothing else reaches a `flatpak` command line.
pub fn is_app_id(id: &str) -> bool {
    let parts: Vec<&str> = id.split('.').collect();
    id.len() <= 255
        && parts.len() >= 3
        && parts.iter().enumerate().all(|(index, part)| {
            let first = part.chars().next();
            first.is_some_and(|first| first != '-' && !(index == 0 && first.is_ascii_digit()))
                && part
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        })
}

/// The apps installed in the Flatpak installation in `dir` (`/var/lib/flatpak` or the
/// user's): those with a deployed `current` branch under `app/`.
pub fn installed_apps(dir: &Path) -> HashSet<String> {
    let Ok(entries) = std::fs::read_dir(dir.join("app")) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join("current").exists())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .filter(|id| is_app_id(id))
        .collect()
}

/// The app of `installed` whose desktop entry the library game `game` is, which Get Games
/// can remove from its page in the library; `None` for a game of another source. Flatpak
/// exports only entries named for the app's ID, alone or followed by a dot and more, so
/// the longest ID the entry's starts with that way is its app.
pub fn removable_app<'a>(game: &GameId, installed: &'a HashSet<String>) -> Option<&'a str> {
    if game.source() != Source::Desktop {
        return None;
    }
    let entry = game.native_id();
    installed
        .iter()
        .filter(|app| {
            entry
                .strip_prefix(app.as_str())
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('.'))
        })
        .max_by_key(|app| app.len())
        .map(String::as_str)
}

/// A piece of work Get Games does with `flatpak`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Job {
    /// Installs an app from Flathub.
    Install {
        /// Its app ID.
        id: String,
        /// Its name, for the screens.
        name: String,
    },
    /// Removes an app.
    Remove {
        /// Its app ID.
        id: String,
        /// Its name, for the screens.
        name: String,
        /// Whether its data in the user's home goes too.
        delete_data: bool,
    },
    /// Downloads the catalogue again.
    Refresh,
}

impl Job {
    /// The app the job is for; `None` for a refresh.
    pub fn app_id(&self) -> Option<&str> {
        match self {
            Self::Install { id, .. } | Self::Remove { id, .. } => Some(id),
            Self::Refresh => None,
        }
    }
}

/// The job that runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Running {
    /// The job.
    pub job: Job,
    /// How far it is, from 0 to 1; `None` while an install checks its sizes and the free
    /// space, or while `flatpak` says nothing.
    pub progress: Option<f64>,
    /// The operation `flatpak` is on and how many it has, when an install has several:
    /// the app, its runtime, their extensions.
    pub step: Option<(u32, u32)>,
    /// Whether the user cancelled it, and it is stopping.
    pub cancelling: bool,
    /// Whether an install still asks for its sizes and checks the free space, before
    /// `flatpak` runs.
    pub checking: bool,
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It did its work.
    Done,
    /// The user cancelled it.
    Cancelled,
    /// It failed, in `flatpak`'s words.
    Failed {
        /// What went wrong: `flatpak`'s last error line.
        error: String,
        /// Flathub could not be reached, or the machine was offline.
        network: bool,
    },
    /// The app would not fit: it needs `needed` bytes free, and `free` are.
    NoSpace {
        /// The free space it needs ([`space_needed`]).
        needed: u64,
        /// The free space there is.
        free: u64,
    },
}

/// Why a job was not queued.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// The catalogue does not offer the app.
    NotOffered,
    /// The app is a store, which installs through Add a Store's flow.
    Store(StoreApp),
    /// The ID is not an app ID.
    InvalidId,
    /// A job for the app is already queued or runs.
    Busy,
}

/// What Get Games' jobs are doing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    /// The job that runs.
    pub running: Option<Running>,
    /// The jobs waiting their turn, first first.
    pub queue: Vec<Job>,
    /// How each app's last job ended, until another for it starts.
    pub outcomes: HashMap<String, Outcome>,
    /// How the last refresh ended.
    pub refreshed: Option<Outcome>,
    /// The sizes Flathub gave for each app asked about.
    pub sizes: HashMap<String, Sizes>,
    /// The machine has no network.
    pub offline: bool,
}

impl State {
    /// Whether a job for the app `id` is queued or runs.
    pub fn busy(&self, id: &str) -> bool {
        self.jobs().any(|job| job.app_id() == Some(id))
    }

    /// Whether a refresh is queued or runs.
    pub fn refreshing(&self) -> bool {
        self.jobs().any(|job| *job == Job::Refresh)
    }

    /// The job that runs and those queued.
    fn jobs(&self) -> impl Iterator<Item = &Job> {
        self.running
            .iter()
            .map(|running| &running.job)
            .chain(&self.queue)
    }
}

/// What a backend reports once a query ends: its standard output, or its standard error.
pub type Answer = Box<dyn FnOnce(Result<String, String>)>;

/// What a backend reports once it measured the free space, in bytes; `None` when it
/// could not.
pub type Free = Box<dyn FnOnce(Option<u64>)>;

/// Runs Get Games' commands off the main thread; every callback runs on the main thread.
pub trait GetGamesBackend {
    /// Runs `argv`, giving `line` each line it prints on standard output, and reports to
    /// `done` once it ends: `Err` with what it printed when it fails.
    fn run(&self, argv: Vec<String>, line: Line, done: Done);

    /// Stops the command [`run`](Self::run) started, which then reports that it failed.
    fn stop(&self);

    /// Runs `argv` in the C locale, so its output reads the same in every language, and
    /// reports what it printed.
    fn query(&self, argv: Vec<String>, answer: Answer);

    /// Measures the free space where the installation keeps its apps.
    fn free_space(&self, free: Free);
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&State)>;
/// Something a service does when a job ends.
type FinishedListener = Box<dyn Fn(&Job, &Outcome)>;

/// Get Games' jobs over their backend.
#[derive(Clone)]
pub struct GetGames {
    inner: Rc<Inner>,
}

struct Inner {
    installation: Installation,
    backend: Box<dyn GetGamesBackend>,
    state: RefCell<State>,
    listeners: RefCell<Vec<Listener>>,
    finished_listeners: RefCell<Vec<FinishedListener>>,
}

impl fmt::Debug for GetGames {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GetGames")
            .field("installation", &self.inner.installation)
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl GetGames {
    /// Jobs on `installation` over `backend`, none yet.
    pub fn new(installation: Installation, backend: Box<dyn GetGamesBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                installation,
                backend,
                state: RefCell::default(),
                listeners: RefCell::default(),
                finished_listeners: RefCell::default(),
            }),
        }
    }

    /// The installation the jobs work on.
    pub fn installation(&self) -> Installation {
        self.inner.installation
    }

    /// The state as it is.
    pub fn state(&self) -> State {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&State) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever a job ends, with how.
    pub fn connect_finished(&self, listener: impl Fn(&Job, &Outcome) + 'static) {
        self.inner
            .finished_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Says whether the machine has a network, which decides whether a failure was the
    /// network's.
    pub fn set_online(&self, online: bool) {
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            let changed = state.offline == online;
            state.offline = !online;
            changed
        };
        if changed {
            self.changed();
        }
    }

    /// Queues installing the app `id` that `offer` offers. A store goes through Add a
    /// Store's flow instead.
    pub fn install(&self, offer: &Offer, id: &str) -> Result<(), Refusal> {
        if let Some(store) = StoreApp::ALL.into_iter().find(|store| store.app_id() == id) {
            return Err(Refusal::Store(store));
        }
        let app = offer.get(id).ok_or(Refusal::NotOffered)?;
        if !is_app_id(&app.id) {
            return Err(Refusal::InvalidId);
        }
        self.queue(Job::Install {
            id: app.id.clone(),
            name: app.name.clone(),
        })
    }

    /// Queues removing the app `id`, called `name`, and its data when `delete_data`.
    pub fn remove(&self, id: &str, name: &str, delete_data: bool) -> Result<(), Refusal> {
        if !is_app_id(id) {
            return Err(Refusal::InvalidId);
        }
        self.queue(Job::Remove {
            id: id.to_owned(),
            name: name.to_owned(),
            delete_data,
        })
    }

    /// Queues downloading the catalogue again, unless that is queued or runs already.
    pub fn refresh(&self) {
        if !self.inner.state.borrow().refreshing() {
            self.inner.state.borrow_mut().queue.push(Job::Refresh);
            self.changed();
            self.start_next();
        }
    }

    /// Cancels the job for the app `id`: a queued one leaves the queue, an install that
    /// runs stops. A removal that runs finishes.
    pub fn cancel(&self, id: &str) {
        let stop = {
            let mut state = self.inner.state.borrow_mut();
            let queued = state.queue.len();
            state.queue.retain(|job| job.app_id() != Some(id));
            let dropped = state.queue.len() != queued;
            match &mut state.running {
                Some(
                    running @ Running {
                        job: Job::Install { .. },
                        cancelling: false,
                        ..
                    },
                ) if running.job.app_id() == Some(id) => {
                    running.cancelling = true;
                    Some(true)
                }
                _ => dropped.then_some(false),
            }
        };
        if let Some(stop) = stop {
            self.changed();
            if stop {
                self.inner.backend.stop();
            }
        }
    }

    /// Asks Flathub for the sizes of the app `id`, for its page; they land in the state.
    pub fn fetch_sizes(&self, id: &str) {
        if !is_app_id(id) {
            return;
        }
        let weak = self.downgrade();
        let id = id.to_owned();
        self.inner.backend.query(
            remote_info_command(self.inner.installation, &id),
            Box::new(move |answer| {
                if let (Some(jobs), Ok(output)) = (weak.upgrade(), answer) {
                    jobs.store_sizes(&id, &output);
                }
            }),
        );
    }

    /// A handle that does not keep the jobs alive, for their own listeners to hold.
    pub fn downgrade(&self) -> WeakGetGames {
        WeakGetGames {
            inner: Rc::downgrade(&self.inner),
        }
    }

    /// Queues `job`, and starts it when nothing runs.
    fn queue(&self, job: Job) -> Result<(), Refusal> {
        {
            let mut state = self.inner.state.borrow_mut();
            let id = job.app_id().unwrap_or_default();
            if state.busy(id) {
                return Err(Refusal::Busy);
            }
            state.outcomes.remove(id);
            state.queue.push(job);
        }
        self.changed();
        self.start_next();
        Ok(())
    }

    /// Starts the first queued job, unless one runs.
    fn start_next(&self) {
        let job = {
            let mut state = self.inner.state.borrow_mut();
            if state.running.is_some() || state.queue.is_empty() {
                return;
            }
            let job = state.queue.remove(0);
            state.running = Some(Running {
                checking: matches!(job, Job::Install { .. }),
                job: job.clone(),
                progress: None,
                step: None,
                cancelling: false,
            });
            job
        };
        self.changed();
        let installation = self.inner.installation;
        match job {
            Job::Install { id, .. } => self.check(id),
            Job::Remove {
                id, delete_data, ..
            } => self
                .run_commands(vec![uninstall_command(installation, &id, delete_data)].into_iter()),
            Job::Refresh => self.run_commands(vec![refresh_command(installation)].into_iter()),
        }
    }

    /// Asks Flathub for the sizes of the app `id` and measures the free space, then
    /// installs it when it fits or either is unknown.
    fn check(&self, id: String) {
        let weak = self.downgrade();
        self.inner.backend.query(
            remote_info_command(self.inner.installation, &id),
            Box::new(move |answer| {
                let Some(jobs) = weak.upgrade() else {
                    return;
                };
                let output = match answer {
                    Ok(output) => output,
                    Err(text) => return jobs.failed(&text),
                };
                let sizes = jobs.store_sizes(&id, &output);
                if jobs.cancelling() {
                    return jobs.finish(Outcome::Cancelled);
                }
                let weak = jobs.downgrade();
                jobs.inner.backend.free_space(Box::new(move |free| {
                    let Some(jobs) = weak.upgrade() else {
                        return;
                    };
                    if jobs.cancelling() {
                        return jobs.finish(Outcome::Cancelled);
                    }
                    if let (Some(sizes), Some(free)) = (sizes, free) {
                        let needed = space_needed(sizes);
                        if needed > free {
                            return jobs.finish(Outcome::NoSpace { needed, free });
                        }
                    }
                    let commands = install_commands(jobs.inner.installation, &id);
                    jobs.run_commands(commands.into_iter());
                }));
            }),
        );
    }

    /// Keeps the sizes `output`, from [`remote_info_command`], gives for the app `id`.
    fn store_sizes(&self, id: &str, output: &str) -> Option<Sizes> {
        let sizes = sizes(output)?;
        self.inner
            .state
            .borrow_mut()
            .sizes
            .insert(id.to_owned(), sizes);
        self.changed();
        Some(sizes)
    }

    /// Runs the next of `commands`, and the rest after it; the job is done when none is
    /// left.
    fn run_commands(&self, mut commands: std::vec::IntoIter<Vec<String>>) {
        let Some(argv) = commands.next() else {
            self.finish(Outcome::Done);
            return;
        };
        if self.cancelling() {
            self.finish(Outcome::Cancelled);
            return;
        }
        let checked = {
            let mut state = self.inner.state.borrow_mut();
            state
                .running
                .as_mut()
                .is_some_and(|running| std::mem::replace(&mut running.checking, false))
        };
        if checked {
            self.changed();
        }
        let reader = RefCell::new(ProgressReader::new());
        let weak = self.downgrade();
        let line = move |line: &str| {
            let mut reader = reader.borrow_mut();
            if let (Some(jobs), Some(progress)) = (weak.upgrade(), reader.read(line)) {
                jobs.set_progress(progress, reader.step());
            }
        };
        let weak = self.downgrade();
        self.inner.backend.run(
            argv,
            Box::new(line),
            Box::new(move |result| {
                let Some(jobs) = weak.upgrade() else {
                    return;
                };
                match result {
                    Ok(()) => jobs.run_commands(commands),
                    Err(_) if jobs.cancelling() => jobs.finish(Outcome::Cancelled),
                    Err(text) => jobs.failed(&text),
                }
            }),
        );
    }

    /// The job that runs failed, `flatpak` having printed `text`.
    fn failed(&self, text: &str) {
        let network = network_failure(text) || self.inner.state.borrow().offline;
        self.finish(Outcome::Failed {
            error: error_message(text).unwrap_or_else(|| text.to_owned()),
            network,
        });
    }

    /// The job that runs is `progress` of the way, on `step`.
    fn set_progress(&self, progress: f64, step: Option<(u32, u32)>) {
        {
            let mut state = self.inner.state.borrow_mut();
            let Some(running) = &mut state.running else {
                return;
            };
            if running.progress == Some(progress) && running.step == step {
                return;
            }
            running.progress = Some(progress);
            running.step = step;
        }
        self.changed();
    }

    /// Whether the user cancelled the job that runs.
    fn cancelling(&self) -> bool {
        self.inner
            .state
            .borrow()
            .running
            .as_ref()
            .is_some_and(|running| running.cancelling)
    }

    /// The job that runs ended as `outcome`: it is kept, the services hear of it, and the
    /// next job starts.
    fn finish(&self, outcome: Outcome) {
        let job = {
            let mut state = self.inner.state.borrow_mut();
            let Some(running) = state.running.take() else {
                return;
            };
            match running.job.app_id() {
                Some(id) => {
                    state.outcomes.insert(id.to_owned(), outcome.clone());
                }
                None => state.refreshed = Some(outcome.clone()),
            }
            running.job
        };
        self.changed();
        for listener in self.inner.finished_listeners.borrow().iter() {
            listener(&job, &outcome);
        }
        self.start_next();
    }

    fn changed(&self) {
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
        }
    }
}

/// A handle on [`GetGames`] that does not keep it alive.
#[derive(Clone, Debug)]
pub struct WeakGetGames {
    inner: Weak<Inner>,
}

impl WeakGetGames {
    /// The jobs, unless they are gone.
    pub fn upgrade(&self) -> Option<GetGames> {
        self.inner.upgrade().map(|inner| GetGames { inner })
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashSet;
    use std::path::Path;

    use super::*;
    use crate::catalogue::{App, Lists, appstream, lists};

    fn words(argv: &[&str]) -> Vec<String> {
        argv.iter().map(|word| (*word).to_owned()).collect()
    }

    /// A capture of GG-1 (#884): what `flatpak` printed and how it ended.
    struct Capture {
        stdout: String,
        stderr: String,
        success: bool,
    }

    fn capture(name: &str) -> Capture {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/flatpak");
        let read = |extension: &str| {
            std::fs::read_to_string(dir.join(format!("{name}.{extension}"))).unwrap()
        };
        Capture {
            stdout: read("stdout"),
            stderr: read("stderr"),
            success: read("status").trim() == "0",
        }
    }

    /// A backend whose work ends when a test says.
    #[derive(Default)]
    struct Fake {
        runs: RefCell<Vec<(Vec<String>, Line, Done)>>,
        ran: RefCell<Vec<Vec<String>>>,
        queries: RefCell<Vec<(Vec<String>, Answer)>>,
        frees: RefCell<Vec<Free>>,
        stops: Cell<usize>,
    }

    impl GetGamesBackend for Rc<Fake> {
        fn run(&self, argv: Vec<String>, line: Line, done: Done) {
            self.ran.borrow_mut().push(argv.clone());
            self.runs.borrow_mut().push((argv, line, done));
        }

        fn stop(&self) {
            self.stops.set(self.stops.get() + 1);
        }

        fn query(&self, argv: Vec<String>, answer: Answer) {
            self.ran.borrow_mut().push(argv.clone());
            self.queries.borrow_mut().push((argv, answer));
        }

        fn free_space(&self, free: Free) {
            self.frees.borrow_mut().push(free);
        }
    }

    impl Fake {
        /// The command that runs prints `capture`'s output, then ends as it did, its
        /// standard error merged into its output as the backend does.
        fn replay(&self, capture: &Capture) {
            let (_, line, done) = self.runs.borrow_mut().remove(0);
            for text in capture.stdout.lines() {
                line(text);
            }
            done(if capture.success {
                Ok(())
            } else {
                Err(format!("{}{}", capture.stdout, capture.stderr))
            });
        }

        /// The query that waits answers as `capture` did.
        fn answer(&self, capture: &Capture) {
            let (_, answer) = self.queries.borrow_mut().remove(0);
            answer(if capture.success {
                Ok(capture.stdout.clone())
            } else {
                Err(capture.stderr.clone())
            });
        }

        /// The free space measured is `bytes`.
        fn free(&self, bytes: Option<u64>) {
            let free = self.frees.borrow_mut().remove(0);
            free(bytes);
        }

        fn last_command(&self) -> Vec<String> {
            self.ran.borrow().last().cloned().unwrap_or_default()
        }
    }

    const GAME: &str = "net.supertuxkart.SuperTuxKart";
    const GIB: u64 = 1_000_000_000;

    fn catalogue() -> Vec<App> {
        let xml = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/catalogue/appstream/flathub/x86_64/active/appstream.xml"),
        )
        .unwrap();
        appstream::parse(&xml, &[]).unwrap()
    }

    fn offer() -> Offer {
        let lists: Lists = lists::parse(lists::DEFAULT).unwrap();
        Offer::new(catalogue(), lists, &HashSet::new())
    }

    fn jobs() -> (GetGames, Rc<Fake>) {
        let fake = Rc::new(Fake::default());
        (
            GetGames::new(Installation::System, Box::new(fake.clone())),
            fake,
        )
    }

    /// Takes an install of `id` past its sizes and the free space check.
    fn past_the_checks(fake: &Fake) {
        fake.answer(&capture("remote-info"));
        fake.free(Some(100 * GIB));
    }

    #[test]
    fn an_install_runs_flatpak_with_y_but_not_noninteractive_from_flathub() {
        assert_eq!(
            install_commands(Installation::System, GAME),
            [words(&[
                "flatpak", "install", "--system", "-y", "flathub", GAME
            ])]
        );
        let user = install_commands(Installation::User, GAME);
        assert_eq!(
            user[0],
            words(&[
                "flatpak",
                "remote-add",
                "--user",
                "--if-not-exists",
                "flathub",
                "https://dl.flathub.org/repo/flathub.flatpakrepo"
            ])
        );
        assert_eq!(
            user[1],
            words(&["flatpak", "install", "--user", "-y", "flathub", GAME])
        );
    }

    #[test]
    fn a_removal_keeps_the_data_unless_asked_and_never_waits_for_a_password() {
        assert_eq!(
            uninstall_command(Installation::System, GAME, false),
            words(&[
                "flatpak",
                "uninstall",
                "--system",
                "--noninteractive",
                "-y",
                GAME
            ])
        );
        assert_eq!(
            uninstall_command(Installation::User, GAME, true),
            words(&[
                "flatpak",
                "uninstall",
                "--user",
                "--noninteractive",
                "-y",
                "--delete-data",
                GAME
            ])
        );
    }

    #[test]
    fn the_sizes_and_the_refresh_have_their_commands() {
        assert_eq!(
            remote_info_command(Installation::System, GAME),
            words(&["flatpak", "remote-info", "--system", "flathub", GAME])
        );
        assert_eq!(
            refresh_command(Installation::User),
            words(&[
                "flatpak",
                "update",
                "--appstream",
                "--user",
                "--noninteractive"
            ])
        );
    }

    #[test]
    fn remote_info_gives_the_download_and_installed_sizes() {
        assert_eq!(
            sizes(&capture("remote-info").stdout),
            Some(Sizes {
                download: 408_700,
                installed: 530_900
            })
        );
        let large = "Download Size: 1.2 GB\nInstalled Size: 3.4 GB\n";
        assert_eq!(
            sizes(large),
            Some(Sizes {
                download: 1_200_000_000,
                installed: 3_400_000_000
            })
        );
        assert_eq!(
            sizes(" Download Size: 131.4 MB\nInstalled Size: 900 bytes\n"),
            Some(Sizes {
                download: 131_400_000,
                installed: 900
            })
        );
        assert_eq!(sizes("Download Size: 1 MB\n"), None);
        assert_eq!(sizes("Download Size: lots\nInstalled Size: 1 kB\n"), None);
    }

    #[test]
    fn an_app_needs_its_installed_size_and_flatpaks_reserve_free() {
        let sizes = Sizes {
            download: 2 * GIB,
            installed: 3 * GIB,
        };
        assert_eq!(space_needed(sizes), 3 * GIB + RESERVE);
    }

    #[test]
    fn installed_apps_are_those_with_a_current_deployment() {
        let dir = tempfile::tempdir().unwrap();
        let app = |id: &str| dir.path().join("app").join(id);
        std::fs::create_dir_all(app(GAME).join("x86_64/stable/active")).unwrap();
        std::os::unix::fs::symlink("x86_64/stable", app(GAME).join("current")).unwrap();
        std::fs::create_dir_all(app("org.srb2.SRB2").join("x86_64/stable")).unwrap();
        std::fs::create_dir_all(app("not-an-id").join("current")).unwrap();
        assert_eq!(installed_apps(dir.path()), HashSet::from([GAME.to_owned()]));
        assert!(installed_apps(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn a_library_game_is_removable_as_the_installed_app_whose_entry_it_is() {
        let installed = HashSet::from([
            GAME.to_owned(),
            "org.example.Game".to_owned(),
            "org.example.Game.Editor".to_owned(),
        ]);
        let desktop = |id: &str| GameId::new(Source::Desktop, id);
        assert_eq!(removable_app(&desktop(GAME), &installed), Some(GAME));
        // Flatpak exports entries named for the app's ID and, after a dot, more.
        assert_eq!(
            removable_app(&desktop("org.example.Game.Level"), &installed),
            Some("org.example.Game")
        );
        assert_eq!(
            removable_app(&desktop("org.example.Game.Editor"), &installed),
            Some("org.example.Game.Editor")
        );
        assert_eq!(
            removable_app(&desktop("org.example.GameTwo"), &installed),
            None
        );
        assert_eq!(removable_app(&desktop("org.srb2.SRB2"), &installed), None);
        assert_eq!(
            removable_app(&GameId::new(Source::Steam, GAME), &installed),
            None
        );
    }

    #[test]
    fn only_app_ids_are_app_ids() {
        assert!(is_app_id(GAME));
        assert!(is_app_id("org.srb2.SRB2"));
        assert!(is_app_id("io.github.theforceengine.tfe.Mod-2"));
        assert!(is_app_id("com.github._0negal.Viper"));
        assert!(!is_app_id("--delete-data"));
        assert!(!is_app_id("org.example"));
        assert!(!is_app_id("org..example.App"));
        assert!(!is_app_id("org.-example.App"));
        assert!(!is_app_id("0rg.example.App"));
        assert!(!is_app_id("org.example.App;rm"));
        assert!(!is_app_id("org.example.App "));
        assert!(!is_app_id(""));
        assert!(!is_app_id(&format!("org.example.{}", "a".repeat(255))));
    }

    #[test]
    fn a_missing_or_day_old_catalogue_wants_refreshing() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        assert!(needs_refresh(None, now));
        assert!(!needs_refresh(Some(now - CATALOGUE_AGE), now));
        assert!(needs_refresh(
            Some(now - CATALOGUE_AGE - Duration::from_secs(1)),
            now
        ));
        assert!(!needs_refresh(Some(now + Duration::from_secs(60)), now));
    }

    #[test]
    fn the_screen_says_whether_a_catalogue_is_there_coming_or_out_of_reach() {
        assert_eq!(catalogue_status(true, true, false), CatalogueStatus::Ready);
        assert_eq!(
            catalogue_status(false, true, true),
            CatalogueStatus::Waiting
        );
        assert_eq!(
            catalogue_status(false, true, false),
            CatalogueStatus::Waiting
        );
        assert_eq!(
            catalogue_status(false, false, false),
            CatalogueStatus::Offline
        );
        assert_eq!(
            catalogue_status(false, false, true),
            CatalogueStatus::Unavailable
        );
    }

    #[test]
    fn only_ids_the_catalogue_offers_reach_the_command() {
        let (jobs, fake) = jobs();
        let offer = offer();
        assert_eq!(
            jobs.install(&offer, "com.example.Unknown"),
            Err(Refusal::NotOffered)
        );
        assert_eq!(
            jobs.install(&offer, "com.steamgriddb.SGDBoop"),
            Err(Refusal::NotOffered)
        );
        assert_eq!(
            jobs.install(&offer, "org.gnome.Calculator"),
            Err(Refusal::NotOffered)
        );
        assert!(fake.ran.borrow().is_empty());
        assert_eq!(jobs.state(), State::default());
    }

    #[test]
    fn a_store_goes_through_add_a_store() {
        let (jobs, fake) = jobs();
        assert_eq!(
            jobs.install(&offer(), "com.heroicgameslauncher.hgl"),
            Err(Refusal::Store(StoreApp::Heroic))
        );
        assert!(fake.ran.borrow().is_empty());
    }

    #[test]
    fn an_install_asks_its_sizes_checks_the_space_then_installs_with_progress() {
        let (jobs, fake) = jobs();
        let finished = Rc::new(RefCell::new(Vec::new()));
        jobs.connect_finished({
            let finished = finished.clone();
            move |job, outcome| finished.borrow_mut().push((job.clone(), outcome.clone()))
        });
        jobs.install(&offer(), GAME).unwrap();
        assert_eq!(
            fake.last_command(),
            remote_info_command(Installation::System, GAME)
        );
        let running = jobs.state().running.unwrap();
        assert_eq!(running.progress, None);
        assert!(running.checking);
        fake.answer(&capture("remote-info"));
        assert_eq!(
            jobs.state().sizes.get(GAME),
            Some(&Sizes {
                download: 408_700,
                installed: 530_900
            })
        );
        assert!(
            fake.runs.borrow().is_empty(),
            "installs before the space is known"
        );
        fake.free(Some(100 * GIB));
        assert!(!jobs.state().running.unwrap().checking);
        assert_eq!(
            fake.last_command(),
            install_commands(Installation::System, GAME)[0]
        );
        let progress = Rc::new(RefCell::new(Vec::new()));
        jobs.connect_changed({
            let progress = progress.clone();
            move |state| {
                if let Some(running) = &state.running {
                    progress.borrow_mut().push((running.progress, running.step));
                }
            }
        });
        fake.replay(&capture("install-runtime"));
        let seen = progress.borrow();
        assert!(seen.contains(&(Some(0.0), Some((1, 5)))), "{seen:?}");
        assert!(seen.iter().any(|(_, step)| *step == Some((4, 5))));
        let fractions: Vec<f64> = seen.iter().filter_map(|(progress, _)| *progress).collect();
        assert!(
            fractions.windows(2).all(|pair| pair[0] <= pair[1]),
            "{fractions:?}"
        );
        assert_eq!(fractions.last(), Some(&1.0));
        let state = jobs.state();
        assert_eq!(state.running, None);
        assert_eq!(state.outcomes.get(GAME), Some(&Outcome::Done));
        assert_eq!(
            finished.borrow().as_slice(),
            [(
                Job::Install {
                    id: GAME.to_owned(),
                    name: "SuperTuxKart".to_owned()
                },
                Outcome::Done
            )]
        );
    }

    #[test]
    fn a_single_operation_has_no_step_but_its_progress() {
        let (jobs, fake) = jobs();
        jobs.install(&offer(), GAME).unwrap();
        past_the_checks(&fake);
        let (_, line, _) = &fake.runs.borrow()[0];
        for text in capture("install").stdout.lines() {
            line(text);
        }
        let running = jobs.state().running.unwrap();
        assert_eq!(running.step, None);
        assert_eq!(running.progress, Some(1.0));
    }

    #[test]
    fn jobs_run_one_at_a_time_and_the_queue_follows() {
        let (jobs, fake) = jobs();
        let offer = offer();
        jobs.install(&offer, GAME).unwrap();
        jobs.install(&offer, "org.srb2.SRB2").unwrap();
        jobs.remove("org.gnome.Chess", "GNOME Chess", false)
            .unwrap();
        assert_eq!(jobs.install(&offer, "org.srb2.SRB2"), Err(Refusal::Busy));
        let state = jobs.state();
        assert!(state.busy(GAME) && state.busy("org.srb2.SRB2") && state.busy("org.gnome.Chess"));
        assert!(!state.busy("org.taisei_project.Taisei"));
        assert_eq!(state.queue.len(), 2);
        assert_eq!(fake.queries.borrow().len(), 1);
        past_the_checks(&fake);
        fake.replay(&capture("install"));
        assert_eq!(
            fake.last_command(),
            remote_info_command(Installation::System, "org.srb2.SRB2")
        );
        past_the_checks(&fake);
        fake.replay(&capture("install"));
        assert_eq!(
            fake.last_command(),
            uninstall_command(Installation::System, "org.gnome.Chess", false)
        );
        fake.replay(&capture("uninstall"));
        let state = jobs.state();
        assert_eq!(state.running, None);
        assert!(state.queue.is_empty());
        assert_eq!(state.outcomes.get("org.gnome.Chess"), Some(&Outcome::Done));
    }

    #[test]
    fn cancelling_a_running_install_stops_flatpak_and_the_next_job_starts() {
        let (jobs, fake) = jobs();
        let offer = offer();
        jobs.install(&offer, GAME).unwrap();
        jobs.install(&offer, "org.srb2.SRB2").unwrap();
        past_the_checks(&fake);
        jobs.cancel(GAME);
        assert_eq!(fake.stops.get(), 1);
        assert!(jobs.state().running.unwrap().cancelling);
        fake.replay(&capture("install-cancelled"));
        assert_eq!(jobs.state().outcomes.get(GAME), Some(&Outcome::Cancelled));
        assert_eq!(
            fake.last_command(),
            remote_info_command(Installation::System, "org.srb2.SRB2")
        );
    }

    #[test]
    fn cancelling_while_the_sizes_are_asked_installs_nothing() {
        let (jobs, fake) = jobs();
        jobs.install(&offer(), GAME).unwrap();
        jobs.cancel(GAME);
        fake.answer(&capture("remote-info"));
        assert!(fake.frees.borrow().is_empty());
        assert!(fake.runs.borrow().is_empty());
        assert_eq!(jobs.state().outcomes.get(GAME), Some(&Outcome::Cancelled));
        assert_eq!(jobs.state().running, None);
    }

    #[test]
    fn cancelling_a_queued_job_drops_it() {
        let (jobs, fake) = jobs();
        let offer = offer();
        jobs.install(&offer, GAME).unwrap();
        jobs.install(&offer, "org.srb2.SRB2").unwrap();
        jobs.cancel("org.srb2.SRB2");
        assert!(jobs.state().queue.is_empty());
        assert_eq!(fake.stops.get(), 0);
        assert!(jobs.state().running.is_some());
    }

    #[test]
    fn a_removal_that_runs_cannot_be_cancelled() {
        let (jobs, fake) = jobs();
        jobs.remove(GAME, "SuperTuxKart", true).unwrap();
        jobs.cancel(GAME);
        assert_eq!(fake.stops.get(), 0);
        assert!(!jobs.state().running.unwrap().cancelling);
        assert_eq!(
            fake.last_command(),
            uninstall_command(Installation::System, GAME, true)
        );
        fake.replay(&capture("uninstall-delete-data"));
        assert_eq!(jobs.state().outcomes.get(GAME), Some(&Outcome::Done));
    }

    #[test]
    fn a_removal_needs_an_app_id_and_says_why_it_failed() {
        let (jobs, fake) = jobs();
        assert_eq!(
            jobs.remove("--all", "Everything", false),
            Err(Refusal::InvalidId)
        );
        assert!(fake.ran.borrow().is_empty());
        jobs.remove("com.voxdsp.Snowboarder", "Snowboarder", false)
            .unwrap();
        fake.replay(&capture("uninstall-not-installed"));
        assert_eq!(
            jobs.state().outcomes.get("com.voxdsp.Snowboarder"),
            Some(&Outcome::Failed {
                error: "No installed refs found for ‘com.voxdsp.Snowboarder’".to_owned(),
                network: false
            })
        );
    }

    #[test]
    fn a_failed_install_says_why_in_flatpaks_words() {
        let (jobs, fake) = jobs();
        jobs.install(&offer(), GAME).unwrap();
        past_the_checks(&fake);
        fake.replay(&capture("install-no-space"));
        let Some(Outcome::Failed { error, network }) = jobs.state().outcomes.get(GAME).cloned()
        else {
            panic!("no failure");
        };
        assert!(error.starts_with("Failed to install com.github.tmewett.BrogueCE"));
        assert!(!network);
    }

    #[test]
    fn an_install_flathub_cannot_be_reached_for_is_a_network_failure() {
        let (jobs, fake) = jobs();
        jobs.install(&offer(), GAME).unwrap();
        fake.answer(&capture("remote-info-offline"));
        assert!(fake.frees.borrow().is_empty());
        assert!(matches!(
            jobs.state().outcomes.get(GAME),
            Some(Outcome::Failed { network: true, .. })
        ));
    }

    #[test]
    fn an_install_that_fails_while_offline_is_a_network_failure() {
        let (jobs, fake) = jobs();
        jobs.set_online(false);
        jobs.install(&offer(), GAME).unwrap();
        past_the_checks(&fake);
        fake.replay(&capture("install-unknown"));
        assert!(matches!(
            jobs.state().outcomes.get(GAME),
            Some(Outcome::Failed { network: true, .. })
        ));
    }

    #[test]
    fn an_app_that_would_not_fit_is_not_installed() {
        let (jobs, fake) = jobs();
        jobs.install(&offer(), GAME).unwrap();
        fake.answer(&capture("remote-info"));
        let needed = 530_900 + RESERVE;
        fake.free(Some(needed - 1));
        assert!(fake.runs.borrow().is_empty());
        assert_eq!(
            jobs.state().outcomes.get(GAME),
            Some(&Outcome::NoSpace {
                needed,
                free: needed - 1
            })
        );
    }

    #[test]
    fn an_app_that_just_fits_or_unmeasured_space_installs() {
        let (jobs, fake) = jobs();
        let offer = offer();
        jobs.install(&offer, GAME).unwrap();
        fake.answer(&capture("remote-info"));
        fake.free(Some(530_900 + RESERVE));
        assert_eq!(fake.runs.borrow().len(), 1);
        fake.replay(&capture("install"));
        jobs.install(&offer, "org.srb2.SRB2").unwrap();
        fake.answer(&capture("remote-info"));
        fake.free(None);
        assert_eq!(fake.runs.borrow().len(), 1);
    }

    #[test]
    fn a_new_job_for_an_app_clears_its_last_outcome() {
        let (jobs, fake) = jobs();
        let offer = offer();
        jobs.install(&offer, GAME).unwrap();
        past_the_checks(&fake);
        fake.replay(&capture("install-unknown"));
        jobs.install(&offer, GAME).unwrap();
        assert_eq!(jobs.state().outcomes.get(GAME), None);
    }

    #[test]
    fn a_refresh_runs_once_however_often_it_is_asked_for() {
        let (jobs, fake) = jobs();
        jobs.refresh();
        jobs.refresh();
        assert!(jobs.state().refreshing());
        assert_eq!(fake.ran.borrow().len(), 1);
        assert_eq!(fake.last_command(), refresh_command(Installation::System));
        fake.replay(&capture("appstream-refresh"));
        let state = jobs.state();
        assert!(!state.refreshing());
        assert_eq!(state.refreshed, Some(Outcome::Done));
        assert!(state.outcomes.is_empty());
    }

    #[test]
    fn the_sizes_of_an_app_page_are_asked_alongside_any_job() {
        let (jobs, fake) = jobs();
        jobs.refresh();
        jobs.fetch_sizes(GAME);
        assert_eq!(
            fake.last_command(),
            remote_info_command(Installation::System, GAME)
        );
        fake.answer(&capture("remote-info"));
        assert!(jobs.state().sizes.contains_key(GAME));
        assert!(jobs.state().refreshing());
        jobs.fetch_sizes("--delete-data");
        assert_eq!(fake.queries.borrow().len(), 0);
    }

    #[test]
    fn sizes_in_terabytes_and_malformed_numbers_read_as_they_say() {
        assert_eq!(
            sizes("Download Size: 2 TB\nInstalled Size: 1.25 TB\n"),
            Some(Sizes {
                download: 2_000_000_000_000,
                installed: 1_250_000_000_000
            })
        );
        assert_eq!(sizes("Download Size: .5 MB\nInstalled Size: 1 MB\n"), None);
        assert_eq!(sizes("Download Size: 1.x MB\nInstalled Size: 1 MB\n"), None);
        // Rust's parser takes a sign, GLib never writes one.
        assert_eq!(sizes("Download Size: +5 MB\nInstalled Size: 1 MB\n"), None);
        assert_eq!(
            sizes("Download Size: 1.+5 MB\nInstalled Size: 1 MB\n"),
            None
        );
        assert_eq!(
            sizes("Download Size: 1. MB\nInstalled Size: 1 MB\n"),
            Some(Sizes {
                download: 1_000_000,
                installed: 1_000_000
            })
        );
    }

    /// How many times `jobs` say they changed while `act` runs.
    fn changes(jobs: &GetGames, act: impl FnOnce()) -> usize {
        let count = Rc::new(Cell::new(0));
        jobs.connect_changed({
            let count = count.clone();
            move |_| count.set(count.get() + 1)
        });
        let before = count.get();
        act();
        count.get() - before
    }

    #[test]
    fn nothing_that_changes_nothing_says_it_changed() {
        let (jobs, fake) = jobs();
        assert_eq!(changes(&jobs, || jobs.set_online(true)), 0);
        assert_eq!(changes(&jobs, || jobs.set_online(false)), 1);
        assert_eq!(changes(&jobs, || jobs.cancel("org.example.Nothing")), 0);
        jobs.install(&offer(), GAME).unwrap();
        past_the_checks(&fake);
        let (_, line, _) = &fake.runs.borrow()[0];
        line("Installing 1/2… ████        20%");
        assert_eq!(
            changes(&jobs, || line("Installing 1/2… ████        20%")),
            0
        );
        assert_eq!(
            changes(&jobs, || line("Installing 2/2…              0%")),
            1
        );
    }

    #[test]
    fn the_jobs_say_their_installation_when_printed() {
        let (jobs, _) = jobs();
        assert!(format!("{jobs:?}").contains("System"));
    }
}
