---
title: "Pindu Launcher - Product and Feature Development Analysis"
author:
  - "OpenAI (original analysis)"
  - "Claude (review and revision against the repository)"
date: "2026-11-19"
toc: true
toc-depth: 2
---

# Executive summary

Pindu Launcher is past the "small GNOME game library" stage. Fifteen days after its first commit it has a launcher-agnostic desktop library, a controller-first fullscreen mode, own-run Windows games through umu and Proton, play history, collections, artwork management, GNOME Shell search, backups, a gamescope session role with overlays over running games, button remapping, a system panel for power, network, Bluetooth, audio and OS updates, and a companion OS, Pindu OS, whose whole user interface it is. The newest release is 0.5.1, tagged on 19 November 2026, one day after 0.5.0.[1][2][18]

The strategic conclusion of the original analysis stands: Pindu should **not** become another Heroic or Lutris. Its position is the layer *above* launchers. It finds games from several owners, shows them as one local library on the desktop and on the couch, and leaves accounts, installs and updates with Steam, Heroic, Lutris, Bottles and Faugus. The project already says so itself: importers are read-only, launchers stay in charge, the library works offline, and the shells never read files or start processes.[1][3]

Checking the analysis against the source changed three things that matter for planning:

1. **Much of the proposed data model already exists.** Source health is stored on every sync and only shown as a ten-second toast. Developer, genres and description are in the schema and on the desktop game page. Launch settings already cover arguments, environment, Proton build, prefix and wrapper commands. Several proposals are therefore smaller than they looked (Library Health, diagnostics, shelves) and one is larger (game identity, because everything is keyed on the per-source game ID). Section 2.5 has the inventory.
2. **The scarce resource is the maintainer, not implementation.** The agent pipeline produced about 600 commits and seven releases in fifteen days. What does not scale is the maintainer's decisions, the line-by-line review of high-risk changes, the human-only work and the checks on real hardware. Issues should therefore put decisions first, and section 14 lists the ones to take before anything is filed.
3. **Pindu OS makes fullscreen mode the interface.** A console has no desktop environment and no keyboard. Fullscreen mode has no route to most of Preferences, to adding a game by hand, to the SteamGridDB key or to a game's details and launch settings. The desktop window can be driven with a controller, so some of that is reachable by leaving fullscreen mode, but typing and file choosers are not, and a desktop window on a television is not the product. Controller parity is a roadmap item of its own, and "works with a controller alone" belongs in every acceptance checklist.

The recommended order of work:

1. **One game, several copies.** Group the same title from different sources under one tile with a preferred copy. This is already in the product spec's backlog.[9]
2. **Library Health and diagnostics.** Show what is already stored: what each source found, when, and what it could not read; fill About's Debugging Information.
3. **Controller parity for consoles.** Decide which desktop-only flows a console needs and bring them into fullscreen mode.
4. **Shelves and metadata from data Pindu already has.** Saved filters as dynamic collections, then the fields the launchers' own files hold, before any online provider.
5. **Launch outcomes.** Record how a launch ended, explain a failure, and offer a one-off safe start.
6. **Save snapshots** for the Windows games Pindu runs itself, within what the sandbox can reach.
7. **Later, each after a decision:** an online metadata provider, emulation (a stated non-goal of the v1 spec), and an external importer contract.

Because a new version currently appears every day or two, the roadmap in section 9 is written as milestones M6 to M9 with gates, the way M1 to M5 were, and leaves version numbers to the release process.

# 1. Scope, method and how to use this document

## 1.1 What this revision is based on

The original analysis was written from the repository's public pages without a clone. This revision re-checked it against:

- **pindu-launcher** at commit `0dcd666` on `main` (19 November 2026, version 0.5.1), from a clone: the README, `AGENTS.md`, `CONTRIBUTING.md`, `SECURITY.md`, `AI_DISCLOSURE.md`, the guides under `docs/` (releasing, metrics, system panel, risk tiers, multi-agent), the product spec and the Cartridges research in the parts that bear on this analysis, the Cargo and Flatpak manifests, the schema migrations, the core model and importers, the UI definitions, the issue forms, the workflows and the risk rules.
- **pindu-os** at commit `772d7c5`: its spec and README.
- **GitHub**: the milestone list, the first page of open issues, and issues #485 and #486.
- **Other products**: Playnite, Heroic, Lutris, Cartridges and Ludusavi, from their own pages and from release coverage.

Limits: nothing was built or run, so statements about behaviour come from the source and the README. The issue tracker was not read beyond the pages named above, so a work package below may repeat an existing issue; search before filing. Claims that still need checking are marked "to confirm" where they appear.

## 1.2 From this document to issues

The repository has a set way to turn a plan into work, and this document is shaped to fit it.

- **The issue form** asks for an imperative title, a type, areas, Context with a link to a spec section, Scope, Acceptance criteria as a checklist, Out of scope, Research and Depends on, and every issue gets one milestone.[3][14]
- **New areas get a short spec page first.** The system panel was not in the product spec, so `docs/system-panel.md` was written before its issues were split: what each section does, how it behaves in the Flatpak, what is out of scope, an Issues table and a Decisions list for the maintainer.[10] Pindu OS's spec says the same of its later work packages.[18]

The suggested path is therefore: this analysis, then the maintainer's decisions (section 14), then one short spec page per workstream in `docs/`, then issues. Each workstream in section 8 is laid out to make that step mechanical:

- *Problem*, *What exists today* and *Proposal* become the spec page and each issue's Context.
- *Flatpak, native and Pindu OS* and *Out of scope* carry over as they are.
- *Decisions* are the maintainer's to settle first.
- *Work packages* are rows sized for one issue and one pull request. "Done when" seeds the acceptance criteria. The IDs (GI-1 and so on) are placeholders until GitHub numbers exist. Labels are suggestions from the repository's own set.

Two sequencing rules apply across workstreams. Schema migrations are an ordered, append-only list, so two open pull requests cannot both add the next one: work packages that change the schema are chained through Depends on. And parallel pull requests that add strings conflict on the translation template, so string-heavy issues in the same area are better run in sequence.

# 2. What Pindu is today

## 2.1 Core proposition

Pindu's proposition is "every PC game on your machine in one place" while each launcher keeps ownership of its games. It imports Steam, Heroic (Epic, GOG, Amazon and sideloaded), Lutris, Bottles, Faugus, desktop entries including Flatpaks, and games added by hand. Store games start through their own launcher. Pindu reads the launchers' files and never writes to them or reads their credential files.[1][3]

The product already has:

- One library across sources, resynced when a launcher's files change. Steam contributes the games it has install manifests for; Heroic contributes owned games whether installed or not.
- Search, five sort orders, and filters for installed, not installed, favourites, Windows and native.
- Favourites, hidden games, collections, a Jump Back In row of six with up to three pinned, and a random Surprise Me pick.
- Play history, with the playtime the launchers recorded.
- Games added by hand: native programs, and Windows games or installers that run through umu and Proton, each in its own prefix, with per-game Proton build, arguments, environment, wrapper command, MangoHud and GameMode.
- Downloadable GE-Proton and Proton-CachyOS builds.
- Artwork from the launchers, from files, or from SteamGridDB with the user's own key in the system keyring; since 0.5.1, Change Artwork pages through SteamGridDB's images and remembers a corrected match.
- GNOME Shell search, and export and import of what only Pindu holds.
- A desktop window and a fullscreen mode over the same models.[1][2]

## 2.2 Fullscreen mode and the session role

Fullscreen mode is a second shell, not a larger desktop view: a controller-first Home, library and game pages, an on-screen keyboard, a hint bar, Quick Access, remappable buttons, switchable designs with the user's own CSS, two accessibility switches, notifications, running tasks and controller information. In native sessions it also offers sound output, Wi-Fi through NetworkManager, Bluetooth through BlueZ and, on Pindu OS, system updates and rollback; power actions work in the Flatpak too, once host access is granted. Add a Store installs Steam, Heroic or Lutris from Flathub with a controller alone.[1][10]

Release 0.5.0 made Pindu a session shell. In a gamescope `--steam` session it takes Steam's place: it brings games to the front and comes back when they end, draws the menu and Quick Access over a running game, shows Steam's own window only when Steam needs an answer, drives MangoApp, hands off to Big Picture, and installs a login session for gamescope-session-plus through its native packages.[1][2]

Pindu therefore serves two kinds of user: someone on a GNOME desktop who wants a clean native library, and someone on a couch or handheld who wants a controller-only shell that can be the whole session.

## 2.3 Pindu OS

Pindu OS is the second kind of user made into a product: an x86_64 bootc image on Fedora 45 that logs in by itself and shows the launcher fullscreen in gamescope. The console image has no desktop, no shell access, no sudo, no SSH and no keyboard, so the launcher is its entire interface. It installs the launcher's release RPM pinned by version and hash, follows final releases only, and waits for the launcher's recorded hardware check before promoting an image from `:testing` to `:stable`.[8][18]

Its spec assigns the launcher three work packages. L-03, the system panel, shipped in 0.4.0 and L-02, the gamescope `--steam` protocol, in 0.5.0.[2] L-01, build provenance on the release RPM, has no counterpart in the launcher's workflows that this review could find. Pindu OS is itself early: it boots and passes its tests in a virtual machine and has not yet been checked on real hardware.[18]

Three consequences for planning follow, and the original analysis did not have them:

- The system panel is not scope creep. It is a dependency another product of the same maintainer was built on.
- A feature the console needs has to work in fullscreen mode with a controller alone.
- An OS rollback puts an older launcher in front of the library a newer one wrote (section 4.9).

## 2.4 Principles and rules that constrain every feature

These are in `AGENTS.md`, `CONTRIBUTING.md` and the risk rules, and every issue inherits them.[3][4][15]

- **Launchers stay owners.** Never write to another launcher's files or read its credentials.
- **Offline first.** The library works without a network. Online calls stay off the import path, and artwork and metadata are fetched and cached when online.
- **Honest sandbox.** A missing permission is explained in place, never shown as a failed launch.
- **Adwaita first.** Stock libadwaita widgets, system accent colour and typography on the desktop; no custom theme.
- **Security by construction.** Secrets live in the keyring; launches are argv lists, never shell strings.
- **Permissions never grow.** A manifest's `finish-args` and bundled modules are the maintainer's call. A feature that seems to need one stops and files an issue.
- **Dependencies need approval.** No crate or CI tool is added, upgraded or removed without the maintainer naming it. The core depends only on rusqlite, serde, serde_json and yaml-rust2.[12]
- **Some work is human-only.** No agent writes the Flathub manifest or drafts content for the submission (#63). The MetaInfo's text and the store screenshots were also made by a person without AI (#58).[30]
- **Test first, in the core.** Logic that needs no GTK goes in `pindu-core` with tests written before the code; parsers run against fixture copies of each launcher's data in native and Flatpak layouts.
- **Every string is translatable.** British spelling, gettext, and the translation template refreshed in the same change.
- **No telemetry.** Every metric comes from CI; the app sends nothing.[6]
- **Risk tiers decide who merges.** The merge queue merges low- and medium-risk pull requests once checks pass. Workflows, build files, manifests, the MetaInfo, the GSettings schema and agent instructions are high risk and wait for the maintainer.

## 2.5 What the data model already holds

The original analysis proposed eight new core concepts. Reading the schema and the core shows which are new and which are extensions.[11]

| Proposed concept | What exists today | What is missing |
| --- | --- | --- |
| One record per source copy | `games`, keyed by `<source>:<native id>`, with install state, path, size, `missing_since` and `user_removed` | Nothing; this is the source record |
| Game identity across sources | Nothing. Favourites, hidden, pins, collections, play sessions, imported playtime, artwork, launch settings and SteamGridDB matches all hang off the per-source ID, and so does the backup format | The grouping layer |
| Metadata with its origin | `games.developer`, `genres` and `description`, filled by Heroic and partly by Steam, shown on the desktop game page; each sync overwrites them with what the importer returned | A place for values from elsewhere that a sync does not overwrite; release year, input and multiplayer fields; any of it in fullscreen mode |
| Dynamic collections | `collections` with fixed members; filters exist but are not saved, and fullscreen's last only until the app quits | A saved filter |
| Launch profiles | `launch_configs`, one row per game: program, arguments, working folder, environment, Proton build, prefix, MangoHud, GameMode, wrappers | Several rows per game, if that is wanted (section 8.7) |
| Save sets and snapshots | Nothing. `backup.rs` covers Pindu's own state, format version 1 | All of it |
| Source health | `source_states`: enabled, resolved folder, user's folder, last sync, and the problems of the last sync | A place to see it; today the problems appear in a ten-second toast with a Details button and `last_sync` is not shown |
| Observable background work | `tasks.rs`: library sync, art fetch, Proton download and store install, with progress and cancel, listed in Quick Access's Activity | New task kinds only |

Two further facts matter for the proposals. Launch logs exist but are opt-in: "Play with Logs" keeps the newest ten per game. And notifications live in memory for the session only, fifty at most.

# 3. What is already strong

## 3.1 Differentiation, and an open space on GNOME

Pindu sits between unified libraries such as Playnite, store and compatibility managers such as Heroic and Lutris, and console shells such as Steam Big Picture. It is native to GNOME and built around Flatpak, portals, gamescope and Proton. It is not tied to a few stores and does not own sign-in, install or update. Its best experience is a simple library and a controller-first shell, not a runner workbench.

The closest existing app was Cartridges, a libadwaita "launcher over launchers". The project's own research records it as end-of-life: archived on Codeberg and flagged on Flathub on 3 September 2026, with 2.13.1 from September 2025 as its last release.[19] The research concludes that its retirement leaves a gap in the GNOME ecosystem that Pindu can fill, and that is the strongest argument for the Flathub listing in section 11.

Few library managers combine a native desktop interface with a credible "this can be the whole session" path. That combination is the part of Pindu that others would find hardest to copy.

## 3.2 Architecture supports controlled growth

`pindu-core` has no GTK and holds importing, launching and storage. `pindu-ui` holds services and two shells over the same GObject list models.[1][3] Grouping, health, metadata and save logic can all live in the core and reach both shells at once. The dependency footprint is small, which keeps the domain model testable: the core's tests need no display.[12]

The importer contract is already a plain data structure, a `Snapshot` of `ImportedGame` values plus the problems met while reading. Any external importer protocol (section 8.11) is that structure written down.

## 3.3 Automation and gates

Every pull request passes formatting, lints with warnings as errors, Rustdoc checks, the unit and fixture tests, data validation, translation checks, version and manifest parity, a minimum-Rust check, a coverage floor for the core, a dependency policy check, a risk classification, and a development Flatpak build with a UI smoke test. After each merge and nightly, the release and aarch64 Flatpaks, an offline source tarball and native packages for three distributions are built and smoke-tested.[6][15] The core holds about 990 tests against floors of 95% lines and 94% regions and functions, with `main` near 97%.[16]

A tag builds and publishes a release with checksums and no manual upload. The release guide also describes a pre-release soak and a hardware checklist; section 4.6 looks at how those have been used so far.[8]

## 3.4 Privacy

No Pindu account and no central service. The library is local, the app goes online only for the artwork and Proton builds the user asks for, and the one key it holds is in the system keyring.[1][7] For software that sees a whole game library and play history, that is worth stating as a product promise, and every proposal below keeps it.

# 4. Risks and constraints

## 4.1 The maintainer's attention is the scarce resource

The repository's first commit is from 4 November 2026. By 19 November it had about 600 commits counting merges, issue and pull request numbers near 500, seven final releases and four release candidates, roughly 35,000 lines in the core and 28,000 in the UI crate.[29] The project's AI disclosure says it is written almost entirely with AI assistance, by agents working from issues, with the maintainer deciding what is built and reviewing it.[15][30]

The original analysis read the one-person security policy as limited capacity. Implementation capacity is not the limit. The limits are:

- **Decisions.** Direction, dependencies and permissions are the maintainer's call, and agents stop and wait for them.
- **High-risk review.** Workflows, build files, manifests, the MetaInfo and the GSettings schema are reviewed line by line by a person.
- **Human-only work.** The Flathub manifest and submission, and the store listing's text and screenshots.
- **Real hardware.** Controllers, gamescope, suspend and Bluetooth only fail on a machine.
- **Coherence.** A pipeline that can build anything quickly will build whatever the issues say.

So the portfolio in section 7 notes what each workstream needs from the maintainer first, and the issues should be written so that an agent never has to guess a product decision.

## 4.2 Scope: a product hierarchy

Fullscreen mode now reaches into networking, Bluetooth, power, audio, gamescope's window protocol and OS updates.[1][10] Each area widens the hardware and distribution matrix. The original analysis called this the main risk and proposed a hierarchy; with Pindu OS in view the hierarchy still holds, but its last layer is an obligation and not an optional extra:

1. Unified library and launch orchestration.
2. Desktop and fullscreen presentation of it.
3. Session conveniences that any gamescope session can use.
4. System management that Pindu OS needs.

New work should strengthen layers 1 and 2 by default. Layer 4 grows only through a Pindu OS work package with its own short spec, as the system panel did, and not through ideas that start in the launcher.

## 4.3 The roadmap and the spec are behind the product

The open milestones are M5 Release, at 82 of 83 issues closed, and Nice to have, at 30 of 33; five are closed.[5] The issue form still says "M1 to M5, or Nice to have".[14] There is no place to put post-release product work.

The product spec is also behind. Its non-goals for v1 rule out being "a session or OS shell" and an "emulation front end".[9] The first was overtaken by 0.5.0 and Pindu OS. The second still stands, and section 8.10 proposes to lift it, which is a decision for the maintainer and not for an issue. The spec's unscheduled backlog already names several items in this analysis, and some that the original left out: merging duplicates across sources, ProtonDB and HowLongToBeat information, a list view beside the grid, selection mode for bulk actions, itch and standalone Legendary importers, and InputPlumber support.[9]

**Recommendation:** create the next milestones with gates (section 9), and bring the spec's non-goals and backlog up to date so that new issues have something current to link to.

## 4.4 Documentation drifts from behaviour

Issue #486 records three cases in which a pull request changed behaviour the README describes and a follow-up had to correct it: the gamescope `--steam` section after #429, #430 and #434; the Power level and Big Picture tile after #456 and #457; and when the controller returns after Big Picture closes, after #476. It proposes a "docs follow behaviour" item in the pull request template, in the review workflow's hand checks and in the review prompt. The maintainer approved the direction on 19 November. The README correction has merged; the gate has not, and its workflow edit needs a person or a merge-capable agent.[17]

For a launcher that acts as a console shell, focus, permissions and hand-off behaviour are hard to diagnose when the documentation lags. The gate should be in place before the feature work in section 8 starts, and the more complex flows deserve explanations in the app so the README is not the only support.

## 4.5 High coverage does not prove the branches are asserted

Issue #485 shows that the crate-wide coverage floor cannot catch a single untested branch: with `main` near 97%, two shipped behaviours were unasserted, in the launch overlay's lingering state and in the gamepad's held-Back handling, and it was the third such finding after #346 and #450. It proposes `cargo mutants --in-diff` on the core lines a pull request changes, as a job in the coverage workflow. The maintainer approved the direction on 19 November. A person has to add the job, and cargo-mutants has to join the approved CI tools in `AGENTS.md`.[16]

This matters for what comes next. Grouping, metadata precedence, launch outcomes and save restores are all rules about state and precedence, which is where unasserted branches hide. The gate should land before that work, and those workstreams should ask for property-style tests in their acceptance criteria.

## 4.6 The release gates are newer than the releases

The release guide describes a pre-release that soaks "a few days" on real machines and a hardware checklist run on one machine with Pindu OS, recorded by a `hardware-check:passed` label.[8] In practice 0.5.0 went from its first release candidate to final in about 27 hours across four candidates, and 0.5.1 followed a day later with the pre-release and soak waived, the hardware label still to be recorded on its release issue according to the repository's hand-off notes.[28][29] Pindu OS, the machine class the checklist names, has not itself run on real hardware yet.[18] This review did not look at the labels on the release issues, so whether the hardware check was recorded for 0.5.0 is something to confirm.

None of this is a defect in the design of the gates. It is the same drift as section 4.4, in a process document: the written soak and the actual cadence disagree. Either final releases slow to match the guide, or the guide says what a fast release may skip and what it may not. The hardware check matters more than the soak length, because Pindu OS promotes on it.

## 4.7 Hardware validation needs breadth

The checklist runs once on one machine.[8] The session product is exposed to variation in graphics drivers, gamescope versions, handhelds against television PCs, controller families, Bluetooth adapters, suspend behaviour, and GNOME desktops against Pindu OS against other gamescope-session-plus distributions. Before a 1.0 label, a small public compatibility matrix from voluntary reports and a repeatable beta checklist across a handful of hardware classes would close most of that gap.

## 4.8 Fullscreen mode cannot yet do everything a console needs

Quick Access offers notifications, activity, designs, controllers, controls, quick settings, filters, performance, the system panel and Add a Store. The game options sheet offers play, stop, favourite, collections, Change Artwork, hide and Open in Launcher.[1] From the source, the fullscreen game page hides the About rows, the description and the launch settings that the desktop page shows, and there is no fullscreen route to Preferences' Sources, Artwork, Windows and Proton pages, to Add Game, to backups or to Play with Logs.

On a desktop that is a reasonable division. On Pindu OS it is a gap. The drawer's Exit Fullscreen Mode leads to the desktop window, which a controller can move through and activate, so switches and lists there are within reach. Anything that needs typing or a file chooser is not, which covers adding a game by hand, entering the SteamGridDB key and picking a folder, and a desktop window drawn at scale 1 on a television is a poor place to send a console owner. Pindu OS plans Windows games through umu as a later work package, which will make choosing and downloading Proton builds necessary too.[18] Section 8.4 treats this as its own workstream and starts with a spike, because this review read the source and did not try a console.

## 4.9 Schema changes are permanent, lightly gated and meet rollback

Migrations are an append-only list tracked in the database's `user_version`, and a build refuses a database with a newer version than it knows.[11] Three things follow.

- **Risk tier.** The risk rules rate the GSettings schema high because a changed key breaks existing installs. A database migration is just as permanent, yet running the repository's classifier on a new file under `store/migrations/` returns medium, so the merge queue may merge it without the maintainer's review.[15] The grouping migration in section 8.1 will be the largest so far.
- **Rollback.** A Pindu OS rollback, or a user reinstalling an older bundle, puts an older launcher in front of a newer library. What the user then sees was not traced in this review and is worth testing before the next migration.
- **Backups.** The backup format is at version 1 and names games by their per-source IDs. Grouping should keep those IDs so that old backups stay valid.

**Recommendation:** rate migrations and the backup format high risk, keep a copy of the library file before each migration, and check the rollback path once on a Pindu OS image.

# 5. Competitive lens: where Pindu should and should not compete

## 5.1 Cartridges

Cartridges was the same idea on GNOME: a libadwaita library over other launchers, started through their URI handlers, with nine importers including itch, standalone Legendary and RetroArch. It never merged duplicates beyond skipping launchers' own desktop entries, had no controller mode in a release, and is now archived.[19]

**What to take from it:** its users need somewhere to go, and they will expect a Flathub install and the importers it had. The spec's backlog already lists itch and Legendary.[9]

## 5.2 Playnite

Playnite's lasting strengths are a unified library that includes games that are not installed, emulator support, automatic metadata from IGDB, themes, extensions and a fullscreen controller mode.[24] Its extensions come in three kinds: generic, library and metadata.[24] It is a Windows program today. Its author said in November 2025 that he plans to move to Linux in 2026 and will try to make a Linux version then, probably desktop mode only and not at parity with the Windows version.[25]

**What to learn:** metadata turns a tile index into a library; duplicates and custom tags matter more as a library grows; an extension story stops the core project from owning every integration; emulation is the main reason people ask for *all* their games in one place.

**What not to copy:** desktop theming, or a plugin surface that undermines GNOME consistency and the sandbox.

**What to watch:** a Linux Playnite would arrive with a large extension ecosystem and no GNOME integration, no session role and no Flatpak-first design. Pindu's answer is the parts it would lack, not a race on extension count.

## 5.3 Heroic Games Launcher

Heroic covers fewer stores and owns more of their lifecycle: install, update, repair and move, Wine and Proton management, ProtonDB and Steam Deck information, cloud saves and categories.[20] In 2026 it moved onto Pindu's ground. Version 2.21 in April added a console-like fullscreen mode for controllers and achievements for GOG games; 2.22 in May let that mode install and update games and added editing of a game's name and cover.[21][22] Its tracker has a request for a filter that shows a game owned on several platforms only once.[23]

**What to learn:** compatibility information belongs at the point of deciding to launch; save resilience is valued; console users expect to install and maintain games without a keyboard. Pindu's Add a Store and Open in Launcher already hand that work to the store.

**What not to copy:** account and store ownership. Pindu's read-only model is simpler and safer, and it is what lets Pindu sit above Heroic as well as beside it.

## 5.4 Lutris

Lutris spans eras and systems by combining launchers, compatibility layers, emulators, runners and community install scripts.[26]

**What to learn:** breadth is valuable and operationally expensive. If Pindu adds emulation or new runners it should do so through a small declarative contract and not absorb an installer and run-configuration system. Lutris also records which store a game came from and that store's ID for it, which Pindu's importer does not read yet and which section 8.1 uses.

## 5.5 Steam Big Picture and OpenGamepadUI

In a gamescope session Pindu stands where Steam's Big Picture normally stands, and it hands off to Big Picture when the user wants Steam's own interface.[1] OpenGamepadUI owns session-level hardware control such as TDP, which the spec left to it.[9] The useful boundary is the one in section 4.2: Pindu does what a console owner needs to run the machine, and no more.

# 6. Product direction: the layer above launchers

> **Pindu is the local, launcher-agnostic game library and session layer for Linux.** It knows which games you have, which copy you prefer, how you like to start them and what you played, and it presents them the same way on a desktop or a couch. Stores keep accounts, installs, updates and their own settings.

A feature fits when it improves one or more of:

- identity, discovery and organisation across launchers;
- launch orchestration and explaining what happened;
- local play history and resilience;
- operation with a controller alone;
- visibility of Linux compatibility;
- diagnostics and trust.

A feature fits badly when it would make Pindu the authoritative owner of store accounts, purchases, install and update manifests, DRM, cloud infrastructure or general package management. Add a Store is the edge of that last item: it installs three named launchers from Flathub and nothing else, and it should stay that narrow.

# 7. Prioritised portfolio

Priority reflects order, not desirability. "Size" is implementation and support cost. The last column is what has to come from the maintainer before agents can start, since that is the real constraint (section 4.1).

| Workstream | Priority | User impact | Fit | Size | Needed from the maintainer first |
| --- | --- | --- | --- | --- | --- |
| Game identity and duplicates (8.1) | P0 | Very high | Very high; in the spec backlog | Large | The grouping model; the risk tier for migrations |
| Library Health (8.2) | P0 | High | Very high | Small to medium | Where it lives |
| Diagnostics (8.3) | P0 | Medium | Very high | Small | Nothing for the desktop part |
| Controller parity (8.4) | P0 for Pindu OS | High on a console | Very high | Medium, after a spike | Which flows a console needs |
| Saved filters, shelves and local metadata (8.5) | P1 | High | High; the spec's own provider order | Medium | Nothing |
| Launch outcomes and failed-launch help (8.6) | P1 | High | Very high | Medium | Whether logs stay opt-in |
| Safe start (8.7) | P1 | Medium | Very high | Small | Nothing |
| Accessibility and controller polish (8.9) | P1 | Medium to high | High | Small each | Nothing |
| Save snapshots (8.8) | P1 to P2 | High for Windows games Pindu runs | High | Medium to large | Scope and storage format |
| Online metadata provider (8.5) | P2 | Medium | Medium to high | Medium | Whether, and which provider |
| Emulation (8.10) | P2 | Very high for a segment | Medium; a spec non-goal today | Large | A product decision |
| External importer contract (8.11) | P2 | High in the long term | Medium | Large | Where extensions run |
| Named launch profiles (8.7) | P3 | Low to medium | High | Medium | Evidence that per-game settings fall short |
| Achievements, remote play (8.12) | P3 | Medium | Medium | Large | Not now |

**P0** fixes the information model, makes source state visible and makes the result usable on a console; each becomes more valuable with every source added. **P1** turns that into daily value. **P2** widens what counts as a game or a source and brings third-party terms or a much larger support surface. **P3** waits for evidence.

# 8. Workstreams

Each workstream follows the layout described in section 1.2. Labels come from the repository's set: `area:core`, `area:import`, `area:ui`, `area:launch`, `area:art`, `area:gamepad`, `area:packaging`, and `type:feature`, `type:spike`, `type:chore`. Every "Done when" assumes the standing rules of section 2.4, so they are not repeated in each row.

## 8.1 Game identity and duplicates (GI)

### Problem

The same title arrives from several sources and shows as several tiles. Heroic lists owned games that are not installed, so a library with Steam and a Heroic store soon shows the same game twice. The spec kept duplicates separate in v1 and put "merging duplicate games across sources into one entry with several launch options" on its backlog.[9]

### What exists today

- One `games` row per source record, keyed `<source>:<native id>`, with all user state hanging off that key (section 2.5).
- The desktop-entry importer already skips the launchers Pindu imports directly, their game shortcuts and Pindu's own entry, so that class of duplicate does not occur.[11]
- The classes that remain: one title owned in two stores; a Lutris entry that wraps a store game; a copy added by hand beside a store copy; native and Windows builds of one title.
- Lutris's database records the store a game came from and that store's ID (`service`, `service_id`), plus `runner` and `year`. The repository's own Lutris fixture has the columns; the importer reads none of them.[11]
- Title normalisation for search exists (`search::normalize`): case, accents, punctuation and a leading "The".

### Proposal

Add a grouping layer and leave the source records as they are.

- A **group** holds two or more source records and names a preferred copy. A game in no group behaves exactly as today.
- `games` rows and their IDs do not change. Play sessions, backups, the search provider and the per-game window IDs in gamescope keep working, and a split restores exactly what was there before.
- The library shows one tile per group. The game page lists the copies with their source, install state and playtime. Play starts the preferred copy, or an installed one when the preferred copy is not installed.
- Matching is conservative and staged:
  1. exact cross-references, such as a Lutris record's store ID against the Steam, Heroic GOG or Heroic Epic record with that ID;
  2. normalised title together with developer or year where both records have one, as a suggestion only;
  3. merge and split by hand, with a split remembered so that the suggestion does not come back.
- Nothing is merged on title alone without the user.

### Flatpak, native and Pindu OS

No permission and no network. Merging, splitting and reviewing suggestions have to work in fullscreen mode with a controller alone.

### Out of scope

- Changing any launcher's data. A group is Pindu's own note about records it does not own.
- Online lookups to find matches. A provider that maps store IDs (section 8.5) can add a matching stage later.
- Regional and revision variants of emulated games (section 8.10).

### Decisions

- **What a group owns.** Either a group is a view, where favourite, hidden, pin and collection membership stay on the source records and a change on the tile applies to every member, or those flags move to the group. The view is smaller, can be undone and keeps old backups valid, and is the recommendation.
- **Exact cross-references.** Group them automatically with an undo, or suggest them like everything else. A Lutris entry whose runner is Steam is one installed copy listed twice, which argues for automatic.
- **Playtime across copies.** Adding it up is right for two copies played separately and wrong when two sources counted the same sessions, as Lutris and Steam do for a wrapped game.
- **The default preferred copy.** For example installed before not installed, then native before Windows, then a source order the user can change.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| GI-1 | Measure duplicates in real libraries and settle the grouping model | A write-up on the issue counts duplicates by class in at least two real libraries and answers the four decisions above | `type:spike` `area:core` | |
| GI-2 | Keep a copy of the library before a schema migration | Opening an older database first writes a copy beside it, named for its schema version; the newest two copies are kept; a test opens a copy at its old version | `type:feature` `area:core` | |
| GI-3 | Read Lutris service IDs, runners and years | The importer returns `service`, `service_id`, `runner` and `year` for each game; fixture tests cover the native and Flatpak layouts; the library looks the same | `type:feature` `area:import` | |
| GI-4 | Add game groups to the library store | A migration and store functions group and ungroup records, set the preferred copy and remember "not the same game"; a resync changes none of it; a member that goes missing leaves the group intact | `type:feature` `area:core` | GI-1, GI-2 |
| GI-5 | Suggest duplicate games | A core function returns suggestions with a reason and a confidence, from cross-references first and then title with developer or year; it writes nothing; property-style tests cover symmetry and remembered splits | `type:feature` `area:core` | GI-3, GI-4 |
| GI-6 | Show a group as one game with a preferred copy | Both shells show one tile per group; the page lists the copies; Play starts the preferred copy; GNOME Shell search gives one result per group | `type:feature` `area:ui` | GI-4 |
| GI-7 | Merge and split games by hand | From the desktop menu and from fullscreen's options sheet; a split is remembered | `type:feature` `area:ui` | GI-6 |
| GI-8 | Review duplicate suggestions | Both shells list the suggestions with accept and dismiss; a dismissed pair never returns | `type:feature` `area:ui` | GI-5, GI-6 |
| GI-9 | Carry game groups in backups | Backup format 2 holds groups and remembered splits; format 1 still imports; a group whose games are absent is skipped and counted | `type:feature` `area:core` | GI-4 |

## 8.2 Library Health and first run (LH)

### Problem

Whether a game can start depends on conditions spread across the app: whether its source was found, native or Flatpak, host access, umu, the MangoHud layer, system services. The honest-sandbox rule gives good explanations, each in the place where the problem bites. Nothing shows the whole picture, and what a sync could not read is gone after ten seconds.

### What exists today

- `source_states` stores, per source, whether it is on, the folder found, the folder the user chose, the last sync and the problems of that sync.[11]
- Preferences, Sources shows each source's folder or "Not found on this computer", an import switch and a folder chooser. It does not show the last sync or the problems.
- After a sync, a toast "N import problems" with a Details button shows for ten seconds.
- Preferences, General shows whether games may start on the host, with the `flatpak override` command. A game that needs it shows a banner with How to Enable.[1]
- Add Game says when umu-launcher is missing and how to install it; the MangoHud switch says why it is greyed out.
- An empty library offers Add a Store and Import Now.[1]
- Notifications cover sync results and missing permissions for the running session only.

### Proposal

One Library Health view over one report that the core computes:

- **Sources:** found or not, where, native or Flatpak, how many games, last sync, and the problems of that sync.
- **Launching:** host access, umu, the Proton builds installed, MangoHud, GameMode, and whether this is a gamescope session.
- **Artwork:** whether a key is stored, and the size of the cache.
- **Library data:** where the library file is, its schema version and the last backup.

Each problem names a next step that already exists: the Preferences page, the override command, How to Install.

The original also proposed a four-step setup sequence on first start. Two corrections apply. The app cannot grant itself host access: by design the user runs a command, so a setup step could only explain it, as Preferences already does. And the app's existing pattern is empty states and prompts in place, which is also what GNOME apps usually do. A lighter first run fits better: after the first sync, show the health report once as a summary of what was found.

### Flatpak, native and Pindu OS

Rows the session cannot have are hidden or explained by the availability rules the system panel already uses.[10] In fullscreen mode the view is a Quick Access level.

### Out of scope

- Repairing anything automatically. The view reports and points; the user acts.
- Signing in to stores, which stays with the stores.

### Decisions

- Where the desktop view lives: a Preferences page, or a page of its own from the main menu.
- A first-run summary, or none.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| LH-1 | Show each source's last sync and problems in Preferences | The Sources page shows when each source last synced and lists that sync's problems, from what is already stored | `type:feature` `area:ui` | |
| LH-2 | Build the library health report in the core | One function returns the four sections above; tests cover a Flatpak without host access and a native install | `type:feature` `area:core` | |
| LH-3 | Add Library Health to the desktop window | The view shows the report and refreshes after a sync; every problem names a next step that exists | `type:feature` `area:ui` | LH-2 |
| LH-4 | Add Library Health to fullscreen mode's Quick Access | The same report as a Quick Access level; rows the session cannot have follow the system panel's rules | `type:feature` `area:ui` | LH-2 |
| LH-5 | Summarise what the first sync found | After the first sync a summary names the launchers found, the games imported and what needs attention, once, in both shells | `type:feature` `area:ui` | LH-3, LH-4 |
| LH-6 | Record when the library was last backed up | An export stores its time and Library Health shows it | `type:feature` `area:core` | LH-2 |

## 8.3 Diagnostics (DX)

### Problem

The environment matrix is wide, the app sends no telemetry, and the bug form asks reporters to paste version output by hand.[6][14]

### What exists today

- The app uses libadwaita's About dialog and leaves its Debugging Information page empty.
- Play with Logs writes one log per launch with the program's output and Proton's log, keeps the newest ten per game, and opens the newest in a text editor.
- The core already has a function that writes a path with `~` for the home folder.

### Proposal

Fill About's Debugging Information with a report built in the core: version and build, how it was installed, distribution, session type and gamescope version, each source's state and game count without titles, the permission tier, which tools were found, the schema version and the last sync's problems. libadwaita supplies Copy and Save, so the desktop needs no new interface. Then ask for the report in the bug form.

A console is harder: nothing can be pasted from it, and fullscreen mode has no About page that this review found.

### Out of scope

- Uploading anything. The user copies or saves the report and decides where it goes.
- Game titles and full paths in the default report.

### Decisions

- Where a console's report goes: a file in a known folder, a USB drive, or text on screen to photograph.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| DX-1 | Fill About's Debugging Information | About carries the report; it holds no game title and no home path, and a test checks that | `type:feature` `area:core` `area:ui` | |
| DX-2 | Ask for the debugging information in the bug form | The bug form asks for the report in place of hand-pasted version output; a `.github/` change, so the maintainer merges it | `type:chore` `area:packaging` | DX-1 |
| DX-3 | Save a diagnostics report from fullscreen mode | As the decision above settles | `type:feature` `area:ui` | DX-1 |
| DX-4 | Add a game's newest log to a report on request | The user picks a game and its newest log is appended, with the same redaction | `type:feature` `area:ui` | DX-1 |

## 8.4 Controller parity for consoles (CP)

### Problem

On Pindu OS fullscreen mode is the interface a console owner is meant to use, and several things the desktop window can do have no fullscreen route (section 4.8).

### What exists today

- Quick Access and the game options sheet as listed in section 4.8.
- The fullscreen game page hides developer, genres, description and the launch settings.
- The desktop window can be driven with a controller: directions move focus, A activates and B goes back. Exit Fullscreen Mode in the drawer leads to it.
- An on-screen keyboard with capitals, symbols and a hidden-text dialog, added for Wi-Fi passphrases and Bluetooth PINs. It belongs to fullscreen mode.[2]

### Proposal

A spike first: go through every desktop-only flow, say whether a console needs it, and file issues for those that do. Likely candidates:

- game details on the fullscreen game page;
- entering the SteamGridDB key with the on-screen keyboard, which also needs a working keyring on Pindu OS, an open risk in its spec;[18]
- turning sources on and off;
- choosing a Proton build for a game and downloading builds;
- adding a game by hand, which needs a way to pick a file with a controller and is the largest of these;
- backups, which need somewhere to go on a console.

### Decisions

- Which of these a console needs, and which stay desktop-only on purpose.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| CP-1 | List what fullscreen mode cannot do that a console needs | A write-up lists each desktop-only flow, whether a console needs it, and the issues to file | `type:spike` `area:ui` | |
| CP-2 | Show a game's details on the fullscreen game page | Developer, genres and description show in fullscreen mode in every shipped design | `type:feature` `area:ui` | |
| CP-3 | Enter the SteamGridDB key in fullscreen mode | The key is typed hidden with the on-screen keyboard and stored in the keyring; without a keyring the entry says so | `type:feature` `area:ui` `area:art` | CP-1 |

## 8.5 Shelves and metadata (MD)

### Problem

A large library needs more ways to slice it than source, install state and platform, and the metadata Pindu has is thin and uneven across sources.

### What exists today

- `games` has developer, genres and description. Heroic fills developer and, for some stores, genres and a description; Steam fills developer. The desktop game page shows them.[11]
- Each sync overwrites those columns with what the importer returned, so a value from anywhere else cannot live there.
- The Steam importer already parses Steam's binary `appinfo.vdf` and reads name, type, platforms, developer and icon from it. The same section holds more: controller support appears in the repository's own test data, and genres, release date and Steam Deck compatibility are expected there (to confirm in MD-1).
- Lutris's database has a `year` column that is not read.
- Views and filters as listed in section 2.1. Fullscreen Home already has sections for recently played, favourites, installed, each launcher and each collection.
- The spec's order for metadata is the sources' own data first, SteamGridDB for art, and ProtonDB, HowLongToBeat and PCGamingWiki on the backlog.[9]

### Proposal

In this order, which differs from the original's "provider first":

1. **Saved filters.** Let a filter be saved as a dynamic collection. Collections already appear in the sidebar, as fullscreen tabs and as Home sections, so a saved filter becomes a shelf with no further work. Useful shelves need no new data: installed and never played, recently added, native, Windows, no longer listed by its source, needs a permission.
2. **Metadata the launchers already hold.** Read release year, genres, controller support and Steam Deck compatibility from the files Pindu already opens. No network, no key.
3. **A place for metadata with its origin.** Keep values from the source, from a provider and from the user apart, prefer them in the reverse of that order, and let a sync touch only the source's values.
4. **An online provider, opt-in, after a spike.** The spike compares candidates on licence and terms, whether a key is needed (on a console it must be typed with the on-screen keyboard and stored in a keyring), which store IDs it maps (which helps section 8.1), the fields it has and its rate limits. Candidates to assess, none of them verified here: SteamGridDB, which is integrated already; IGDB, which Playnite uses; PCGamingWiki; and Wikidata, which is openly licensed and carries store identifiers.

Whatever is chosen follows the existing network rules: nothing is fetched during an import, a refresh is a visible task that can be cancelled, each value shows where it came from and when, and a key lives in the keyring.

### Out of scope

- Ratings, prices and store pages as content. A link out is enough.
- Completion-time data until a provider decision exists.

### Decisions

- Whether an online provider is wanted at all, given what local data covers, and which one.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| MD-1 | Find what metadata the launchers' own files hold | A write-up lists, per source, the fields available offline and how reliable they are in real libraries, and which to import | `type:spike` `area:import` | |
| MD-2 | Save a filter as a dynamic collection | A saved filter appears in the sidebar, as a fullscreen tab and as a Home section, and is renamed and deleted like a collection | `type:feature` `area:core` `area:ui` | GI-4, for migration order |
| MD-3 | Filter the library by genre and by never played | Both filters work in both shells and can be saved | `type:feature` `area:core` `area:ui` | MD-2 |
| MD-4 | Keep metadata apart from source fields, with its origin | A table holds values by field with origin and time; reading prefers the user's value, then a provider's, then the source's; a sync changes source values only | `type:feature` `area:core` | MD-2, for migration order |
| MD-5 | Import release year, genres and controller support from Steam and Lutris | The fields MD-1 found reliable are imported offline, with fixture tests | `type:feature` `area:import` | MD-1, MD-4 |
| MD-6 | Choose an online metadata provider | A write-up compares the candidates on the criteria above and recommends one, or none | `type:spike` `area:core` | |
| MD-7 | Fetch metadata on request and cache it | Off by default; a refresh is a cancellable task in Activity; nothing is fetched during an import; each value records its provider | `type:feature` `area:core` `area:ui` | MD-4, MD-6 |
| MD-8 | Edit a game's details by hand | The user's value wins and survives syncs and refreshes | `type:feature` `area:ui` | MD-4 |

## 8.6 Launch outcomes and failed-launch help (CL)

### Problem

When a game fails to start or exits at once, Pindu has little to say about why: outcomes are not recorded, and the program's output is kept only if the user chose Play with Logs beforehand. Heroic shows compatibility information before the user chooses how to launch.[20]

### What exists today

- The launch overlay: a starting screen, Cancel after five seconds, and timeouts for a hand-off that never answers.[2]
- For games Pindu runs itself: a Proton build per game with a default, a prefix, MangoHud, GameMode, and the Steam Linux Runtime for native games.
- A missing permission, a missing umu and an unavailable MangoHud are each explained in place.
- Logs are opt-in through Play with Logs.
- Nothing records how a launch ended.

### Proposal

1. **Record the outcome** of each launch of a game Pindu runs: the route, the Proton build, start and end, the exit status, and whether a window appeared where the session can tell. For a hand-off, record whether the launcher answered. Keep the last few per game.
2. **Explain the last failure** on the game page and in the options sheet, with next steps that exist: How to Enable, How to Install, another Proton build, Start Safely (section 8.7), Open Log.
3. **Link out** for Steam games: the ProtonDB and PCGamingWiki pages for the Steam app ID, opened in the browser. The app makes no request itself. Other games get the links once grouping or metadata gives them a Steam ID.
4. **Steam Deck compatibility** from Steam's own files, if MD-1 confirms it is there.

### Out of scope

- Fetching ProtonDB ratings. To this review's knowledge ProtonDB documents no public API, so anything beyond a link starts with a spike on its terms.
- Choosing a runtime for store games. Their launchers keep that.

### Decisions

- Whether every launch of a game Pindu runs keeps a small log, or logs stay opt-in.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| CL-1 | Record how each launch ended | The store keeps the last outcomes per game as described; tests cover a clean exit, a failed start and a hand-off that never answers | `type:feature` `area:core` `area:launch` | MD-4, for migration order |
| CL-2 | Explain a failed launch and offer the next step | Both shells show the reason for the last failure and only actions that exist for that game | `type:feature` `area:ui` `area:launch` | CL-1, LP-1 |
| CL-3 | Link Steam games to ProtonDB and PCGamingWiki | The game page and options sheet open both pages for a Steam game; no request is made by the app | `type:feature` `area:ui` | |
| CL-4 | Show Steam Deck compatibility from Steam's own files | Steam games show Steam's rating, read offline | `type:feature` `area:import` `area:ui` | MD-1, MD-4 |

## 8.7 Safe start, and named launch profiles (LP)

The original proposed named launch profiles per game, such as TV, Handheld, Safe mode and Debug. Per-game launch settings already hold everything a profile would override, including wrapper commands (section 2.5). Named profiles add a second dimension to those settings in two shells, each of which needs a controller interface for it, and nothing yet shows that per-game settings fall short.

One of the examples is needed now: a way to start a game once with nothing added, to tell a broken overlay or wrapper from a broken game.

**Proposal.** Start Safely: a one-off start that ignores the game's wrappers, MangoHud, GameMode and its own environment variables, and writes a log. It is offered from the desktop menu, from fullscreen's options sheet, and as a next step after a failed launch.

**Deferred.** Named profiles, at P3. If demand appears, `launch_configs` becomes several rows per game with one default, and store games stay untouched because their launchers own their settings. The original's "gamescope resolution and scaling hints" is left out: Pindu does not wrap games in a gamescope of its own, and inside a session those settings belong to the session.

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| LP-1 | Start a game once in safe mode | A game Pindu runs can be started once without wrappers, overlay, GameMode or its own environment, with a log, from both shells; its saved settings are unchanged | `type:feature` `area:launch` `area:ui` | |

## 8.8 Save snapshots (SV)

### Problem

Saves are the one part of a library that cannot be downloaded again, and Pindu's backup covers only its own state. Heroic offers cloud saves for its stores.[20]

### What exists today

- Backups of collections, favourites, hidden games, games added by hand and tracked playtime.[1]
- Each Windows game added by hand runs in a prefix of its own, by default under `~/Games/pindu-launcher/prefixes`, which the Flatpak may write to.[13]

### Constraints the original did not have

- **The sandbox.** The Flatpak can reach `~/Games`, the launchers' folders read-only, and folders the user picks through the file chooser. It cannot read `~/.local/share` or `~/.config`, where most native games save, and wider access is a permission, which is the maintainer's call.[13]
- **The format.** The core has no archive crate. An archive means a new dependency or an external tool; a plain folder copy needs neither.
- **Save locations are a solved data problem.** Ludusavi keeps a manifest of save locations drawn mainly from PCGamingWiki, covers Steam with Proton, Heroic and Lutris, and has a command line for scripting.[27] Its licence terms need checking before anything is reused.
- **Store cloud saves** stay with the stores.

### Proposal

A first version for the Windows games Pindu runs in a prefix it manages:

- Snapshot the user-profile folders of the prefix when the game exits, and before any restore.
- Keep the last few snapshots with their time and size.
- Restore after taking a safety snapshot, and never while the game is running.
- Optionally copy snapshots to a folder the user picks, which can be one that Syncthing, Nextcloud or a drive already syncs. There is no Pindu cloud.

Save folders added by hand for other games can follow, with the Flatpak's limits explained in place. Whatever is built shows exactly which paths a snapshot includes, writes it atomically, restores only inside the prefix or the folders the user configured, and leaves out caches and credential stores by default.

### Flatpak, native and Pindu OS

Picking a destination folder needs a file chooser a controller can drive, so a console's first version can keep snapshots on the machine.

### Out of scope

- A Pindu account or cloud.
- Saves that Steam or Heroic already sync.
- Emulated games.

### Decisions

- The first version's scope: managed prefixes only, or more.
- Folder copies or an archive, which decides whether a dependency is needed.
- Ludusavi: call it when it is installed, use its manifest, or neither.

### Work packages

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| SV-1 | Decide what a save snapshot holds and how it is stored | A write-up answers the three decisions above, with measured snapshot sizes for a few real prefixes | `type:spike` `area:core` | |
| SV-2 | Snapshot a Windows game's saves when it exits | A snapshot is written after a clean exit; the last few are kept; a failed write leaves the earlier ones intact | `type:feature` `area:core` `area:launch` | SV-1 |
| SV-3 | Restore a save snapshot | Both shells list a game's snapshots; a restore first takes a safety snapshot and refuses while the game runs | `type:feature` `area:core` `area:ui` | SV-2 |
| SV-4 | Copy save snapshots to a folder the user picks | The folder is picked through the file chooser and survives a restart; a folder that is gone is reported, not treated as a failure of the game | `type:feature` `area:ui` | SV-2 |
| SV-5 | Add save folders by hand for any game | A folder the app cannot reach is explained in place | `type:feature` `area:core` `area:ui` | SV-3 |

## 8.9 Accessibility and controller polish (AC)

Fullscreen mode has Reduce Motion, which also follows the system setting, one Larger Text step of about a quarter, remappable buttons, a confirm and back swap and a choice of button prompts. The desktop window follows the system's text size. High contrast turns the user's own stylesheet off.[1] Restart, Power Off and Forget already ask before acting in the cases that lose something.

The next steps should make the controller shell dependable at living-room distance and for more people, not add visual novelty:

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| AC-1 | Offer several text sizes in fullscreen mode | More than one step, checked in every shipped design at 1080p | `type:feature` `area:ui` | |
| AC-2 | Let destructive actions need a held button | An option makes Power Off, Restart, Forget and Stop need a hold; off by default | `type:feature` `area:ui` `area:gamepad` | |
| AC-3 | Check the fullscreen designs under high contrast and reduced transparency | A write-up with the issues found, per design | `type:spike` `area:ui` | |
| AC-4 | Review the desktop window with a screen reader | A write-up with the issues found, per page | `type:spike` `area:ui` | |
| AC-5 | Add navigation repeat and dead-zone settings | The settings change navigation only and never what a game receives | `type:feature` `area:gamepad` | |

## 8.10 Emulation (EM)

Emulation is a main reason people choose Playnite or Lutris, and it would take Pindu from "PC games across launchers" to a complete library.[24][26] It is also a stated non-goal of the v1 spec, so the first step is a decision and not an issue.[9]

It should not come early. It adds platforms and file patterns, multi-disc sets, BIOS requirements, cores, per-platform artwork, compressed formats, controller-mapping assumptions and regional variants, which is enough to unsettle the simpler PC-game model.

If the non-goal is lifted, the first version stays narrow and declarative, as the original proposed: the user defines an emulator profile (name, program or Flatpak app, argument template, file extensions) and adds a ROM folder for a platform; Pindu scans read-only, creates source records and starts them with argv lists. It does not install emulators or manage BIOS files, cores or emulator settings.

The source adds constraints the original did not list:

- The `platform` column accepts only `windows` and `linux`, and widening a check constraint means rebuilding the table.[11]
- `Source` is a closed set and part of every game ID.
- In the Flatpak, a ROM folder is reached through the file chooser, and starting an emulator on the host needs host access. The app already translates a chooser path to its host path for source folders.
- Variants only group sensibly once section 8.1 exists, and a library with ROMs needs the benchmark in section 10.4 first.

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| EM-1 | Decide whether emulation joins the product, and its first shape | A short spec page in `docs/` with the maintainer's decision recorded, and an Issues table if it is a yes | `type:spike` `area:core` | GI-6, PQ-5 |

## 8.11 External importer contract (XP)

Every built-in source adds maintenance, and Playnite's extensions show the alternative.[24] The original recommended a process-isolated, versioned JSON protocol over an in-process Rust plugin interface, and that stands: an in-process interface would be fragile across compiler and crate versions and would widen the trust surface.

Two points sharpen it.

- **The contract already exists in the core.** An importer returns a snapshot: its games, each with ID, title, install state, platform, playtime, a launch description and artwork, and the problems it met. A protocol is that structure written down, plus a probe and the paths to watch.
- **A separate process is not a sandbox.** A helper inside the Flatpak sees only what the sandbox sees, which rules out most launchers' data. A helper on the host runs with the user's rights through host access. The design has to say which it is, because that decides whether "safe" is true.

`Source` being a closed set is the main structural cost, as for emulation. Launch descriptions should stay limited to the kinds that exist: a URI, a Flatpak app, or an argv list on the host where the user has granted access. Never a shell string.

**Rollout.** Write the contract as a short spec, prove it by moving one low-risk built-in importer behind it, and promise no stability until two integrations from outside exist.

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| XP-1 | Write the external importer contract as a short spec | A spec page in `docs/` that says where a helper runs, what it may return and how it is versioned, with the maintainer's decision recorded | `type:spike` `area:import` | GI-6 |

## 8.12 Deferred, and the rest of the spec's backlog

- **Achievements.** Heroic now shows them for GOG.[21] They depend on store APIs or source-specific caches. A careful version shows only what a launcher's local data already holds, per copy under a group, and never needs a Pindu account. It waits for grouping and metadata.
- **Remote play and streaming.** Not central to a local library and session layer.
- **Spec backlog items not covered above:** a list view beside the grid, selection mode for bulk actions, itch and standalone Legendary importers, InputPlumber support and HowLongToBeat information.[9] The two importers are natural follow-ups now that Cartridges has retired. A list view and bulk selection would both help the duplicate review in section 8.1 and could move up with it.

## 8.13 Process, quality and distribution (PQ, DS)

These are not features, but the features above depend on several of them.

| ID | Issue | Done when | Labels | Depends on |
| --- | --- | --- | --- | --- |
| PQ-1 | Rate database migrations and the backup format as high risk | The risk rules and `docs/risk-tiers.md` rate `store/migrations/` and the backup format high, and the classifier's tests cover it; a `.github/` change, so the maintainer merges it | `type:chore` `area:packaging` | |
| PQ-2 | Create the next milestones with their gates | The milestones of section 9 exist with gates; the issue form and `AGENTS.md` name them; the maintainer's work | `type:chore` | |
| PQ-3 | Add a spec page per workstream to `docs/` | Each workstream the maintainer accepts has a page laid out like `docs/system-panel.md`, with its decisions recorded | `type:chore` | The decisions of section 14 |
| PQ-4 | Bring the spec's non-goals and backlog up to date | The session-shell non-goal is revised, emulation is decided either way, and the backlog matches the tracker; the maintainer's work, since the PDF is an export | `type:chore` | |
| PQ-5 | Benchmark the library at 10,000 games | A repeatable measurement of load, sort, filter and search at 10,000 records with artwork paths, with the numbers recorded | `type:chore` `area:core` | |
| PQ-6 | Attach build provenance to the release RPM | `gh attestation verify` passes on a release RPM, as Pindu OS's L-01 asks; a workflow change, so the maintainer merges it | `type:chore` `area:packaging` | |
| PQ-7 | Say in the release guide what a fast release may skip | `docs/releasing.md` matches how releases are cut, and names what is never skipped | `type:chore` | |
| DS-1 | Bump the AUR and nixpkgs recipes when a final release is published | A final release opens the bump pull request by itself, or the recipes are dropped on purpose | `type:chore` `area:packaging` | |

Already filed and approved, so not to be filed again: the docs-follow-behaviour gate (#486) and the changed-line mutation gate (#485).[16][17]

Human-only, so no work package is drafted here: the Flathub submission (#63), and new store screenshots (section 11.3).

# 9. Milestones

The original laid the roadmap out as versions 0.6 to 0.9. Versions currently move faster than themes: 0.4.0 to 0.5.1 took three days.[29] The repository's own convention fits better: milestones are the epics, each with a gate that must pass before the next starts.[3] The four below continue M1 to M5. A version is cut whenever a milestone's work is ready to ship.

## 9.1 M6: One library

**Goal:** the library feels like one library and says what it knows, before more sources are added.

**Contents:** the two approved gates (#485, #486); PQ-1 to PQ-4; GI-1 to GI-9; LH-1 to LH-6; DX-1 and DX-2; CP-1 and CP-2; MD-2 and MD-3.

**Gate:** with Steam, Heroic and Lutris present, a game owned in two of them shows once with both copies listed; every source says when it last synced and what it could not read; About carries a report with no game title and no home path in it; and grouping and Library Health work with a controller alone.

## 9.2 M7: Launch confidence

**Goal:** Pindu is the best place to start a game on Linux and to find out why one did not start.

**Contents:** CL-1 to CL-4; LP-1; MD-1, MD-4, MD-5 and MD-8; CP-3 and the issues CP-1 files; AC-1 to AC-5; DX-3 and DX-4; PQ-6 and PQ-7.

**Gate:** a launch that fails names its cause and a next step in both shells; a game can be started once in safe mode; and a console can do everything CP-1 said it needs.

## 9.3 M8: Resilience

**Goal:** a user can trust Pindu with a large library and with saves.

**Contents:** SV-1 to SV-5; MD-6 and MD-7; PQ-5; DS-1.

**Gate:** a Windows game Pindu runs can be put back to an earlier save from either shell, and the library stays responsive at 10,000 games by PQ-5's numbers.

## 9.4 M9: Breadth

**Goal:** widen what counts as a game or a source without making the core unmaintainable.

**Contents:** EM-1 and XP-1 and whatever they lead to; the itch and Legendary importers; the list view and bulk selection if they have not moved up.

**Gate:** every new kind of game or source shares the grouping, collection and history model, with no separate interface for it.

## 9.5 What 1.0 should promise

A 1.0 should mean more than a long feature list:

- Every released database version migrates, which is already the practice, and the path back after a rollback is tested.
- Older backup formats import.
- The built-in sources behave as documented.
- If an external importer contract exists, its compatibility policy is written down.
- Desktop and fullscreen flows are checked on a small, published set of hardware.
- The support policy for the Flatpak, the native packages and Pindu OS is stated.
- The app is on Flathub.
- No known defect loses data, breaks a group or leaves a launch stuck.

# 10. Architecture notes for implementers

## 10.1 Core first, then both shells

The architecture keeps decisions that can be tested in `pindu-core` and the shells thin.[3][4] Every workstream above follows that: the group, the health report, the diagnostics report, the saved filter, the metadata resolver, the launch outcome and the snapshot are core types with tests, and each shell observes them. Section 2.5 lists what each builds on.

## 10.2 Store where a value came from

Title from Steam, cover from the user, description from a provider, playtime from two sources, preferred copy by the user's choice: the interface can only explain a conflict, and a sync can only avoid undoing a decision, if each value records its origin. Artwork already does this with an origin per image. Developer, genres and description do not, and each sync replaces them (section 8.5).

## 10.3 Background work stays visible and cancellable

Quick Access's Activity already lists library syncs, art fetches, Proton downloads and store installs with progress and cancel.[1] Metadata refreshes, duplicate matching over a large library, snapshots, restores and ROM scans should be new kinds of the same task, with progress, cancellation where it is safe and a recoverable partial state.

## 10.4 Large libraries

Several stores plus emulation can produce tens of thousands of records. The core already computes sort and search keys once per game and re-checks only the games a filter change can affect. Before emulation, measure (PQ-5), then index the fields that filters and grouping use, load artwork and details lazily, avoid a full rescan when a file-watch event names a narrower change, and bound image caches.

## 10.5 Migrations

One migration per pull request, chained through Depends on. A copy of the library before each one (GI-2). High-risk review (PQ-1). Additive where possible: a grouping layer beside `games` and not a rewrite of it.

# 11. Distribution and adoption

## 11.1 Flathub

The Flathub submission is issue #63 in the Nice to have milestone, human-only, and labelled blocked and on hold; this review did not look into what blocks it.[3][5] For a GNOME app, Flathub is the main channel for discovery and for updates: a bundle downloaded from GitHub does not update itself.[1] With Cartridges retired, the project's research found no maintained app of its kind there.[19]

**Recommendation:** treat the listing as a product milestone and a 1.0 promise. It stays the maintainer's own work, so nothing is drafted for it here.

## 11.2 Other packages

Recipes for COPR, the AUR and nixpkgs are in the repository and follow final releases; the AUR and nixpkgs ones are bumped through a pull request of their own after each.[8] With final releases days apart that is a recurring chore, and the hand-off notes list the 0.5.1 bumps as still open.[28] Either the bump follows a release by itself (DS-1) or the channels are dropped on purpose. This review did not check whether they are published or used.

## 11.3 Store screenshots

The MetaInfo lists five screenshots, all of the desktop window, at URLs pinned to the `v0.1.0` tag.[2] Nothing shows fullscreen mode, Windows games or the session role, which is most of what has been built since.

The original called a refresh a low-effort improvement. It is more constrained than that. The store screenshots were made by a person without AI, under a human-only issue (#58), and they show freely licensed games with their credits; the README's fullscreen screenshots show commercial games' art and were captured in an agent session.[1][30] New store screenshots are therefore the maintainer's to take, from the demo library of free games: fullscreen Home, the fullscreen library, and Quick Access over a running game.

## 11.4 Translations

There is one translation so far, Dutch, through Weblate.[1][2] Library Health, grouping and failed-launch help add many strings. Spacing the string-heavy issues out, and not merging them in one burst, lets translators keep up and avoids the template conflicts noted in section 1.2.

# 12. Product signals without telemetry

The no-telemetry position is worth keeping.[6] Prioritisation can still rest on evidence:

- The diagnostics report (DX-1) pasted into a bug report gives install type, session, sources and capabilities in one step.
- The feature form could ask which user journey is blocked, and not only what to build.
- A public hardware matrix from voluntary reports (section 4.7).
- Release issues with a checklist for volunteer testers of a pre-release.
- Local counts in Library Health, such as sources, games, groups and suggestions, let a user describe their library without sharing it.

# 13. What not to build

- **A store client.** No account login, purchases, entitlement refresh, install, update, repair or DRM for the stores' own games. Open in Launcher and Add a Store are the hand-offs, and they exist.
- **A Pindu cloud.** The spec's non-goals already say no accounts and no cloud sync.[9] Saves and state travel through folders the user picks.
- **Shell hooks.** Wrapper commands already cover running something around a game, as arguments and never as a shell string. If commands before or after a launch are ever wanted, they take the same form: a program and its arguments.
- **Desktop theming.** Designs belong to fullscreen mode. The desktop window stays Adwaita.
- **A binary plugin interface.** See section 8.11.
- **System features without a Pindu OS work package.** See section 4.2.

# 14. Decisions to take before issues are filed

Agents stop when a product decision is missing. Settling these first lets the issues be written so that none has to.

| # | Decision | Recommendation here | Blocks |
| --- | --- | --- | --- |
| 1 | The next milestones and their gates | M6 to M9 as in section 9 | Every issue, since each needs a milestone |
| 2 | Where this analysis and the workstream spec pages live, and how the AI disclosure records them | `docs/`, one page per workstream | PQ-3 |
| 3 | The risk tier of migrations and the backup format | High | GI-4 and every later schema change |
| 4 | What a group owns | A view over the source records | GI-4 onwards |
| 5 | Exact cross-references: group automatically or suggest | Automatic, with an undo | GI-5, GI-8 |
| 6 | Playtime across copies | Decide from GI-1's findings | GI-6 |
| 7 | Where Library Health lives, and whether there is a first-run summary | A page of its own; a one-time summary | LH-3, LH-5 |
| 8 | Where a console's diagnostics report goes | Decide after CP-1 | DX-3 |
| 9 | Which desktop-only flows a console needs | Decide from CP-1 | CP-3 and its follow-ups |
| 10 | Logs for every launch of a game Pindu runs, or opt-in only | A small bounded log per launch | CL-1, CL-2 |
| 11 | Named launch profiles | Not now; Start Safely only | LP-1's scope |
| 12 | Save snapshots: scope, format, Ludusavi | Managed prefixes first; decide the rest from SV-1 | SV-2 onwards |
| 13 | An online metadata provider | Local data first; decide from MD-6 | MD-7 |
| 14 | Emulation, a non-goal of the v1 spec | Decide in EM-1, not before M9 | EM |
| 15 | Where external importers would run | Decide in XP-1 | XP |
| 16 | The written soak against the release cadence | Say what a fast release may skip | PQ-7 |
| 17 | The Pindu OS work package for build provenance | Do it, since the OS spec expects it | PQ-6 |

# Appendix A. What changed from the original analysis

**Corrected against the repository**

- The newest release is 0.5.1, not 0.5.0; the system panel shipped in 0.4.0 and the gamescope session work in 0.5.0.
- Developer, genres and description are already in the schema and on the desktop game page. The original said the product had no such layer.
- Source health is already stored. Library Health is mostly a view, not a new model.
- Launch settings already include arguments, environment, working folder, Proton build, prefix and wrapper commands. Named profiles were demoted and replaced by Start Safely.
- Duplicates between desktop entries and launchers are already prevented at import.
- Per-game logs are opt-in through Play with Logs, not always present.
- The app cannot grant itself host access, so the proposed setup step for it could only explain a command.
- Save backups are limited by the sandbox to the games folder and folders the user picks; an archive format needs a dependency decision.
- Store screenshots are human-only work and need freely licensed art, so "low effort" did not hold.
- COPR, AUR and nixpkgs recipes exist and are maintained; the original called them contemplated.
- Issues #485 and #486 are both approved; the text now says what remains of each.
- "Low star count" was removed because it was not checked.

**Corrected against other sources**

- Cartridges, the closest app, was missing from the comparison and is end-of-life.
- Heroic's console mode and achievements are dated and scoped: 2.21 and 2.22, GOG achievements.
- Playnite is a Windows program with a Linux version planned; the `playnite.org` reference was dropped because it was not verified.

**Added**

- Pindu OS as a consumer of the launcher, and what follows from it.
- The product spec's non-goals and backlog, which already name several proposals and conflict with one.
- The maintainer's attention as the constraint, with what each workstream needs from him first.
- Controller parity as a workstream.
- The risk tier of migrations, the rollback path and the provenance work package.
- The gap between the written release gates and the release cadence.
- Local metadata before any online provider, which is also the spec's order.

**Restructured**

- Recommendations became workstreams with a fixed layout, decisions and work packages sized for one issue each.
- The roadmap became milestones with gates in place of version numbers.
- The top-ten backlog and the closing assessment were removed as repetitions of the summary and the work packages.

# Sources

Repository files were read from a clone at commit `0dcd666` (pindu-launcher) and `772d7c5` (pindu-os) on 19 November 2026. Web pages were read the same day unless marked.

1. [Pindu Launcher README](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/README.md) - features, sources, installing, fullscreen mode, gamescope, architecture, licences.
2. [MetaInfo](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in) - release notes for 0.1.0 to 0.5.1, store description and screenshots.
3. [AGENTS.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/AGENTS.md) - architecture, issue conventions, milestones, safety boundaries, testing and dependency rules.
4. [CONTRIBUTING.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/CONTRIBUTING.md) - layout, conventions and quality gates.
5. [Milestones](https://github.com/Tim-van-Oudheusden/pindu-launcher/milestones) and the [issue list](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues) - M5 Release and Nice to have with their counts, and the labels on #63.
6. [docs/metrics/README.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/metrics/README.md) - every gate, and the no-telemetry statement.
7. [SECURITY.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/SECURITY.md) - scope, and the one-person statement.
8. [docs/releasing.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/releasing.md) - cutting a release, the soak, distribution recipes and the hardware checklist.
9. [Product spec](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/Adwaita%20Game%20Library%20%E2%80%94%20Main%20Spec.pdf) - non-goals, the duplicates rule, metadata providers and the unscheduled backlog.
10. [docs/system-panel.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/system-panel.md) - the system panel's spec, its availability rules, Issues table and Decisions.
11. [pindu-core source](https://github.com/Tim-van-Oudheusden/pindu-launcher/tree/main/crates/pindu-core) - `src/store/migrations/`, `src/store.rs`, `src/model.rs`, `src/sources/`, `src/library.rs`, `src/backup.rs`, `src/logs.rs`, `src/tasks.rs`, and the fixtures under `tests/`.
12. [Cargo manifests](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/crates/pindu-core/Cargo.toml) - the core's and the UI crate's dependencies.
13. [Flatpak release manifest](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher.yml) - the sandbox's permissions.
14. [Issue forms](https://github.com/Tim-van-Oudheusden/pindu-launcher/tree/main/.github/ISSUE_TEMPLATE) - the feature, bug and release forms.
15. [docs/risk-tiers.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/risk-tiers.md), `.github/risk-assessment.yml`, `scripts/tier-classify.sh` and [docs/multi-agent.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/docs/multi-agent.md) - risk tiers, who merges what, and how agents work. The classifier was run on a sample migration path.
16. [Issue #485](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/485) - the changed-line mutation gate, with coverage figures.
17. [Issue #486](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/486) - the docs-follow-behaviour gate and its three cases.
18. [Pindu OS spec](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/spec.md) and [README](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/README.md) - design, work packages, status and risks.
19. `docs/Research — Cartridges.pdf` in the launcher repository, and [Cartridges on apps.gnome.org](https://apps.gnome.org/Cartridges/) - its design, importers, last release and retirement. The retirement date comes from the research document.
20. [Heroic Games Launcher README](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher) - carried over from the original analysis, not read again.
21. [GamingOnLinux on Heroic 2.21](https://www.gamingonlinux.com/2026/04/heroic-games-launcher-gets-a-slick-full-screen-console-like-mode/), 23 April 2026 - console-like fullscreen mode and GOG achievements.
22. [GamingOnLinux on Heroic 2.22](https://gamingonlinux.com/2026/05/heroic-games-launcher-v2-22-brings-library-editing-big-screen-console-mode-improvements), 18 May 2026 - library editing, and installing and updating in console mode.
23. [Heroic issue #4261](https://github.com/Heroic-Games-Launcher/HeroicGamesLauncher/issues/4261) - a request for a filter that removes duplicate games.
24. [Playnite](https://playnite.link/) and its [extension documentation](https://api.playnite.link/docs/manual/features/extensionsSupport/extensionsSupportOverview.html) - features and extension kinds.
25. [GamingOnLinux on Playnite's Linux plans](https://www.gamingonlinux.com/2025/11/playnite-may-get-a-linux-version-during-2026-as-the-creator-plans-a-move-to-linux/), 18 November 2025.
26. [Lutris repository](https://github.com/lutris/lutris) - carried over from the original analysis, not read again.
27. [Ludusavi](https://github.com/mtkennerly/ludusavi) - its README: the manifest, supported libraries and command line.
28. `.claude/session-summary.md` in the launcher repository - the agents' hand-off notes for 19 November.
29. Git history and tags of the launcher repository - commit counts, and the dates of `v0.1.0` to `v0.5.1`.
30. [AI_DISCLOSURE.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/main/AI_DISCLOSURE.md) - how much is AI-assisted, and what a person made without AI.
