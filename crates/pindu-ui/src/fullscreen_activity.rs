//! Notifications' Running tasks and Quick Access's Refresh Library. While anything runs in
//! the background (see `task_service`) the Notifications section lists each task, under a
//! Running heading, with its progress and, where the user can cancel it, Cancel; the
//! heading and the list go once nothing runs. Refresh Library runs the same sync as
//! `app.import` and is unavailable while a sync runs.

use std::time::Duration;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::panels::QuickAccessSection;
use pindu_core::tasks::{Cancel, Progress, Task, TaskId, TaskKind};

use crate::application::PinduApplication;
use crate::quick_access::PinduQuickAccess;

/// How often the progress bars of tasks with no telling how far they are move.
const PULSE_INTERVAL: Duration = Duration::from_millis(150);

impl PinduQuickAccess {
    /// Cancelling a task, and following the tasks and the sync: the Running tasks, and
    /// whether Refresh Library is available.
    pub(crate) fn setup_activity(
        &self,
        application: &PinduApplication,
        actions: &impl IsA<gio::ActionMap>,
    ) {
        self.add_host_action(
            actions,
            "cancel-task",
            Some(glib::VariantTy::UINT64),
            |quick_access, target| {
                let Some(id) = target.and_then(u64::from_variant) else {
                    return;
                };
                if let Some(application) = quick_access.application() {
                    application.task_service().cancel(TaskId(id));
                }
            },
        );
        application.task_service().connect_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move || quick_access.tasks_changed()
        ));
        let sync = application.sync_service();
        sync.connect_syncing_notify(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |sync| quick_access.show_refresh(sync.syncing())
        ));
        self.show_refresh(sync.syncing());
        self.tasks_changed();
    }

    /// Fills the Running list with the running tasks, oldest first, keeping focus on the
    /// task it was on, and moves the bars of those with no telling how far they are.
    pub(crate) fn show_activity(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let imp = self.imp();
        let list = imp.activity.get();
        let focused = self
            .focus_widget()
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

    /// A task started, moved on, was cancelled or ended: the Running tasks show while any
    /// runs, and follow them while Notifications shows. Once nothing runs, focus on a task
    /// moves to the first notification.
    fn tasks_changed(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let tasks = application.task_service().tasks();
        let imp = self.imp();
        let list = imp.activity.get();
        let had_focus = self
            .focus_widget()
            .is_some_and(|focus| focus.is_ancestor(&list));
        let running = !tasks.is_empty();
        self.update_shown(|shown| shown.running = running);
        if !running {
            list.remove_all();
            imp.activity_shown.take();
            if had_focus && let Some(row) = imp.notifications.row_at_index(0) {
                row.grab_focus();
            }
            return;
        }
        if !self.shows_section(QuickAccessSection::Notifications) {
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

    /// Refresh Library is unavailable while a sync runs; focus on either of its rows moves
    /// to the row beside it.
    fn show_refresh(&self, syncing: bool) {
        let imp = self.imp();
        let focused = [imp.refresh_row.get(), imp.quick_refresh_row.get()]
            .into_iter()
            .find(|row| row.has_focus());
        // Quick's row follows Manage's (`data/ui/quick_access.blp`).
        imp.refresh_row.set_sensitive(!syncing);
        if let Some(row) = focused.filter(|_| syncing) {
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || quick_access.focus_beside(&row)
            ));
        }
    }

    /// Moves the bars of tasks with no telling how far they are while Notifications shows.
    fn pulse_activity(&self) {
        let imp = self.imp();
        if imp.activity_pulse.borrow().is_some() {
            return;
        }
        let source = glib::timeout_add_local(
            PULSE_INTERVAL,
            glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                #[upgrade_or]
                glib::ControlFlow::Break,
                move || {
                    let imp = quick_access.imp();
                    if !quick_access.shows_section(QuickAccessSection::Notifications) {
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
        (_, TaskKind::MetadataFetch) => "Wikidata".to_owned(),
        (Cancel::Available, TaskKind::ProtonDownload) => gettext("Downloading"),
        (Cancel::Unavailable, TaskKind::ProtonDownload) => gettext("Installing"),
        (_, TaskKind::StoreInstall) => gettext("From Flathub"),
        (_, TaskKind::SaveSnapshot) => gettext("Backing up saves"),
    }
}

/// The icon of a task of `kind`.
fn kind_icon(kind: TaskKind) -> &'static str {
    match kind {
        TaskKind::LibrarySync => "view-refresh-symbolic",
        TaskKind::ArtFetch => "image-x-generic-symbolic",
        TaskKind::MetadataFetch => "text-x-generic-symbolic",
        TaskKind::ProtonDownload => "folder-download-symbolic",
        TaskKind::StoreInstall => "system-software-install-symbolic",
        TaskKind::SaveSnapshot => "document-save-symbolic",
    }
}
