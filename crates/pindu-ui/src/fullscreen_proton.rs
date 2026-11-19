//! Fullscreen mode's Proton list: the entries of the desktop's Proton row
//! (`proton_choice`) as rows of a list box with a check on the chosen one, which a
//! controller moves through and A picks. The first row stands for no build named, then
//! the builds on this machine, newest first. A chosen build that is gone stays listed, so
//! the choice is kept.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::proton_choice::{FirstChoice, entries, not_installed};
use crate::proton_launch;

/// What is called with each build the user picks.
type Changed = Rc<dyn Fn(Option<String>)>;

/// A list box of Proton builds with a check on the chosen one.
pub struct ProtonChecks {
    list: gtk::ListBox,
    first: FirstChoice,
    /// The listed builds by internal name, `None` for the first row, with their checks;
    /// empty until read.
    checks: RefCell<Vec<(Option<String>, gtk::CheckButton)>>,
    /// The build to check once the list is read.
    pending: RefCell<Option<String>>,
    /// Set while the list changes the check itself, so only the user's choices count.
    filling: Cell<bool>,
    /// Called with each build the user picks.
    changed: RefCell<Option<Changed>>,
}

impl fmt::Debug for ProtonChecks {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ProtonChecks")
            .field("list", &self.list)
            .field("first", &self.first)
            .field("checks", &self.checks)
            .field("pending", &self.pending)
            .finish_non_exhaustive()
    }
}

impl ProtonChecks {
    /// Fills `list` with the builds on this machine, and checks `selected` once they are
    /// read.
    pub fn new(list: &gtk::ListBox, first: FirstChoice, selected: Option<String>) -> Rc<Self> {
        let checks = Rc::new(Self {
            list: list.clone(),
            first,
            checks: RefCell::default(),
            pending: RefCell::new(selected),
            filling: Cell::new(false),
            changed: RefCell::default(),
        });
        checks.read_builds();
        checks
    }

    /// Reads the builds on this machine again, after one was installed or removed,
    /// keeping the choice.
    pub fn reload(self: &Rc<Self>) {
        let selected = self.selected();
        self.pending.replace(selected);
        self.read_builds();
    }

    fn read_builds(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        glib::spawn_future_local(async move {
            let builds = gio::spawn_blocking(proton_launch::builds)
                .await
                .unwrap_or_default();
            if let Some(checks) = weak.upgrade() {
                // Filling again takes focus away from a row that had it.
                let had_focus = checks
                    .list
                    .root()
                    .and_then(|root| gtk::prelude::RootExt::focus(&root))
                    .is_some_and(|focus| focus.is_ancestor(&checks.list));
                let pending = checks.pending.take();
                checks.list.remove_all();
                checks.checks.borrow_mut().clear();
                for (label, name) in entries(checks.first, builds) {
                    checks.append(&label, name);
                }
                checks.select(pending);
                if had_focus && let Some(row) = checks.focus_target() {
                    row.grab_focus();
                }
            }
        });
    }

    /// Adds a row for the build `name` (`None` for the first row), labelled `label`.
    fn append(self: &Rc<Self>, label: &str, name: Option<String>) -> gtk::CheckButton {
        let check = gtk::CheckButton::builder()
            .valign(gtk::Align::Center)
            .can_focus(false)
            .build();
        if let Some((_, first)) = self.checks.borrow().first() {
            check.set_group(Some(first));
        }
        let row = adw::ActionRow::builder()
            .title(label)
            .use_markup(false)
            .build();
        row.add_prefix(&check);
        row.set_activatable_widget(Some(&check));
        let weak = Rc::downgrade(self);
        let chosen = name.clone();
        check.connect_toggled(move |check| {
            let Some(checks) = weak.upgrade() else {
                return;
            };
            if !check.is_active() || checks.filling.get() {
                return;
            }
            let changed = checks.changed.borrow().clone();
            if let Some(changed) = changed {
                changed(chosen.clone());
            }
        });
        self.list.append(&row);
        self.checks.borrow_mut().push((name, check.clone()));
        check
    }

    /// Checks a build by internal name, or the first row for `None`. Before the list is
    /// read, it is checked once it is.
    pub fn select(self: &Rc<Self>, name: Option<String>) {
        if self.checks.borrow().is_empty() {
            self.pending.replace(name);
            return;
        }
        let found = self
            .checks
            .borrow()
            .iter()
            .find(|(listed, _)| *listed == name)
            .map(|(_, check)| check.clone());
        self.filling.set(true);
        let check = found.unwrap_or_else(|| {
            let wanted = name.unwrap_or_default();
            self.append(&not_installed(&wanted), Some(wanted))
        });
        check.set_active(true);
        self.filling.set(false);
    }

    /// The chosen build by internal name; `None` for the first row.
    pub fn selected(&self) -> Option<String> {
        let checks = self.checks.borrow();
        match checks.iter().find(|(_, check)| check.is_active()) {
            Some((name, _)) => name.clone(),
            // The list is not read yet: keep the build asked for.
            None => self.pending.borrow().clone(),
        }
    }

    /// The row of the chosen build, where focus goes as the list shows; the first row
    /// until the list is read.
    pub fn focus_target(&self) -> Option<gtk::Widget> {
        self.checks
            .borrow()
            .iter()
            .find(|(_, check)| check.is_active())
            .and_then(|(_, check)| check.ancestor(gtk::ListBoxRow::static_type()))
            .or_else(|| self.list.row_at_index(0).map(|row| row.upcast()))
    }

    /// Calls `changed` with each build the user picks.
    pub fn connect_changed(&self, changed: impl Fn(Option<String>) + 'static) {
        self.changed.replace(Some(Rc::new(changed)));
    }
}
