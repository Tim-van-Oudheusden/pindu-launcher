//! Takes save snapshots of Windows games (see `pindu_core::saves`) on a worker thread,
//! each a task in Quick Access's Activity while it runs, and copies each new one to the
//! folder the user picked in Preferences, if any.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::model::{Game, GameId};
use pindu_core::notifications::Kind;
use pindu_core::paths::default_save_snapshots_dir;
use pindu_core::saves::{self, Copied, Reason, Taken};
use pindu_core::tasks::TaskKind;

use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::file_service;
use crate::launch_service::now;
use crate::task_service::TaskService;

thread_local! {
    /// The games whose snapshot is being written, so a second one never clears the
    /// first's partial folder.
    static TAKING: RefCell<HashSet<GameId>> = RefCell::default();
}

/// The GSettings key holding the folder snapshots are also copied to, as the sandbox
/// reaches it; empty when they are kept only on this computer.
pub const COPY_FOLDER_KEY: &str = "save-copy-folder";

/// The folder the user picked for copies of snapshots, as the sandbox reaches it.
pub fn copy_folder() -> Option<PathBuf> {
    let folder = gio::Settings::new(APP_ID).string(COPY_FOLDER_KEY);
    (!folder.is_empty()).then(|| PathBuf::from(folder.as_str()))
}

/// Takes a snapshot of the saves in `prefix`, the managed prefix of `game`, which just
/// exited cleanly, then prunes the game's older snapshots, and copies the newest snapshot
/// to the folder the user picked for copies. A failure leaves the earlier snapshots as
/// they were and is said with `toast`; a copy that could not be made, as when the folder
/// is gone, is said with `toast` and a notification, never as a failure of the game. A
/// snapshot of the game still being written makes this one wait for the next exit.
pub fn snapshot_after_exit(game: &Game, prefix: PathBuf, toast: impl Fn(adw::Toast) + 'static) {
    let id = game.id.clone();
    if !TAKING.with_borrow_mut(|taking| taking.insert(id.clone())) {
        glib::g_info!("pindu", "A snapshot of {id}'s saves is still being written");
        return;
    }
    let title = game.title.clone();
    let tasks = TaskService::of_application();
    // Translators: {title} is a game's title.
    let label = gettext("Saves of “{title}”").replace("{title}", &title);
    let task = tasks
        .as_ref()
        .map(|tasks| tasks.start(TaskKind::SaveSnapshot, &label, None));
    let taken_at = now();
    let copy_folder = copy_folder();
    glib::spawn_future_local(async move {
        let game_dir = saves::game_snapshots_dir(&default_save_snapshots_dir(), &id);
        let taken = {
            let (id, game_dir, copy_folder) = (id.clone(), game_dir.clone(), copy_folder.clone());
            gio::spawn_blocking(move || {
                let taken = saves::take(&prefix, &game_dir, &id, Reason::Exit, taken_at)?;
                let pruned = saves::prune(&game_dir);
                let copied = copy_folder.and_then(|folder| copy(&taken, &folder));
                Ok::<_, io::Error>((taken, pruned, copied))
            })
            .await
        };
        TAKING.with_borrow_mut(|taking| taking.remove(&id));
        if let (Some(tasks), Some(task)) = (tasks, task) {
            tasks.finish(task);
        }
        match taken {
            Ok(Ok((taken, pruned, copied))) => {
                match taken {
                    Taken::Written(snapshot) => glib::g_info!(
                        "pindu",
                        "Saved a snapshot of {id}'s saves, {} bytes, in {}",
                        snapshot.manifest.size(),
                        snapshot.dir.display()
                    ),
                    Taken::Unchanged(_) => {
                        glib::g_info!("pindu", "{id}'s saves are as their last snapshot");
                    }
                    Taken::NothingToSave => glib::g_info!("pindu", "{id} has no saves to snapshot"),
                }
                if let Err(error) = pruned {
                    glib::g_warning!(
                        "pindu",
                        "Could not remove old snapshots in {}: {error}",
                        game_dir.display()
                    );
                }
                if let (Some(copied), Some(folder)) = (copied, copy_folder) {
                    report_copy(&id, &title, &folder, copied, &toast).await;
                }
            }
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not snapshot the saves of {id}: {error}");
                toast(adw::Toast::new(
                    &gettext("Could not back up the saves of “{title}”").replace("{title}", &title),
                ));
            }
            Err(_) => glib::g_warning!("pindu", "Could not snapshot the saves of {id}"),
        }
    });
}

/// Copies the game's newest snapshot, new or as the saves still are, into `folder`, so one
/// missed while the folder was gone follows on the next exit, then prunes the copies there
/// as the snapshots folder is pruned. `None` when there is no snapshot to copy.
fn copy(taken: &Taken, folder: &Path) -> Option<io::Result<Copied>> {
    let (Taken::Written(snapshot) | Taken::Unchanged(snapshot)) = taken else {
        return None;
    };
    let copied = saves::copy_to(snapshot, folder);
    if let Ok(Copied::Written(_)) = copied {
        let copies = saves::game_snapshots_dir(folder, &snapshot.manifest.game);
        if let Err(error) = saves::prune(&copies) {
            glib::g_warning!(
                "pindu",
                "Could not remove old copies of snapshots in {}: {error}",
                copies.display()
            );
        }
    }
    Some(copied)
}

/// Logs a copy made, and says with `toast` and a notification that one could not be made.
async fn report_copy(
    id: &GameId,
    title: &str,
    folder: &Path,
    copied: io::Result<Copied>,
    toast: &impl Fn(adw::Toast),
) {
    let body = match copied {
        Ok(Copied::Written(copy)) => {
            glib::g_info!("pindu", "Copied {id}'s snapshot to {}", copy.dir.display());
            return;
        }
        Ok(Copied::AlreadyThere) => return,
        Ok(Copied::FolderMissing) => {
            glib::g_warning!(
                "pindu",
                "Did not copy {id}'s snapshot: {} is not there",
                folder.display()
            );
            let shown = file_service::host_path(&gio::File::for_path(folder))
                .await
                .unwrap_or_else(|| folder.to_owned());
            // Translators: {folder} is the folder picked for copies of save snapshots.
            gettext(
                "{folder} is not there. The snapshot stays on this computer, and the next one \
                 is copied once the folder is back.",
            )
            .replace("{folder}", &shown.display().to_string())
        }
        Err(error) => {
            glib::g_warning!(
                "pindu",
                "Could not copy {id}'s snapshot to {}: {error}",
                folder.display()
            );
            error.to_string()
        }
    };
    // Translators: {title} is a game's title.
    let message = gettext("Could not copy the saves of “{title}”").replace("{title}", title);
    toast(adw::Toast::new(&message));
    if let Some(application) = gio::Application::default().and_downcast::<PinduApplication>() {
        application
            .notification_service()
            .post(Kind::SaveCopyFailed, &message, &body, None);
    }
}
