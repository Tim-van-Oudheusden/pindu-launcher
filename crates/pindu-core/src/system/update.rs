//! OS updates on Pindu OS, for the System Update entry of fullscreen mode's Quick Access:
//! the installed version, an update bootc has staged for the next start, and the previous
//! version to roll back to. `bootc status` needs root, so Pindu OS writes
//! `bootc status --format=json --format-version=1` to [`STATUS_FILE`] itself; the launcher
//! reads that file, starts [`ROLLBACK_UNIT`] to roll back, and restarts the machine. An
//! [`UpdateBackend`] does the reading, the unit and the restart; [`SystemUpdate`] decides
//! what shows and runs the flows over it.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use serde_json::Value;

/// Where Pindu OS writes `bootc status --format=json --format-version=1`, readable by
/// everyone, at boot, after every update check and after a rollback.
pub const STATUS_FILE: &str = "/run/pindu-os/update-status.json";

/// Where os-release is, in the order to look: `/etc/os-release` first, and
/// `/usr/lib/os-release` without it (os-release(5)).
pub const OS_RELEASE_FILES: [&str; 2] = ["/etc/os-release", "/usr/lib/os-release"];

/// The unit Pindu OS ships to run `bootc rollback` as root, which polkit lets the console's
/// user start, and nothing else through systemd.
pub const ROLLBACK_UNIT: &str = "pindu-os-rollback.service";

/// The os-release `ID` of Pindu OS.
const PINDU_OS_ID: &str = "pindu-os";

/// What os-release says that the System Update section needs.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OsRelease {
    /// The system's `ID`, such as `pindu-os` or `fedora`.
    pub id: Option<String>,
    /// The image's version (`IMAGE_VERSION`), such as `45.20261101.0`.
    pub image_version: Option<String>,
}

impl OsRelease {
    /// Reads the text of an os-release file.
    pub fn parse(text: &str) -> Self {
        let mut release = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            let value = Some(unquote(value.trim())).filter(|value| !value.is_empty());
            match key {
                "ID" => release.id = value,
                "IMAGE_VERSION" => release.image_version = value,
                _ => {}
            }
        }
        release
    }

    /// Whether the system is Pindu OS.
    pub fn is_pindu_os(&self) -> bool {
        self.id.as_deref() == Some(PINDU_OS_ID)
    }
}

/// An os-release value as a shell reads it: single quotes taken off, or double quotes
/// taken off with their backslash escapes undone.
fn unquote(value: &str) -> String {
    if let Some(inner) = value
        .strip_prefix('\'')
        .and_then(|value| value.strip_suffix('\''))
    {
        return inner.to_owned();
    }
    let Some(inner) = value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
    else {
        return value.to_owned();
    };
    let mut unescaped = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(char) = chars.next() {
        match char {
            '\\' => unescaped.extend(chars.next()),
            _ => unescaped.push(char),
        }
    }
    unescaped
}

/// A deployment bootc lists: the one booted, one staged for the next start, or the one
/// to roll back to.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Deployment {
    /// The image's version (its `org.opencontainers.image.version` label), when it has one.
    pub version: Option<String>,
}

/// What `bootc status --format=json --format-version=1` says about the deployments.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct BootcStatus {
    /// The deployment the machine runs.
    pub booted: Option<Deployment>,
    /// An update downloaded and staged, which the next start applies. A download-only one
    /// waits for `bootc upgrade --from-downloaded` rather than a restart, so it is left out.
    pub staged: Option<Deployment>,
    /// The previous deployment, which a rollback starts next time.
    pub rollback: Option<Deployment>,
    /// Whether a rollback is queued: the next start boots the rollback deployment.
    pub rollback_queued: bool,
}

/// Why a status file could not be read.
#[derive(Debug)]
pub enum StatusError {
    /// It is not JSON.
    Json(serde_json::Error),
    /// It has no `status` object.
    NoStatus,
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Json(error) => write!(f, "not JSON: {error}"),
            Self::NoStatus => f.write_str("no status object"),
        }
    }
}

impl std::error::Error for StatusError {}

impl BootcStatus {
    /// Reads the JSON `bootc status --format=json --format-version=1` prints.
    pub fn parse(json: &str) -> Result<Self, StatusError> {
        let host: Value = serde_json::from_str(json).map_err(StatusError::Json)?;
        let status = host
            .get("status")
            .filter(|status| status.is_object())
            .ok_or(StatusError::NoStatus)?;
        let deployment = |name: &str| {
            status
                .get(name)
                .filter(|entry| entry.is_object())
                .map(|entry| Deployment {
                    version: entry
                        .pointer("/image/version")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                })
        };
        let download_only = status
            .pointer("/staged/downloadOnly")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        Ok(Self {
            booted: deployment("booted"),
            staged: deployment("staged").filter(|_| !download_only),
            rollback: deployment("rollback"),
            rollback_queued: status
                .get("rollbackQueued")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        })
    }
}

/// Something the System Update level offers to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateAction {
    /// Restart into the staged update.
    RestartToUpdate,
    /// Roll back to the previous version, then restart into it.
    RollBack,
    /// Restart into the rollback already queued.
    Restart,
}

/// What runs on the user's say-so.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    /// The rollback unit runs.
    RollingBack,
    /// The machine was asked to restart.
    Restarting,
}

/// What the System Update entry and level show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateState {
    /// Whether the entry shows: on Pindu OS, once its status file exists.
    pub available: bool,
    /// The installed version: os-release's `IMAGE_VERSION`, or else the booted
    /// deployment's.
    pub installed: Option<String>,
    /// What bootc says, as the status file last had it.
    pub status: BootcStatus,
    /// What runs on the user's say-so, if anything.
    pub busy: Option<Busy>,
}

impl UpdateState {
    /// Whether an update waits for the next start, which the entry's subtitle says. A
    /// queued rollback discards it.
    pub fn update_ready(&self) -> bool {
        self.status.staged.is_some() && !self.status.rollback_queued
    }

    /// What the level offers, top to bottom: Restart with a rollback queued; otherwise
    /// Restart to Update with an update staged and Roll Back with a previous version.
    /// Nothing while an action runs.
    pub fn actions(&self) -> Vec<UpdateAction> {
        if !self.available || self.busy.is_some() {
            return Vec::new();
        }
        if self.status.rollback_queued {
            return vec![UpdateAction::Restart];
        }
        [
            self.update_ready().then_some(UpdateAction::RestartToUpdate),
            self.status
                .rollback
                .is_some()
                .then_some(UpdateAction::RollBack),
        ]
        .into_iter()
        .flatten()
        .collect()
    }
}

/// What the backend read: the text of os-release and of the status file, each `None`
/// when the file is not there.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Facts {
    /// The text of os-release.
    pub os_release: Option<String>,
    /// The text of [`STATUS_FILE`].
    pub status: Option<String>,
}

/// What a backend reports once an action ends: `Err` says why it did not run or failed.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;

/// Reads the facts and runs the actions of the System Update section: plain files, the
/// rollback unit through systemd and the restart through logind. Every callback runs on
/// the main thread.
pub trait UpdateBackend {
    /// Reads os-release and [`STATUS_FILE`], and reports what it found to `done`.
    fn read(&self, done: Box<dyn FnOnce(Facts)>);

    /// Calls `changed` whenever [`STATUS_FILE`] changes, appears or goes away.
    fn watch(&self, changed: Box<dyn Fn()>);

    /// Starts [`ROLLBACK_UNIT`] and reports to `done` once its job has finished.
    fn roll_back(&self, done: Done);

    /// Asks the system to restart, and reports to `done` if it refuses.
    fn restart(&self, done: Done);
}

/// Something that went wrong, for the shells to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The status file could not be read; the level shows no actions.
    Status(String),
    /// The rollback unit did not run, or failed.
    RollBack(String),
    /// The system refused to restart.
    Restart(String),
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&UpdateState)>;
/// Something a shell does when an action fails.
type FailureListener = Box<dyn Fn(&Failure)>;

/// The System Update section over its backend: reads the state, follows the status file
/// on Pindu OS, and runs Restart and Roll Back.
#[derive(Clone)]
pub struct SystemUpdate {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn UpdateBackend>,
    state: RefCell<UpdateState>,
    /// Whether the backend follows the status file already.
    watching: Cell<bool>,
    listeners: RefCell<Vec<Listener>>,
    failure_listeners: RefCell<Vec<FailureListener>>,
}

impl fmt::Debug for SystemUpdate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemUpdate")
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl SystemUpdate {
    /// The section over `backend`, hidden until the first [`refresh`](Self::refresh) reads
    /// it, so the shells can connect to it first.
    pub fn new(backend: Box<dyn UpdateBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                state: RefCell::default(),
                watching: Cell::new(false),
                listeners: RefCell::default(),
                failure_listeners: RefCell::default(),
            }),
        }
    }

    /// The state as last read.
    pub fn state(&self) -> UpdateState {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&UpdateState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever something goes wrong.
    pub fn connect_failed(&self, listener: impl Fn(&Failure) + 'static) {
        self.inner
            .failure_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Reads the state again: the level opens.
    pub fn refresh(&self) {
        let weak = self.downgrade();
        self.inner.backend.read(Box::new(move |facts| {
            if let Some(update) = upgrade(&weak) {
                update.take(facts);
            }
        }));
    }

    /// Restarts the machine, when the level offers Restart to Update or Restart.
    pub fn restart(&self) {
        let actions = self.inner.state.borrow().actions();
        if actions.contains(&UpdateAction::RestartToUpdate)
            || actions.contains(&UpdateAction::Restart)
        {
            self.set_busy(Some(Busy::Restarting));
            self.reboot();
        }
    }

    /// Rolls back to the previous version and then restarts, when the level offers it.
    /// The shell asks first.
    pub fn roll_back(&self) {
        if !self
            .inner
            .state
            .borrow()
            .actions()
            .contains(&UpdateAction::RollBack)
        {
            return;
        }
        self.set_busy(Some(Busy::RollingBack));
        let weak = self.downgrade();
        self.inner.backend.roll_back(Box::new(move |result| {
            let Some(update) = upgrade(&weak) else {
                return;
            };
            match result {
                Ok(()) => {
                    update.set_busy(Some(Busy::Restarting));
                    update.reboot();
                }
                Err(error) => {
                    update.set_busy(None);
                    update.fail(&Failure::RollBack(error));
                }
            }
        }));
    }

    /// Asks the backend to restart; a refusal offers the actions again.
    fn reboot(&self) {
        let weak = self.downgrade();
        self.inner.backend.restart(Box::new(move |result| {
            if let (Some(update), Err(error)) = (upgrade(&weak), result) {
                update.set_busy(None);
                update.fail(&Failure::Restart(error));
            }
        }));
    }

    /// Takes what the backend read, and follows the status file from the first read that
    /// finds Pindu OS on: it may appear only after the launcher started.
    fn take(&self, facts: Facts) {
        let release = OsRelease::parse(facts.os_release.as_deref().unwrap_or_default());
        let pindu_os = release.is_pindu_os();
        let status = match facts.status.as_deref().map(BootcStatus::parse) {
            Some(Ok(status)) => status,
            Some(Err(error)) => {
                if pindu_os {
                    self.fail(&Failure::Status(error.to_string()));
                }
                BootcStatus::default()
            }
            None => BootcStatus::default(),
        };
        if pindu_os && !self.inner.watching.replace(true) {
            let weak = self.downgrade();
            self.inner.backend.watch(Box::new(move || {
                if let Some(update) = upgrade(&weak) {
                    update.refresh();
                }
            }));
        }
        let installed = release.image_version.or_else(|| {
            status
                .booted
                .as_ref()
                .and_then(|booted| booted.version.clone())
        });
        let changed = {
            let mut state = self.inner.state.borrow_mut();
            let read = UpdateState {
                available: pindu_os && facts.status.is_some(),
                installed,
                status,
                busy: state.busy,
            };
            let changed = *state != read;
            *state = read;
            changed
        };
        if changed {
            self.changed();
        }
    }

    fn set_busy(&self, busy: Option<Busy>) {
        self.inner.state.borrow_mut().busy = busy;
        self.changed();
    }

    fn changed(&self) {
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
        }
    }

    fn fail(&self, failure: &Failure) {
        for listener in self.inner.failure_listeners.borrow().iter() {
            listener(failure);
        }
    }

    fn downgrade(&self) -> Weak<Inner> {
        Rc::downgrade(&self.inner)
    }
}

/// The section again from a backend's callback, unless it is gone.
fn upgrade(weak: &Weak<Inner>) -> Option<SystemUpdate> {
    weak.upgrade().map(|inner| SystemUpdate { inner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(path: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(path);
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
    }

    fn status(name: &str) -> BootcStatus {
        BootcStatus::parse(&fixture(&format!("bootc_status/{name}.json"))).unwrap()
    }

    fn version(version: &str) -> Option<Deployment> {
        Some(Deployment {
            version: Some(version.to_owned()),
        })
    }

    #[test]
    fn pindu_os_names_itself_and_its_image_version() {
        let release = OsRelease::parse(&fixture("os_release/pindu-os"));

        assert!(release.is_pindu_os());
        assert_eq!(release.image_version.as_deref(), Some("45.20261108.0"));
    }

    #[test]
    fn another_distribution_is_not_pindu_os_and_has_no_image_version() {
        let release = OsRelease::parse(&fixture("os_release/fedora"));

        assert!(!release.is_pindu_os());
        assert_eq!(release.id.as_deref(), Some("fedora"));
        assert_eq!(release.image_version, None);
    }

    #[test]
    fn os_release_values_lose_their_quotes_and_escapes() {
        let release = OsRelease::parse(
            "# a comment\nID=\"pindu-os\"\nIMAGE_VERSION='45.1'\nNAME=\"A \\\"quoted\\\" name\"\n",
        );

        assert!(release.is_pindu_os());
        assert_eq!(release.image_version.as_deref(), Some("45.1"));
        let escaped = OsRelease::parse("IMAGE_VERSION=\"45.\\$1\"");
        assert_eq!(escaped.image_version.as_deref(), Some("45.$1"));
    }

    #[test]
    fn an_empty_image_version_is_none() {
        assert_eq!(OsRelease::parse("IMAGE_VERSION=\n").image_version, None);
    }

    #[test]
    fn a_booted_system_without_updates_has_only_its_booted_deployment() {
        let status = status("booted-only");

        assert_eq!(status.booted, version("45.20261108.0"));
        assert_eq!(status.staged, None);
        assert_eq!(status.rollback, None);
        assert!(!status.rollback_queued);
    }

    #[test]
    fn a_staged_update_and_a_rollback_deployment_carry_their_versions() {
        let status = status("staged-and-rollback");

        assert_eq!(status.staged, version("45.20261115.1"));
        assert_eq!(status.booted, version("45.20261108.0"));
        assert_eq!(status.rollback, version("45.20261101.0"));
        assert!(!status.rollback_queued);
    }

    #[test]
    fn a_queued_rollback_is_read() {
        let status = status("rollback-queued");

        assert!(status.rollback_queued);
        assert_eq!(status.rollback, version("45.20261101.0"));
    }

    #[test]
    fn a_download_only_update_is_not_staged_for_the_next_start() {
        assert_eq!(status("staged-download-only").staged, None);
    }

    #[test]
    fn deployments_without_a_version_label_are_still_listed() {
        let status = status("no-versions");
        let unnamed = Some(Deployment { version: None });

        assert_eq!(status.booted, unnamed);
        assert_eq!(status.staged, unnamed);
        assert_eq!(status.rollback, unnamed);
    }

    #[test]
    fn a_file_that_is_not_bootc_status_is_an_error() {
        assert!(matches!(
            BootcStatus::parse("not json"),
            Err(StatusError::Json(_))
        ));
        assert!(matches!(
            BootcStatus::parse("{\"kind\": \"BootcHost\"}"),
            Err(StatusError::NoStatus)
        ));
    }

    /// A backend that answers reads with `facts`, records what it was asked to do and
    /// keeps each action's `Done` until the test finishes it.
    #[derive(Default)]
    struct Fake {
        facts: RefCell<Facts>,
        reads: Cell<usize>,
        watcher: RefCell<Option<Box<dyn Fn()>>>,
        watches: Cell<usize>,
        rollbacks: RefCell<Vec<Done>>,
        restarts: RefCell<Vec<Done>>,
    }

    struct FakeBackend(Rc<Fake>);

    impl UpdateBackend for FakeBackend {
        fn read(&self, done: Box<dyn FnOnce(Facts)>) {
            self.0.reads.set(self.0.reads.get() + 1);
            let facts = self.0.facts.borrow().clone();
            done(facts);
        }

        fn watch(&self, changed: Box<dyn Fn()>) {
            self.0.watches.set(self.0.watches.get() + 1);
            self.0.watcher.replace(Some(changed));
        }

        fn roll_back(&self, done: Done) {
            self.0.rollbacks.borrow_mut().push(done);
        }

        fn restart(&self, done: Done) {
            self.0.restarts.borrow_mut().push(done);
        }
    }

    impl Fake {
        fn finish_rollback(&self, result: Result<(), String>) {
            let done = self.rollbacks.borrow_mut().remove(0);
            done(result);
        }

        fn finish_restart(&self, result: Result<(), String>) {
            let done = self.restarts.borrow_mut().remove(0);
            done(result);
        }

        /// The status file changes to `status`.
        fn change_status(&self, status: Option<&str>) {
            self.facts.borrow_mut().status = status.map(fixture);
            let watcher = self.watcher.borrow();
            watcher.as_ref().expect("the status file is followed")();
        }
    }

    fn pindu_os(status: Option<&str>) -> Facts {
        Facts {
            os_release: Some(fixture("os_release/pindu-os")),
            status: status.map(fixture),
        }
    }

    fn section(facts: Facts) -> (SystemUpdate, Rc<Fake>, Rc<RefCell<Vec<Failure>>>) {
        let fake = Rc::new(Fake {
            facts: RefCell::new(facts),
            ..Fake::default()
        });
        let update = SystemUpdate::new(Box::new(FakeBackend(fake.clone())));
        let failures = Rc::new(RefCell::new(Vec::new()));
        update.connect_failed({
            let failures = failures.clone();
            move |failure| failures.borrow_mut().push(failure.clone())
        });
        update.refresh();
        (update, fake, failures)
    }

    const STAGED: &str = "bootc_status/staged.json";
    const STAGED_AND_ROLLBACK: &str = "bootc_status/staged-and-rollback.json";
    const ROLLBACK_QUEUED: &str = "bootc_status/rollback-queued.json";

    #[test]
    fn another_distribution_hides_the_section_and_follows_no_file() {
        let (update, fake, failures) = section(Facts {
            os_release: Some(fixture("os_release/fedora")),
            status: None,
        });

        assert!(!update.state().available);
        assert!(update.state().actions().is_empty());
        assert!(fake.watcher.borrow().is_none());
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn no_os_release_at_all_hides_the_section() {
        let (update, _, failures) = section(Facts::default());

        assert!(!update.state().available);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn pindu_os_shows_the_section_once_its_status_file_appears() {
        let (update, fake, _) = section(pindu_os(None));
        assert!(!update.state().available);
        let changes = Rc::new(Cell::new(0));
        update.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });

        fake.change_status(Some(STAGED));

        assert!(update.state().available);
        assert!(update.state().update_ready());
        assert_eq!(changes.get(), 1);
    }

    #[test]
    fn pindu_os_without_updates_shows_its_version_and_offers_nothing() {
        let (update, _, _) = section(pindu_os(Some("bootc_status/booted-only.json")));
        let state = update.state();

        assert!(state.available);
        assert_eq!(state.installed.as_deref(), Some("45.20261108.0"));
        assert!(!state.update_ready());
        assert!(state.actions().is_empty());
    }

    #[test]
    fn the_booted_version_stands_in_for_a_missing_image_version() {
        let (update, _, _) = section(Facts {
            os_release: Some("ID=pindu-os\n".to_owned()),
            status: Some(fixture(STAGED)),
        });

        assert_eq!(update.state().installed.as_deref(), Some("45.20261108.0"));
    }

    #[test]
    fn a_staged_update_offers_restart_to_update() {
        let (update, _, _) = section(pindu_os(Some(STAGED)));

        assert!(update.state().update_ready());
        assert_eq!(update.state().actions(), [UpdateAction::RestartToUpdate]);
    }

    #[test]
    fn a_previous_version_offers_roll_back_after_restart_to_update() {
        let (update, _, _) = section(pindu_os(Some(STAGED_AND_ROLLBACK)));

        assert_eq!(
            update.state().actions(),
            [UpdateAction::RestartToUpdate, UpdateAction::RollBack]
        );
    }

    #[test]
    fn a_queued_rollback_offers_only_restart() {
        let (update, _, _) = section(pindu_os(Some(ROLLBACK_QUEUED)));

        assert!(!update.state().update_ready());
        assert_eq!(update.state().actions(), [UpdateAction::Restart]);
    }

    #[test]
    fn a_broken_status_file_offers_nothing_and_is_reported() {
        let (update, _, failures) = section(Facts {
            os_release: Some(fixture("os_release/pindu-os")),
            status: Some("{".to_owned()),
        });

        assert!(update.state().available);
        assert_eq!(update.state().installed.as_deref(), Some("45.20261108.0"));
        assert!(update.state().actions().is_empty());
        assert!(matches!(failures.borrow().as_slice(), [Failure::Status(_)]));
    }

    #[test]
    fn refreshing_reads_the_files_again() {
        let (update, fake, _) = section(pindu_os(Some("bootc_status/booted-only.json")));
        fake.facts.borrow_mut().status = Some(fixture(STAGED));

        update.refresh();

        assert_eq!(fake.reads.get(), 2);
        assert!(update.state().update_ready());
    }

    #[test]
    fn restart_to_update_restarts_and_waits_for_the_machine_to_go_down() {
        let (update, fake, _) = section(pindu_os(Some(STAGED)));

        update.restart();

        assert_eq!(fake.restarts.borrow().len(), 1);
        assert_eq!(update.state().busy, Some(Busy::Restarting));
        assert!(update.state().actions().is_empty());
    }

    #[test]
    fn a_refused_restart_is_reported_and_offers_the_action_again() {
        let (update, fake, failures) = section(pindu_os(Some(STAGED)));
        update.restart();

        fake.finish_restart(Err("Operation inhibited".to_owned()));

        assert_eq!(update.state().busy, None);
        assert_eq!(update.state().actions(), [UpdateAction::RestartToUpdate]);
        assert_eq!(
            *failures.borrow(),
            [Failure::Restart("Operation inhibited".to_owned())]
        );
    }

    #[test]
    fn restart_does_nothing_when_nothing_waits_for_it() {
        let (update, fake, _) = section(pindu_os(Some("bootc_status/booted-only.json")));

        update.restart();

        assert!(fake.restarts.borrow().is_empty());
        assert_eq!(update.state().busy, None);
    }

    #[test]
    fn a_queued_rollback_restarts_into_it() {
        let (update, fake, _) = section(pindu_os(Some(ROLLBACK_QUEUED)));

        update.restart();

        assert_eq!(fake.restarts.borrow().len(), 1);
    }

    #[test]
    fn rolling_back_runs_the_unit_then_restarts() {
        let (update, fake, failures) = section(pindu_os(Some(STAGED_AND_ROLLBACK)));

        update.roll_back();
        assert_eq!(fake.rollbacks.borrow().len(), 1);
        assert!(fake.restarts.borrow().is_empty());
        assert_eq!(update.state().busy, Some(Busy::RollingBack));
        assert!(update.state().actions().is_empty());

        fake.finish_rollback(Ok(()));

        assert_eq!(fake.restarts.borrow().len(), 1);
        assert_eq!(update.state().busy, Some(Busy::Restarting));
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn a_failed_rollback_is_reported_and_does_not_restart() {
        let (update, fake, failures) = section(pindu_os(Some(STAGED_AND_ROLLBACK)));
        update.roll_back();

        fake.finish_rollback(Err("failed".to_owned()));

        assert!(fake.restarts.borrow().is_empty());
        assert_eq!(update.state().busy, None);
        assert_eq!(*failures.borrow(), [Failure::RollBack("failed".to_owned())]);
    }

    #[test]
    fn a_rollback_whose_restart_is_refused_leaves_the_rollback_queued_to_restart_into() {
        let (update, fake, failures) = section(pindu_os(Some(STAGED_AND_ROLLBACK)));
        update.roll_back();
        fake.finish_rollback(Ok(()));
        fake.change_status(Some(ROLLBACK_QUEUED));

        fake.finish_restart(Err("Operation inhibited".to_owned()));

        assert_eq!(update.state().busy, None);
        assert_eq!(update.state().actions(), [UpdateAction::Restart]);
        assert_eq!(
            *failures.borrow(),
            [Failure::Restart("Operation inhibited".to_owned())]
        );
    }

    #[test]
    fn rolling_back_needs_a_previous_version_and_no_queued_rollback() {
        let (update, fake, _) = section(pindu_os(Some(STAGED)));
        update.roll_back();
        let (queued, queued_fake, _) = section(pindu_os(Some(ROLLBACK_QUEUED)));
        queued.roll_back();

        assert!(fake.rollbacks.borrow().is_empty());
        assert!(queued_fake.rollbacks.borrow().is_empty());
    }

    #[test]
    fn a_second_press_while_rolling_back_starts_nothing_more() {
        let (update, fake, _) = section(pindu_os(Some(STAGED_AND_ROLLBACK)));

        update.roll_back();
        update.roll_back();
        update.restart();

        assert_eq!(fake.rollbacks.borrow().len(), 1);
        assert!(fake.restarts.borrow().is_empty());
    }

    #[test]
    fn the_status_file_is_followed_once_however_often_it_is_read() {
        let (update, fake, _) = section(pindu_os(Some(STAGED)));

        update.refresh();
        fake.change_status(Some(STAGED));

        assert_eq!(fake.watches.get(), 1);
    }
}
