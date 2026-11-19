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
- **Fullscreen.** The report is reachable from fullscreen mode with a controller alone and delivered where #520's
  write-up recommends: a file in a known folder, a USB drive, or text on screen to photograph.
- **A game's log, on request.** The user picks a game and its newest log (Play with Logs keeps the newest ten per
  game) is appended to the report with the same redaction: no home path, and the chosen game's title only because the
  user chose it.

## Flatpak, native and Pindu OS

The report says which of them it came from: how the app was installed, the permission tier (in the Flatpak, whether
games may start on the host) and whether this is a gamescope session. On the desktop, Save goes through libadwaita's
dialog and the file chooser portal, so the Flatpak needs no new permission. A console is the hard case: on Pindu OS
nothing can be pasted and fullscreen mode is the whole interface, so where its report goes is #518's job, settled by
#520.

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

- Where a console's report goes is settled after #520 (CP-1), whose write-up recommends the destination for #518
  (DX-3).
