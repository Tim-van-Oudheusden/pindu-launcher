//! Takes save snapshots of Windows games (see `pindu_core::saves`) on a worker thread,
//! each a task in Quick Access's Activity while it runs.

use std::cell::RefCell;
use std::collections::HashSet;
use std::path::PathBuf;

use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::{Game, GameId};
use pindu_core::paths::default_save_snapshots_dir;
use pindu_core::saves::{self, Reason, Taken};
use pindu_core::tasks::TaskKind;

use crate::launch_service::now;
use crate::task_service::TaskService;

thread_local! {
    /// The games whose snapshot is being written, so a second one never clears the
    /// first's partial folder.
    static TAKING: RefCell<HashSet<GameId>> = RefCell::default();
}

/// Takes a snapshot of the saves in `prefix`, the managed prefix of `game`, which just
/// exited cleanly, then prunes the game's older snapshots. A failure leaves the earlier
/// snapshots as they were and is said with `toast`; a snapshot of the game still being
/// written makes this one wait for the next exit.
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
    glib::spawn_future_local(async move {
        let game_dir = saves::game_snapshots_dir(&default_save_snapshots_dir(), &id);
        let taken = {
            let (id, game_dir) = (id.clone(), game_dir.clone());
            gio::spawn_blocking(move || {
                let taken = saves::take(&prefix, &game_dir, &id, Reason::Exit, taken_at)?;
                let pruned = saves::prune(&game_dir);
                Ok::<_, std::io::Error>((taken, pruned))
            })
            .await
        };
        TAKING.with_borrow_mut(|taking| taking.remove(&id));
        if let (Some(tasks), Some(task)) = (tasks, task) {
            tasks.finish(task);
        }
        match taken {
            Ok(Ok((taken, pruned))) => {
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
