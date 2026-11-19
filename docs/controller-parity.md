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
- **The SteamGridDB key.** It is typed hidden with the on-screen keyboard's hidden-text dialog (the one Wi-Fi
  passphrases and Bluetooth PINs use) and stored in the keyring through oo7, never in GSettings, files or logs.
  Without a working keyring the entry says so in place.
- **A game's Proton build.** For a game Pindu runs through Proton, Quick Access's Performance level lists the
  default and the builds on this machine, newest first, as check rows a controller moves through. It is the same
  setting as the desktop game page's Proton row and applies the next time the game starts. Other games have no list.

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
| #568 Choose a game's Proton build in fullscreen mode | Quick Access, Performance: the default or a build on this machine, the desktop page's setting | #520 |

The issues #520 files join this table when they exist.

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Which desktop-only flows a console needs: settled by #520 (CP-1)'s write-up, whose recommendation is adopted.
