//! The set-up checklist (#794): what a new console needs before its first game, in the
//! order it needs it, shown in fullscreen mode in place of the empty Home. Each entry
//! leads to the screen that already does the step (Quick Access's Bluetooth, Network and
//! Design, Add a Store), and is marked done from the live state.

use crate::add_store::StoreApp;

/// Where the checklist is, kept from one start to the next.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SetupState {
    /// Fullscreen mode has not started with nothing set up yet.
    #[default]
    Unseen,
    /// It shows in place of the empty Home.
    Active,
    /// The owner hid it; Quick Access can bring it back.
    Dismissed,
    /// A game is in the library: it is gone for good.
    Done,
}

impl SetupState {
    /// The name it is kept under.
    pub fn name(self) -> &'static str {
        match self {
            Self::Unseen => "unseen",
            Self::Active => "active",
            Self::Dismissed => "dismissed",
            Self::Done => "done",
        }
    }

    /// The state kept under `name` (see [`Self::name`]), if one is.
    pub fn from_name(name: &str) -> Option<Self> {
        [Self::Unseen, Self::Active, Self::Dismissed, Self::Done]
            .into_iter()
            .find(|state| state.name() == name)
    }

    /// Fullscreen mode started, the library having `games` or not and a store being
    /// installed (`store`) or not: with nothing set up, the checklist starts.
    #[must_use]
    pub fn started(self, games: bool, store: bool) -> Self {
        match self {
            Self::Unseen if !games && !store => Self::Active,
            state => state,
        }
    }

    /// The library changed: a game in it ends the checklist for good.
    #[must_use]
    pub fn library_changed(self, games: bool) -> Self {
        if games { Self::Done } else { self }
    }

    /// The owner hid the checklist.
    #[must_use]
    pub fn dismissed(self) -> Self {
        match self {
            Self::Active => Self::Dismissed,
            state => state,
        }
    }

    /// Quick Access brought the hidden checklist back.
    #[must_use]
    pub fn restored(self) -> Self {
        match self {
            Self::Dismissed => Self::Active,
            state => state,
        }
    }

    /// Whether the checklist shows in place of the empty Home.
    pub fn shows(self) -> bool {
        self == Self::Active
    }

    /// Whether Quick Access offers to bring the checklist back.
    pub fn restorable(self) -> bool {
        self == Self::Dismissed
    }
}

/// What decides the checklist's entries and which of them are done, as it is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupFacts {
    /// A controller is connected.
    pub controller: bool,
    /// The system panel offers Bluetooth (Pindu OS), where a wireless controller pairs.
    pub bluetooth: bool,
    /// The system panel offers the network (Pindu OS).
    pub network: bool,
    /// The machine is online.
    pub online: bool,
    /// The owner picked a fullscreen design, so it is not the one it came with.
    pub design_changed: bool,
    /// The stores installed, in the order Add a Store lists them.
    pub stores: Vec<StoreApp>,
}

/// An entry of the checklist, and the screen it leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupEntry {
    /// Connect a controller: Quick Access's Bluetooth where the system panel offers it
    /// (`bluetooth`), else its Controller section. A cable or a keyboard gets this far.
    Controller {
        /// The system panel offers Bluetooth.
        bluetooth: bool,
    },
    /// Join a network: Quick Access's Network.
    Network,
    /// Pick the fullscreen design: Quick Access's Design section.
    Design,
    /// Install a store: Add a Store.
    Store,
    /// Open the installed store and sign in to it; `None` until one is installed.
    SignIn(Option<StoreApp>),
}

/// An entry and whether it is done.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupItem {
    /// The entry.
    pub entry: SetupEntry,
    /// Whether the live state says it is done.
    pub done: bool,
}

/// The checklist's entries for `facts`, in the order a new console needs them: a
/// controller, the network where the system panel offers it, the design until it was
/// changed, a store, then signing in to it. Signing in is never marked done: it shows as
/// games in the library, which end the checklist.
pub fn checklist(facts: &SetupFacts) -> Vec<SetupItem> {
    let store = facts
        .stores
        .iter()
        .copied()
        .find(|&store| store == StoreApp::Steam)
        .or_else(|| facts.stores.first().copied());
    [
        Some((
            SetupEntry::Controller {
                bluetooth: facts.bluetooth,
            },
            facts.controller,
        )),
        facts.network.then_some((SetupEntry::Network, facts.online)),
        (!facts.design_changed).then_some((SetupEntry::Design, false)),
        Some((SetupEntry::Store, store.is_some())),
        Some((SetupEntry::SignIn(store), false)),
    ]
    .into_iter()
    .flatten()
    .map(|(entry, done)| SetupItem { entry, done })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fullscreen_start_with_nothing_set_up_starts_the_checklist_and_a_game_ends_it() {
        use SetupState::*;
        assert_eq!(Unseen.started(false, false), Active);
        assert_eq!(Unseen.started(true, false), Unseen, "a library with games");
        assert_eq!(Unseen.started(false, true), Unseen, "a store already there");
        assert_eq!(Dismissed.started(false, false), Dismissed);
        assert_eq!(Done.started(false, false), Done);
        for state in [Unseen, Active, Dismissed, Done] {
            assert_eq!(state.library_changed(true), Done, "{state:?}");
        }
        assert_eq!(Active.library_changed(false), Active);
        assert_eq!(Active.dismissed(), Dismissed);
        assert_eq!(Dismissed.restored(), Active);
        assert_eq!(Done.restored(), Done, "gone for good");
        assert!(Active.shows());
        assert!(!Dismissed.shows());
        assert!(Dismissed.restorable());
        assert!(!Done.restorable());
        for state in [Unseen, Active, Dismissed, Done] {
            assert_eq!(SetupState::from_name(state.name()), Some(state));
        }
        assert_eq!(SetupState::from_name("other"), None);
    }

    /// A new console: nothing connected, offline, the design as it came, no store, and
    /// the system panel offering Bluetooth and the network.
    fn fresh() -> SetupFacts {
        SetupFacts {
            controller: false,
            bluetooth: true,
            network: true,
            online: false,
            design_changed: false,
            stores: Vec::new(),
        }
    }

    fn entries(items: &[SetupItem]) -> Vec<SetupEntry> {
        items.iter().map(|item| item.entry).collect()
    }

    #[test]
    fn a_new_console_lists_controller_network_design_store_and_sign_in_in_that_order() {
        use SetupEntry::*;
        let items = checklist(&fresh());
        assert_eq!(
            entries(&items),
            [
                Controller { bluetooth: true },
                Network,
                Design,
                Store,
                SignIn(None),
            ]
        );
        assert!(items.iter().all(|item| !item.done), "nothing is done yet");
    }

    #[test]
    fn an_entry_that_does_not_apply_stays_out() {
        use SetupEntry::*;
        let facts = SetupFacts {
            bluetooth: false,
            network: false,
            design_changed: true,
            ..fresh()
        };
        assert_eq!(
            entries(&checklist(&facts)),
            [Controller { bluetooth: false }, Store, SignIn(None)],
            "no network section, and the design was changed"
        );
    }

    #[test]
    fn each_entry_is_done_from_the_live_state() {
        use SetupEntry::*;
        let facts = SetupFacts {
            controller: true,
            online: true,
            stores: vec![StoreApp::Steam],
            ..fresh()
        };
        let done: Vec<(SetupEntry, bool)> = checklist(&facts)
            .into_iter()
            .map(|item| (item.entry, item.done))
            .collect();
        assert_eq!(
            done,
            [
                (Controller { bluetooth: true }, true),
                (Network, true),
                (Design, false),
                (Store, true),
                (SignIn(Some(StoreApp::Steam)), false),
            ],
            "signing in shows only as games in the library, which end the list"
        );
    }

    #[test]
    fn sign_in_opens_steam_when_it_is_installed_else_the_first_store() {
        let sign_in = |stores: Vec<StoreApp>| {
            checklist(&SetupFacts { stores, ..fresh() })
                .last()
                .map(|item| item.entry)
        };
        assert_eq!(
            sign_in(vec![StoreApp::Lutris, StoreApp::Steam]),
            Some(SetupEntry::SignIn(Some(StoreApp::Steam)))
        );
        assert_eq!(
            sign_in(vec![StoreApp::Lutris, StoreApp::Heroic]),
            Some(SetupEntry::SignIn(Some(StoreApp::Lutris)))
        );
    }
}
