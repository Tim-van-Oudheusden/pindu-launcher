//! Keeps what runs in the background (see `pindu_core::tasks`) for the shells: the sync,
//! SteamGridDB and Proton download services report their tasks here, with a
//! `gio::Cancellable` for those the user can cancel, and Quick Access's Activity lists
//! them and cancels them.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gtk::gio;
use gtk::prelude::*;
use pindu_core::tasks::{Progress, Task, TaskId, TaskKind, Tasks};

use crate::application::PinduApplication;

/// Something the shells do when the tasks change.
type Listener = Box<dyn Fn()>;

#[derive(Clone, Default)]
pub struct TaskService {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    tasks: RefCell<Tasks>,
    /// What stops each cancellable task's work.
    cancellables: RefCell<HashMap<TaskId, gio::Cancellable>>,
    listeners: RefCell<Vec<Listener>>,
}

impl std::fmt::Debug for TaskService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TaskService")
            .field("tasks", &self.inner.tasks.borrow())
            .finish_non_exhaustive()
    }
}

impl TaskService {
    pub fn new() -> Self {
        Self::default()
    }

    /// The running application's, for services that work outside any window.
    pub fn of_application() -> Option<Self> {
        gio::Application::default()
            .and_downcast::<PinduApplication>()
            .map(|application| application.task_service().clone())
    }

    /// Lists a running task of `kind` called `label`; the user can cancel it when it comes
    /// with a `cancellable`, which cancelling it cancels.
    pub fn start(
        &self,
        kind: TaskKind,
        label: &str,
        cancellable: Option<&gio::Cancellable>,
    ) -> TaskId {
        let id = self
            .inner
            .tasks
            .borrow_mut()
            .start(kind, label, cancellable.is_some());
        if let Some(cancellable) = cancellable {
            self.inner
                .cancellables
                .borrow_mut()
                .insert(id, cancellable.clone());
        }
        self.changed();
        id
    }

    /// Says how far the task `id` is.
    pub fn set_progress(&self, id: TaskId, progress: Progress) {
        let changed = self.inner.tasks.borrow_mut().set_progress(id, progress);
        if changed {
            self.changed();
        }
    }

    /// The task `id` reached a part of its work that must run to its end.
    pub fn no_longer_cancellable(&self, id: TaskId) {
        let changed = self.inner.tasks.borrow_mut().no_longer_cancellable(id);
        if changed {
            self.changed();
        }
    }

    /// Cancels the task `id`'s work, if the user can cancel it now; it stays listed until
    /// its work ends.
    pub fn cancel(&self, id: TaskId) {
        let cancelled = self.inner.tasks.borrow_mut().cancel(id);
        if !cancelled {
            return;
        }
        let cancellable = self.inner.cancellables.borrow().get(&id).cloned();
        if let Some(cancellable) = cancellable {
            cancellable.cancel();
        }
        self.changed();
    }

    /// Takes the task `id` off the list: its work ended.
    pub fn finish(&self, id: TaskId) {
        self.inner.tasks.borrow_mut().finish(id);
        self.inner.cancellables.borrow_mut().remove(&id);
        self.changed();
    }

    /// The running tasks, oldest first.
    pub fn tasks(&self) -> Vec<Task> {
        self.inner.tasks.borrow().tasks().to_vec()
    }

    /// Whether nothing runs.
    pub fn is_empty(&self) -> bool {
        self.inner.tasks.borrow().is_empty()
    }

    /// Calls `listener` whenever a task starts, moves on, is cancelled or ends.
    pub fn connect_changed(&self, listener: impl Fn() + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    fn changed(&self) {
        for listener in self.inner.listeners.borrow().iter() {
            listener();
        }
    }
}
