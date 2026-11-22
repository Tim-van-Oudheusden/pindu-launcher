//! Takes save snapshots (see `pindu_core::saves`) of the prefixes Pindu manages for
//! Windows games and of the save folders the user added for any game, on a worker
//! thread, each a task in Quick Access's Activity while it runs, copies each new one to
//! the folder the user picked in Preferences, if any, keeps each game's save folders,
//! lists a game's snapshots, and restores one. A restore holds back the game's launch
//! until it is through.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::io;
use std::path::{Path, PathBuf};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::model::{Game, GameId};
use pindu_core::notifications::Kind;
use pindu_core::paths::default_save_snapshots_dir;
use pindu_core::saves::{
    self, Copied, NewFolderError, Reach, Reason, RestoreError, Roots, Snapshot, Taken,
};
use pindu_core::tasks::TaskKind;

use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::file_service;
use crate::library_service::LibraryService;
use crate::permission_service;
use crate::proton_launch;
use crate::session_service::{SessionService, now};
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

/// The GSettings key holding the save folders the user added, by game ID, as the sandbox
/// reaches them.
pub const FOLDERS_KEY: &str = "save-folders";

/// The save folders the user added for the game `id`, as the sandbox reaches them, in the
/// order they were added.
pub fn save_folders(id: &GameId) -> Vec<PathBuf> {
    all_save_folders()
        .remove(id.as_str())
        .unwrap_or_default()
        .into_iter()
        .map(PathBuf::from)
        .collect()
}

/// Every game's save folders, by game ID.
fn all_save_folders() -> HashMap<String, Vec<String>> {
    gio::Settings::new(APP_ID)
        .value(FOLDERS_KEY)
        .get()
        .unwrap_or_default()
}

/// Keeps `folders` as the save folders of the game `id`.
fn set_save_folders(id: &GameId, folders: &[PathBuf]) {
    let mut all = all_save_folders();
    if folders.is_empty() {
        all.remove(id.as_str());
    } else {
        all.insert(
            id.as_str().to_owned(),
            folders
                .iter()
                .map(|folder| folder.to_string_lossy().into_owned())
                .collect(),
        );
    }
    if let Err(error) = gio::Settings::new(APP_ID).set_value(FOLDERS_KEY, &all.to_variant()) {
        glib::g_warning!("pindu", "Could not keep the save folders of {id}: {error}");
    }
}

/// A save folder of a game, as Save Snapshots lists it.
#[derive(Debug, Clone)]
pub struct SaveFolder {
    /// The folder, as the sandbox reaches it.
    pub path: PathBuf,
    /// The folder, as the host names it.
    pub shown: PathBuf,
    /// Whether the app can reach it now.
    pub reach: Reach,
}

/// The save folders of the game `id`, each with the host's path and whether the app can
/// reach it now, which is checked on a worker thread.
pub async fn folders(id: &GameId) -> Vec<SaveFolder> {
    let paths = save_folders(id);
    let reaches = {
        let (paths, count) = (paths.clone(), paths.len());
        gio::spawn_blocking(move || paths.iter().map(|path| saves::reach(path)).collect())
            .await
            .unwrap_or_else(|_| vec![Reach::Denied; count])
    };
    let mut folders = Vec::new();
    for (path, reach) in paths.into_iter().zip(reaches) {
        let shown = shown(&path).await;
        folders.push(SaveFolder { path, shown, reach });
    }
    folders
}

/// `folder` as the host names it: in the Flatpak, the folder a document portal path
/// stands for.
pub async fn shown(folder: &Path) -> PathBuf {
    file_service::host_path(&gio::File::for_path(folder))
        .await
        .unwrap_or_else(|| folder.to_owned())
}

/// Adds `folder`, picked through the file chooser, to the save folders of the game `id`.
/// The path the sandbox reaches is the one kept, as snapshots read it from inside. An
/// error is what to tell the user.
pub fn add_folder(id: &GameId, folder: PathBuf) -> Result<(), String> {
    let mut folders = save_folders(id);
    let (snapshots, copies) = (default_save_snapshots_dir(), copy_folder());
    let kept: Vec<&Path> = std::iter::once(snapshots.as_path())
        .chain(copies.as_deref())
        .collect();
    match saves::check_new_folder(&folder, &folders, &kept) {
        Ok(()) => {
            folders.push(folder);
            set_save_folders(id, &folders);
            Ok(())
        }
        Err(NewFolderError::AlreadyAdded) => Err(gettext("This folder is a save folder already")),
        Err(NewFolderError::HoldsSnapshots) => Err(gettext(
            "Save snapshots are kept in this folder or around it, so it cannot be a save folder",
        )),
    }
}

/// Removes `folder` from the save folders of the game `id`. The snapshots that hold it
/// keep their copies.
pub fn remove_folder(id: &GameId, folder: &Path) {
    let mut folders = save_folders(id);
    folders.retain(|added| added != folder);
    set_save_folders(id, &folders);
}

/// What to tell the user, in place, about a save folder the app cannot reach (`reach`);
/// `None` for one it reaches.
pub fn unreachable_text(reach: Reach) -> Option<String> {
    Some(match reach {
        Reach::Reachable => return None,
        Reach::Gone => gettext(
            "This folder is not there: it was moved or removed, or its drive is not plugged \
             in. Snapshots leave it out until it is back; if it moved, remove it and add it \
             again.",
        ),
        Reach::NotAFolder => gettext("This is no longer a folder. Snapshots leave it out."),
        Reach::Denied if permission_service::in_flatpak() => gettext(
            "Pindu’s sandbox cannot read this folder. Remove it and add it again through the \
             file chooser, which lets Pindu read it.",
        ),
        Reach::Denied => gettext(
            "Pindu may not read this folder. Snapshots leave it out until its permissions \
             let Pindu read it.",
        ),
    })
}

/// The prefix Pindu manages for `game`, if it is a Windows game in one.
async fn managed_prefix(game: &Game, library: &LibraryService) -> Option<PathBuf> {
    if !saves::offers_snapshots(game) {
        return None;
    }
    let config = library.launch_config(&game.id).await.unwrap_or_default();
    saves::managed_prefix(game, &config, &proton_launch::defaults())
}

/// Restores `snapshot` of `game` into the prefix Pindu manages for it and its save
/// folders, on a worker thread and as a task in Activity, after a safety snapshot of the
/// saves it replaces; then prunes the game's older snapshots. It is refused while the
/// game runs or a snapshot of it is being written, and the game does not start until it
/// is through. An error is what to tell the user.
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
        return Err(busy(&title));
    }
    let prefix = managed_prefix(game, library).await;
    let other_app = saves::offers_snapshots(game) && prefix.is_none();
    let folders = save_folders(&id);
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
            let roots = Roots {
                prefix: prefix.as_deref(),
                folders: &folders,
            };
            let safety = saves::restore(roots, &game_dir, &id, &snapshot, false, taken_at)?;
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
        RestoreError::NoPrefix if other_app => {
            // Translators: {title} is a game's title.
            gettext("“{title}” runs in a prefix another app manages, so its saves are not restored")
                .replace("{title}", &title)
        }
        // Translators: {title} is a game's title.
        RestoreError::NoPrefix => gettext(
            "The prefix of “{title}” has no user folders: start the game once, then restore",
        )
        .replace("{title}", &title),
        // Translators: {folder} is a folder, {title} a game's title.
        RestoreError::NotASaveFolder(folder) => gettext(
            "{folder} is no longer a save folder of “{title}”: add it again to restore this \
             snapshot",
        )
        .replace("{folder}", &shown(&folder).await.display().to_string())
        .replace("{title}", &title),
        RestoreError::FolderUnreachable(folder, _) => {
            // Translators: {folder} is a save folder.
            gettext("Pindu cannot reach {folder}, so nothing was restored")
                .replace("{folder}", &shown(&folder).await.display().to_string())
        }
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

/// What to tell the user who restores or snapshots the saves of the game `title` while
/// they are being copied.
fn busy(title: &str) -> String {
    // Translators: {title} is a game's title.
    gettext("The saves of “{title}” are being copied: try again in a moment")
        .replace("{title}", title)
}

/// The GSettings key holding the folder snapshots are also copied to, as the sandbox
/// reaches it; empty when they are kept only on this computer.
pub const COPY_FOLDER_KEY: &str = "save-copy-folder";

/// The folder the user picked for copies of snapshots, as the sandbox reaches it.
pub fn copy_folder() -> Option<PathBuf> {
    let folder = gio::Settings::new(APP_ID).string(COPY_FOLDER_KEY);
    (!folder.is_empty()).then(|| PathBuf::from(folder.as_str()))
}

/// Takes a snapshot of the saves of `game`, which just exited cleanly: of `prefix`, the
/// prefix Pindu manages for it, if any, and its save folders; then prunes the older ones
/// and copies the newest to the folder the user picked for copies. A failure is said with
/// `toast`, never as a failure of the game. A snapshot of the game still being written
/// makes this one wait for the next exit.
pub fn snapshot_after_exit(
    game: &Game,
    prefix: Option<PathBuf>,
    toast: impl Fn(adw::Toast) + 'static,
) {
    let game = game.clone();
    glib::spawn_future_local(async move {
        if let Some(Err(message)) = snapshot(&game, prefix, Reason::Exit, &toast).await {
            toast(adw::Toast::new(&message));
        }
    });
}

/// Takes a snapshot of the saves of `game` by hand: of the prefix Pindu manages for it,
/// if any, and its save folders, pruned and copied as one taken on exit. It is refused
/// while they are being restored or copied. Returns what to tell the user.
pub async fn snapshot_now(
    game: &Game,
    library: &LibraryService,
    toast: impl Fn(adw::Toast),
) -> String {
    let title = &game.title;
    if restoring(&game.id) {
        return busy(title);
    }
    let prefix = managed_prefix(game, library).await;
    match snapshot(game, prefix, Reason::Manual, &toast).await {
        // Translators: {title} is a game's title.
        Some(Ok(Taken::Written(_))) => gettext("Took a snapshot of the saves of “{title}”"),
        Some(Ok(Taken::Unchanged(_))) => {
            // Translators: {title} is a game's title.
            gettext("The saves of “{title}” are as their newest snapshot holds them")
        }
        // Translators: {title} is a game's title.
        Some(Ok(Taken::NothingToSave)) => gettext("“{title}” has no saves to take a snapshot of"),
        Some(Err(message)) => return message,
        None => return busy(title),
    }
    .replace("{title}", title)
}

/// Takes a snapshot of the saves of `game` in `prefix`, the prefix Pindu manages for it,
/// if any, and its save folders, for `reason`, then prunes the game's older snapshots,
/// and copies the newest snapshot to the folder the user picked for copies. A save folder
/// the app cannot reach is left out and said with `toast`; a copy that could not be made,
/// as when the folder is gone, is said with `toast` and a notification. A failure leaves
/// the earlier snapshots as they were; its error is what to tell the user. `None` when a
/// snapshot of the game is still being written.
async fn snapshot(
    game: &Game,
    prefix: Option<PathBuf>,
    reason: Reason,
    toast: &impl Fn(adw::Toast),
) -> Option<Result<Taken, String>> {
    let (id, title) = (game.id.clone(), game.title.clone());
    let folders = save_folders(&id);
    if prefix.is_none() && folders.is_empty() {
        return Some(Ok(Taken::NothingToSave));
    }
    if !TAKING.with_borrow_mut(|taking| taking.insert(id.clone())) {
        glib::g_info!("pindu", "A snapshot of {id}'s saves is still being written");
        return None;
    }
    let tasks = TaskService::of_application();
    // Translators: {title} is a game's title.
    let label = gettext("Saves of “{title}”").replace("{title}", &title);
    let task = tasks
        .as_ref()
        .map(|tasks| tasks.start(TaskKind::SaveSnapshot, &label, None));
    let (taken_at, copy_folder) = (now(), copy_folder());
    let game_dir = saves::game_snapshots_dir(&default_save_snapshots_dir(), &id);
    let taken = {
        let (id, game_dir, copy_folder) = (id.clone(), game_dir.clone(), copy_folder.clone());
        gio::spawn_blocking(move || {
            let unreachable: Vec<PathBuf> = folders
                .iter()
                .filter(|folder| saves::reach(folder) != Reach::Reachable)
                .cloned()
                .collect();
            let roots = Roots {
                prefix: prefix.as_deref(),
                folders: &folders,
            };
            let taken = saves::take(roots, &game_dir, &id, reason, taken_at)?;
            let pruned = saves::prune(&game_dir);
            let copied = copy_folder.and_then(|folder| copy(&taken, &folder));
            Ok::<_, io::Error>((taken, pruned, copied, unreachable))
        })
        .await
    };
    TAKING.with_borrow_mut(|taking| taking.remove(&id));
    if let (Some(tasks), Some(task)) = (tasks, task) {
        tasks.finish(task);
    }
    let error = match taken {
        Ok(Ok((taken, pruned, copied, unreachable))) => {
            match &taken {
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
            if !unreachable.is_empty() {
                glib::g_warning!(
                    "pindu",
                    "Left the save folders {unreachable:?} of {id} out: they cannot be reached"
                );
                toast(adw::Toast::new(
                    // Translators: {title} is a game's title.
                    &gettext(
                        "A save folder of “{title}” cannot be reached and is left out: Save \
                         Snapshots says why",
                    )
                    .replace("{title}", &title),
                ));
            }
            if let Err(error) = pruned {
                glib::g_warning!(
                    "pindu",
                    "Could not remove old snapshots in {}: {error}",
                    game_dir.display()
                );
            }
            if let (Some(copied), Some(folder)) = (copied, copy_folder) {
                report_copy(&id, &title, &folder, copied, toast).await;
            }
            return Some(Ok(taken));
        }
        Ok(Err(error)) => error.to_string(),
        Err(_) => "the snapshot stopped".to_owned(),
    };
    glib::g_warning!("pindu", "Could not snapshot the saves of {id}: {error}");
    // Translators: {title} is a game's title.
    let message = gettext("Could not back up the saves of “{title}”");
    Some(Err(message.replace("{title}", &title)))
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
            // Translators: {folder} is the folder picked for copies of save snapshots.
            gettext(
                "{folder} is not there. The snapshot stays on this computer, and the next one \
                 is copied once the folder is back.",
            )
            .replace("{folder}", &shown(folder).await.display().to_string())
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
