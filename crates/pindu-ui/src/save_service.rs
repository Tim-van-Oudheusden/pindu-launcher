//! Takes save snapshots of Windows games (see `pindu_core::saves`) on a worker thread,
//! each a task in Quick Access's Activity while it runs, lists a game's snapshots, and
//! restores one. A restore holds back the game's launch until it is through.

use std::cell::RefCell;
use std::collections::HashSet;
use std::io;
use std::path::PathBuf;

use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::model::{Game, GameId};
use pindu_core::paths::default_save_snapshots_dir;
use pindu_core::saves::{self, Reason, RestoreError, Snapshot, Taken};
use pindu_core::tasks::TaskKind;

use crate::launch_service::now;
use crate::library_service::LibraryService;
use crate::proton_launch;
use crate::session_service::SessionService;
use crate::task_service::TaskService;

thread_local! {
    /// The games whose snapshot is being written, so a second one never clears the
    /// first's partial folder.
    static TAKING: RefCell<HashSet<GameId>> = RefCell::default();
    /// The games whose saves are being restored, which do not start until it is through.
    static RESTORING: RefCell<HashSet<GameId>> = RefCell::default();
}

/// Whether the saves of `id` are being restored, so the game must not start yet.
pub fn restoring(id: &GameId) -> bool {
    RESTORING.with_borrow(|restoring| restoring.contains(id))
}

/// The snapshots of the game `id`, newest first, read on a worker thread.
pub async fn snapshots(id: &GameId) -> io::Result<Vec<Snapshot>> {
    let game_dir = saves::game_snapshots_dir(&default_save_snapshots_dir(), id);
    gio::spawn_blocking(move || saves::list(&game_dir))
        .await
        .unwrap_or_else(|_| Err(io::Error::other("the snapshots could not be read")))
}

/// Restores `snapshot` of `game` into the prefix Pindu manages for it, on a worker thread
/// and as a task in Activity, after a safety snapshot of the saves it replaces; then
/// prunes the game's older snapshots. It is refused while the game runs or a snapshot of
/// it is being written, and the game does not start until it is through. An error is
/// what to tell the user.
pub async fn restore(
    game: &Game,
    snapshot: Snapshot,
    library: &LibraryService,
    sessions: &SessionService,
) -> Result<(), String> {
    let (id, title) = (game.id.clone(), game.title.clone());
    if sessions.is_playing(&id) {
        return Err(quit_first(&title));
    }
    if TAKING.with_borrow(|taking| taking.contains(&id))
        || !RESTORING.with_borrow_mut(|restoring| restoring.insert(id.clone()))
    {
        // Translators: {title} is a game's title.
        return Err(
            gettext("The saves of “{title}” are being copied: try again in a moment")
                .replace("{title}", &title),
        );
    }
    let config = library.launch_config(&id).await.unwrap_or_default();
    let Some(prefix) = saves::managed_prefix(game, &config, &proton_launch::defaults()) else {
        RESTORING.with_borrow_mut(|restoring| restoring.remove(&id));
        // Translators: {title} is a game's title.
        return Err(gettext(
            "“{title}” runs in a prefix another app manages, so its saves are not restored",
        )
        .replace("{title}", &title));
    };
    let tasks = TaskService::of_application();
    // Translators: {title} is a game's title.
    let label = gettext("Restoring the saves of “{title}”").replace("{title}", &title);
    let task = tasks
        .as_ref()
        .map(|tasks| tasks.start(TaskKind::SaveSnapshot, &label, None));
    let (taken_at, game_dir) = (
        now(),
        saves::game_snapshots_dir(&default_save_snapshots_dir(), &id),
    );
    let restored = {
        let id = id.clone();
        gio::spawn_blocking(move || {
            let safety = saves::restore(&prefix, &game_dir, &id, &snapshot, false, taken_at)?;
            if let Err(error) = saves::prune(&game_dir) {
                glib::g_warning!(
                    "pindu",
                    "Could not remove old snapshots in {}: {error}",
                    game_dir.display()
                );
            }
            Ok::<_, RestoreError>(safety)
        })
        .await
    };
    RESTORING.with_borrow_mut(|restoring| restoring.remove(&id));
    if let (Some(tasks), Some(task)) = (tasks, task) {
        tasks.finish(task);
    }
    let error = match restored {
        Ok(Ok(safety)) => {
            if let Taken::Written(safety) = safety {
                glib::g_info!(
                    "pindu",
                    "Kept the saves of {id} from before the restore in {}",
                    safety.dir.display()
                );
            }
            return Ok(());
        }
        Ok(Err(error)) => error,
        Err(_) => RestoreError::Io(io::Error::other("the restore stopped")),
    };
    glib::g_warning!("pindu", "Could not restore the saves of {id}: {error:?}");
    Err(match error {
        RestoreError::Running => quit_first(&title),
        // Translators: {title} is a game's title.
        RestoreError::NoPrefix => gettext(
            "The prefix of “{title}” has no user folders: start the game once, then restore",
        )
        .replace("{title}", &title),
        RestoreError::NotThisGame | RestoreError::Refused(_) | RestoreError::Io(_) => {
            // Translators: {title} is a game's title.
            gettext("Could not restore the saves of “{title}”").replace("{title}", &title)
        }
    })
}

/// What to tell the user who restores the saves of the game `title` while it runs.
fn quit_first(title: &str) -> String {
    // Translators: {title} is a game's title.
    gettext("Quit “{title}” before restoring its saves").replace("{title}", title)
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
