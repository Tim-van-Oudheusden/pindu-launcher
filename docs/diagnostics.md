# Diagnostics

The environment matrix is wide (Flatpak or native, desktop or gamescope session, which launchers, which tools), the
app sends no telemetry, and the bug form asks reporters to paste version output by hand. Diagnostics builds one report
in the core that a user can copy, save and attach to a bug, with no game title and no home path in it. It is
workstream **DX** of the [product analysis](product-analysis.md#83-diagnostics-dx), milestones M6 One library (#516,
#517) and M7 Launch confidence (#518, #519).

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **The report.** A core function builds it: version and build, how it was installed, distribution, session type and
  gamescope version, each source's state and game count **without titles**, the permission tier, which tools were
  found, the schema version and the last sync's problems. Paths are written with `~` for the home folder, through the
  core function that already does that. A test checks that the report holds no game title and no home path.
- **Desktop.** About's Debugging Information page, which libadwaita's About dialog has and the app leaves empty today,
  carries the report. libadwaita supplies Copy and Save, so no new interface is needed.
- **Bug form.** `.github/ISSUE_TEMPLATE/bug.yml` has a required "Debugging information" field in place of the
  hand-pasted version output. It says where to find the report: on the desktop, About Pindu launcher, Troubleshooting,
  Debugging Information, Copy or Save; in fullscreen mode, Quick Access's diagnostics report, which also saves a copy
  under `reports/` in the app's data folder. GitHub has no check for issue forms, so a change is checked by parsing the
  YAML and previewing the form on GitHub.
- **Fullscreen.** Quick Access's **Diagnostics Report** shows the same report, with the same redaction, as large
  monospace text in pages that fit a 1080p screen, with a counter such as "Page 1 of 3", for a phone to photograph
  page by page. LB and RB, or Previous Page and Next Page, turn the pages; Up and Down scroll a page taller than the
  panel, which widens while the report shows. Each time it opens, a copy is saved to
  `$XDG_DATA_HOME/pindu-launcher/reports/` (`~/.local/share/…`, or the Flatpak's own data folder), named after the
  local time, and the newest ten are kept as Play with Logs keeps logs; the screen names the file with `~`. The core
  (`pindu_core::report_screen`) splits the pages and names and prunes the copies; `diagnostics_service` writes them.
- **A game's log, on request.** The user picks a game and its newest log (Play with Logs keeps the newest ten per
  game) is appended to the report with the same redaction: no home path, and the chosen game's title only because the
  user chose it.

## Flatpak, native and Pindu OS

The report says which of them it came from: how the app was installed, the permission tier (in the Flatpak, whether
games may start on the host) and whether this is a gamescope session. On the desktop, Save goes through libadwaita's
dialog and the file chooser portal, so the Flatpak needs no new permission. A console is the hard case: on Pindu OS
nothing can be pasted and fullscreen mode is the whole interface, so its report goes on screen and into the app's own
data folder, which the Flatpak can already write, as #520 recommended. A USB drive waits until Pindu OS mounts
removable drives and fullscreen mode has a folder picker (#569); a QR code was rejected because it needs a new crate.

## Out of scope

- Uploading anything: the user copies or saves the report and decides where it goes.
- Game titles and full home paths in the default report.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #516 Fill About's Debugging Information | The core report and About's Debugging Information page | |
| #517 Ask for the debugging information in the bug form | `.github/ISSUE_TEMPLATE/bug.yml` | #516 |
| #518 Save a diagnostics report from fullscreen mode | The report from fullscreen mode, delivered where #520 recommends | #516, #520 |
| #519 Add a game's newest log to a report on request | A chosen game's newest log, redacted the same way | #516 |

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Where a console's report goes was settled by #520 (CP-1): on screen in pages to photograph, plus a dated copy in
  `~/.local/share/pindu-launcher/reports/` keeping the newest ten; no USB drive and no QR code for now.
