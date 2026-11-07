//! Keeps the session's notifications (see `pindu_core::notifications`) for the shells:
//! the services post what happened in the background, and Quick Access lists it and marks
//! it read.

use std::cell::RefCell;
use std::rc::Rc;

use pindu_core::notifications::{Kind, Log, Notification, Target};

use crate::launch_service::now;

/// Something the shells do when the log changes.
type Listener = Box<dyn Fn()>;

/// The session's notifications, shared by the services and the shells.
#[derive(Clone, Default)]
pub struct NotificationService {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    log: RefCell<Log>,
    listeners: RefCell<Vec<Listener>>,
}

impl std::fmt::Debug for NotificationService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NotificationService")
            .field("log", &self.inner.log.borrow())
            .finish_non_exhaustive()
    }
}

impl NotificationService {
    /// A service with no notifications.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an unread notification, now, opening `target` when the user picks it.
    pub fn post(&self, kind: Kind, title: &str, body: &str, target: Option<Target>) {
        let notification = Notification::new(kind, title, body, now());
        let notification = match target {
            Some(target) => notification.with_target(target),
            None => notification,
        };
        self.inner.log.borrow_mut().add(notification);
        self.changed();
    }

    /// The notifications, newest first.
    pub fn entries(&self) -> Vec<Notification> {
        self.inner.log.borrow().entries().cloned().collect()
    }

    /// How many notifications the user has not seen.
    pub fn unread(&self) -> usize {
        self.inner.log.borrow().unread()
    }

    /// Marks every notification read: the user opened the list.
    pub fn mark_all_read(&self) {
        let changed = self.inner.log.borrow_mut().mark_all_read();
        if changed {
            self.changed();
        }
    }

    /// Calls `listener` whenever a notification is posted or the log is read.
    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    fn changed(&self) {
        for listener in self.inner.listeners.borrow().iter() {
            listener();
        }
    }
}
