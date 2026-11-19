# Accessibility polish

Fullscreen mode has Reduce Motion, which also follows the system setting, one Larger Text step of about a quarter,
remappable buttons, a confirm and back swap and a choice of button prompts. The desktop window follows the system's
text size, and high contrast turns the user's own stylesheet off. This workstream makes the controller shell
dependable at living-room distance and for more people; it adds no visual novelty. It is workstream **AC** of the
[product analysis](product-analysis.md#89-accessibility-and-controller-polish-ac), milestone M7 Launch confidence.

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **Text sizes.** Several text sizes in fullscreen mode replace the single Larger Text step, each checked in every
  shipped design at 1080p.
- **Held buttons for destructive actions.** An option, off by default, makes Power Off, Restart, Forget and Stop need
  a held button, with visible hold progress. The hold logic lives in the core and is tested. Restart, Power Off and
  Forget already ask before acting in the cases that lose something; the hold is for people who want more.
- **High contrast and reduced transparency.** A spike runs every shipped fullscreen design with high contrast and
  with reduced transparency (Broadway or gamescope headless) and files an issue per real defect, in M7.
- **Screen reader.** A spike walks every desktop page with Orca, or by inspecting the accessibility tree, notes
  unlabelled controls, wrong roles and focus traps, and files an issue per real defect, in M7.
- **Navigation repeat and dead zone.** Settings for the navigation repeat delay and rate and for the stick dead zone
  used for navigation. They change navigation only and never what a game receives. The repeat and dead-zone logic is
  tested in the core.

## Flatpak, native and Pindu OS

The same in every install: these are fullscreen-mode settings and checks of the app's own interface, with no
permission and no network. They matter most on Pindu OS, where fullscreen mode is the whole interface and is read
from a sofa; every setting works with a controller alone. The navigation settings act only on how the launcher moves
its own focus, so a game, in a gamescope session or on a desktop, receives the controller unchanged.

## Out of scope

- Visual novelty or new designs.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #541 Offer several text sizes in fullscreen mode | Several text sizes, checked in every shipped design at 1080p | |
| #542 Let destructive actions need a held button | A held button for Power Off, Restart, Forget and Stop; off by default | |
| #543 Check the fullscreen designs under high contrast and reduced transparency | Spike: what breaks, per design, and an issue per defect | |
| #544 Review the desktop window with a screen reader | Spike: what breaks, per page, and an issue per defect | |
| #545 Add navigation repeat and dead-zone settings | Repeat delay and rate, stick dead zone, for navigation only | |

The issues #543 and #544 file join this table when they exist.

## Decisions

Taken by the maintainer on 2026-11-19 (analysis
[section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- None of the 2026-11-19 decisions concern this workstream; its issues carry no open product question.
- What gets fixed after the two reviews: settled by #543 (AC-3)'s write-up for high contrast and reduced transparency,
  and by #544 (AC-4)'s for the screen reader, each filing an issue per real defect.
