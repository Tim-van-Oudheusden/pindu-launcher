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
- **A small log for every launch.** Every launch of a game Pindu runs keeps a small, bounded log beside Play with
  Logs' full logs, so a failure has output to show even when Play with Logs was not chosen.
- **Explain the last failure** on the game page and in fullscreen mode's options sheet, offering only the next steps
  that exist for that game: How to Enable, How to Install, another Proton build, Start Safely, Open Log.
- **Link out** for Steam games: the desktop game page and fullscreen mode's options sheet open the ProtonDB and
  PCGamingWiki pages for the Steam app ID in the browser, through the portal. The app makes no request itself. Other
  games get the links once [game identity](game-identity.md) or [metadata](shelves-and-metadata.md) gives them a Steam
  ID.
- **Steam Deck compatibility.** Steam's rating is read offline from Steam's own files, where #523 found it, into the
  metadata table's source values (#526), and shown on both game pages.
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
