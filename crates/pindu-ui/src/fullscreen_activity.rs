//! Quick Access's Activity and Refresh Library entries. Activity shows while anything runs
//! in the background (see `task_service`) and leads to a level listing each task with its
//! progress and, where the user can cancel it, Cancel; it goes once nothing runs. Refresh
//! Library runs the same sync as `app.import` and is unavailable while a sync runs.

use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::{Panel, QuickAccessLevel};
use pindu_core::tasks::{Cancel, Progress, Task, TaskId, TaskKind};

use crate::application::PinduApplication;
use crate::sync_service::SyncService;
use crate::window::PinduWindow;

/// How often the progress bars of tasks with no telling how far they are move.
const PULSE_INTERVAL: Duration = Duration::from_millis(150);

impl PinduWindow {
    /// Cancelling a task, and following the tasks and the sync: the Activity entry and
    /// level, and whether Refresh Library is available.
    pub(crate) fn setup_activity(&self, application: &PinduApplication, sync: &SyncService) {
        let cancel = gio::ActionEntry::builder("cancel-task")
            .parameter_type(Some(glib::VariantTy::UINT64))
            .activate(|window: &Self, _, target| {
                let Some(id) = target.and_then(u64::from_variant) else {
                    return;
                };
                if let Some(application) = window.application().and_downcast::<PinduApplication>() {
                    application.task_service().cancel(TaskId(id));
                }
            })
            .build();
        self.add_action_entries([cancel]);
        application.task_service().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.tasks_changed()
        ));
        sync.connect_syncing_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |sync| window.show_refresh(sync.syncing())
        ));
        self.show_refresh(sync.syncing());
        self.tasks_changed();
    }

    /// Fills the Activity level with the running tasks, oldest first, keeping focus on
    /// the task it was on, and moves the bars of those with no telling how far they are.
    pub(crate) fn show_activity(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let imp = self.imp();
        let list = imp.quick_access_activity.get();
        let focused = gtk::prelude::GtkWindowExt::focus(self)
            .filter(|focus| focus.is_ancestor(&list))
            .and_then(|focus| {
                let row = focus.ancestor(gtk::ListBoxRow::static_type())?;
                let index = row.downcast_ref::<gtk::ListBoxRow>()?.index();
                let shown = imp.activity_shown.borrow();
                shown
                    .get(usize::try_from(index).ok()?)
                    .map(|(task, _)| task.id)
            });
        list.remove_all();
        let tasks = application.task_service().tasks();
        let mut shown = Vec::with_capacity(tasks.len());
        for task in tasks {
            let (row, bar) = task_row(&task);
            list.append(&row);
            shown.push((task, bar));
        }
        let index = focused.and_then(|id| shown.iter().position(|(task, _)| task.id == id));
        imp.activity_shown.replace(shown);
        if focused.is_some() {
            let row = index
                .and_then(|index| list.row_at_index(index as i32))
                .or_else(|| list.row_at_index(0));
            if let Some(row) = row {
                row.grab_focus();
            }
        }
        self.pulse_activity();
    }

    /// A task started, moved on, was cancelled or ended: the Activity entry shows while
    /// any runs, and the level follows them while it is open, leaving for Quick Access's
    /// entries once nothing runs.
    fn tasks_changed(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let tasks = application.task_service().tasks();
        let imp = self.imp();
        let row = &imp.quick_access_activity_row;
        let had_focus = row.has_focus();
        row.set_visible(!tasks.is_empty());
        if tasks.is_empty() {
            if had_focus {
                self.activity_entry().grab_focus();
            }
            self.leave_activity();
            return;
        }
        if self.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::Activity)) {
            return;
        }
        // Only progress changed: the bars move, and the rows (and focus) stay.
        let same = {
            let shown = imp.activity_shown.borrow();
            shown.len() == tasks.len()
                && shown
                    .iter()
                    .zip(&tasks)
                    .all(|((shown, _), task)| same_row(shown, task))
        };
        if !same {
            self.show_activity();
            return;
        }
        let mut shown = imp.activity_shown.borrow_mut();
        for ((shown, bar), task) in shown.iter_mut().zip(tasks) {
            if let Progress::Fraction(fraction) = task.progress {
                bar.set_fraction(fraction);
            }
            *shown = task;
        }
    }

    /// The entry focus goes to for Activity: Activity itself while it shows, else Refresh
    /// Library, or the notifications while a sync keeps that unavailable.
    pub(crate) fn activity_entry(&self) -> gtk::Widget {
        let imp = self.imp();
        let row = if imp.quick_access_activity_row.is_visible() {
            &imp.quick_access_activity_row
        } else if imp.quick_access_refresh_row.is_sensitive() {
            &imp.quick_access_refresh_row
        } else {
            &imp.quick_access_notifications_row
        };
        row.get().upcast()
    }

    /// Refresh Library is unavailable while a sync runs; if it had focus, Activity, which
    /// now lists the sync, takes it once it is shown.
    fn show_refresh(&self, syncing: bool) {
        let row = &self.imp().quick_access_refresh_row;
        let had_focus = row.has_focus();
        row.set_sensitive(!syncing);
        if had_focus && syncing {
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    window.activity_entry().grab_focus();
                }
            ));
        }
    }

    /// Moves the bars of tasks with no telling how far they are while Activity shows.
    fn pulse_activity(&self) {
        let imp = self.imp();
        if imp.activity_pulse.borrow().is_some() {
            return;
        }
        let source = glib::timeout_add_local(
            PULSE_INTERVAL,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    let imp = window.imp();
                    if window.open_panel() != Some(Panel::QuickAccess(QuickAccessLevel::Activity)) {
                        imp.activity_pulse.take();
                        return glib::ControlFlow::Break;
                    }
                    for (task, bar) in imp.activity_shown.borrow().iter() {
                        if task.progress == Progress::Working {
                            bar.pulse();
                        }
                    }
                    glib::ControlFlow::Continue
                }
            ),
        );
        imp.activity_pulse.replace(Some(source));
    }
}

/// Whether `task` shows in the row made for `shown`: all but its progress match.
fn same_row(shown: &Task, task: &Task) -> bool {
    shown.id == task.id
        && shown.kind == task.kind
        && shown.label == task.label
        && shown.cancel == task.cancel
        && matches!(shown.progress, Progress::Working) == matches!(task.progress, Progress::Working)
}

/// A row for `task`: its icon, label and what it is doing, Cancel while the user can
/// cancel it (which A on the row does too), and its progress bar.
fn task_row(task: &Task) -> (gtk::ListBoxRow, gtk::ProgressBar) {
    let header = gtk::Box::builder().spacing(16).build();
    header.append(
        &gtk::Image::builder()
            .icon_name(kind_icon(task.kind))
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    let text = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(2)
        .hexpand(true)
        .valign(gtk::Align::Center)
        .build();
    text.append(
        &gtk::Label::builder()
            .label(&task.label)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    text.append(
        &gtk::Label::builder()
            .label(task_state(task))
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["dim-label", "caption"])
            .build(),
    );
    header.append(&text);
    let cancellable = task.cancel == Cancel::Available;
    let target = task.id.0.to_variant();
    if cancellable {
        // The row takes focus and A; the button is for the pointer.
        let button = gtk::Button::builder()
            .label(gettext("Cancel"))
            .valign(gtk::Align::Center)
            .can_focus(false)
            .action_name("win.cancel-task")
            .action_target(&target)
            .build();
        header.append(&button);
    }
    let bar = gtk::ProgressBar::builder().pulse_step(0.1).build();
    match task.progress {
        Progress::Fraction(fraction) => {
            bar.set_fraction(fraction);
            bar.set_show_text(true);
        }
        Progress::Working => bar.pulse(),
    }
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(10)
        .margin_top(10)
        .margin_bottom(10)
        .margin_start(12)
        .margin_end(12)
        .build();
    content.append(&header);
    content.append(&bar);
    let row = gtk::ListBoxRow::builder()
        .child(&content)
        .activatable(cancellable)
        .css_classes(["task"])
        .build();
    if cancellable {
        row.set_action_name(Some("win.cancel-task"));
        row.set_action_target_value(Some(&target));
    }
    (row, bar)
}

/// What a task is doing, under its label.
fn task_state(task: &Task) -> String {
    match (task.cancel, task.kind) {
        (Cancel::Requested, _) => gettext("Cancelling…"),
        (_, TaskKind::LibrarySync) => gettext("Reading your launchers"),
        // A name, so it is not translated.
        (_, TaskKind::ArtFetch) => "SteamGridDB".to_owned(),
        (Cancel::Available, TaskKind::ProtonDownload) => gettext("Downloading"),
        (Cancel::Unavailable, TaskKind::ProtonDownload) => gettext("Installing"),
    }
}

/// The icon of a task of `kind`.
fn kind_icon(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::LibrarySync => "view-refresh-symbolic",
        TaskKind::ArtFetch => "image-x-generic-symbolic",
        TaskKind::ProtonDownload => "folder-download-symbolic",
    }
}
