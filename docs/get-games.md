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

A new console has no catalogue until something asks Flatpak for one. When Get Games opens and the catalogue is missing or more than a day old, the launcher runs `flatpak update --appstream --noninteractive` for that installation as a task, and shows the previous catalogue meanwhile; once it ends, the catalogue is read again. That command prints nothing and succeeds even offline (#884), so only the catalogue file tells whether it came. With no catalogue and no network, the screen says so and offers to try again; with no catalogue after a refresh on a network, it says the catalogue could not be downloaded.

### 6.3 What counts as a game

An app is offered when all of these hold:

- it has one of the lists file's `categories` (by default only `Game`, the freedesktop main category; its additional categories, such as `ActionGame`, `StrategyGame` or `Emulator`, become the catalogue's shelves);
- it is not on the blocklist;
- it is an application (`desktop-application`, or `desktop` in older entries), not a runtime, add-on, console application or generic component;
- it is not end-of-life.

Or it is on the **extras** list: named non-game apps a console owner reasonably wants. An extra must be an application and not blocked or end-of-life either.

Categories are declared by each app's developer, so the Game category contains tools and miscategorised apps. The blocklist exists for that: GG-1 (#884) found 62 tools among Flathub's 799 Game applications, beside 53 launchers and 11 streaming clients, which stay.

The catalogue carries no end-of-life state; Flathub keeps it in the repository summary. The launcher reads it from `flatpak remote-ls --app --all --columns=application,options` (an `eol=` or `eol-rebase=` option), and offers everything when that command fails, for instance offline. Flathub already leaves almost every end-of-life app out of the catalogue.

### 6.4 The lists file

One YAML file holds every editorial choice. The launcher ships a default, `data/get-games.yaml`, built into the program. A distributor replaces it whole with `/usr/share/pindu-launcher/distributor/get-games.yaml`; the two are never merged, so a distributor's file states every list it wants. Pindu OS ships one ([decision 0015](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/decisions/0015-get-games.md)), and its format is the one the launcher reads:

```yaml
version: 1
categories:
  - Game # freedesktop's main category for games
blocklist:
  - com.steamgriddb.SGDBoop # writes artwork into a desktop Steam's folders from links on SteamGridDB's website
extras:
  - tv.kodi.Kodi # a media centre made for the television, with gamepad and remote control support
picks:
  - net.supertuxkart.SuperTuxKart # kart racing for up to four players on one screen
```

- `version` is required and must be `1`.
- Each list is a sequence of strings: Flathub app IDs, or AppStream categories for `categories`. Every entry carries a comment with its reason.
- A missing list is empty, except `categories`, which is then `[Game]`.
- An ID that is not in the catalogue is skipped silently, so the file survives apps being renamed or removed.
- A distributor's file that cannot be read (bad YAML, another version, an entry that is not a string) is logged, and the launcher's default applies.

### 6.5 Order

The catalogue has no popularity data. Its shelves come in this order: the lists file's picks in its order; Launchers (Steam, Heroic and Lutris, section 7.3); one shelf per game category that has games (Action, Adventure, Arcade, Board, Blocks, Card, Kids, Logic, Role playing, Shooter, Simulation, Sports, Strategy, then Emulators); More games, for games in no such category (201 of Flathub's 799); then Extras. Within a shelf, apps go by name. A game shows on every shelf of its categories; the three stores show only under Launchers. Sorting by popularity would need Flathub's web API and is left out until someone has confirmed that API and its terms (section 16).

Search matches as each character is typed, as the library's search does (case and accents ignored): apps whose name matches come first, then apps whose summary or a keyword matches.

### 6.6 Reading it

The catalogue is large and changes at most daily. It is parsed once per catalogue version, off the main thread, into a compact index kept in the cache folder (`~/.cache/pindu-launcher/catalogue/system.json` or `user.json`; not in the library database, and safe to delete). Startup and browsing read the index. It holds only the applications the lists' categories and extras let through, with their texts in the session's language when the catalogue has them, and is made again when the catalogue's time or size, the session's languages, or the lists' categories or extras change.

The launcher reads the uncompressed `appstream.xml` that Flatpak deploys beside `appstream.xml.gz`, with `quick-xml` (approved on #884), so it needs no gzip. GG-1 measured 160 ms to parse Flathub's 52 MB catalogue on a Ryzen 7 7800X3D, and a 2.3 MB index for its 797 games. No catalogue (Flatpak has not downloaded one yet) and a broken one are told apart, so the screens can say which.

### 6.7 What an app shows

Name, icon, one-line summary, description, screenshots, developer, licence (free or proprietary, in words), age rating derived from the catalogue's content rating (the youngest age its OARS rating suits, by the Common Sense Media ages AppStream maps it to; all ages for an empty rating), download and installed size (from `flatpak remote-info`, fetched when the page opens), and whether Flathub has verified the developer (`flathub::verification::verified` in the catalogue; 495 of 804 games were verified when GG-1 measured).

Unverified and proprietary apps are shown with a plain label, not hidden. Hiding them is a lists-file decision, not a code one.

Text is "from Flathub" in words. Flathub's logo is used only if its brand guidance allows.

## 7. Installing and removing

### 7.1 Install

- The command is Add a Store's, with the app ID from the catalogue: `flatpak install -y flathub <id>` into the chosen installation (`--system` or `--user`; the user installation gets the Flathub remote first). Arguments are an argv list; an app ID is never spliced into a shell string, only IDs the catalogue offers are accepted, and every ID is checked to be an app ID before it reaches a command line. The stores (section 7.3) are refused here and go through Add a Store.
- It passes `-y` but not `--noninteractive`, which prints no progress at all (#884, the maintainer's choice there); its standard input is closed, so a question `-y` does not answer reads the end of input instead of waiting. Add a Store installs the same way, which also gave its progress bar the numbers it had never seen.
- One job runs at a time, installs and removals alike; more are queued. Each is a task: it survives leaving the screen, shows progress in Quick Access ("Waiting", "Checking the space", "Installing 2 of 5"), an install can be cancelled there, and each ends in a notification. A cancelled install keeps the runtimes it finished until Pindu OS's cleanup removes them.
- The first game also downloads a runtime, often larger than the game. The progress reader counts each of flatpak's operations; Quick Access says "Installing 2 of 3" so a long first install does not look stuck.
- Before starting, the launcher asks `flatpak remote-info` (in the C locale, so its sizes read the same in every language) for the download and installed sizes, and refuses when the installed size plus the 500 MB Flatpak keeps free (`min-free-space-size`) is more than the free space, saying how much is needed. A runtime the app needs is not in those sizes; Flatpak's own check catches that. When the sizes or the free space cannot be found, the install goes ahead.
- A failure shows flatpak's own last error line, as Add a Store does, and leaves the app page ready to try again; one Flathub could not be reached for says so in words.
- An install or removal that ends syncs the library, so a game appears or goes without a restart.

### 7.2 Remove

- `flatpak uninstall --noninteractive -y`, for an app Get Games can see as installed. It cannot be cancelled once it runs.
- A confirmation asks first and offers to delete the app's data too (`--delete-data`), off by default.
- Reachable from the app's page in Get Games, and as **Uninstall** on the game's page in the library (the options sheet in fullscreen mode, the menu on the desktop) for a game whose desktop entry a Flatpak in Get Games' installation exports: Flatpak names exported entries for the app's ID, alone or followed by a dot, so the entry's ID says which app it is (`get_games::removable_app`, #888).
- The installed marks and the app page's action follow the installation: the launcher watches the `.changed` file flatpak touches after every install, update or removal, so changes the Pindu OS update timer, a terminal or another session makes show without a restart.
- On Pindu OS this needs the polkit change in pindu-os#66. Without it the command fails; the launcher shows that failure like any other.
- Unused runtimes are not the launcher's to remove. Pindu OS cleans them up in its update service. `flatpak uninstall` itself also takes the runtime and extensions that installing the app pulled in, once nothing else uses them (#884).

### 7.3 The three stores

Steam, Heroic and Lutris are in Flathub's Game category. They appear in Get Games on a Launchers shelf, and choosing one runs the existing Add a Store flow, which also explains signing in. One code path, two doors. Add a Store stays where it is for first-run set-up.

## 8. Screens

### 8.1 Fullscreen mode

- **Home:** a Get Games tile with the other sections in each design (in the Library row in Shelves), with its art (`data/home/get-games.svg`). It shows in the native build only: the Flatpak cannot run `flatpak` on the host without host access.
- **Catalogue:** rows of cards. Picks first, then Launchers, then one shelf per category that has games, then More, then Extras. LB and RB jump between shelves. An installed app's card says Installed. Back from an app's page, focus returns to that app's card.
- **Search:** Y opens the search keyboard over the catalogue, matching name, summary and keywords as each character is typed; results show as a grid.
- **App page:** icon, name, summary, the facts from section 6.7, screenshots that LB and RB page, the description. One main action that changes with state: Install, Cancel with the step and percent, Play (opens the game's library page and starts it) or Open (an extra), and Remove beside it, which asks first and offers to delete the app's data too.
- **States:** getting the catalogue, offline, no catalogue (refresh failed), nothing found, install failed (flatpak's own message), not enough space (what it needs and what is free). Each says what happened and what to do next; the first three offer Try Again.

It is built from the widgets fullscreen mode already has: the home tiles' button style, the search keyboard, the hint bar and an `AdwAlertDialog`. No new design language.

### 8.2 Desktop window

A Get Games item in the sidebar, beside Apps, in the native build only (#889): it shows the same catalogue page in the library's place, with its shelves, its search field (results as a grid) and every state of section 8.1, and a card opens the app page as a navigation page above it, with the same actions. The page's header bar has the sidebar toggle. It came last (GG-5), since a desktop has a software centre and Pindu OS has no desktop.

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

- **A real catalogue.** Measured by GG-1 on #884: the number of games, the miscategorisation rate, which fields the catalogue carries (verification yes, end-of-life no) and the parse cost. The HP laptop's parse time is not measured yet; the hardware checklist carries it.
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
