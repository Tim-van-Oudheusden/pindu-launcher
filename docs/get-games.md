# Get Games: a games catalogue from Flathub inside Pindu launcher

Spec and implementation plan. Written 2026-11-24. It replaces the earlier plan of the same date, a fork of Bazaar with controller support; section 2 records why.

Committed as `docs/get-games.md` on 2026-11-24 (#890), with an entry in `docs/spec-amendments.md` (section 3). Since it was written:

- [pindu-os#66](https://github.com/Tim-van-Oudheusden/pindu-os/issues/66), what the image has to allow and provide (uninstall rights, runtime cleanup, the console's lists), is done: [decision 0015](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/decisions/0015-get-games.md).
- [#830](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/830), the Apps section that opens game launchers and installed non-game apps, has shipped ([`apps.md`](apps.md)).
- The launcher work packages in section 13 are issues: GG-1 #884, GG-2 #885, GG-3 #886, GG-4 #887, GG-6 #888, GG-5 #889. The maintainer's decisions (H-1) are recorded on #884.

## 1. Summary

A Pindu OS console can install three game stores through Add a Store, and nothing else. **Get Games** closes that gap inside Pindu launcher: a small catalogue of the games on Flathub, plus a short list of extras, that the owner browses, installs and removes with a controller.

It is not a general app store and not a separate program. It reads the catalogue file that Flatpak already downloads, filters it to games, and installs through the route Add a Store already uses. An installed game then shows up in the library by itself, because the launcher already imports Flatpak games.

Most of what a store needs already exists in the launcher (section 4). The new parts are a catalogue model, a browse screen, an app page and uninstall.

## 2. Why this and not a fork

Three options were considered.

| | Get Games (this plan) | Fork of Bazaar | GNOME Software |
| --- | --- | --- | --- |
| Licence allows it | Own code, GPL-3.0-or-later | Yes: GPL-3.0-or-later | Yes: GPL-2.0-or-later |
| New program in the image | No | Yes, plus WebKitGTK | Yes, plus package back ends |
| Controller support | Already there | To be written in C, in upstream's code | To be written |
| On-screen keyboard | Already there | To be written | To be written |
| Ongoing cost | Our own code | A merge with every upstream release, for as long as it ships | The same, on a larger code base |
| New repository and release pipeline | No | Yes | Yes |
| A second app reading the controller | No | Yes | Yes |
| Scope | Games, and a few extras | All of Flathub | All of Flathub and more |

The licence was never the obstacle: Bazaar's source headers read GPL-3.0-or-later, the same licence as both Pindu projects, and a fork that kept the licence, the notices and the credits and published its source would have been fine. The fork lost on cost. Bazaar moves fast (about 2,600 commits by August 2026), and a controller layer touches exactly the interface files that change most. The launcher, by contrast, already has controller navigation, a search keyboard, an install flow and a task list, all written and tested.

What the fork would have given that this plan does not: every app on Flathub, reviews, Flathub accounts, and a team that maintains the store. Section 12 says what Pindu takes on instead.

If Bazaar itself ever gains controller support, shipping it unmodified beside Get Games costs one package and is worth reconsidering then.

## 3. Goals, non-goals and one amendment

### Goals

- With a controller alone: find a game on Flathub by browsing or by searching, read what it is, install it, play it, remove it.
- An installed game is in the library without a restart.
- The catalogue works from a file on disk. Browsing needs no network once the catalogue is there; only screenshots and installs do.
- A distributor decides what the catalogue shows, through one file, without a launcher release.
- No new program, service or permission on the system.

### Non-goals

- **Not all of Flathub.** Games, and a named list of extras. Everything else is out.
- **No store client.** No accounts, purchases, reviews, ratings submission or wish lists.
- **No update screen.** On Pindu OS a timer updates every Flatpak; elsewhere that is the system's job.
- **No remotes.** Flathub only. No adding remotes, no bundles, no `.flatpakref` files.
- **No parental controls.** Age ratings are shown, not enforced.
- **Not in the Flatpak build** (section 5).

### The amendment

The launcher's standing non-goal reads "No store front: buying, downloading and updating store games stays in Steam and Heroic", and the product analysis lists "a store client" under what not to build. Both were written about the stores' own games, and both still hold: Get Games does not sign in to anything or touch a store's library.

It is still the launcher downloading games, which the launcher did not do before. `docs/spec-amendments.md` gets an entry that says so plainly: Add a Store installed three named apps from Flathub; Get Games extends that to Flathub's games. The non-goal's wording stays, with this as its recorded limit. The maintainer confirmed it on #884 (H-1).

## 4. What already exists

| A store needs | The launcher has |
| --- | --- |
| Installing a Flatpak from Flathub | `add_store`: the host's `flatpak install`, system installation when it has a `flathub` remote, the user's otherwise |
| Install progress and errors | `ProgressReader` reads flatpak's progress lines; `error_message` extracts flatpak's own error |
| Rights to install on Pindu OS | The polkit rule for Flathub installs and updates |
| Controller navigation | Fullscreen mode |
| Text entry without a keyboard | The search keyboard (`osk.rs`) |
| Background work that outlives a screen | Tasks and notifications |
| A new game appearing in the library | The desktop source imports every `Categories=Game` entry in the Flatpak exports; Add a Store already triggers a sync when an install finishes |
| Fetching and caching images | The artwork services and cache |
| Reading YAML | `yaml-rust2`, already a dependency of `pindu-core` |
| Updating installed Flatpaks | Pindu OS's update timer |

What does not exist: reading the catalogue, the screens, uninstall, and install for an app ID that is not one of three.

## 5. Where it lives

**In pindu-launcher, as a generalisation of Add a Store.**

- `crates/pindu-core`: a `catalogue` module (the word "store" already means storage in the core) for reading, filtering and searching; the install and uninstall commands, generalised from `add_store`. GTK-free and test-first, as the core is.
- `crates/pindu-ui`: a service that runs the commands and fetches screenshots, and the screens in both shells. Shells read nothing and spawn nothing themselves, as now.
- `data/`: the default lists file and the section's art.

**Only in the native build.** Add a Store already draws this line: the native build (the RPM Pindu OS ships) runs the host's `flatpak`; the Flatpak build cannot, gets no permission to, and hands off to the software centre instead. Get Games follows it. The Flatpak build can also only read the installation's `exports` folder, not its catalogue, and widening a manifest's `finish-args` is not agent work. So the Flatpak build does not show Get Games. Its users are on a desktop that has a software centre.

**On Pindu OS** nothing is added to the image. pindu-os#66 extends the polkit rule, cleans up unused runtimes and ships the console's lists.

## 6. The catalogue

### 6.1 Source

Every Flatpak remote publishes an AppStream catalogue: one compressed XML file describing each app, with its name, summary, description, categories, licence, age rating and screenshot addresses, beside a folder of icons. Flatpak downloads it into the installation:

- system: `/var/lib/flatpak/appstream/flathub/x86_64/active/`
- user: `~/.local/share/flatpak/appstream/flathub/x86_64/active/`

Get Games reads that file. It does not call Flathub's web API. This keeps browsing offline and fast, takes no dependency on a web service's shape or terms, and uses data whose signature Flatpak has already verified.

The catalogue comes from the same installation that installs go to (`Installation::from_system_remotes`).

### 6.2 Refresh

A new console has no catalogue until something asks Flatpak for one. When Get Games opens and the catalogue is missing or more than a day old, the launcher runs `flatpak update --appstream` for that installation as a task, and shows the previous catalogue meanwhile. With no catalogue and no network, the screen says so and offers to try again.

### 6.3 What counts as a game

An app is offered when all of these hold:

- it has the category `Game` (the freedesktop main category; its additional categories, such as `ActionGame`, `StrategyGame` or `Emulator`, become the catalogue's shelves);
- it is not on the blocklist;
- it is an application, not a runtime, extension or add-on;
- it is not end-of-life.

Or it is on the **extras** list: named non-game apps a console owner reasonably wants.

Categories are declared by each app's developer, so the Game category contains tools and miscategorised apps. The blocklist exists for that, and GG-1 measures how much of it there is.

### 6.4 The lists file

One YAML file holds every editorial choice. The launcher ships a default; a distributor replaces it with a file under `/usr/share` (proposed: `/usr/share/pindu-launcher/distributor/get-games.yaml`; GG-2 fixes the path and format). Pindu OS already ships a file at that path, in a format of its own proposal: `version: 1` with `categories`, `blocklist`, `extras` and `picks` as lists of app IDs, each with a comment ([decision 0015](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/decisions/0015-get-games.md)). GG-2 settles on one format, and the example below follows whichever it is.

```yaml
# Illustrative. The real entries are the maintainer's to choose.
categories: [Game]
block:
  - id: com.example.GameTool      # a modding tool, needs a mouse and a file manager
extras:
  - id: org.mozilla.firefox       # a browser, for store sign-ins that open a web page
  - id: tv.kodi.Kodi              # media player with a controller interface
picks:
  - id: net.supertuxkart.SuperTuxKart
  - id: org.libretro.RetroArch
```

Every entry carries a comment with its reason. An ID that is not in the catalogue is skipped silently, so the file survives apps being renamed or removed.

### 6.5 Order

The catalogue has no popularity data. The first version orders by the lists file's picks, then by category shelf, then by name. Sorting by popularity would need Flathub's web API and is left out until someone has confirmed that API and its terms (section 16).

### 6.6 Reading it

The catalogue is large and changes at most daily. It is parsed once per catalogue version, off the main thread, into a compact index kept in the cache folder (not in the library database, and safe to delete). Startup and browsing read the index.

Parsing needs an XML reader and gzip in `pindu-core`. That is a new direct dependency, which AGENTS.md reserves for the maintainer (H-1). `flate2` is already in the lock file through other crates; an XML reader is not.

### 6.7 What an app shows

Name, icon, one-line summary, description, screenshots, developer, licence (free or proprietary, in words), age rating derived from the catalogue's content rating, download and installed size (from `flatpak remote-info`, fetched when the page opens), and whether Flathub has verified the developer, if the catalogue carries that.

Unverified and proprietary apps are shown with a plain label, not hidden. Hiding them is a lists-file decision, not a code one.

Text is "from Flathub" in words. Flathub's logo is used only if its brand guidance allows.

## 7. Installing and removing

### 7.1 Install

- The command is Add a Store's, with the app ID from the catalogue: `flatpak install`, non-interactive, from `flathub`, into the chosen installation. Arguments are an argv list; an app ID is never spliced into a shell string, and only IDs that came out of the catalogue are accepted.
- One install runs at a time; more are queued. Each is a task: it survives leaving the screen, shows progress in Quick Access, can be cancelled, and ends in a notification.
- The first game also downloads a runtime, often larger than the game. The progress reader already counts each operation; the page says "Installing 2 of 3" in words so a long first install does not look stuck.
- Before starting, compare the download and installed sizes with free space and say so if it will not fit.
- A failure shows flatpak's own last error line, as Add a Store does, and leaves the app page ready to try again.

### 7.2 Remove

- `flatpak uninstall`, non-interactive, for an app Get Games can see as installed.
- A confirmation asks first and offers to delete the app's data too (`--delete-data`), off by default.
- Reachable from the app's page in Get Games and from the game's page in the library, for a game that came from a Flatpak.
- On Pindu OS this needs the polkit change in pindu-os#66. Without it the command fails; the launcher shows that failure like any other.
- Unused runtimes are not the launcher's to remove. Pindu OS cleans them up in its update service.

### 7.3 The three stores

Steam, Heroic and Lutris are in Flathub's Game category. They appear in Get Games on a Launchers shelf, and choosing one runs the existing Add a Store flow, which also explains signing in. One code path, two doors. Add a Store stays where it is for first-run set-up.

## 8. Screens

### 8.1 Fullscreen mode

- **Home:** a Get Games section in the place sections have in each design, with its art (`data/home/get-games.svg`). It appears in every design.
- **Catalogue:** rows of cards. Picks first, then Launchers, then one shelf per category that has games, then Extras. Shoulder buttons jump between shelves. An installed app is marked.
- **Search:** the existing search keyboard, matching name, summary and keywords as each character is typed.
- **App page:** icon, name, summary, the facts from section 6.7, screenshots that page left and right, the description. One main action that changes with state: Install, Cancel with progress, Play or Open, and Remove behind a second button.
- **States:** no catalogue yet, refreshing, offline, nothing found, install failed, not enough space. Each says what happened and what to do next.

It is built from the widgets fullscreen mode already has. No new design language.

### 8.2 Desktop window

A Get Games item in the sidebar, in the native build: the same catalogue as a grid with a search field, and the app page as a navigation page. It comes last (GG-5), since a desktop has a software centre and Pindu OS has no desktop.

## 9. After an install

- **A game** appears in the library: the launcher syncs when the install finishes, and the desktop source imports it. Get Games changes nothing in the library model.
- **An extra** is not a game and does not appear in the library. It appears in the Apps section (#830, shipped). GG-4's Open action still opens an installed extra from its own page.
- **Updates** arrive from Pindu OS's timer. A running game keeps its version until it starts again.

## 10. Pindu OS

Done in [pindu-os#66](https://github.com/Tim-van-Oudheusden/pindu-os/issues/66) ([decision 0015](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/decisions/0015-get-games.md)):

- allow `pindu` to uninstall apps and runtimes without a prompt; remotes stay locked;
- confirm and test that `pindu` can refresh the catalogue;
- remove unused runtimes in the existing update service;
- ship the console's lists file;
- a decision record that also records the two alternatives in section 2.

No package is added and no pin changes. Unused runtimes are removed by `pindu-os-flatpak-update.service`, as root, after its update.

## 11. Apps section

Shipped in [#830](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/830) ([`apps.md`](apps.md)). It lists installed applications and opens them: the game launchers the games list leaves out, and the extras Get Games installs. It installs and removes nothing.

## 12. Maintenance burden

### What can break, and how often

| Thing | Who changes it | How often | What it costs |
| --- | --- | --- | --- |
| flatpak's progress and error output | Flatpak | Rarely | Already carried for Add a Store; tests pin the format |
| The AppStream catalogue format | AppStream | Very rarely; versioned and stable for years | A parser fix |
| Where Flatpak keeps the catalogue | Flatpak | Very rarely | One path |
| Flathub's use of categories | App developers | Constantly, in small ways | Blocklist edits |
| The lists file | The maintainer | When wanted | A few lines; on Pindu OS, an image update |
| Each new fullscreen design | The maintainer | Per design | The catalogue and app page must work in it |

There is no upstream to merge and no second release pipeline.

### What Pindu now owns

- **A store's failure cases:** a full disk, a dropped connection halfway, an app that disappears from Flathub between browsing and installing, a runtime that fails to download.
- **Curation.** The quality of the catalogue is the quality of the lists file. A neglected blocklist shows junk.
- **Trust presentation.** What an unverified or proprietary app looks like is Pindu's choice and Pindu's responsibility. Get Games installs software; it must never offer anything that is not in the signed Flathub catalogue.
- **Screens multiplied by designs.** Every fullscreen design has to carry the catalogue and the app page.

### Sizing

Rough, for agents working test-first in the existing code base: S is a day or two, M under a week, L more.

## 13. Work packages

`GG-n` is launcher work, `OS-22` is pindu-os, `H-n` needs a person.

| ID | Work | Done when | Size | After |
| --- | --- | --- | --- | --- |
| H-1 | Decide: the amendment in section 3, the XML and gzip dependency, the name "Get Games", the stores' place (7.3), the first extras | Recorded on the GG-1 issue (#884) | S | |
| GG-1 (#884) | Spike on a real Flathub catalogue: how many games, how many miscategorised, what the catalogue carries (verification, age ratings, end-of-life), parse time and index size on the test laptop, the output of `flatpak remote-info` and `flatpak uninstall`, a first blocklist | A write-up, and a fixture catalogue cut from the real one for the tests | S | H-1 |
| GG-2 (#885) | Catalogue core: locate, parse, filter, the lists file with a distributor override, the cached index, search | Core tests over the fixture: the filter rules, the lists, search, a missing or broken catalogue | M | GG-1 |
| GG-3 (#886) | Install and remove for any catalogue app: commands, queue, progress, cancel, sizes and the free-space check, tasks and notifications, sync on completion, the catalogue refresh | Core tests with a fake backend, as `add_store` has; an install started on one screen finishes after leaving it | M | GG-2 |
| GG-4 (#887) | Fullscreen Get Games: the Home section, catalogue, search, app page, every state, in every design | A controller alone gets from Home to a playing game and back to removing it | L | GG-3 |
| GG-6 (#888) | Remove from a game's page in the library; installed marks that stay true when something else installs or removes an app | Removing a Flatpak game from its page works; the catalogue's marks follow the system | S | GG-4 |
| GG-5 (#889) | The desktop window's Get Games, native build only | The same journey with a mouse | M | GG-4 |
| OS-22 | pindu-os#66 (done) | See the issue | S | lists file after GG-2 |
| L-04 | #830 (done) | See the issue | M | |
| H-2 | Device test on Pindu OS with a controller alone: refresh, browse, search, install a game, play, install an extra, open it from Apps, remove both, check the disk | Each step works; anything that does not is an issue. GG-4 adds it to the hardware checklist | S | GG-4, OS-22, L-04 |

GG-5 is last on purpose and can be dropped without hurting the console.

## 14. Testing

- **Core, test-first:** the fixture catalogue from GG-1 drives the filter, the lists file, search and the index. Install and remove run against a fake backend that replays real flatpak output, including failures, as `add_store`'s tests do.
- **No test talks to Flathub.** The fixture is a file in the repository.
- **UI:** the states in section 8.1 each have a test that reaches them from the fake backend.
- **Pindu OS:** the VM guest check evaluates the polkit rule from the launcher's process for uninstall and catalogue refresh.
- **On hardware (H-2):** a real install, because only a real Flathub download shows the first-runtime wait, the disk check and the return to the library.

## 15. Risks and open decisions

| Risk | Why it matters | What to do |
| --- | --- | --- |
| The Game category is noisy | A catalogue full of tools and abandoned games reads as broken | GG-1 measures it; picks first; a maintained blocklist |
| A long first install | The first runtime can be far larger than the game | Say which step is running; make the task visible in Quick Access |
| The disk fills | Consoles have one disk and no file manager | The free-space check; runtime cleanup in pindu-os#66 |
| Scope creep toward a general store | "Just add this app too" is how extras become all of Flathub | Extras stay a named list with a reason per entry |
| The catalogue is large | A slow parse on an old laptop stalls the first visit | Parse once, off the main thread, into an index; GG-1 measures on the test laptop |
| Games that need a mouse | Many Flathub games are not controller games | Nothing in the catalogue says which are. Accept it in the first version; picks favour controller games |
| The Flatpak build has no Get Games | Two builds, two feature sets | Already true of Add a Store; say so in the README |

Decisions for H-1 (#884 records that each was taken on 2026-11-24, not always which option):

1. Confirm the amendment to the "no store front" non-goal.
2. Approve an XML reader and gzip as direct dependencies of `pindu-core`.
3. The name. "Get Games" sits beside "Add Game" and "Add a Store"; if three near-identical names confuse, rename one.
4. Whether the three stores show in Get Games (proposed: yes, through the Add a Store flow).
5. The first extras, and whether unverified apps are shown.

## 16. What I could not check

- **A real catalogue.** The number of games, the miscategorisation rate, whether Flathub's verification and end-of-life state are in the catalogue file, and the parse cost are all GG-1's to measure. The file's location and general contents are standard Flatpak and AppStream behaviour.
- **Flathub's web API.** I found only an old forum thread naming a v1 endpoint and no usage terms. The plan does not depend on it.
- **The details polkit receives on uninstall**, and that a catalogue refresh needs no prompt on the image. pindu-os#66 checked both: flatpak's helper passes only the ref on uninstall, so the rule allows any ref (Flathub is the only remote), and `flatpak update --appstream --system --noninteractive` runs as `pindu` with no polkit agent.
- **Flathub's brand guidance** for using its name and logo in another interface.
- **The live Bazaar repository on GNOME GitLab**, which refused automated reading. The licence finding in section 2 comes from the GitHub copy upstream archived on 2026-08-18, the day it moved.

## 17. Sources

- [pindu-launcher](https://github.com/Tim-van-Oudheusden/pindu-launcher): `AGENTS.md`, `docs/spec-amendments.md`, `docs/product-analysis.md` (section 13), `crates/pindu-core/src/add_store.rs`, `crates/pindu-core/src/sources/desktop.rs`, `crates/pindu-core/src/osk.rs`, the development Flatpak manifest
- [pindu-os](https://github.com/Tim-van-Oudheusden/pindu-os): `AGENTS.md`, `docs/decisions/0013-game-sources.md`, the polkit rules, the Flatpak update units, `tests/container/flatpak.sh`
- [Flathub's Game category](https://flathub.org/ja/apps/category/game/14)
- [freedesktop.org registry of additional categories](https://specifications.freedesktop.org/menu/latest/additional-category-registry.html)
- [AppStream catalogue data](https://sources.debian.org/data/main/a/appstream/1.1.2-1/docs/html/chap-CatalogData.html)
- [Flathub Discourse: an app miscategorised under Games](https://discourse.flathub.org/t/k3x-is-listed-in-games/1919)
- [Flathub Discourse: does Flathub expose a public API](https://discourse.flathub.org/t/does-flathub-expose-a-public-api-for-querying-applications/1765)
- [bazaar-org/bazaar on GitHub](https://github.com/bazaar-org/bazaar) (archived; licence and commit count)
- [GNOME Software's copyright file in Debian](https://sources.debian.org/src/gnome-software/49.3-1/debian/copyright)
