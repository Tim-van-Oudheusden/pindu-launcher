# Launch outcomes and safe start

When a game fails to start or exits at once, Pindu has little to say about why: nothing records how a launch ended,
and the program's output is kept only if the user chose Play with Logs beforehand. This workstream records each
launch's outcome, explains the last failure with a next step that exists, links Steam games to the compatibility
pages people already use, shows Steam Deck compatibility from Steam's own files, and adds Start Safely to tell a
broken overlay or wrapper from a broken game. It is workstreams **CL** and **LP** of the product analysis
([8.6](product-analysis.md#86-launch-outcomes-and-failed-launch-help-cl),
[8.7](product-analysis.md#87-safe-start-and-named-launch-profiles-lp)), milestone M7 Launch confidence.

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **Record the outcome** of each launch of a game Pindu runs: the route, the Proton build, start and end, the exit
  status, and whether a window appeared where the session can tell. For a hand-off to a launcher, whether the launcher
  answered. The store keeps the last few outcomes per game.

  As built (#531): `launch_outcomes` (migration 0009) holds a row per launch, written as `pending` when it begins
  and finished when it ends; `pindu_core::launch_outcome` has the types and `Store::begin_launch`, `end_launch` and
  `launch_outcomes` read and write them. The route is `host`, `proton` (with the build's internal name), `runtime` or
  `hand-off`. A game Pindu runs ends `exited` with its status, `signalled` with the signal (Stop's `SIGTERM` is not a
  failure), `lost` when Pindu lost track of it, or `not-started` when it never started, including a missing umu or
  Proton build. A hand-off ends `handed-off` when the launcher answered, `hand-off-failed` when it could not be
  opened, or `no-answer` after the launch overlay's hand-off timeout (10 seconds); a launcher that answers later still
  counts as answering. A launch Pindu stopped following because it quit first stays `pending`. Whether a window
  appeared is recorded only in a `--steam` gamescope, from the game's app ID among gamescope's focusable or focused
  apps; elsewhere it is unknown. The newest five per game are kept, and a game's outcomes go with it; backups do not
  include them.
- **A small log for every launch.** Every launch of a game Pindu runs keeps a small, bounded log beside Play with
  Logs' full logs, so a failure has output to show even when Play with Logs was not chosen.

  As built (#531): a launch without Play with Logs or Start Safely keeps a short log in the game's logs folder,
  `<start time>.short.log`: the usual header, then the last 64 KiB of the game's (or umu's) output and errors, read
  through a pipe and written when it exits, with a line saying how many earlier lines were left out. The newest five
  short logs per game are kept, apart from the newest ten full logs, which Open Log and the diagnostics report still
  use. The outcome names its log, short or full. A host game whose launcher tracks its playtime itself (Faugus) keeps
  no short log, since its output would have nowhere to go once Pindu quits.
- **Explain the last failure** on the game page and in fullscreen mode's options sheet, offering only the next steps
  that exist for that game: How to Enable, How to Install, another Proton build, Start Safely, Open Log.

  As built (#532): `pindu_core::failed_launch::explain` reads the game's newest outcome with what is known now
  (whether the game needs a permission the sandbox lacks, whether umu-launcher is installed, the Proton builds on
  the machine, and whether the launch's log is still kept). A launch that ended well, was stopped, lost track of,
  is still running or was handed to a launcher that answered is not explained. Otherwise the reason is one of: did
  not start; umu-launcher is not installed; the Proton build it was to run with is gone; no Proton build is
  installed; quit with an error code, or quit straight after it started (within 10 seconds); crashed (`SIGILL`,
  `SIGABRT`, `SIGBUS`, `SIGFPE` or `SIGSEGV`); something else ended it by a signal; the launcher could not be
  opened; or the launcher did not answer. The steps, in this order: **How to Enable** (Preferences at its
  Permissions group) for a host game that needs the permission; **How to Install umu-launcher** for a game run
  through umu without it; **Choose Another Proton Build** for a Windows game when a build other than the one it
  ran with is installed; **Start Safely** for a game Pindu runs, unless a missing permission, umu-launcher or
  Proton build would stop it too; **Open Log** while the launch's log, full or short, is kept. A failed hand-off
  offers none. The desktop game page shows a Last Launch Failed group with the reason and a button per step;
  Choose Another Proton Build moves focus to the page's Proton row. In fullscreen mode the game page shows the
  reason and points to Options, whose sheet shows the reason and the steps above its actions (Up from the first
  action), and Choose Another Proton Build opens Quick Access's Performance level. Both read the outcome again
  whenever a launch of the game begins or ends.
- **Link out** for Steam games: the desktop game page and fullscreen mode's options sheet open the ProtonDB and
  PCGamingWiki pages for the Steam app ID in the browser, through the portal. The app makes no request itself. Other
  games get the links once [game identity](game-identity.md) or [metadata](shelves-and-metadata.md) gives them a Steam
  ID.
- **Steam Deck compatibility.** Steam's rating is read offline from Steam's own files, where #523 found it, into the
  metadata table's source values (#526), and shown on both game pages.

  As built (#534): the Steam importer reads `common.steam_deck_compatibility` from `appinfo.vdf` on every sync:
  `category` 0 to 3 becomes `DeckRating` Unknown, Unsupported, Playable or Verified (any other number, or no map,
  means no rating), and a non-zero `test_timestamp` becomes when Valve tested the game. It is the game's
  `deck-compatibility` source value in `game_metadata`, stored as `{"rating": "verified", "tested_at": …}`, and the
  About group of the game page, desktop and fullscreen alike, shows it in a Steam Deck row in words ("Verified,
  tested" and the day in the locale's format), with no Valve artwork. The Deck `tests` list and the SteamOS and
  Steam Machine ratings next to it are not read.
- **Start Safely.** A one-off start of a game Pindu runs that ignores its wrappers, MangoHud, GameMode and its own
  environment variables, and writes a log. Its saved settings are unchanged. The launch argv is built and tested in
  the core. It is offered from the desktop menu, from fullscreen mode's options sheet, and as a next step after a
  failed launch.

## Flatpak, native and Pindu OS

Outcomes, logs and Start Safely cover the games Pindu runs itself, in every install. In the Flatpak those games start
on the host only with the host-spawn override; a launch refused for a missing permission is explained in place, as
today, and never recorded or shown as a failed game. Whether a window appeared is recorded only where the session can
tell, such as a gamescope session on Pindu OS; elsewhere the outcome leaves it unknown. The links open through the
portal, so the Flatpak needs no new permission, and everything works with a controller alone in fullscreen mode.

## Out of scope

- Fetching ProtonDB ratings: ProtonDB documents no public API, so anything beyond a link starts with a spike on its
  terms.
- Choosing a runtime for store games: their launchers keep that.
- Named launch profiles (TV, Handheld, Debug), deferred to P3. Per-game launch settings already hold everything a
  profile would override.
- gamescope resolution and scaling hints: Pindu does not wrap games in a gamescope of its own, and inside a session
  those settings belong to the session.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #531 Record how each launch ended | Outcomes per game and the bounded log per launch | #526, for migration order |
| #532 Explain a failed launch and offer the next step | The last failure and its next steps in both shells | #531, #535 |
| #533 Link Steam games to ProtonDB and PCGamingWiki | Links out through the portal; no request by the app | |
| #534 Show Steam Deck compatibility from Steam's own files | Steam's rating, read offline, on both game pages | #523, #526 |
| #535 Start a game once in safe mode | Start Safely from both shells | |

#531 adds the last migration of the current chain, after #526's.

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Every launch of a game Pindu runs keeps a **small, bounded log**; Play with Logs stays for the full output.
- No named launch profiles now; **Start Safely only**.
