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
  Symbolic links are never followed or copied. The registry, `drive_c/ProgramData` and saves inside a game's install
  folder are not covered. Ludusavi is neither called nor its manifest used.
- **Snapshot on exit.** After a Windows game in a prefix Pindu manages exits cleanly, which is umu ending by itself
  with status 0, its saves are copied as a plain folder, atomically. It runs as a task in Activity, and a failed write
  says so in a toast and leaves the earlier snapshots intact. See [How snapshots are stored](#how-snapshots-are-stored).
- **Restore.** Both shells list a game's snapshots with time, size and the exact paths included. A restore first takes
  a safety snapshot, refuses while the game runs, and restores only inside the prefix or the folders the user
  configured.
- **A copy elsewhere.** The user can pick a folder through the file chooser portal, which can be one that Syncthing,
  Nextcloud or a drive already syncs; snapshots are copied there too, and the choice survives a restart. A folder that
  is gone is reported, not treated as a failure of the game. There is no Pindu cloud.
- **Save folders by hand.** The user can add save folders for any game through the file chooser; snapshots and
  restores cover them, and a folder the app cannot reach is explained in place.

## How snapshots are stored

`crates/pindu-core/src/saves.rs` holds the logic, `crates/pindu-ui/src/save_service.rs` runs it on a worker thread
after `proton_launch` sees the game exit.

- **Managed prefixes only.** A prefix is managed when Pindu chose it: `<prefixes folder>/<native ID>`, by default
  `~/Games/pindu-launcher/prefixes/<native ID>`. A prefix the user pointed at another tool's folder is left out.
- **Layout.** `~/Games/pindu-launcher/save-snapshots/<native ID>/<UTC time, as 2026-11-19T18-42-07Z>/`, holding
  `files/`, which mirrors the paths under `drive_c/users/`, and `snapshot.json`: the format version, game ID, prefix,
  time, reason (`exit` or `before-restore`), the user folders included, the exclusion rules used, and every file with
  its path, size and modification time. Snapshots sit beside the prefixes, so removing a prefix keeps them, and on
  the same filesystem, so on btrfs the copies share the files' blocks until the game rewrites them.
- **Atomic.** A snapshot is written into `.<time>.partial/`, every file and `snapshot.json` flushed to disk, then
  renamed into place. A failed write removes its partial folder, and one a crash left behind is removed before the
  next snapshot.
- **Unchanged saves are skipped.** When every file's path, size and modification time equal the newest snapshot's,
  none is written, so quitting from a menu five times does not push out the last five saves.
- **Retention.** The newest five snapshots taken on exit and, separately, the newest two taken before a restore are
  kept; older ones are removed after each new snapshot.

Excluded by default, with paths relative to a user folder and names matched whatever their case:

- `Temp`, `AppData/Local/Temp`, `AppData/Local/Programs`, `AppData/Local/dxvk`, `AppData/Roaming/wine_gecko`,
  `AppData/Local/D3DSCache`, `AppData/Local/NVIDIA`;
- `AppData/Local/Epic Games`, `AppData/Local/EpicGamesLauncher`, `AppData/Local/EpicOnlineServicesUIHelper`,
  `AppData/Roaming/EasyAntiCheat`;
- `AppData/Roaming/Microsoft` and `AppData/Local/Microsoft` (credential stores);
- `Pictures`, `Videos`, `Music` and `Downloads`, and the same names under `Documents`;
- at any depth, folders named `Local Storage`, `Session Storage`, `IndexedDB`, `*cache*`, `Crashes`, `CrashDumps` and
  `Crash Reports`, and files named `Cookies`, `Cookies-journal`, `Login Data` and `Web Data`.

## Flatpak, native and Pindu OS

The first version needs no new permission. Prefixes Pindu manages live by default under
`~/Games/pindu-launcher/prefixes`, which the Flatpak may write to. The Flatpak cannot read `~/.local/share` or
`~/.config`, where most native games save; it reaches those only through folders the user picks in the file chooser,
and a folder it cannot reach is explained in place, never shown as an error. Wider access would be a permission, which
is the maintainer's call. A native install has no such limit. Picking a destination folder needs a file chooser a
controller can drive, so on a console the first version keeps snapshots on the machine.

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
