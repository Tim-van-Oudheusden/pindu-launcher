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
  sync. Every next step is a working link or button: a **Needs Attention** group at the top lists each problem with
  **Open Sources** (Preferences at its Sources page), **Copy Command** (the `flatpak override` command) or **How to
  Install** (umu-launcher's instructions). The UI-side `HealthService` (`crates/pindu-ui/src/health_service.rs`)
  gathers the report's facts through the existing services, for both shells.
- **Fullscreen.** The same report is a Quick Access level (`QuickAccessLevel::LibraryHealth`, `fullscreen_health.rs`)
  over the same `HealthService`, its entry subtitled with how many problems need attention. Every row takes focus, so
  it reads with a controller alone; a problem's row takes its next step on A where fullscreen mode can
  (`NextStep::reach_in_fullscreen`): Preferences at its Sources page, Preferences at the host access command, or
  umu-launcher's instructions. Over a running game the row says the step waits for the game to end, since Preferences
  and web pages open over the launcher; in a gamescope session, which has no browser, How to Install shows the address
  to read on another device. Rows the session cannot have, such as host access outside the Flatpak, are left out.
- **First-run summary.** After the first sync that finds something, a summary names the launchers found, the games
  imported and what needs attention, once, in both shells, and points at Library Health. Once dismissed it never shows
  again. A sync owes it while it is the library's first or the library had no game before it
  (`health::summary_owed`), so a new console whose first syncs find nothing gets it once its first games come in
  (#810). It does not show when the sync found no launcher and no game, nor, while the set-up checklist shows, before
  the first games: a store that has run but holds nothing yet never covers the checklist. The core decides
  (`health::is_first_sync`, `health::summary_owed` and `health::first_sync_summary` over the report gathered after
  the sync); the `first-sync-summary-shown` GSettings key records the dismissal. Both shells show it as an Adwaita
  alert (`first_sync_summary.rs`), read and closed with a controller alone in fullscreen mode, whose Open Library
  Health opens the desktop page or Quick Access's level.
- **Last backup.** A successful export records its time in the `last-backup` GSettings key (seconds since the Unix
  epoch, 0 for never), so it stays out of the migration chain; Library data shows it. The library service reads it
  for the report (`LibraryService::last_backup`). The copies kept before a migration do not count as backups here.

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
- A multi-step setup sequence on first start **on the desktop**: empty states and prompts in place stay the pattern
  there, and the first-run summary replaces it. Fullscreen mode now has a set-up checklist (#794), see Decisions.

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

Changed by the maintainer on 2026-11-23 for consoles (#794): the reasoning against a set-up sequence was about the
desktop app, where the desktop provides the network, Bluetooth and a way to open a store. A console has none of those
outside the launcher, so **fullscreen mode shows a set-up checklist** in place of an empty Home when it starts with no
games and no store installed (`pindu_core::setup`, `setup_checklist.rs`): Connect a Controller (Quick Access's
Bluetooth, or its Controller section without one), Join a Network where the system panel offers it, Choose a Design
until the design was changed, Add a Store, and Sign In to the installed store (Steam through Big Picture, Heroic and
Lutris in their own window, #797; the Flatpak opens Heroic and Lutris only with host access, and without it the entry
says to sign in on a desktop). Each entry opens the screen that already does the step and is marked done from the
live state; none is forced. It is gone for good once a game is in the library. Hide This List keeps it hidden (the
`fullscreen-setup` GSettings key), and Quick Access → Manage → Set-Up Checklist brings it back. It shows for any
fullscreen start with nothing set up, not only on Pindu OS; the desktop window shows nothing of it. A first controller
connects by cable, or the keyboard drives the list, before a wireless one pairs over Bluetooth.
