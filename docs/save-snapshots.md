# Save snapshots

Saves are the one part of a library that cannot be downloaded again, and Pindu's backup covers only its own state.
Save snapshots copy a Windows game's saves out of the prefix Pindu manages for it when the game exits, keep the last
few, and put one back on request. It is workstream **SV** of the
[product analysis](product-analysis.md#88-save-snapshots-sv), milestone M8 Resilience.

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **What a snapshot holds.** The spike in #536 settled it: every real folder under the prefix's `drive_c/users/`
  (Proton prefixes have `steamuser` and `Public`), minus temporary files, installed software and runtimes, launcher
  and anti-cheat runtimes, credential stores, embedded browser profiles, caches, crash dumps and media libraries.
  Symbolic links are never followed or copied. The registry and `drive_c/ProgramData` are not covered, nor saves
  inside a game's install folder unless the user adds it as a save folder. Ludusavi is neither called nor its manifest
  used.
- **Snapshot on exit.** After a Windows game in a prefix Pindu manages exits cleanly, which is umu ending by itself
  with status 0, its saves are copied as a plain folder, atomically. It runs as a task in Activity, and a failed write
  says so in a toast and leaves the earlier snapshots intact. See [How snapshots are stored](#how-snapshots-are-stored).
- **Restore.** Both shells list a game's snapshots with time, size and the exact paths included: **Save Snapshots…**
  in the desktop game page's menu, and Save Snapshots in fullscreen mode's options sheet, which a controller drives
  alone. A restore asks first, takes a safety snapshot, refuses while the game runs, and restores only inside the
  prefix's user folders and the game's save folders. See [How a restore works](#how-a-restore-works).
- **A copy elsewhere.** The user can pick a folder through the file chooser portal, which can be one that Syncthing,
  Nextcloud or a drive already syncs; snapshots are copied there too, and the choice survives a restart. A folder that
  is gone is reported, not treated as a failure of the game. There is no Pindu cloud.
- **Save folders by hand.** The user can add save folders for any game through the file chooser; snapshots and
  restores cover them, and a folder the app cannot reach is explained in place. See [Save folders](#save-folders).

## How snapshots are stored

`crates/pindu-core/src/saves.rs` holds the logic, `crates/pindu-ui/src/save_service.rs` runs it on a worker thread
after `proton_launch` or `launch_service` sees the game exit, and when the user takes one by hand.

- **Managed prefixes only.** A prefix is managed when Pindu chose it: `<prefixes folder>/<native ID>`, by default
  `~/Games/pindu-launcher/prefixes/<native ID>`. A prefix the user pointed at another tool's folder is left out; the
  game's save folders are still covered.
- **Layout.** `~/Games/pindu-launcher/save-snapshots/<native ID>/<UTC time, as 2026-11-19T18-42-07Z>/`, holding
  `files/`, which mirrors the paths under `drive_c/users/`, `folders/<index>/` for each save folder, and
  `snapshot.json`: the format version (2 since save folders; format 1 is still read), game ID, prefix (`null` for a
  game with save folders alone), time, reason (`exit`, `manual` or `before-restore`), the user folders included, the
  exclusion rules used, every file with its path, size and modification time, and each save folder with its path and
  files. Snapshots sit beside the prefixes, so removing a prefix keeps them, and on the same filesystem, so on btrfs
  the copies share the files' blocks until the game rewrites them.
- **Atomic.** A snapshot is written into `.<time>.partial/`, every file and `snapshot.json` flushed to disk, then
  renamed into place. A failed write removes its partial folder, and one a crash left behind is removed before the
  next snapshot.
- **Unchanged saves are skipped.** When every file's path, size and modification time equal the newest snapshot's,
  in the prefix and in each save folder, none is written, so quitting from a menu five times does not push out the
  last five saves.
- **Retention.** The newest five snapshots taken on exit or by hand and, separately, the newest two taken before a
  restore are kept; older ones are removed after each new snapshot and after each restore.
- **Copies to a folder the user picks.** Preferences → Windows → Save Snapshots → Copy Snapshots To picks a folder
  through the file chooser portal; GSettings key `save-copy-folder` keeps it, so it survives a restart. In the
  Flatpak the key holds the document portal path the sandbox writes through, and Preferences shows the host's path,
  as for source folders. After each snapshot on exit or by hand, the game's newest snapshot is copied to
  `<folder>/<native ID>/<snapshot name>/`, laid out as above, the same atomic way (`saves::copy_to`); a snapshot
  already there is skipped, so one missed while the folder was gone follows on the next exit. The copies are pruned
  by the same retention rules. A folder that is gone, such as a drive that is not plugged in, is not made again: a
  toast and a notification in Quick Access say the snapshot stays on this computer, and the game's exit is not
  treated as a failure. A copy that fails otherwise is reported the same way and leaves no partial folder.

Excluded by default, with paths relative to a user folder and names matched whatever their case:

- `Temp`, `AppData/Local/Temp`, `AppData/Local/Programs`, `AppData/Local/dxvk`, `AppData/Roaming/wine_gecko`,
  `AppData/Local/D3DSCache`, `AppData/Local/NVIDIA`;
- `AppData/Local/Epic Games`, `AppData/Local/EpicGamesLauncher`, `AppData/Local/EpicOnlineServicesUIHelper`,
  `AppData/Roaming/EasyAntiCheat`;
- `AppData/Roaming/Microsoft` and `AppData/Local/Microsoft` (credential stores);
- `Pictures`, `Videos`, `Music` and `Downloads`, and the same names under `Documents`;
- at any depth, folders named `Local Storage`, `Session Storage`, `IndexedDB`, `*cache*`, `Crashes`, `CrashDumps` and
  `Crash Reports`, and files named `Cookies`, `Cookies-journal`, `Login Data` and `Web Data`.

In a save folder only the rules for names at any depth apply; the paths above are a prefix's user folders' own.

## Save folders

Any game can have save folders: a native game's saves, a Windows game's saves in its install folder, or a game in a
prefix another app manages. Snapshots hold them beside the prefix, if the game has one Pindu manages.

- **Added through the file chooser, on the desktop.** **Save Snapshots…** in the desktop game page's menu lists the
  game's save folders under Save Folders, and Add Folder picks one through the file chooser portal; GSettings key
  `save-folders` keeps them by game ID, so they survive a restart, without a change to the library's database. In the
  Flatpak it holds the document portal path the sandbox reads and writes through, and the list shows the host's path.
  A folder that is one of the game's already, or that holds the snapshots folder or the folder copies go to, or lies
  in either, is refused with a toast saying why. Fullscreen mode lists a game's save folders and removes one, but
  cannot add one: its file chooser runs in another process that a controller cannot drive (#520). Its options sheet
  offers Save Snapshots for a Windows game we run and for any game with save folders. Removing a save folder keeps
  the snapshots' copies of it.
- **When snapshots are taken.** After a game Pindu starts itself, and whose session it tracks, exits cleanly, as for
  a managed prefix, and with Take Snapshot Now in Save Snapshots, for any game, including one a store launcher starts.
  A snapshot taken by hand counts with those taken on exit.
- **A folder the app cannot reach is explained in place.** Each folder in the list is checked when the dialog opens.
  One that is gone (moved, removed, on a drive that is not plugged in, or, in the Flatpak, no longer shared through
  the document portal), that is no longer a folder, or that the app may not read says so in its row, with what to
  do: in the Flatpak, to remove it and add it again through the file chooser, which lets the sandbox read it. A
  snapshot leaves such a folder out and says so in a toast, and the rest of the saves are still copied.

## How a restore works

`saves::restore` does it, and `save_service::restore` runs it on a worker thread as a task in Activity; the dialog in
`crates/pindu-ui/src/save_snapshots_dialog.rs` lists the snapshots and asks before a restore.

- **Never while the game runs.** A restore is refused while the game runs or a snapshot of it is being written, and
  the game does not start until the restore is through. The dialog says to quit the game and turns Restore off.
- **Checked before anything is written.** The snapshot must be the game's own. A snapshot holding the prefix needs a
  prefix Pindu manages with a real `drive_c/users/` folder, and every file it holds there must lie in a user folder,
  outside the exclusion rules. Every save folder it holds must be one of the game's save folders now and one the app
  can reach, and every file it holds there must lie below that folder, outside the rules for names at any depth. No
  file may have `..` or a symbolic link on its way, in the prefix, the save folder or the snapshot. Otherwise nothing
  is touched.
- **A safety snapshot first.** The saves being replaced, in the prefix and every save folder the app can reach, are
  snapshotted with the reason `before-restore`, unless they are as the newest snapshot holds them already. The newest
  two of those are kept, apart from the five taken on exit or by hand, so restores never push out the snapshots taken
  on exit, and old ones are pruned only after the restore, never the one being restored.
- **Mirror, not merge.** Each file is copied beside its place, flushed to disk, given its recorded modification time
  and renamed over the old one, so a failed copy leaves that file as it was. Files under the user folders, or in a
  save folder the snapshot holds, that the snapshot does not hold are removed, except what the exclusion rules leave
  out: caches, runtimes, credential stores and the rest stay as they are. Nothing outside `drive_c/users/` and those
  save folders is touched; a save folder added after the snapshot was taken stays as it is.

## Flatpak, native and Pindu OS

The first version needs no new permission. Prefixes Pindu manages live by default under
`~/Games/pindu-launcher/prefixes`, which the Flatpak may write to. The Flatpak cannot read `~/.local/share` or
`~/.config`, where most native games save; it reaches those only through folders the user picks in the file chooser,
and a folder it cannot reach is explained in place, never shown as an error. Wider access would be a permission, which
is the maintainer's call. A native install has no such limit. Picking a destination folder or a save folder needs a
file chooser a controller can drive, so on a console the first version keeps snapshots on the machine and save
folders are added from the desktop.

## Out of scope

- A Pindu account or cloud.
- Saves that Steam or Heroic already sync: store cloud saves stay with the stores.
- Emulated games.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #536 Decide what a save snapshot holds and how it is stored | Spike: included folders, measured sizes, folder layout, Ludusavi | |
| #537 Snapshot a Windows game's saves when it exits | Atomic folder copies after a clean exit, the last few kept | #536 |
| #538 Restore a save snapshot | The list in both shells, the safety snapshot, no restore while running | #537 |
| #539 Copy save snapshots to a folder the user picks | A destination folder through the file chooser | #537 |
| #540 Add save folders by hand for any game | Save folders for any game, explained where the app cannot reach them | #538 |

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- First version covers **managed prefixes only**.
- Snapshots are **plain folder copies**: no archive crate, no new dependency.
- Ludusavi: settled by #536 (SV-1)'s write-up, whose recommendation is adopted.
