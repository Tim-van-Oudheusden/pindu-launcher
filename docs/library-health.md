# Library Health

Whether a game can start depends on conditions spread across the app: whether its source was found, native or
Flatpak, host access, umu, the MangoHud layer, system services. Each problem is explained where it bites, but nothing
shows the whole picture, and what a sync could not read is gone with a ten-second toast. Library Health is one view
over one report the core computes, plus a one-time summary of what the first sync found. It is workstream **LH** of
the [product analysis](product-analysis.md#82-library-health-and-first-run-lh), milestone M6 One library.

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **Sources in Preferences first.** Preferences, Sources shows when each source last synced and lists that sync's
  problems, from what `source_states` already stores. Nothing new is stored.
- **One report from the core**, in four sections:
  - **Sources:** found or not, where, native or Flatpak, how many games, last sync, and the problems of that sync.
  - **Launching:** host access, umu, the Proton builds installed, MangoHud, GameMode, and whether this is a gamescope
    session.
  - **Artwork:** whether a SteamGridDB key is stored, and the size of the cache.
  - **Library data:** where the library file is, its schema version and the last backup.

  Each problem names a next step that already exists: the Preferences page, the `flatpak override` command, How to
  Install.
- **Desktop.** A Library Health page of its own, opened from the main menu, shows the report and refreshes after a
  sync. Every next step is a working link or button.
- **Fullscreen.** The same report is a Quick Access level.
- **First-run summary.** After the first sync, a summary names the launchers found, the games imported and what needs
  attention, once, in both shells, and points at Library Health. Once dismissed it never shows again.
- **Last backup.** An export records its time, and Library data shows it. It is stored without a database migration
  (for example a GSettings key), so it stays out of the migration chain.

## Flatpak, native and Pindu OS

Rows the session cannot have are hidden or explained by the availability rules the
[system panel](system-panel.md#what-is-available) already uses; a missing permission is never shown as an error. In
the Flatpak, Launching says whether games may start on the host and names the `flatpak override` command, as
Preferences, General does; the core's tests cover a Flatpak without host access and a native install. On Pindu OS and
in any gamescope session the report says it is one, and fullscreen mode's Quick Access level is the place to read it
with a controller alone. No permission is added.

## Out of scope

- Repairing anything automatically: the view reports and points, the user acts. The app cannot grant itself host
  access; by design the user runs the command.
- Signing in to stores, which stays with the stores.
- A multi-step setup sequence on first start: empty states and prompts in place stay the pattern, and the first-run
  summary replaces it.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #510 Show each source's last sync and problems in Preferences | Preferences, Sources: last sync and problems from `source_states` | |
| #511 Build the library health report in the core | The four-section report and its next steps | |
| #512 Add Library Health to the desktop window | A page of its own from the main menu | #511 |
| #513 Add Library Health to fullscreen mode's Quick Access | The report as a Quick Access level | #511 |
| #514 Summarise what the first sync found | The one-time first-run summary in both shells | #512, #513 |
| #515 Record when the library was last backed up | The export's time, shown under Library data | #511 |

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Library Health is a **page of its own**, opened from the main menu, in the desktop window; in fullscreen mode it is a
  Quick Access level.
- There is a **one-time first-run summary** after the first sync.
