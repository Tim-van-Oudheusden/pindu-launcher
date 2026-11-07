//! What Pindu is doing in the background, which Quick Access's Activity lists: library
//! syncs, SteamGridDB art fetches and Proton downloads, each with how far it is and
//! whether the user can cancel it. A task leaves the list when its work ends.

/// What a task does, which picks its icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskKind {
    /// Reading the launchers' files into the library.
    LibrarySync,
    /// Fetching missing art from SteamGridDB.
    ArtFetch,
    /// Downloading and installing a Proton build.
    ProtonDownload,
}

/// How far a task is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    /// Running, with no telling how far.
    Working,
    /// This much done, from 0 to 1.
    Fraction(f64),
}

impl Progress {
    /// `done` of `total` steps, as a run over many games; working while there is no
    /// telling how many.
    pub fn of(done: usize, total: usize) -> Self {
        if total == 0 {
            Self::Working
        } else {
            Self::Fraction((done as f64 / total as f64).min(1.0))
        }
    }
}

/// Whether the user can cancel a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cancel {
    /// Its work cannot be stopped (now).
    Unavailable,
    /// The user can cancel it.
    Available,
    /// The user cancelled it, and its work is stopping.
    Requested,
}

/// A task's handle, unique for the session: a number, so a shell can name it in an
/// action's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

/// One running task.
#[derive(Debug, Clone, PartialEq)]
pub struct Task {
    /// Its handle.
    pub id: TaskId,
    /// What it does.
    pub kind: TaskKind,
    /// What the list calls it.
    pub label: String,
    /// How far it is.
    pub progress: Progress,
    /// Whether the user can cancel it.
    pub cancel: Cancel,
}

/// The running tasks, oldest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Tasks {
    next: u64,
    tasks: Vec<Task>,
}

impl Tasks {
    /// No tasks.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a running task of `kind` called `label`, working with no telling how far, and
    /// which the user can cancel when `cancellable`.
    pub fn start(&mut self, kind: TaskKind, label: &str, cancellable: bool) -> TaskId {
        let id = TaskId(self.next);
        self.next += 1;
        self.tasks.push(Task {
            id,
            kind,
            label: label.to_owned(),
            progress: Progress::Working,
            cancel: if cancellable {
                Cancel::Available
            } else {
                Cancel::Unavailable
            },
        });
        id
    }

    /// Says how far the task `id` is; says whether that changed anything, so a shell
    /// redraws only then. A task that ended is left alone.
    pub fn set_progress(&mut self, id: TaskId, progress: Progress) -> bool {
        match self.task_mut(id) {
            Some(task) if task.progress != progress => {
                task.progress = progress;
                true
            }
            _ => false,
        }
    }

    /// The user cancels the task `id`. Says whether its work should stop: only once, and
    /// only for a task that can be cancelled. The task stays listed, cancelling, until its
    /// work ends.
    pub fn cancel(&mut self, id: TaskId) -> bool {
        match self.task_mut(id) {
            Some(task) if task.cancel == Cancel::Available => {
                task.cancel = Cancel::Requested;
                true
            }
            _ => false,
        }
    }

    /// The task `id` can no longer be cancelled: its work reached a part that must run to
    /// its end, as a Proton install unpacking. Says whether that changed anything.
    pub fn no_longer_cancellable(&mut self, id: TaskId) -> bool {
        match self.task_mut(id) {
            Some(task) if task.cancel == Cancel::Available => {
                task.cancel = Cancel::Unavailable;
                true
            }
            _ => false,
        }
    }

    /// Takes the task `id` off the list: its work ended, done, failed or cancelled.
    pub fn finish(&mut self, id: TaskId) {
        self.tasks.retain(|task| task.id != id);
    }

    /// Whether no task runs, which hides Quick Access's Activity.
    pub fn is_empty(&self) -> bool {
        self.tasks.is_empty()
    }

    /// The running tasks, oldest first.
    pub fn tasks(&self) -> &[Task] {
        &self.tasks
    }

    fn task_mut(&mut self, id: TaskId) -> Option<&mut Task> {
        self.tasks.iter_mut().find(|task| task.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(tasks: &Tasks) -> Vec<&str> {
        tasks
            .tasks()
            .iter()
            .map(|task| task.label.as_str())
            .collect()
    }

    #[test]
    fn started_tasks_list_oldest_first_and_working() {
        let mut tasks = Tasks::new();

        let sync = tasks.start(TaskKind::LibrarySync, "Refreshing Library", false);
        let download = tasks.start(TaskKind::ProtonDownload, "GE-Proton10-1", true);

        assert_ne!(sync, download);
        assert_eq!(labels(&tasks), ["Refreshing Library", "GE-Proton10-1"]);
        assert_eq!(tasks.tasks()[0].id, sync);
        assert_eq!(tasks.tasks()[1].kind, TaskKind::ProtonDownload);
        assert_eq!(tasks.tasks()[0].cancel, Cancel::Unavailable);
        assert_eq!(tasks.tasks()[1].cancel, Cancel::Available);
        assert!(
            tasks
                .tasks()
                .iter()
                .all(|task| task.progress == Progress::Working)
        );
    }

    #[test]
    fn progress_updates_its_task_and_says_whether_it_changed() {
        let mut tasks = Tasks::new();
        let sync = tasks.start(TaskKind::LibrarySync, "Refreshing Library", false);
        let art = tasks.start(TaskKind::ArtFetch, "Getting Artwork", true);

        assert!(tasks.set_progress(art, Progress::Fraction(0.25)));

        assert_eq!(tasks.tasks()[1].progress, Progress::Fraction(0.25));
        assert_eq!(tasks.tasks()[0].progress, Progress::Working);
        assert!(!tasks.set_progress(art, Progress::Fraction(0.25)));
        assert!(!tasks.set_progress(sync, Progress::Working));
        assert!(tasks.set_progress(sync, Progress::Fraction(1.0)));
    }

    #[test]
    fn a_finished_task_leaves_the_list_and_ignores_later_progress() {
        let mut tasks = Tasks::new();
        let sync = tasks.start(TaskKind::LibrarySync, "Refreshing Library", false);
        let art = tasks.start(TaskKind::ArtFetch, "Getting Artwork", true);

        tasks.finish(sync);

        assert_eq!(labels(&tasks), ["Getting Artwork"]);
        assert!(!tasks.is_empty());
        assert!(!tasks.set_progress(sync, Progress::Fraction(0.5)));
        tasks.finish(art);
        assert!(tasks.is_empty());
        tasks.finish(art);
        assert!(tasks.is_empty());
    }

    #[test]
    fn cancelling_asks_a_cancellable_task_to_stop_once_and_keeps_it_listed_until_it_ends() {
        let mut tasks = Tasks::new();
        let download = tasks.start(TaskKind::ProtonDownload, "GE-Proton10-1", true);

        assert!(tasks.cancel(download));

        assert_eq!(tasks.tasks()[0].cancel, Cancel::Requested);
        assert!(!tasks.cancel(download));
        tasks.finish(download);
        assert!(tasks.is_empty());
        assert!(!tasks.cancel(download));
    }

    #[test]
    fn a_task_that_cannot_be_cancelled_keeps_running() {
        let mut tasks = Tasks::new();
        let sync = tasks.start(TaskKind::LibrarySync, "Refreshing Library", false);

        assert!(!tasks.cancel(sync));

        assert_eq!(tasks.tasks()[0].cancel, Cancel::Unavailable);
    }

    #[test]
    fn a_task_can_stop_being_cancellable_part_way() {
        let mut tasks = Tasks::new();
        let download = tasks.start(TaskKind::ProtonDownload, "GE-Proton10-1", true);

        assert!(tasks.no_longer_cancellable(download));

        assert_eq!(tasks.tasks()[0].cancel, Cancel::Unavailable);
        assert!(!tasks.cancel(download));
        assert!(!tasks.no_longer_cancellable(download));
    }

    #[test]
    fn progress_over_counted_steps_is_their_share_done() {
        assert_eq!(Progress::of(1, 4), Progress::Fraction(0.25));
        assert_eq!(Progress::of(0, 3), Progress::Fraction(0.0));
        assert_eq!(Progress::of(5, 4), Progress::Fraction(1.0));
        assert_eq!(Progress::of(0, 0), Progress::Working);
    }
}
