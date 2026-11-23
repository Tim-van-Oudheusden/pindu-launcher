# Spec amendments

[`docs/Adwaita Game Library — Main Spec.pdf`](Adwaita%20Game%20Library%20%E2%80%94%20Main%20Spec.pdf) is an export,
so changes to it are recorded here until the maintainer folds them into the spec. Read this page with the PDF: where the
two disagree, this page wins, and an issue wins over both. The reasons behind the amendments below are in the product
analysis ([`docs/product-analysis.md`](product-analysis.md), sections 4.3 and 8.12), issue #551 (PQ-4).

## Non-goals for v1 (spec, page 1)

| Spec says | Status now |
| --- | --- |
| No store front: buying, downloading and updating store games stays in Steam and Heroic | **Stands.** Open in Launcher and Add a Store, which installs Steam, Heroic or Lutris from Flathub, are hand-offs, not a store client (analysis section 13) |
| Not a session or OS shell: no gamescope session, TDP or hardware control (OpenGamepadUI's territory) | **Revised.** Overtaken by 0.5.0 and Pindu OS, see below |
| No emulation front end | **Undecided, still a non-goal.** #546 (EM-1, M9 Breadth) wrote the spec page and a recommendation, [`docs/emulation.md`](emulation.md); the decision stays with the maintainer. Until the maintainer decides, no issue builds emulation |
| No Xbox name, logo or brand green | **Stands**, as AGENTS.md's Safety Boundaries put it: no console branding |
| No accounts or cloud sync | **Stands.** Saves and state travel through folders the user picks (#539) |

### Session and OS shell, revised

Pindu is a session shell. Since 0.5.0, in a gamescope `--steam` session fullscreen mode takes Steam's place: it brings
games to the front and comes back when they end (#390, #404), draws the menu and Quick Access over a running game
(#391, #432), drives MangoApp (#392), hands off to Steam Big Picture (#456) and installs a login session for
gamescope-session-plus through its native packages (#393). Since 0.4.0 the system panel offers power, network,
Bluetooth, audio output and OS updates (#317, [`docs/system-panel.md`](system-panel.md)). On Pindu OS the launcher is
the whole user interface.

What still holds is a hierarchy (analysis section 4.2): the unified library and launching first, the desktop and
fullscreen shells over it second, session conveniences any gamescope session can use third, and system management that
Pindu OS needs last. That last layer grows only through a Pindu OS work package with its own short spec, as the system
panel did. TDP, performance tuning and other hardware control remain out of scope unless such a work package asks for
them.

### First run on a console, revised

The product analysis (section 8.2) turned down a set-up sequence on first start in favour of empty states and prompts in
place, and [`docs/library-health.md`](library-health.md#out-of-scope) listed it as out of scope. That reasoning was about
the desktop app. On Pindu OS fullscreen mode is the whole interface, and the first run on real hardware (pindu-os#51)
showed a new owner finding each step by hand, in the wrong order. Since #794 fullscreen mode shows a set-up checklist in
place of an empty Home when it starts with no games and no store installed: controller, network, design, store, sign
in, each opening the screen that already does it. The desktop keeps empty states and prompts in place. The details are
in [`docs/library-health.md`](library-health.md#decisions).

## Gamescope and handhelds (spec, page 10)

The spec put gamescope focus atoms, overlays, TDP and controller remapping out of scope for v1, pending a spike. The
spike ran (#56, #377); the focus protocol (#390), overlays over a running game (#391, #392) and rebindable buttons in
fullscreen mode (#458) have shipped. TDP stays out of scope, as above.

## Releases (spec, milestones)

The spec plans a Flathub release in M5. Since #226 M5 is the first GitHub release, GitHub Releases are the release
channel, and the Flathub submission (#63, `human-only`) is in Nice to have. After M5 come M6 One library, M7 Launch
confidence, M8 Resilience and M9 Breadth (#549); the milestone descriptions on GitHub hold the gates.

## Backlog

The spec's unscheduled backlog (page 31), its duplicates rule (page 6) and its metadata backlog (page 13), mapped to
the tracker:

| Spec backlog item | Issues | Milestone |
| --- | --- | --- |
| Merging duplicate games across sources into one entry with several launch options (the spec kept duplicates separate in v1) | #501 settles the grouping model; #504 group store, #505 suggestions, #506 one tile with a preferred copy, #507 merge and split by hand, #508 review suggestions, #509 groups in backups | M6 One library |
| List view next to the grid | #558 | M9 Breadth |
| Selection mode for bulk actions | #559 | M9 Breadth |
| itch importer | #556 | M9 Breadth |
| Standalone Legendary importer | #557 | M9 Breadth |
| ProtonDB tier | #533 links Steam games to ProtonDB and PCGamingWiki pages, without fetching a tier; #534 shows Steam Deck compatibility from Steam's own files | M7 Launch confidence |
| PCGamingWiki | #533; also a candidate in #528's provider comparison | M7 Launch confidence, M8 Resilience |
| HowLongToBeat | No issue of its own; an online provider is chosen, or none, in #528, then fetched in #529 | M8 Resilience |
| Gamescope focus integration | Done: #56, #377, #390 (0.5.0) | M4 Fullscreen mode, Nice to have |
| InputPlumber support | No issue; deferred (analysis section 8.12) | — |

Items the analysis adds beyond the spec are tracked as #501–#559 in M6 to M9; the analysis's section 9 lists them by
milestone. Two M9 spikes wrote spec pages whose recommendations the maintainer has yet to decide on: emulation
([`docs/emulation.md`](emulation.md), #546) and the external importer contract
([`docs/external-importers.md`](external-importers.md), #547).
