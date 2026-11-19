# Controller parity

On Pindu OS fullscreen mode is the whole interface: no desktop, no keyboard. Several things the desktop window can do
have no fullscreen route: the fullscreen game page hides developer, genres, description and the launch settings, and
nothing in fullscreen mode reaches Preferences' Sources, Artwork, Windows and Proton pages, Add Game, backups or Play
with Logs. Controller parity closes the gaps a console owner actually needs, starting with a spike that decides which
those are. It is workstream **CP** of the [product analysis](product-analysis.md#84-controller-parity-for-consoles-cp),
milestones M6 One library (#520, #521) and M7 Launch confidence (#522 and the issues #520 files).

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **The list.** A spike goes through every desktop-only flow, says whether a console needs it and why, recommends
  where a console's diagnostics report goes ([diagnostics](diagnostics.md), #518), and files one issue per flow a
  console needs, in M7. The candidates it weighs:
  - game details on the fullscreen game page;
  - entering the SteamGridDB key with the on-screen keyboard;
  - turning sources on and off;
  - choosing a Proton build for a game and downloading builds;
  - adding a game by hand, which needs a way to pick a file with a controller and is the largest of these;
  - backups, which need somewhere to go on a console;
  - Play with Logs and the other Preferences pages.
- **Game details.** Developer, genres and description show on the fullscreen game page in every shipped design, as
  they do on the desktop page; a long description can be scrolled with a controller.
- **The SteamGridDB key.** Quick Access → Settings → Artwork. It is typed hidden with the on-screen keyboard's
  hidden-text dialog (the one Wi-Fi passphrases and Bluetooth PINs use) and stored in the keyring through oo7, never in
  GSettings, files or logs. Without a working keyring the entry says so in place and cannot be chosen. The Prefer
  SteamGridDB Art switch sits beside it, as #520 decided.
- **Sources.** Quick Access's Sources level lists every imported source with its icon, whether it was found on this
  computer and its Import switch, the same setting as Preferences, Sources; changing it syncs as it does there. The
  folder override and Show in Sidebar stay desktop-only.
- **Duplicates.** Quick Access's Duplicates level lists the duplicate suggestions and the groups the sync made, as the
  desktop's Duplicates page does ([game identity](game-identity.md#reviewing-duplicates)). A on a suggestion opens to
  Group and Not the Same Game; A on an automatic group opens to Undo Grouping.
- **A game's Proton build.** For a game Pindu runs through Proton, Quick Access's Performance level lists the
  default and the builds on this machine, newest first, as check rows a controller moves through. It is the same
  setting as the desktop game page's Proton row and applies the next time the game starts. Other games have no list.
- **Proton builds.** Quick Access's **Proton** level picks the default Proton build and the default MangoHud and
  GameMode for new Windows games (the same settings as Preferences' Windows page), lists the downloaded builds, which
  A asks to remove, and, once the player checks, the GE-Proton and Proton-CachyOS releases, which A downloads. A
  download is a task in Activity that can be cancelled there or with A on its row. The default prefix folder and
  the folder save snapshots are copied to stay desktop-only: they need the portal's folder chooser, which grants
  access to a folder the sandbox cannot otherwise write to.
- **Adding a game by hand.** Quick Access's Add a Game adds a native game, an installed Windows game or a Windows
  installer's game in fullscreen mode. Fullscreen mode draws its own picker, as a portal's file chooser runs in another
  process that controller navigation cannot drive. It starts at the Games folder, the home folder and the drives
  mounted under `/run/media/<user>`, lists folders and only the files the step accepts (`pindu_core::file_picker`,
  read through `file_service`), and says that it shows only what the launcher can already read. The title is
  prefilled from the program and typed with the on-screen keyboard. A Windows game's Proton build is picked from the
  same list as Performance's (the default, then the builds on this machine), and it runs through umu as on the
  desktop. Arguments and the working folder stay the desktop dialog's.
- **Collections.** View or X on a collection in the menu drawer's Collections leads to Rename…, Move Up, Move Down
  and Delete…, the desktop sidebar's collection menu. A name, new or changed, is typed with the on-screen keyboard
  in the passphrase dialog, showing its text; Delete asks first, with Cancel chosen.

## Flatpak, native and Pindu OS

The fullscreen routes are the same in every install; on a desktop they are a convenience, on Pindu OS they are the
only way. The desktop window can be driven with a controller (directions move focus, A activates, B goes back) and
Exit Fullscreen Mode leads to it, so switches and lists there are within reach; typing and file choosers are not, and
a desktop window drawn at scale 1 on a television is not the product. The SteamGridDB key needs a working keyring,
an open risk in the Pindu OS spec; in the Flatpak the keyring is reached through the Secret portal as it is today. No
permission is added.

## Out of scope

- Rebuilding desktop-only flows that #520 decides stay desktop-only.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #520 List what fullscreen mode cannot do that a console needs | Spike: every desktop-only flow, the issues to file, #518's destination | This page |
| #521 Show a game's details on the fullscreen game page | Developer, genres and description in every shipped design | |
| #522 Enter the SteamGridDB key in fullscreen mode | The hidden-text dialog and the keyring | #520 |
| #565 Turn sources on and off in fullscreen mode | Quick Access's Sources level with each source's Import switch | |
| #568 Choose a game's Proton build in fullscreen mode | Quick Access, Performance: the default or a build on this machine, the desktop page's setting | #520 |
| #567 Download and remove Proton builds in fullscreen mode | Proton downloads and removal, the defaults for new Windows games | |
| #569 Add a game by hand in fullscreen mode | The fullscreen picker, the steps, the title on the on-screen keyboard | #520 |
| #566 Rename, reorder and delete collections in fullscreen mode | The drawer's level for one collection; names typed with the on-screen keyboard | #520 |

The issues #520 files join this table when they exist.

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Which desktop-only flows a console needs: settled by #520 (CP-1)'s write-up, whose recommendation is adopted.
