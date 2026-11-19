# Emulation

Emulation is a main reason people pick Playnite or Lutris, and it would take Pindu from "PC games across launchers" to
a complete library. It is also a stated non-goal of the v1 spec ([Spec amendments](spec-amendments.md)), so the first
step is a decision and not a feature. This page is workstream **EM** of the
[product analysis](product-analysis.md#810-emulation-em), milestone M9 Breadth, written as spike #546 (EM-1) asks: what
a first version would be, what it costs, how it behaves in the Flatpak, and a recommendation.

**Nothing here is decided.** Whether emulation joins the product, and any build work, are the maintainer's call (see
[Decisions](#decisions)). Until then, no issue builds emulation.

## What a first version would be

Narrow and declarative, as the analysis proposes. Pindu does not install emulators or manage them; it lists ROMs the
user already has and starts them with an emulator the user already has.

- **Emulator profiles**, defined by the user in Preferences: a name, a program (a host program by its path, or a
  Flatpak app by its ID, such as `org.libretro.RetroArch`), an argument template, and the file extensions it opens.
  The template is a list of arguments, one of them `{rom}`, which becomes the ROM's path as one argument, never split
  and never passed through a shell: `["-L", "/usr/lib64/libretro/snes9x_libretro.so", "{rom}"]`. Pindu ships no
  profiles and names no cores.
- **ROM folders**: a folder, the system it holds (a name the user types, such as `Super Nintendo`, shown as the
  folder's label and its source view), and the profile that starts it.
- **A read-only scan** on every sync: each file below the folder whose extension the profile lists is a game. Pindu
  never writes to the folder, opens an archive or reads a ROM's contents.
  - **Title:** the file name without its extension and without trailing tags in brackets or parentheses
    (`Chrono Trigger (USA) (Rev 1).sfc` is `Chrono Trigger`); the full file name shows on the game page.
  - **Multi-disc and multi-file sets:** a file that another listed file names is not a game of its own. An `.m3u`
    playlist is the game and the discs it lists are left out; a `.cue` or `.gdi` sheet is the game and its tracks are
    left out. Disc files with no playlist each show as a game, and the user can merge them by hand (#507).
  - **Compressed formats** (`.zip`, `.7z`, `.chd`, `.rvz`, …) are only extensions a profile lists; the emulator opens
    them.
- **Launches as argv lists**: the profile's program and its template with `{rom}` filled in, as a host command (see
  [Flatpak, native and Pindu OS](#flatpak-native-and-pindu-os)). Pindu times the session as it times every game it
  starts, and a failed launch reports like any other host command ([Launch outcomes](launch-outcomes.md)).
- **The same model as every other game.** A ROM is a `games` row like any other: favourites, hiding, collections,
  play history, "Jump back in", Library Health, backups and both shells work unchanged, which is what
  the M9 gate asks ("every new kind of game or source shares the grouping, collection and history model, with no
  separate interface for it").
- **Artwork:** no source art. The generated placeholder shows until the user picks art or SteamGridDB finds the title
  by name, as for a game added by hand.

## What it costs in the schema and the code

### Game IDs and the `Source` set

`Source` is a closed set and its string form is the prefix of every game ID (`<source>:<native id>`, `model.rs`). A
first version adds **one** variant, `Source::Emulator` (`emulator`), not one per system or per emulator:

- **Native ID:** `<ROM folder ID>/<path below the folder>`, for example `emulator:3/Chrono Trigger (USA).sfc`. A
  profile change does not change any ID, because the profile is how a game starts, not which game it is. Editing a
  ROM folder's path keeps its ID, so moving a collection to another disk keeps play history; removing a folder and
  adding it again makes new games. Hashing file contents is left out: it reads gigabytes for disc images.
- **No migration for the variant itself.** `games.source` and `source_states.source` carry no `CHECK`.
- **Code that names every source:** issue #556 (the itch importer) shows the list a new source touches: `model.rs`,
  `IMPORTED_SOURCES` in `sources/sync.rs`, `sources/location.rs` and `sources/watch.rs`, `launch.rs`, and the UI's
  `sources.rs` and `launch_service.rs`. Emulation also adds its place in `DEFAULT_SOURCE_ORDER` (`game_groups.rs`),
  last: a ROM is never a better copy than a store's own build.
- **Backups:** an `emulator:` ID is new text in the same format. A build older than the change cannot parse it and
  leaves that game out of the import, as it would any game it does not know.

### The `platform` column

`games.platform` is `CHECK (platform IN ('windows', 'linux'))` (migration 0001), and it means the operating system a
build targets; the desktop library's Windows and Native toggles filter on it. SQLite cannot change a `CHECK` in place:
widening it means rebuilding `games`, as migration 0004 rebuilt `artwork`, but `games` is the table every other one
points at (launch configs, artwork, collections, play sessions, groups, source details), so the rebuild runs with
foreign keys off and is the riskiest kind of migration (#548 rates migrations high risk).

This page proposes **not widening it**. A ROM's `platform` stays `NULL` (unknown), which is true: the emulated system
is a different axis from the host OS. The system lives with the ROM folder instead, in new tables (below). The cost:
with the Windows or Native toggle on, ROMs are filtered out, which matches what the toggles mean.

### New tables

One migration, appended after the chain this roadmap fixes (0006 to MD-2, MD-4, CL-1):

| Table | Holds |
| --- | --- |
| `emulator_profiles` | ID, name, program kind (`host` or `flatpak`), program path or app ID, argument template (JSON list), extensions (JSON list) |
| `rom_folders` | ID, path (the host path), system name, profile ID |

The game's folder comes from its native ID, so no per-game table is needed. `launch_kind` needs no change: a ROM is
`host-command` with a `launch_configs` row rebuilt from its profile on every sync, so editing a profile changes how
every one of its games starts.

### Grouping

[Game identity](game-identity.md) applies unchanged. A ROM has no store cross-reference, so it is never grouped
automatically. Equal normalised titles from different sources are a suggestion, so a ROM and a store re-release of
the same title (a ROM named `Okami` and a store's `Ōkami` normalise alike) come up for review, and the
user decides. Regional and revision variants of one ROM are one source, so no suggestion pairs them; the user merges
them by hand. Suggesting variants within one system is left for later (game identity's out of scope names it).

### Library scale

A ROM folder can hold thousands of files, so a library with ROMs passes 10,000 games sooner than a PC library. PQ-5
measured the core at 10,000 games ([Library scale](metrics/library-scale.md)); for this page the same `library_bench`
ran at 30,000 games (164,631 artwork rows, 5 runs, same machine and day as PQ-5's table):

| Step | 10,000 games, median (ms) | 30,000 games, median (ms) |
| --- | ---: | ---: |
| Load: open the store, `library::load` | 80.94 | 265.85 |
| Load: sort and search keys | 4.54 | 14.28 |
| Sort, slowest mode (NameDescending) | 0.86 | 3.61 |
| Filter, slowest view (Steam) | 0.15 | 0.72 |
| Search, "the aether" (10 keystrokes) | 3.79 | 11.82 |

Every step grows about linearly. At 30,000 games a sort still fits in a 16 ms frame on the core's side, a search
keystroke costs about 1.2 ms, and the load, the one slow step, runs off the main thread. What PQ-5 leaves out still
holds: GTK's list model overhead and widget creation are not measured. The first build issue measures a scan of a large
folder through the document portal, which these numbers do not cover.

## Flatpak, native and Pindu OS

- **Choosing a ROM folder.** In the Flatpak, a folder is reached through the file chooser, and the document portal
  keeps access to it across restarts. Pindu stores the folder's host path, translated from the portal path as it
  already does for source folders (`file_service::host_path`, `paths::document_portal_parts`), and scans through the
  portal path. No `finish-args` change: no `--filesystem` for ROM folders. Natively the folder is read directly.
- **Starting an emulator** happens on the host, because emulators are not in Pindu's sandbox. A host program runs
  as a host command; a Flatpak emulator runs as `flatpak run --file-forwarding <app ID> <arguments> @@ <rom> @@` on the
  host, so the emulator's own sandbox gets the ROM through the document portal even without filesystem access to the
  folder. An `.m3u` or `.cue` names files beside it, which file forwarding does not share, so multi-file sets need the
  emulator's own access to the folder; the profile dialog says so.
- **Host access.** In the Flatpak both kinds need host spawn, the `--talk-name=org.freedesktop.Flatpak` override
  Preferences already explains for host launches (`permissions::needs_host_spawn`). Without it ROM games show as
  needing host access, like a Faugus or hand-added game; the scan works either way.
- **Native packages and Pindu OS** start the emulator directly. On Pindu OS the profile and folder dialogs must work
  with a controller alone, since fullscreen mode is the whole interface.

## Out of scope

- Installing emulators, cores or BIOS files, checking that a BIOS is present, or changing an emulator's settings.
- Shipped emulator profiles, a list of known systems, or core recommendations.
- Reading ROM headers, hashing files, or matching against No-Intro or Redump databases.
- Per-system artwork shapes (box art that is not 2:3) and per-system metadata.
- Controller mapping inside emulators, which stays the emulator's own.
- Save states and emulator save files (save snapshots cover Windows games in managed prefixes only).
- Suggesting regional and revision variants as one game.

## Issues

A draft, to be filed only if the maintainer decides yes:

| Issue | Covers | Depends on |
| --- | --- | --- |
| EM-2 Add emulator profiles and ROM folders to the library store | The migration (`emulator_profiles`, `rom_folders`), `Source::Emulator`, store functions, backup of profiles and folders | The decision, CL-1 #531 (the last migration in the chain) |
| EM-3 Scan ROM folders read-only | The scanner: extensions, title tags, `.m3u`/`.cue`/`.gdi` sets, native IDs, launch configs from profiles; a scan benchmark at 30,000 files through the document portal | EM-2 |
| EM-4 Start ROMs with their emulator profile | Argv from the template, host programs and `flatpak run --file-forwarding`, the host spawn tier | EM-3 |
| EM-5 Edit emulator profiles and ROM folders in both shells | Preferences pages and fullscreen mode's settings, with a controller alone; the file chooser and host path | EM-2 |

## Recommendation

**Not yet; revisit after M9's other breadth work, and if it is a yes, build exactly the narrow version above.**

- The model can take it without a table rebuild: one `Source` variant, two tables, no `platform` change, an existing
  launch kind and no new Flatpak permission. The core stays responsive at 30,000 games.
- The cost is ongoing rather than up front: every emulator, multi-file format and Flatpak file-access case becomes a
  support question in a project that has so far kept to "PC games across launchers" (the README's own promise).
- The external importer contract (#547, XP-1) is the cheaper way to let emulation in without owning it: a
  helper that scans ROM folders and returns argv launches would give the same games through the same model, and keep
  the maintenance outside the core. If the maintainer adopts that contract, this page recommends trying ROMs as its
  first outside integration before building EM-2 to EM-5.

## Decisions

**The decision is the maintainer's and is pending.** Decided by the maintainer on 2026-11-19 (analysis
[section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)): this page carries a recommendation; the
decision itself, and any build work, stay with the maintainer, and #546 stays open until then. The choices for the
maintainer:

- **Whether emulation joins the product** (the v1 non-goal in [Spec amendments](spec-amendments.md)): yes with the
  narrow first version, through the external importer contract, or no.
- **Not widening `platform`**: the emulated system kept beside the ROM folder, and `platform` left `NULL`, instead of
  rebuilding `games`.
- **One `emulator` source** with `<folder ID>/<path>` native IDs, rather than a source per system or IDs from file
  hashes.
- **No shipped profiles**: the user names the program and arguments; Pindu names no emulator or core.
