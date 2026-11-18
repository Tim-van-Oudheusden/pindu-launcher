//! Suspend, restart and power off, for the Power entry of fullscreen mode's menu drawer,
//! through logind. Pindu OS boots into fullscreen mode with no desktop, so these are the
//! only way to turn the console off with a controller. A [`PowerBackend`] asks logind what
//! the session may do (`CanSuspend`, `CanReboot`, `CanPowerOff`) and does it; [`SystemPower`]
//! decides which entries show, by the shared rules of [`super::ServiceFacts`], and runs
//! them. [`asks_first`] says when the shell asks before it runs one, and a refusal by a
//! program holding a block inhibitor is reported with the programs' names.
//!
//! Natively the backend speaks to logind on the system bus. The Flatpak has none, so with
//! the host-spawn override it asks the host's logind through `busctl` ([`busctl`]), and
//! without it every action shows greyed out, saying why ([`Facts::sandboxed`]).
//!
//! The batteries are [`crate::power`]'s; this module is only about the machine's power
//! state.

pub mod busctl;

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use super::{Availability, Permission, ServiceFacts};

/// Something the Power level offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PowerAction {
    /// Suspend to RAM (logind's `Suspend`).
    Suspend,
    /// Restart (logind's `Reboot`).
    Restart,
    /// Power off (logind's `PowerOff`).
    PowerOff,
}

impl PowerAction {
    /// Every action, in the order the level lists them.
    pub const ALL: [Self; 3] = [Self::Suspend, Self::Restart, Self::PowerOff];

    /// What an inhibitor has to name to stop this action, as logind's `ListInhibitors`
    /// names it: `sleep` for Suspend, `shutdown` for Restart and Power Off.
    pub fn inhibited_by(self) -> &'static str {
        match self {
            Self::Suspend => "sleep",
            Self::Restart | Self::PowerOff => "shutdown",
        }
    }
}

/// Whether pressing `action` asks first: Restart and Power Off do while a game we started
/// is running, as its unsaved progress would be lost. Suspend never does: the game goes
/// on where it was when the machine wakes.
pub fn asks_first(action: PowerAction, game_running: bool) -> bool {
    game_running && action != PowerAction::Suspend
}

/// What the backend found out about logind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Facts {
    /// Whether the system bus can be reached: natively, or on the host through host spawn
    /// in the Flatpak.
    pub system_bus: bool,
    /// Whether logind has an owner on the system bus or can be activated.
    pub logind: bool,
    /// What `CanSuspend` answered.
    pub can_suspend: Permission,
    /// What `CanReboot` answered.
    pub can_reboot: Permission,
    /// What `CanPowerOff` answered.
    pub can_power_off: Permission,
    /// Whether the app runs in the Flatpak and may not run commands on the host, so it
    /// cannot ask logind at all.
    pub sandboxed: bool,
}

impl Facts {
    /// What logind answered for `action`.
    fn permission(&self, action: PowerAction) -> Permission {
        match action {
            PowerAction::Suspend => self.can_suspend,
            PowerAction::Restart => self.can_reboot,
            PowerAction::PowerOff => self.can_power_off,
        }
    }
}

/// An entry of the Power level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PowerEntry {
    /// What it does.
    pub action: PowerAction,
    /// Whether the system lets the session do it; when not, the entry shows insensitive
    /// and says so.
    pub allowed: bool,
}

/// What the Power entry and level show.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PowerState {
    /// The level's entries, top to bottom; none at all leaves them out of the drawer's
    /// Power level.
    pub entries: Vec<PowerEntry>,
    /// Whether the entries are greyed out because the app may not run commands on the
    /// host, which Preferences explains, rather than because the system does not allow
    /// them.
    pub sandboxed: bool,
}

impl PowerState {
    /// The state `facts` give: an entry for each action logind answers `yes`, `challenge`
    /// or `no` for, none without a system bus or logind, and every action greyed out in
    /// the Flatpak without host spawn, where logind cannot be asked.
    pub fn from_facts(facts: &Facts) -> Self {
        if facts.sandboxed {
            let entries = PowerAction::ALL.map(|action| PowerEntry {
                action,
                allowed: false,
            });
            return Self {
                entries: entries.to_vec(),
                sandboxed: true,
            };
        }
        let entries = PowerAction::ALL
            .into_iter()
            .filter_map(|action| {
                let service = ServiceFacts {
                    system_bus: facts.system_bus,
                    service: facts.logind,
                    hardware: true,
                    permission: facts.permission(action),
                };
                match service.availability() {
                    Availability::Hidden => None,
                    Availability::Denied => Some(PowerEntry {
                        action,
                        allowed: false,
                    }),
                    Availability::Shown => Some(PowerEntry {
                        action,
                        allowed: true,
                    }),
                }
            })
            .collect();
        Self {
            entries,
            sandboxed: false,
        }
    }

    /// Whether the level lists Suspend, Restart or Power Off.
    pub fn shown(&self) -> bool {
        !self.entries.is_empty()
    }

    /// Whether the level offers `action`, allowed.
    pub fn allows(&self, action: PowerAction) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.action == action && entry.allowed)
    }
}

/// A lock a program holds on the machine's sleep or shutdown, as logind's `ListInhibitors`
/// lists it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Inhibitor {
    /// What it stops, colon-separated, such as `shutdown:sleep`.
    pub what: String,
    /// Who holds it: a program's name, such as `GNOME Software`.
    pub who: String,
    /// Why it holds it.
    pub why: String,
    /// `block`, which stops the action, or `delay`, which only holds it off for a moment.
    pub mode: String,
}

impl Inhibitor {
    /// Whether it stops `action`: a `block` lock on what the action needs.
    pub fn blocks(&self, action: PowerAction) -> bool {
        let needed = action.inhibited_by();
        self.mode == "block" && self.what.split(':').any(|what| what == needed)
    }
}

/// What a backend reports once an action ends: `Err` says why logind refused it.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;

/// Asks logind what the session may do and does it. Every callback runs on the main
/// thread.
pub trait PowerBackend {
    /// Asks logind whether the session can suspend, restart and power off, and reports
    /// what it found to `done`. Without a system bus or logind it reports so, and touches
    /// nothing.
    fn read(&self, done: Box<dyn FnOnce(Facts)>);

    /// Calls `changed` whenever logind gets or loses its owner on the system bus
    /// (`NameOwnerChanged`).
    fn watch(&self, changed: Box<dyn Fn()>);

    /// Asks logind to do `action`, letting polkit ask where the session has an agent for
    /// it, and reports to `done`.
    fn run(&self, action: PowerAction, done: Done);

    /// Lists logind's inhibitors to `done`; none when it cannot.
    fn inhibitors(&self, done: Box<dyn FnOnce(Vec<Inhibitor>)>);
}

/// Something that went wrong, for the shells to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// Programs hold a block inhibitor that stops the action; they are named, each once.
    Inhibited {
        /// What was refused.
        action: PowerAction,
        /// Who holds the inhibitors, in logind's order.
        holders: Vec<String>,
    },
    /// logind refused the action for another reason, or could not be asked.
    Refused {
        /// What was refused.
        action: PowerAction,
        /// Why, as logind said it.
        error: String,
    },
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&PowerState)>;
/// Something a shell does when an action fails.
type FailureListener = Box<dyn Fn(&Failure)>;

/// The Power section over its backend: reads what logind allows, follows logind coming
/// and going, and runs Suspend, Restart and Power Off.
#[derive(Clone)]
pub struct SystemPower {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn PowerBackend>,
    state: RefCell<PowerState>,
    /// The action logind was asked for and has not answered yet.
    running: Cell<Option<PowerAction>>,
    /// Whether the backend follows logind on the bus already.
    watching: Cell<bool>,
    listeners: RefCell<Vec<Listener>>,
    failure_listeners: RefCell<Vec<FailureListener>>,
}

impl fmt::Debug for SystemPower {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SystemPower")
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl SystemPower {
    /// The section over `backend`, hidden until the first [`refresh`](Self::refresh)
    /// reads it, so the shells can connect to it first.
    pub fn new(backend: Box<dyn PowerBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                state: RefCell::default(),
                running: Cell::new(None),
                watching: Cell::new(false),
                listeners: RefCell::default(),
                failure_listeners: RefCell::default(),
            }),
        }
    }

    /// The state as last read.
    pub fn state(&self) -> PowerState {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&PowerState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever logind refuses an action.
    pub fn connect_failed(&self, listener: impl Fn(&Failure) + 'static) {
        self.inner
            .failure_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Reads what logind allows again: fullscreen mode starts, or logind came or went.
    pub fn refresh(&self) {
        let weak = self.downgrade();
        self.inner.backend.read(Box::new(move |facts| {
            if let Some(power) = upgrade(&weak) {
                power.take(&facts);
            }
        }));
    }

    /// Runs `action` when the level offers it allowed, and nothing runs yet. The shell
    /// asks first where [`asks_first`] says so.
    pub fn run(&self, action: PowerAction) {
        if self.inner.running.get().is_some() || !self.inner.state.borrow().allows(action) {
            return;
        }
        self.inner.running.set(Some(action));
        let weak = self.downgrade();
        self.inner.backend.run(
            action,
            Box::new(move |result| {
                let Some(power) = upgrade(&weak) else {
                    return;
                };
                match result {
                    Ok(()) => power.inner.running.set(None),
                    Err(error) => power.refused(action, error),
                }
            }),
        );
    }

    /// logind refused `action`: names the programs whose block inhibitors stop it, or
    /// else passes on logind's reason.
    fn refused(&self, action: PowerAction, error: String) {
        let weak = self.downgrade();
        self.inner.backend.inhibitors(Box::new(move |inhibitors| {
            let Some(power) = upgrade(&weak) else {
                return;
            };
            power.inner.running.set(None);
            let mut holders: Vec<String> = Vec::new();
            for held in inhibitors.iter().filter(|held| held.blocks(action)) {
                if !holders.contains(&held.who) {
                    holders.push(held.who.clone());
                }
            }
            let failure = if holders.is_empty() {
                Failure::Refused { action, error }
            } else {
                Failure::Inhibited { action, holders }
            };
            for listener in power.inner.failure_listeners.borrow().iter() {
                listener(&failure);
            }
        }));
    }

    /// Takes what the backend read, and follows logind from the first read that reaches
    /// the system bus.
    fn take(&self, facts: &Facts) {
        if facts.system_bus && !self.inner.watching.replace(true) {
            let weak = self.downgrade();
            self.inner.backend.watch(Box::new(move || {
                if let Some(power) = upgrade(&weak) {
                    power.refresh();
                }
            }));
        }
        let read = PowerState::from_facts(facts);
        if *self.inner.state.borrow() == read {
            return;
        }
        self.inner.state.replace(read);
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
        }
    }

    fn downgrade(&self) -> Weak<Inner> {
        Rc::downgrade(&self.inner)
    }
}

/// The section again from a backend's callback, unless it is gone.
fn upgrade(weak: &Weak<Inner>) -> Option<SystemPower> {
    weak.upgrade().map(|inner| SystemPower { inner })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A backend that answers reads with `facts` and lists `inhibitors`, records what it
    /// was asked to do, and keeps each action's `Done` until the test finishes it.
    #[derive(Default)]
    struct Fake {
        facts: RefCell<Facts>,
        reads: Cell<usize>,
        watcher: RefCell<Option<Box<dyn Fn()>>>,
        watches: Cell<usize>,
        runs: RefCell<Vec<(PowerAction, Done)>>,
        inhibitors: RefCell<Vec<Inhibitor>>,
    }

    struct FakeBackend(Rc<Fake>);

    impl PowerBackend for FakeBackend {
        fn read(&self, done: Box<dyn FnOnce(Facts)>) {
            self.0.reads.set(self.0.reads.get() + 1);
            let facts = *self.0.facts.borrow();
            done(facts);
        }

        fn watch(&self, changed: Box<dyn Fn()>) {
            self.0.watches.set(self.0.watches.get() + 1);
            self.0.watcher.replace(Some(changed));
        }

        fn run(&self, action: PowerAction, done: Done) {
            self.0.runs.borrow_mut().push((action, done));
        }

        fn inhibitors(&self, done: Box<dyn FnOnce(Vec<Inhibitor>)>) {
            let inhibitors = self.0.inhibitors.borrow().clone();
            done(inhibitors);
        }
    }

    impl Fake {
        /// What the backend was asked to run, oldest first.
        fn asked(&self) -> Vec<PowerAction> {
            let runs = self.runs.borrow();
            runs.iter().map(|(action, _)| *action).collect()
        }

        /// logind answers the oldest action asked for.
        fn finish(&self, result: Result<(), String>) {
            let (_, done) = self.runs.borrow_mut().remove(0);
            done(result);
        }

        /// logind comes or goes on the bus, now answering `facts`.
        fn owner_changed(&self, facts: Facts) {
            self.facts.replace(facts);
            let watcher = self.watcher.borrow();
            watcher.as_ref().expect("logind is followed")();
        }
    }

    /// logind on the system bus, answering `answer` for every action.
    fn logind(answer: Permission) -> Facts {
        Facts {
            system_bus: true,
            logind: true,
            can_suspend: answer,
            can_reboot: answer,
            can_power_off: answer,
            ..Facts::default()
        }
    }

    fn section(facts: Facts) -> (SystemPower, Rc<Fake>, Rc<RefCell<Vec<Failure>>>) {
        let fake = Rc::new(Fake {
            facts: RefCell::new(facts),
            ..Fake::default()
        });
        let power = SystemPower::new(Box::new(FakeBackend(fake.clone())));
        let failures = Rc::new(RefCell::new(Vec::new()));
        power.connect_failed({
            let failures = failures.clone();
            move |failure| failures.borrow_mut().push(failure.clone())
        });
        power.refresh();
        (power, fake, failures)
    }

    fn entry(action: PowerAction, allowed: bool) -> PowerEntry {
        PowerEntry { action, allowed }
    }

    fn inhibitor(what: &str, who: &str, mode: &str) -> Inhibitor {
        Inhibitor {
            what: what.to_owned(),
            who: who.to_owned(),
            why: "Working".to_owned(),
            mode: mode.to_owned(),
        }
    }

    // What shows.

    #[test]
    fn the_section_is_hidden_until_logind_is_read() {
        let power = SystemPower::new(Box::new(FakeBackend(Rc::default())));

        assert!(!power.state().shown());
        assert!(power.state().entries.is_empty());
        assert!(format!("{power:?}").starts_with("SystemPower"));
    }

    #[test]
    fn the_flatpak_without_a_system_bus_has_no_power_entry_and_follows_nothing() {
        let (power, fake, failures) = section(Facts::default());

        assert!(!power.state().shown());
        assert_eq!(fake.watches.get(), 0);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn without_logind_there_is_no_power_entry() {
        let (power, _, _) = section(Facts {
            logind: false,
            ..logind(Permission::Yes)
        });

        assert!(!power.state().shown());
    }

    #[test]
    fn suspend_restart_and_power_off_show_in_that_order_when_allowed() {
        let (power, _, _) = section(logind(Permission::Yes));

        assert!(power.state().shown());
        assert_eq!(
            power.state().entries,
            [
                entry(PowerAction::Suspend, true),
                entry(PowerAction::Restart, true),
                entry(PowerAction::PowerOff, true),
            ]
        );
    }

    #[test]
    fn challenge_counts_as_allowed() {
        let (power, _, _) = section(logind(Permission::Challenge));

        for action in PowerAction::ALL {
            assert!(power.state().allows(action), "{action:?}");
        }
    }

    #[test]
    fn no_leaves_the_entry_not_allowed_and_na_hides_it() {
        let (power, _, _) = section(Facts {
            can_suspend: Permission::Na,
            can_reboot: Permission::No,
            ..logind(Permission::Yes)
        });

        assert_eq!(
            power.state().entries,
            [
                entry(PowerAction::Restart, false),
                entry(PowerAction::PowerOff, true),
            ]
        );
        assert!(!power.state().allows(PowerAction::Restart));
    }

    #[test]
    fn every_action_not_possible_hides_the_power_entry() {
        let (power, _, _) = section(logind(Permission::Na));

        assert!(!power.state().shown());
    }

    #[test]
    fn every_action_not_allowed_still_shows_the_power_entry() {
        let (power, _, _) = section(logind(Permission::No));

        assert!(power.state().shown());
        assert!(power.state().entries.iter().all(|entry| !entry.allowed));
    }

    #[test]
    fn the_flatpak_without_host_spawn_greys_out_every_action_saying_why() {
        let (power, fake, failures) = section(Facts {
            sandboxed: true,
            ..Facts::default()
        });

        assert!(power.state().shown());
        assert_eq!(
            power.state().entries,
            PowerAction::ALL.map(|action| entry(action, false))
        );
        assert!(power.state().sandboxed);
        power.run(PowerAction::PowerOff);
        assert!(fake.asked().is_empty());
        assert_eq!(fake.watches.get(), 0);
        assert!(failures.borrow().is_empty());
    }

    #[test]
    fn actions_logind_does_not_allow_are_not_greyed_out_for_the_sandbox() {
        let (power, _, _) = section(logind(Permission::No));

        assert!(!power.state().sandboxed);
    }

    #[test]
    fn logind_coming_and_going_reads_it_again_and_tells_the_shells() {
        let (power, fake, _) = section(logind(Permission::Yes));
        let changes = Rc::new(Cell::new(0));
        power.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });

        fake.owner_changed(Facts {
            logind: false,
            ..logind(Permission::Yes)
        });
        assert!(!power.state().shown());
        fake.owner_changed(logind(Permission::Yes));

        assert!(power.state().shown());
        assert_eq!(fake.reads.get(), 3);
        assert_eq!(changes.get(), 2);
    }

    #[test]
    fn a_read_that_changes_nothing_tells_no_one() {
        let (power, _, _) = section(logind(Permission::Yes));
        let changes = Rc::new(Cell::new(0));
        power.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });

        power.refresh();

        assert_eq!(changes.get(), 0);
    }

    #[test]
    fn logind_is_followed_once_however_often_it_is_read() {
        let (power, fake, _) = section(logind(Permission::Yes));

        power.refresh();
        power.refresh();

        assert_eq!(fake.watches.get(), 1);
    }

    // Running an action.

    #[test]
    fn each_allowed_action_asks_logind() {
        for action in PowerAction::ALL {
            let (power, fake, failures) = section(logind(Permission::Yes));

            power.run(action);
            assert_eq!(fake.asked(), [action]);
            fake.finish(Ok(()));

            assert!(fake.asked().is_empty());
            assert!(failures.borrow().is_empty(), "{action:?}");
        }
    }

    #[test]
    fn an_action_not_allowed_or_not_offered_does_nothing() {
        let (power, fake, _) = section(Facts {
            can_suspend: Permission::Na,
            can_reboot: Permission::No,
            ..logind(Permission::Yes)
        });

        power.run(PowerAction::Suspend);
        power.run(PowerAction::Restart);

        assert!(fake.asked().is_empty());
    }

    #[test]
    fn nothing_runs_in_the_flatpak() {
        let (power, fake, _) = section(Facts::default());

        power.run(PowerAction::PowerOff);

        assert!(fake.asked().is_empty());
    }

    #[test]
    fn a_second_press_while_logind_has_not_answered_does_nothing() {
        let (power, fake, _) = section(logind(Permission::Yes));

        power.run(PowerAction::Restart);
        power.run(PowerAction::Restart);
        power.run(PowerAction::Suspend);

        assert_eq!(fake.asked(), [PowerAction::Restart]);
    }

    #[test]
    fn once_logind_has_answered_another_action_can_run() {
        let (power, fake, _) = section(logind(Permission::Yes));
        power.run(PowerAction::Suspend);
        fake.finish(Ok(()));

        power.run(PowerAction::Suspend);

        assert_eq!(fake.asked(), [PowerAction::Suspend]);
    }

    #[test]
    fn a_refusal_by_a_block_inhibitor_names_who_holds_it_once() {
        let (power, fake, failures) = section(logind(Permission::Yes));
        let inhibitors = vec![
            inhibitor("shutdown", "GNOME Software", "block"),
            inhibitor("sleep", "Steam", "block"),
            inhibitor("shutdown:sleep", "Steam", "delay"),
            inhibitor("idle:shutdown", "Dolphin", "block"),
            inhibitor("shutdown:sleep", "GNOME Software", "block"),
        ];
        fake.inhibitors.replace(inhibitors);

        power.run(PowerAction::PowerOff);
        fake.finish(Err("Operation inhibited".to_owned()));

        let expected = Failure::Inhibited {
            action: PowerAction::PowerOff,
            holders: vec!["GNOME Software".to_owned(), "Dolphin".to_owned()],
        };
        assert_eq!(*failures.borrow(), [expected]);
    }

    #[test]
    fn suspend_is_stopped_by_sleep_inhibitors_only() {
        let (power, fake, failures) = section(logind(Permission::Yes));
        let inhibitors = vec![
            inhibitor("shutdown", "GNOME Software", "block"),
            inhibitor("sleep", "Steam", "block"),
        ];
        fake.inhibitors.replace(inhibitors);

        power.run(PowerAction::Suspend);
        fake.finish(Err("Operation inhibited".to_owned()));

        let expected = Failure::Inhibited {
            action: PowerAction::Suspend,
            holders: vec!["Steam".to_owned()],
        };
        assert_eq!(*failures.borrow(), [expected]);
    }

    #[test]
    fn a_refusal_without_a_blocking_inhibitor_passes_on_logind_s_reason() {
        let (power, fake, failures) = section(logind(Permission::Yes));
        let delay = inhibitor("shutdown", "Steam", "delay");
        fake.inhibitors.replace(vec![delay]);

        power.run(PowerAction::Restart);
        fake.finish(Err("Access denied".to_owned()));

        let expected = Failure::Refused {
            action: PowerAction::Restart,
            error: "Access denied".to_owned(),
        };
        assert_eq!(*failures.borrow(), [expected]);
    }

    #[test]
    fn after_a_refusal_the_action_can_be_pressed_again() {
        let (power, fake, _) = section(logind(Permission::Yes));
        power.run(PowerAction::Restart);
        fake.finish(Err("Operation inhibited".to_owned()));

        power.run(PowerAction::Restart);

        assert_eq!(fake.asked(), [PowerAction::Restart]);
    }

    #[test]
    fn an_answer_after_the_section_is_gone_is_dropped() {
        let (power, fake, failures) = section(logind(Permission::Yes));
        power.run(PowerAction::Restart);
        power.run(PowerAction::PowerOff);
        drop(power);

        fake.finish(Err("Operation inhibited".to_owned()));

        assert!(failures.borrow().is_empty());
    }

    // Asking first.

    #[test]
    fn restart_and_power_off_ask_first_while_a_game_we_started_runs() {
        assert!(asks_first(PowerAction::Restart, true));
        assert!(asks_first(PowerAction::PowerOff, true));
    }

    #[test]
    fn suspend_never_asks_and_nothing_asks_without_a_game() {
        assert!(!asks_first(PowerAction::Suspend, true));
        for action in PowerAction::ALL {
            assert!(!asks_first(action, false), "{action:?}");
        }
    }

    #[test]
    fn inhibitors_stop_what_logind_names_for_each_action() {
        assert_eq!(PowerAction::Suspend.inhibited_by(), "sleep");
        assert_eq!(PowerAction::Restart.inhibited_by(), "shutdown");
        assert_eq!(PowerAction::PowerOff.inhibited_by(), "shutdown");
        let power_key = inhibitor("handle-power-key", "logind", "block");
        assert!(!power_key.blocks(PowerAction::PowerOff));
    }
}
