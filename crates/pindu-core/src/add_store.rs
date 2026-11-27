//! The "add a store" flow (#378): installs Steam, Heroic or Lutris from Flathub, so a
//! library that starts empty, as on Pindu OS, gets a launcher to import from.

use std::cell::RefCell;
use std::fmt;
use std::rc::{Rc, Weak};

use crate::get_games;
use crate::launch::StoreClient;
use crate::model::{GameId, Source};
use crate::sources::desktop;
use crate::sources::location::{HostDirs, Location};

/// The name of the Flathub remote.
pub const FLATHUB: &str = "flathub";

/// Where Flathub's remote description is, for `flatpak remote-add`.
pub const FLATHUB_REPO: &str = "https://dl.flathub.org/repo/flathub.flatpakrepo";

/// A store the flow offers, by its Flathub app.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StoreApp {
    /// Steam (`com.valvesoftware.Steam`).
    Steam,
    /// Heroic Games Launcher (`com.heroicgameslauncher.hgl`), for Epic, GOG and Amazon.
    Heroic,
    /// Lutris (`net.lutris.Lutris`).
    Lutris,
}

impl StoreApp {
    /// The stores, in the order the flow lists them.
    pub const ALL: [Self; 3] = [Self::Steam, Self::Heroic, Self::Lutris];

    /// The app's ID on Flathub.
    pub fn app_id(self) -> &'static str {
        match self {
            Self::Steam => "com.valvesoftware.Steam",
            Self::Heroic => "com.heroicgameslauncher.hgl",
            Self::Lutris => "net.lutris.Lutris",
        }
    }

    /// The desktop entries that show the app installed: its Flatpak's, then a native
    /// package's.
    fn desktop_files(self) -> [&'static str; 2] {
        match self {
            Self::Steam => ["com.valvesoftware.Steam.desktop", "steam.desktop"],
            Self::Heroic => ["com.heroicgameslauncher.hgl.desktop", "heroic.desktop"],
            Self::Lutris => ["net.lutris.Lutris.desktop", "lutris.desktop"],
        }
    }

    /// The library sources the app's data folder feeds.
    fn sources(self) -> &'static [Source] {
        match self {
            Self::Steam => &[Source::Steam],
            Self::Heroic => &[
                Source::HeroicEpic,
                Source::HeroicGog,
                Source::HeroicAmazon,
                Source::HeroicSideload,
            ],
            Self::Lutris => &[Source::Lutris],
        }
    }
}

/// The Flatpak installation an install goes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installation {
    /// The system installation, which has a Flathub remote.
    System,
    /// The user's installation, Flathub added to it first when it lacks it.
    User,
}

/// The command that lists the system installation's remotes, one name a line.
pub const REMOTES_COMMAND: [&str; 4] = ["flatpak", "remotes", "--system", "--columns=name"];

impl Installation {
    /// Where to install, from what [`REMOTES_COMMAND`] printed: the system installation
    /// when it has a remote called `flathub`, the user's otherwise.
    pub fn from_system_remotes(remotes: &str) -> Self {
        if remotes.lines().any(|name| name.trim() == FLATHUB) {
            Self::System
        } else {
            Self::User
        }
    }
}

/// The commands that install `app` into `installation`, in order, each an argv list: the
/// user installation gets Flathub first, unless it has it already. They are Get Games'
/// ([`get_games::install_commands`]), whose output reports progress.
pub fn install_commands(installation: Installation, app: StoreApp) -> Vec<Vec<String>> {
    get_games::install_commands(installation, app.app_id())
}

/// Follows what `flatpak install` prints on standard output while it runs, without a
/// terminal: a line such as `Installing 2/3…` as each of its operations starts (the app,
/// its runtime, their extensions), then lines such as `Installing 2/3… ███▌   45%  1.2 MB/s`
/// as one moves on. Words are translated, so it reads only the numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProgressReader {
    /// The operation that runs, from 1.
    operation: u32,
    /// How many operations the install has.
    operations: u32,
}

impl ProgressReader {
    /// A reader that has seen nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// How far the whole install is after `line`, from 0 to 1, when the line says.
    pub fn read(&mut self, line: &str) -> Option<f64> {
        let words = || line.split_whitespace();
        let count = words().find_map(operation_count);
        if let Some((operation, operations)) = count {
            self.operation = operation;
            self.operations = operations;
        }
        let percent = match words().find_map(percent) {
            Some(percent) => percent,
            None if count.is_some() => 0,
            None => return None,
        };
        let (operation, operations) = (self.operation.max(1), self.operations.max(1));
        Some((f64::from(operation - 1) + f64::from(percent) / 100.0) / f64::from(operations))
    }

    /// The operation that runs and how many the install has, once it has said and when it
    /// has more than one.
    pub fn step(&self) -> Option<(u32, u32)> {
        (self.operations > 1).then_some((self.operation, self.operations))
    }
}

/// The operation and the count in a word such as `2/3…`.
fn operation_count(word: &str) -> Option<(u32, u32)> {
    let word = word.trim_end_matches(['…', '.']);
    let (operation, operations) = word.split_once('/')?;
    let operation: u32 = operation.parse().ok()?;
    let operations: u32 = operations.parse().ok()?;
    (1..=operations)
        .contains(&operation)
        .then_some((operation, operations))
}

/// The percentage in a word such as `45%`, up to 100.
fn percent(word: &str) -> Option<u32> {
    let percent: u32 = word.strip_suffix('%')?.parse().ok()?;
    Some(percent.min(100))
}

/// What went wrong, from what `flatpak` printed on standard error: its last error line,
/// without the `error:` before it, or else its last line; `None` when it printed nothing.
pub fn error_message(stderr: &str) -> Option<String> {
    let lines = || {
        stderr
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
    };
    let error = lines().rev().find_map(|line| {
        let (prefix, rest) = line.split_at_checked("error:".len())?;
        prefix
            .eq_ignore_ascii_case("error:")
            .then(|| rest.trim_start())
    });
    error.or_else(|| lines().next_back()).map(str::to_owned)
}

/// What `flatpak` (through libostree, libcurl and GLib) prints when it cannot reach
/// Flathub: the host's name does not resolve, no connection is made, or it times out.
/// They are matched without case, in English: words the user's language translated fall
/// back on whether the machine is still online.
const NETWORK_ERRORS: [&str; 9] = [
    "resolve host",
    "name resolution",
    "name or service not known",
    "couldn't connect",
    "could not connect",
    "network is unreachable",
    "no route to host",
    "timeout was reached",
    "connection timed out",
];

/// Whether what `flatpak` printed on standard error says that Flathub could not be
/// reached ([`NETWORK_ERRORS`]).
pub(crate) fn network_failure(stderr: &str) -> bool {
    let stderr = stderr.to_lowercase();
    NETWORK_ERRORS.iter().any(|words| stderr.contains(words))
}

/// The stores installed on this machine, natively or as Flatpaks, that `dirs` show: a
/// store counts when the desktop shows its app ([`desktop::application_dirs`]) or the
/// library's importers find its data folder ([`Location`]).
pub fn installed_stores(dirs: &HostDirs) -> Vec<StoreApp> {
    let applications = desktop::application_dirs(dirs);
    StoreApp::ALL
        .into_iter()
        .filter(|app| {
            let shown = app.desktop_files().iter().any(|file| {
                applications
                    .iter()
                    .any(|folder| folder.join(file).is_file())
            });
            shown
                || app.sources().iter().any(|source| {
                    Location::of(*source, dirs)
                        .is_some_and(|location| location.resolve(None).is_ok())
                })
        })
        .collect()
}

/// The install of `app`'s client the desktop shows ([`desktop::application_dirs`]): the
/// Flatpak when its entry is there, else the host's package when its own is; `None`
/// without either. Unlike the library's data folder, this is known before the store's
/// first start.
pub fn store_client(app: StoreApp, dirs: &HostDirs) -> Option<StoreClient> {
    let applications = desktop::application_dirs(dirs);
    let shown = |file: &str| {
        applications
            .iter()
            .any(|folder| folder.join(file).is_file())
    };
    let [flatpak, host] = app.desktop_files();
    if shown(flatpak) {
        Some(StoreClient::Flatpak)
    } else if shown(host) {
        Some(StoreClient::Host)
    } else {
        None
    }
}

/// The command that opens `app`'s `client` with its own window, for its first sign-in:
/// `flatpak run` with its app ID, or its command on the host's `PATH`.
pub fn open_command(app: StoreApp, client: StoreClient) -> Vec<String> {
    match client {
        StoreClient::Flatpak => ["flatpak", "run", app.app_id()].map(str::to_owned).to_vec(),
        StoreClient::Host => vec![
            match app {
                StoreApp::Steam => "steam",
                StoreApp::Heroic => "heroic",
                StoreApp::Lutris => "lutris",
            }
            .to_owned(),
        ],
    }
}

/// The ID a `gamescope --steam` session follows `app` under once it is opened, as it
/// follows a game handed to its store launcher (`gamescope::StoreLaunch`): the store's own
/// desktop entry, which the library never imports as a game.
pub fn opened_id(app: StoreApp) -> GameId {
    GameId::new(Source::Desktop, app.app_id())
}

/// How the flow installs a store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Method {
    /// The host's `flatpak`, run from the launcher: the native build.
    #[default]
    Flatpak,
    /// The software centre, which the OpenURI portal opens at the app's `appstream://`
    /// page: the Flatpak build, which cannot run the host's `flatpak` and gets no
    /// permission to.
    SoftwareCentre,
}

/// Where the flow is.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Step {
    /// The stores show, to choose one.
    #[default]
    Choose,
    /// A store installs.
    Installing {
        /// The store.
        app: StoreApp,
        /// How far the install is, from 0 to 1.
        progress: f64,
        /// Whether the user cancelled it, and it is stopping.
        cancelling: bool,
    },
    /// A store installed: the next step is to sign in to it.
    Installed(StoreApp),
    /// The software centre opened at a store's page, to install it there.
    Opened(StoreApp),
    /// A store did not install, or the software centre did not open.
    Failed {
        /// The store.
        app: StoreApp,
        /// What went wrong, in `flatpak`'s words.
        error: String,
        /// The network failed it: Flathub could not be reached, or the machine went
        /// offline while it ran.
        network: bool,
    },
}

/// What the flow shows.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct AddStoreState {
    /// How it installs.
    pub method: Method,
    /// The stores already installed, which it marks.
    pub installed: Vec<StoreApp>,
    /// Where it is.
    pub step: Step,
    /// The machine has no network, so nothing can install until it has one.
    pub offline: bool,
    /// The installed stores have been looked for at least once, so `installed` says
    /// which are there.
    pub detected: bool,
}

impl AddStoreState {
    /// Whether `app` can be chosen: it is not installed, no install runs, and the machine
    /// is online.
    pub fn can_choose(&self, app: StoreApp) -> bool {
        !self.offline
            && !self.installed.contains(&app)
            && !matches!(self.step, Step::Installing { .. })
    }
}

/// What a backend reports once a command ends, or the software centre opened: `Err`
/// holds what went wrong, such as the command's standard error.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;

/// What a backend reports once it found the installed stores.
pub type Detected = Box<dyn FnOnce(Vec<StoreApp>)>;

/// What a backend gives each line a command prints.
pub type Line = Box<dyn Fn(&str)>;

/// Finds the installed stores and runs the commands of the flow, off the main thread;
/// every callback runs on the main thread.
pub trait AddStoreBackend {
    /// Finds the installed stores ([`installed_stores`]) and reports them to `done`.
    fn detect(&self, done: Detected);

    /// Runs `argv`, giving `line` each line it prints on standard output, and reports to
    /// `done` once it ends: `Err` with its standard error when it fails.
    fn run(&self, argv: Vec<String>, line: Line, done: Done);

    /// Stops the command that runs, which then reports that it failed.
    fn stop(&self);

    /// Opens `uri` through the OpenURI portal, and reports to `done`.
    fn open(&self, uri: String, done: Done);
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&AddStoreState)>;
/// Something a service does when a store installed.
type InstalledListener = Box<dyn Fn(StoreApp)>;

/// The flow over its backend.
#[derive(Clone)]
pub struct AddStore {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn AddStoreBackend>,
    state: RefCell<AddStoreState>,
    listeners: RefCell<Vec<Listener>>,
    installed_listeners: RefCell<Vec<InstalledListener>>,
}

impl fmt::Debug for AddStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AddStore")
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl AddStore {
    /// The flow, installing by `method` over `backend`, with nothing installed until
    /// [`refresh`](Self::refresh) finds out.
    pub fn new(method: Method, backend: Box<dyn AddStoreBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                state: RefCell::new(AddStoreState {
                    method,
                    ..AddStoreState::default()
                }),
                listeners: RefCell::default(),
                installed_listeners: RefCell::default(),
            }),
        }
    }

    /// The state as it is.
    pub fn state(&self) -> AddStoreState {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&AddStoreState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` once a store has installed.
    pub fn connect_installed(&self, listener: impl Fn(StoreApp) + 'static) {
        self.inner
            .installed_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Finds out again which stores are installed: the flow opens.
    pub fn refresh(&self) {
        let weak = self.downgrade();
        self.inner.backend.detect(Box::new(move |stores| {
            if let Some(flow) = upgrade(&weak) {
                let changed = {
                    let mut state = flow.inner.state.borrow_mut();
                    let changed = state.installed != stores || !state.detected;
                    state.installed = stores;
                    state.detected = true;
                    changed
                };
                if changed {
                    flow.changed();
                }
            }
        }));
    }

    /// The machine went online or offline.
    pub fn set_online(&self, online: bool) {
        {
            let mut state = self.inner.state.borrow_mut();
            if state.offline != online {
                return;
            }
            state.offline = !online;
        }
        self.changed();
    }

    /// Installs `app`, or opens its page in the software centre.
    pub fn choose(&self, app: StoreApp) {
        let (method, can) = {
            let state = self.inner.state.borrow();
            (state.method, state.can_choose(app))
        };
        if !can {
            return;
        }
        match method {
            Method::SoftwareCentre => self.open(app),
            Method::Flatpak => self.install(app),
        }
    }

    /// Opens `app`'s page in the software centre.
    fn open(&self, app: StoreApp) {
        let weak = self.downgrade();
        let uri = format!("appstream://{}", app.app_id());
        self.inner.backend.open(
            uri,
            Box::new(move |result| {
                if let Some(flow) = upgrade(&weak) {
                    flow.set_step(match result {
                        Ok(()) => Step::Opened(app),
                        Err(error) => Step::Failed {
                            app,
                            error,
                            network: false,
                        },
                    });
                }
            }),
        );
    }

    /// Installs `app` with the host's `flatpak`: into the system installation when it has
    /// Flathub, else into the user's. A system installation that cannot be listed counts
    /// as one without Flathub.
    fn install(&self, app: StoreApp) {
        self.set_step(Step::Installing {
            app,
            progress: 0.0,
            cancelling: false,
        });
        let remotes = Rc::new(RefCell::new(String::new()));
        let weak = self.downgrade();
        let line = {
            let remotes = remotes.clone();
            move |line: &str| {
                let mut remotes = remotes.borrow_mut();
                remotes.push_str(line);
                remotes.push('\n');
            }
        };
        self.inner.backend.run(
            REMOTES_COMMAND.map(str::to_owned).to_vec(),
            Box::new(line),
            Box::new(move |result| {
                let Some(flow) = upgrade(&weak) else {
                    return;
                };
                let installation = match result {
                    Ok(()) => Installation::from_system_remotes(&remotes.borrow()),
                    Err(_) => Installation::User,
                };
                flow.run_commands(app, install_commands(installation, app).into_iter());
            }),
        );
    }

    /// Runs the next of `commands`, which install `app`, and the rest after it; the
    /// install is done when none is left.
    fn run_commands(&self, app: StoreApp, mut commands: std::vec::IntoIter<Vec<String>>) {
        let Some(argv) = commands.next() else {
            self.installed(app);
            return;
        };
        if self.cancelling() {
            self.set_step(Step::Choose);
            return;
        }
        let reader = RefCell::new(ProgressReader::new());
        let weak = self.downgrade();
        let line = move |line: &str| {
            let progress = reader.borrow_mut().read(line);
            if let (Some(flow), Some(progress)) = (upgrade(&weak), progress) {
                flow.set_progress(progress);
            }
        };
        let weak = self.downgrade();
        self.inner.backend.run(
            argv,
            Box::new(line),
            Box::new(move |result| {
                let Some(flow) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(()) => flow.run_commands(app, commands),
                    Err(_) if flow.cancelling() => flow.set_step(Step::Choose),
                    Err(text) => {
                        let network = network_failure(&text) || flow.inner.state.borrow().offline;
                        flow.set_step(Step::Failed {
                            app,
                            error: error_message(&text).unwrap_or(text),
                            network,
                        });
                    }
                }
            }),
        );
    }

    /// `app` installed: it shows as installed, the services hear of it, and the stores
    /// are looked for again.
    fn installed(&self, app: StoreApp) {
        {
            let mut state = self.inner.state.borrow_mut();
            if !state.installed.contains(&app) {
                state.installed.push(app);
            }
            state.step = Step::Installed(app);
        }
        self.changed();
        for listener in self.inner.installed_listeners.borrow().iter() {
            listener(app);
        }
        self.refresh();
    }

    /// The install that runs is `progress` of the way.
    fn set_progress(&self, progress: f64) {
        {
            let mut state = self.inner.state.borrow_mut();
            let Step::Installing {
                progress: shown, ..
            } = &mut state.step
            else {
                return;
            };
            if *shown == progress {
                return;
            }
            *shown = progress;
        }
        self.changed();
    }

    /// Leaves an installed, opened or failed store for the stores again.
    pub fn dismiss(&self) {
        let step = self.inner.state.borrow().step.clone();
        if matches!(
            step,
            Step::Installed(_) | Step::Opened(_) | Step::Failed { .. }
        ) {
            self.set_step(Step::Choose);
        }
    }

    /// Stops the install that runs; the flow goes back to the stores once it has.
    pub fn cancel(&self) {
        {
            let mut state = self.inner.state.borrow_mut();
            let Step::Installing {
                cancelling: cancelling @ false,
                ..
            } = &mut state.step
            else {
                return;
            };
            *cancelling = true;
        }
        self.changed();
        self.inner.backend.stop();
    }

    /// Whether the user cancelled the install that runs.
    fn cancelling(&self) -> bool {
        matches!(
            self.inner.state.borrow().step,
            Step::Installing {
                cancelling: true,
                ..
            }
        )
    }

    fn set_step(&self, step: Step) {
        self.inner.state.borrow_mut().step = step;
        self.changed();
    }

    fn changed(&self) {
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
        }
    }

    /// A handle that does not keep the flow alive, for its own listeners to hold.
    pub fn downgrade(&self) -> WeakAddStore {
        WeakAddStore {
            inner: Rc::downgrade(&self.inner),
        }
    }
}

/// A handle on an [`AddStore`] that does not keep it alive.
#[derive(Clone, Debug)]
pub struct WeakAddStore {
    inner: Weak<Inner>,
}

impl WeakAddStore {
    /// The flow, unless it is gone.
    pub fn upgrade(&self) -> Option<AddStore> {
        self.inner.upgrade().map(|inner| AddStore { inner })
    }
}

/// The flow again from a backend's callback, unless it is gone.
fn upgrade(weak: &WeakAddStore) -> Option<AddStore> {
    weak.upgrade()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn argv(words: &[&str]) -> Vec<String> {
        words.iter().map(|word| (*word).to_owned()).collect()
    }

    /// A backend whose work ends when a test says.
    #[derive(Default)]
    struct Fake {
        detected: RefCell<Vec<Detected>>,
        runs: RefCell<Vec<(Vec<String>, Line, Done)>>,
        ran: RefCell<Vec<Vec<String>>>,
        stops: Cell<usize>,
        opens: RefCell<Vec<(String, Done)>>,
    }

    impl AddStoreBackend for Rc<Fake> {
        fn detect(&self, done: Detected) {
            self.detected.borrow_mut().push(done);
        }

        fn run(&self, argv: Vec<String>, line: Line, done: Done) {
            self.ran.borrow_mut().push(argv.clone());
            self.runs.borrow_mut().push((argv, line, done));
        }

        fn stop(&self) {
            self.stops.set(self.stops.get() + 1);
        }

        fn open(&self, uri: String, done: Done) {
            self.opens.borrow_mut().push((uri, done));
        }
    }

    impl Fake {
        /// The command that runs prints `lines`.
        fn print(&self, lines: &[&str]) {
            let runs = self.runs.borrow();
            let (_, line, _) = runs.last().expect("a command runs");
            for text in lines {
                line(text);
            }
        }

        /// The command that runs ends with `result`.
        fn end(&self, result: Result<(), &str>) {
            let (_, _, done) = self.runs.borrow_mut().pop().expect("a command runs");
            done(result.map_err(str::to_owned));
        }
    }

    fn flow(method: Method) -> (AddStore, Rc<Fake>) {
        let fake = Rc::new(Fake::default());
        (AddStore::new(method, Box::new(fake.clone())), fake)
    }

    #[test]
    fn the_flatpak_build_opens_the_stores_page_in_the_software_centre() {
        let (flow, fake) = flow(Method::SoftwareCentre);
        flow.choose(StoreApp::Lutris);
        let (uri, done) = fake.opens.borrow_mut().pop().expect("a page opens");
        assert_eq!(uri, "appstream://net.lutris.Lutris");
        done(Ok(()));
        assert_eq!(flow.state().step, Step::Opened(StoreApp::Lutris));
        assert!(fake.ran.borrow().is_empty(), "nothing runs on the host");
    }

    #[test]
    fn the_system_installation_installs_from_its_flathub_without_adding_it() {
        assert_eq!(
            install_commands(Installation::System, StoreApp::Steam),
            [argv(&[
                "flatpak",
                "install",
                "--system",
                "-y",
                "flathub",
                "com.valvesoftware.Steam",
            ])]
        );
    }

    #[test]
    fn the_user_installation_gets_flathub_if_it_lacks_it_then_installs_from_it() {
        assert_eq!(
            install_commands(Installation::User, StoreApp::Heroic),
            [
                argv(&[
                    "flatpak",
                    "remote-add",
                    "--user",
                    "--if-not-exists",
                    "flathub",
                    "https://dl.flathub.org/repo/flathub.flatpakrepo",
                ]),
                argv(&[
                    "flatpak",
                    "install",
                    "--user",
                    "-y",
                    "flathub",
                    "com.heroicgameslauncher.hgl",
                ]),
            ]
        );
    }

    #[test]
    fn a_system_flathub_remote_picks_the_system_installation_and_none_the_users() {
        assert_eq!(
            Installation::from_system_remotes("fedora\nflathub\n"),
            Installation::System
        );
        assert_eq!(
            Installation::from_system_remotes("  flathub  \n"),
            Installation::System
        );
        assert_eq!(
            Installation::from_system_remotes("fedora\nflathub-beta\n"),
            Installation::User
        );
        assert_eq!(Installation::from_system_remotes(""), Installation::User);
    }

    #[test]
    fn a_progress_line_counts_the_operations_before_it_and_its_own_percent() {
        let mut reader = ProgressReader::new();
        assert_eq!(
            reader.read("Installing 2/4… ██████████          50%  1.2 MB/s  00:10"),
            Some(0.375)
        );
    }

    #[test]
    fn the_step_is_the_operation_and_count_of_an_install_with_several() {
        let mut reader = ProgressReader::new();
        assert_eq!(reader.step(), None);
        reader.read("Installing 1/1…");
        assert_eq!(reader.step(), None);
        reader.read("Installing 2/3…        0%");
        assert_eq!(reader.step(), Some((2, 3)));
    }

    #[test]
    fn an_operation_starting_counts_the_operations_before_it() {
        let mut reader = ProgressReader::new();
        assert_eq!(reader.read("Installing 1/2…"), Some(0.0));
        assert_eq!(
            reader.read("Installing 1/2… ████     40%  3.0 MB/s"),
            Some(0.2)
        );
        assert_eq!(reader.read("Installing 2/2…"), Some(0.5));
    }

    #[test]
    fn other_lines_say_nothing_and_a_single_operation_counts_alone() {
        let mut reader = ProgressReader::new();
        for line in [
            "Looking for matches…",
            "Required runtime for com.valvesoftware.Steam/x86_64/stable (runtime/org.freedesktop.Platform/x86_64/24.08) found in remote flathub",
            " 1. [✓] com.valvesoftware.Steam  stable  i  flathub  < 15.3 MB",
            "",
            "Installation complete.",
        ] {
            assert_eq!(reader.read(line), None, "{line}");
        }
        assert_eq!(
            reader.read("Installing… █████████     45%  1.0 MB/s"),
            Some(0.45)
        );
    }

    #[test]
    fn the_error_is_flatpaks_last_error_line_without_its_prefix() {
        let stderr = "Looking for matches…\nWarning: something minor\nerror: Unable to load summary from remote flathub: Could not resolve hostname\n";
        assert_eq!(
            error_message(stderr).as_deref(),
            Some("Unable to load summary from remote flathub: Could not resolve hostname")
        );
    }

    fn dirs(home: &std::path::Path) -> HostDirs {
        HostDirs {
            home: home.to_owned(),
            data_home: home.join(".local/share"),
            config_home: home.join(".config"),
            host_usr: home.join("usr"),
            system_flatpak: home.join("var/lib/flatpak"),
        }
    }

    fn touch(path: &std::path::Path) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, "").unwrap();
    }

    #[test]
    fn a_store_the_desktop_shows_is_installed() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(
            ".local/share/flatpak/exports/share/applications/com.valvesoftware.Steam.desktop",
        ));
        touch(
            &home
                .path()
                .join("usr/share/applications/net.lutris.Lutris.desktop"),
        );
        assert_eq!(
            installed_stores(&dirs(home.path())),
            [StoreApp::Steam, StoreApp::Lutris]
        );
    }

    #[test]
    fn a_store_whose_data_folder_the_importers_find_is_installed() {
        let home = tempfile::tempdir().unwrap();
        touch(&home.path().join(
            ".var/app/com.heroicgameslauncher.hgl/config/heroic/store_cache/gog_library.json",
        ));
        assert_eq!(installed_stores(&dirs(home.path())), [StoreApp::Heroic]);
        let empty = tempfile::tempdir().unwrap();
        assert_eq!(installed_stores(&dirs(empty.path())), []);
    }

    #[test]
    fn the_installed_client_is_the_flatpak_by_its_entry_else_the_hosts() {
        let flatpak = tempfile::tempdir().unwrap();
        touch(
            &flatpak
                .path()
                .join("var/lib/flatpak/exports/share/applications/com.valvesoftware.Steam.desktop"),
        );
        touch(&flatpak.path().join("usr/share/applications/steam.desktop"));
        assert_eq!(
            store_client(StoreApp::Steam, &dirs(flatpak.path())),
            Some(StoreClient::Flatpak)
        );
        let host = tempfile::tempdir().unwrap();
        touch(&host.path().join("usr/share/applications/steam.desktop"));
        touch(&host.path().join("usr/share/applications/lutris.desktop"));
        touch(&host.path().join(
            ".local/share/flatpak/exports/share/applications/com.heroicgameslauncher.hgl.desktop",
        ));
        let dirs = dirs(host.path());
        assert_eq!(
            store_client(StoreApp::Steam, &dirs),
            Some(StoreClient::Host)
        );
        assert_eq!(
            store_client(StoreApp::Lutris, &dirs),
            Some(StoreClient::Host)
        );
        assert_eq!(
            store_client(StoreApp::Heroic, &dirs),
            Some(StoreClient::Flatpak)
        );
        let none = tempfile::tempdir().unwrap();
        assert_eq!(
            store_client(StoreApp::Steam, &self::dirs(none.path())),
            None
        );
    }

    #[test]
    fn a_store_opens_through_flatpak_run_or_its_host_command() {
        assert_eq!(
            open_command(StoreApp::Heroic, StoreClient::Flatpak),
            argv(&["flatpak", "run", "com.heroicgameslauncher.hgl"])
        );
        assert_eq!(
            open_command(StoreApp::Lutris, StoreClient::Flatpak),
            argv(&["flatpak", "run", "net.lutris.Lutris"])
        );
        assert_eq!(
            open_command(StoreApp::Heroic, StoreClient::Host),
            argv(&["heroic"])
        );
        assert_eq!(
            open_command(StoreApp::Lutris, StoreClient::Host),
            argv(&["lutris"])
        );
        assert_eq!(
            open_command(StoreApp::Steam, StoreClient::Host),
            argv(&["steam"])
        );
    }

    #[test]
    fn each_store_opens_as_a_session_of_its_own() {
        let ids: Vec<_> = StoreApp::ALL.into_iter().map(opened_id).collect();
        assert_ne!(ids[0], ids[1]);
        assert_ne!(ids[1], ids[2]);
        assert_ne!(ids[0], ids[2]);
        assert_eq!(opened_id(StoreApp::Heroic), opened_id(StoreApp::Heroic));
    }

    #[test]
    fn a_native_install_asks_for_the_system_remotes_then_installs_with_progress() {
        let (flow, fake) = flow(Method::Flatpak);
        let installed = Rc::new(RefCell::new(Vec::new()));
        flow.connect_installed({
            let installed = installed.clone();
            move |app| installed.borrow_mut().push(app)
        });
        flow.choose(StoreApp::Steam);
        assert_eq!(
            flow.state().step,
            Step::Installing {
                app: StoreApp::Steam,
                progress: 0.0,
                cancelling: false,
            }
        );
        assert_eq!(fake.ran.borrow().last(), Some(&argv(&REMOTES_COMMAND)));
        fake.print(&["fedora", "flathub"]);
        fake.end(Ok(()));
        assert_eq!(
            fake.ran.borrow().last(),
            install_commands(Installation::System, StoreApp::Steam).last()
        );
        fake.print(&["Installing 2/2…"]);
        assert_eq!(
            flow.state().step,
            Step::Installing {
                app: StoreApp::Steam,
                progress: 0.5,
                cancelling: false,
            }
        );
        fake.end(Ok(()));
        assert_eq!(flow.state().step, Step::Installed(StoreApp::Steam));
        assert_eq!(*installed.borrow(), [StoreApp::Steam]);
        let detect = fake.detected.borrow_mut().pop().expect("it looks again");
        detect(vec![StoreApp::Steam]);
        assert_eq!(flow.state().installed, [StoreApp::Steam]);
    }

    #[test]
    fn without_a_system_flathub_or_its_list_flathub_is_added_for_the_user_first() {
        for remotes in [Ok(()), Err("error: No system installation")] {
            let (flow, fake) = flow(Method::Flatpak);
            flow.choose(StoreApp::Heroic);
            fake.print(&["fedora"]);
            fake.end(remotes);
            let commands = install_commands(Installation::User, StoreApp::Heroic);
            assert_eq!(fake.ran.borrow().last(), commands.first());
            fake.end(Ok(()));
            assert_eq!(fake.ran.borrow().last(), commands.last());
            fake.end(Ok(()));
            assert_eq!(flow.state().step, Step::Installed(StoreApp::Heroic));
        }
    }

    #[test]
    fn a_failed_install_says_why_in_flatpaks_words() {
        let (flow, fake) = flow(Method::Flatpak);
        flow.choose(StoreApp::Steam);
        fake.print(&["flathub"]);
        fake.end(Ok(()));
        fake.end(Err(
            "Looking for matches…\nerror: Flatpak system operation Deploy not allowed for user\n",
        ));
        assert_eq!(
            flow.state().step,
            Step::Failed {
                app: StoreApp::Steam,
                error: "Flatpak system operation Deploy not allowed for user".to_owned(),
                network: false,
            }
        );
    }

    #[test]
    fn cancelling_stops_the_command_and_goes_back_to_the_stores() {
        let (flow, fake) = flow(Method::Flatpak);
        flow.choose(StoreApp::Lutris);
        fake.print(&[]);
        fake.end(Ok(()));
        flow.cancel();
        assert_eq!(fake.stops.get(), 1);
        assert_eq!(
            flow.state().step,
            Step::Installing {
                app: StoreApp::Lutris,
                progress: 0.0,
                cancelling: true,
            }
        );
        let ran = fake.ran.borrow().len();
        fake.end(Err("killed"));
        assert_eq!(flow.state().step, Step::Choose);
        assert_eq!(fake.ran.borrow().len(), ran, "nothing more runs");
    }

    #[test]
    fn the_flow_says_once_it_has_looked_for_the_stores_even_when_there_are_none() {
        let (flow, fake) = flow(Method::Flatpak);
        let heard = Rc::new(Cell::new(0));
        flow.connect_changed({
            let heard = heard.clone();
            move |_| heard.set(heard.get() + 1)
        });
        assert!(!flow.state().detected);
        flow.refresh();
        let detect = fake.detected.borrow_mut().pop().expect("it looks");
        detect(Vec::new());
        assert!(flow.state().detected);
        assert_eq!(heard.get(), 1, "none installed is news the first time");
        flow.refresh();
        let detect = fake.detected.borrow_mut().pop().expect("it looks again");
        detect(Vec::new());
        assert_eq!(heard.get(), 1, "and not again");
    }

    #[test]
    fn an_installed_store_or_one_chosen_while_another_installs_is_not_installed() {
        let (flow, fake) = flow(Method::Flatpak);
        flow.refresh();
        let detect = fake.detected.borrow_mut().pop().expect("it looks");
        detect(vec![StoreApp::Steam]);
        flow.choose(StoreApp::Steam);
        assert_eq!(flow.state().step, Step::Choose);
        assert!(fake.ran.borrow().is_empty());
        flow.choose(StoreApp::Lutris);
        flow.choose(StoreApp::Heroic);
        assert_eq!(fake.ran.borrow().len(), 1, "only Lutris's install runs");
    }

    #[test]
    fn done_goes_back_to_the_stores_but_not_while_one_installs() {
        let (flow, fake) = flow(Method::Flatpak);
        flow.choose(StoreApp::Steam);
        flow.dismiss();
        assert!(matches!(flow.state().step, Step::Installing { .. }));
        fake.print(&["flathub"]);
        fake.end(Ok(()));
        fake.end(Err("error: no network"));
        flow.dismiss();
        assert_eq!(flow.state().step, Step::Choose);
    }

    #[test]
    fn offline_no_store_can_be_chosen_until_the_network_is_back() {
        let (flow, fake) = flow(Method::Flatpak);
        let heard = Rc::new(Cell::new(0));
        flow.connect_changed({
            let heard = heard.clone();
            move |_| heard.set(heard.get() + 1)
        });
        flow.set_online(false);
        assert!(flow.state().offline);
        assert_eq!(heard.get(), 1);
        assert!(!flow.state().can_choose(StoreApp::Steam));
        flow.choose(StoreApp::Steam);
        assert_eq!(flow.state().step, Step::Choose);
        assert!(fake.ran.borrow().is_empty(), "nothing installs offline");
        flow.set_online(false);
        assert_eq!(heard.get(), 1, "no change, no news");
        flow.set_online(true);
        assert!(!flow.state().offline);
        flow.choose(StoreApp::Steam);
        assert!(matches!(flow.state().step, Step::Installing { .. }));
    }

    #[test]
    fn a_failed_install_that_could_not_reach_flathub_is_a_network_failure() {
        for (stderr, network) in [
            (
                "error: Unable to load summary from remote flathub: While fetching https://dl.flathub.org/repo/summary.idx: [6] Couldn't resolve host name",
                true,
            ),
            (
                "error: Unable to load summary from remote flathub: Could not resolve hostname",
                true,
            ),
            ("error: [7] Couldn't connect to server", true),
            ("error: Timeout was reached", true),
            (
                "error: Error resolving “dl.flathub.org”: Name or service not known",
                true,
            ),
            (
                "error: Error resolving “dl.flathub.org”: Temporary failure in name resolution",
                true,
            ),
            ("error: Network is unreachable", true),
            (
                "error: Flatpak system operation Deploy not allowed for user",
                false,
            ),
            ("error: No space left on device", false),
        ] {
            let (flow, fake) = flow(Method::Flatpak);
            flow.choose(StoreApp::Steam);
            fake.print(&["flathub"]);
            fake.end(Ok(()));
            fake.end(Err(stderr));
            let Step::Failed { network: found, .. } = flow.state().step else {
                panic!("{stderr}: the install failed");
            };
            assert_eq!(found, network, "{stderr}");
        }
    }

    #[test]
    fn an_install_that_fails_after_the_network_went_away_is_a_network_failure() {
        let (flow, fake) = flow(Method::Flatpak);
        flow.choose(StoreApp::Heroic);
        fake.print(&["flathub"]);
        fake.end(Ok(()));
        flow.set_online(false);
        fake.end(Err("error: something flatpak words in another language"));
        assert!(matches!(
            flow.state().step,
            Step::Failed { network: true, .. }
        ));
    }
}
