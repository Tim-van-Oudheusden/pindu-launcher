//! The Proton row of the game dialog, the game page and Preferences: a first entry for
//! "no build named", then the builds on this machine, newest first. A build that is gone
//! stays listed, so saving keeps it.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::proton::ProtonBuild;

use crate::proton_launch;

/// What the first entry of the list stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FirstChoice {
    /// The newest build at each start: the default in Preferences.
    Newest,
    /// The default from Preferences: a game's choice.
    Default,
}

/// A Proton combo row and the builds it lists.
#[derive(Debug)]
pub struct ProtonChoice {
    row: adw::ComboRow,
    list: gtk::StringList,
    first: FirstChoice,
    /// The listed builds by internal name, `None` for the first entry; empty until read.
    names: RefCell<Vec<Option<String>>>,
    /// The build to select once the list is read.
    pending: RefCell<Option<String>>,
    /// Set while the list changes the selection itself, so only the user's choices count.
    filling: Cell<bool>,
}

impl ProtonChoice {
    /// Fills `row`, whose model must be a `GtkStringList`, with the builds on this
    /// machine, and selects `selected` once they are read.
    pub fn new(row: &adw::ComboRow, first: FirstChoice, selected: Option<String>) -> Rc<Self> {
        let list = row
            .model()
            .and_downcast::<gtk::StringList>()
            .expect("the Proton row lists strings");
        let choice = Rc::new(Self {
            row: row.clone(),
            list,
            first,
            names: RefCell::default(),
            pending: RefCell::new(selected),
            filling: Cell::new(false),
        });
        choice.read_builds();
        choice
    }

    /// Reads the builds on this machine again, after one was installed or removed,
    /// keeping the choice.
    pub fn reload(self: &Rc<Self>) {
        let selected = self.selected();
        self.names.borrow_mut().clear();
        self.pending.replace(selected);
        self.read_builds();
    }

    fn read_builds(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let builds = gio::spawn_blocking(proton_launch::builds)
                .await
                .unwrap_or_default();
            if let Some(choice) = weak.upgrade() {
                choice.fill(builds);
            }
        });
    }

    fn fill(&self, builds: Vec<ProtonBuild>) {
        let name_of = |name: &str| {
            builds
                .iter()
                .find(|build| build.name == name)
                .map_or_else(|| name.to_owned(), |build| build.display_name.clone())
        };
        let shown = match self.first {
            FirstChoice::Newest => None,
            FirstChoice::Default => proton_launch::defaults().proton_version,
        };
        let proton = match shown {
            Some(name) => Some(name_of(&name)),
            None => builds.first().map(|build| build.display_name.clone()),
        };
        let first = match (self.first, proton) {
            (FirstChoice::Newest, Some(proton)) => {
                gettext("Newest ({proton})").replace("{proton}", &proton)
            }
            (FirstChoice::Newest, None) => gettext("Newest"),
            (FirstChoice::Default, Some(proton)) => {
                gettext("Default ({proton})").replace("{proton}", &proton)
            }
            (FirstChoice::Default, None) => gettext("Default"),
        };
        self.row.set_subtitle(&if builds.is_empty() {
            gettext(
                "No Proton found. Install one on the Proton page of Preferences, or through Steam.",
            )
        } else {
            String::new()
        });
        let mut labels = vec![first];
        let mut names = vec![None];
        for build in builds {
            labels.push(build.display_name);
            names.push(Some(build.name));
        }
        let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
        self.filling.set(true);
        self.list.splice(0, self.list.n_items(), &refs);
        self.names.replace(names);
        self.filling.set(false);
        let pending = self.pending.take();
        self.select(pending);
    }

    /// Selects a build by internal name, or the first entry for `None`. Before the list
    /// is read, it is selected once it is.
    pub fn select(&self, name: Option<String>) {
        if self.names.borrow().is_empty() {
            self.pending.replace(name);
            return;
        }
        let found = self
            .names
            .borrow()
            .iter()
            .position(|listed| *listed == name);
        self.filling.set(true);
        let index = found.unwrap_or_else(|| {
            let wanted = name.unwrap_or_default();
            self.list
                .append(&gettext("{proton} (not installed)").replace("{proton}", &wanted));
            self.names.borrow_mut().push(Some(wanted));
            self.names.borrow().len() - 1
        });
        self.row.set_selected(index as u32);
        self.filling.set(false);
    }

    /// The chosen build by internal name; `None` for the first entry.
    pub fn selected(&self) -> Option<String> {
        let names = self.names.borrow();
        match names.get(self.row.selected() as usize) {
            Some(name) => name.clone(),
            // The list is not read yet: keep the build asked for.
            None => self.pending.borrow().clone(),
        }
    }

    /// Calls `changed` with each build the user picks.
    pub fn connect_changed(self: &Rc<Self>, changed: impl Fn(Option<String>) + 'static) {
        let weak = Rc::downgrade(self);
        self.row.connect_selected_notify(move |_| {
            if let Some(choice) = weak.upgrade()
                && !choice.filling.get()
            {
                changed(choice.selected());
            }
        });
    }
}
