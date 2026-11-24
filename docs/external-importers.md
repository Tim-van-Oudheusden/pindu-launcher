# External importers

Every built-in source adds maintenance: the itch importer (#556) touched 25 files, from `model.rs` to the launch
service and the translation template. Playnite's extensions show the alternative: sources written and kept outside
the core. This page is workstream **XP** of the
[product analysis](product-analysis.md#811-external-importer-contract-xp), milestone M9 Breadth, written as spike #547
(XP-1) asks: where a helper runs and what that means for safety, what it may return, which launches it may describe,
how the contract is versioned, and a rollout that proves it on a built-in importer.

**Decided on 2026-11-23.** The maintainer adopted the contract as this page recommends, at protocol `0`, with one
addition: a helper is installed from Preferences (see [Decisions](#decisions) and
[Installing a helper from Preferences](#installing-a-helper-from-preferences)).

## The contract already exists

An importer is a function from a source folder to a `Snapshot` (`sources.rs`): the source, its games as
`ImportedGame` values, and the problems met while reading, as messages. Around it the core needs two more things per
source: whether the source is on this machine and where (`sources::location`, `ResolveError::NotFound`), and the files
whose change means a resync (`sources::watch::key_paths`). An external importer is those three written down as a
process protocol:

| Request | Answers | Built-in equivalent |
| --- | --- | --- |
| `probe` | Whether the source is present, its folder, and the paths to watch | `Location::resolve`, `watch::key_paths` |
| `import` | A snapshot: games and problems | `<source>::import(root)` |

A separate process speaking versioned JSON, not an in-process Rust plugin: Rust has no stable ABI, so a plugin would
break with every compiler and crate update, and it would run inside Pindu with all of Pindu's rights.

## Where a helper runs

A separate process is not a sandbox. Where the helper runs decides what it can reach, so a helper's manifest says
which of the three it is, and Pindu shows it when the user turns the helper on.

| Where | How Pindu starts it | What the helper can reach | Launches it may describe |
| --- | --- | --- | --- |
| **Sub-sandbox** (Flatpak only) | `flatpak-spawn --sandbox --no-network --sandbox-expose-path-ro=<folder>` for the helper's own folder and each folder it declares | The helper's own folder and the declared folders, read-only, and only if Pindu's sandbox sees them: the launcher folders in `finish-args`, or a folder the user chose through the file chooser. No network, no display, no session bus, none of Pindu's data | URI, Flatpak app |
| **Pindu's sandbox** (native packages and Pindu OS, where Pindu has no sandbox) | A child process | Everything the user can reach | URI, Flatpak app, host argv |
| **Host** (Flatpak, with host spawn) | `flatpak-spawn --host`, which needs the `--talk-name=org.freedesktop.Flatpak` override Preferences explains | Everything the user can reach | URI, Flatpak app, host argv |

- **Only the sub-sandbox is safe** in the sense that a broken or hostile helper cannot read beyond what it declared or
  send it anywhere. It also cannot write Pindu's library, which a plain child process inside Pindu's own sandbox
  could, because that sandbox holds Pindu's data folder read-write and has the network (`--share=network`). Pindu
  never runs a helper as a plain child inside its Flatpak sandbox for that reason.
- **Natively and on the host, a helper runs with the user's rights.** Pindu says so ("Runs with your permissions"),
  as it does for host launches, and treats installing one like installing any program.
- **No tier escalation.** A helper may only describe launches its own tier could start: a sub-sandboxed helper cannot
  return a host argv, or a helper the user trusted with nothing would get Pindu to run any command on the host. A
  host argv from a sub-sandboxed helper is dropped with a problem naming the game.

### Where helpers live

A helper is a folder holding `importer.json` and the program. Pindu finds helpers in two places:

- **Installed helpers** in `$XDG_DATA_HOME/pindu-launcher/importers/<helper ID>/` (in the Flatpak, Pindu's own data
  folder). The folder is the installed form: Preferences installs into it, and a folder copied there by hand works the
  same, which helper authors need while developing.
- **Bundled helpers**, shipped in the package rather than the user's importers folder, in
  `<libexecdir>/pindu-launcher/importers/<helper ID>/` (`/app/libexec/pindu-launcher/importers/` in the Flatpak).
  Bottles is the first ([Rollout](#rollout)).

Pindu lists helpers in Preferences and fullscreen mode's Sources beside the built-in sources, each **off** until the
user turns it on, showing where it runs, the folders it reads and the URI schemes it uses. Pindu never downloads or
updates a helper.

```json
{
  "protocol": 0,
  "id": "example-launcher",
  "name": "Example Launcher",
  "version": "0.3.0",
  "program": ["./example-importer"],
  "runs": "sub-sandbox",
  "reads": ["~/.local/share/example-launcher", "~/.var/app/com.example.Launcher/data/example-launcher"],
  "uri_schemes": ["example"]
}
```

`id` is lowercase letters, digits and `-`, at most 64 characters, and is the name of the helper's folder. `program` is
an argv list, never a shell string: its first entry is a file inside the helper's folder, relative to it, and the
rest are its arguments. `runs` is `sub-sandbox` or `host`; natively both run as a child process. `reads` lists the
candidate folders, each absolute or under `~/`, in the order `Location::of` tries them for a built-in source; the
probe picks one. A user's folder with the ID of a bundled helper never runs.

The sub-sandbox cannot see Pindu's data folder, so Pindu exposes an installed helper's own folder read-only beside the
folders it declares. A bundled helper needs nothing exposed: the sub-sandbox sees `/app`. Both were checked with
Flatpak 1.18 from inside the development Flatpak: without `--sandbox-expose-path-ro`, a helper in the data folder is
not found in the sub-sandbox; with it, the helper runs and cannot write to its folder; and `/app/bin` is listed.

### Installing a helper from Preferences

Preferences → Sources installs a helper from a **local `.tar.gz` the user chooses in the file chooser**, never a
download from a URL. A download would need integrity checks and signatures, and would put the network on the import
path; a file the user chose comes through the file chooser, so no `finish-args` change. Pindu already unpacks tarballs
with `tar` (`proton_download_service.rs`); the workspace has no zip reader, so `.zip` would need a new dependency.

1. **Package.** A `.tar.gz` holding `importer.json` and the program, at the top level or inside exactly one top-level
   folder.
2. **Nothing reaches the importers folder unchecked.** The archive is unpacked into a work folder beside it, on the
   same filesystem. An absolute path, a `..`, a link or a special file fails the install, and nothing is written
   outside the work folder while unpacking.
3. **Limits.** An archive over 16 MiB, or one that unpacks to more than 64 MiB or 1,000 entries, is refused. These
   are defaults the maintainer may change.
4. **Manifest.** The install fails with a reason when `importer.json` is missing or invalid: a protocol Pindu does not
   speak, an `id` outside lowercase letters, digits and `-`, a `runs` that is neither `sub-sandbox` nor `host`, a
   `program` whose first entry is not a file inside the folder, or `file`, `http` or `https` in `uri_schemes`.
5. **Confirmation.** Before installing, Pindu shows the helper's name and version, where it runs (with "Runs with your
   permissions" for a host helper, and for every helper natively), the folders it reads and its URI schemes. The user
   confirms or cancels.
6. **One rename.** The checked folder is moved to `importers/<helper ID>/` in one rename, as
   `proton_downloads::finish_install` does. The work folder is deleted however the install ends.
7. **Off.** An installed helper is off until the user turns it on.
8. **Replacing.** An archive whose `id` is already installed asks to replace it. The games keep their IDs. The helper
   is turned off again unless `runs`, `reads` and `uri_schemes` are unchanged.
9. **Removing.** Each installed helper has a Remove action that deletes its folder and its `external_importers` row.
   Its games get `missing_since`, as the games of a source that is no longer on the machine do, and keep their data.
10. **Bundled helpers.** A bundled helper cannot be removed, and an archive with its `id` is refused.

## The protocol

Pindu runs the helper once per request, with the request as one JSON document on standard input, and reads one JSON
document from standard output. Standard error goes to Pindu's log. The helper exits 0 when it answered, non-zero when
it could not, in which case the source shows a problem and its games stay as they were, as a built-in source that
cannot be read does (`sync_source`). A helper that runs longer than 30 seconds is killed (through `flatpak-spawn`,
`timeout` kills it where it runs, since killing `flatpak-spawn` alone would leave it running); an answer over 32 MiB
is refused. Both are problems of the source, not failures of Pindu.

### Probe

```json
{"protocol": 0, "request": "probe", "candidates": ["/home/user/.local/share/example-launcher"]}
```

```json
{"protocol": 0, "found": true, "root": "/home/user/.local/share/example-launcher",
 "watch": ["/home/user/.local/share/example-launcher/library.json"]}
```

`found: false` is a source that is not on this machine (an empty source, never an error); Pindu then watches the
candidates, where it would appear. `root` must be one of the candidates or below one. `watch` lists files or folders
below `root` and replaces `key_paths` for the helper; a path outside `root` is ignored.

### Import

```json
{"protocol": 0, "request": "import", "root": "/home/user/.local/share/example-launcher", "utc_offset": 3600}
```

The answer is the `Snapshot` written down:

```json
{
  "protocol": 0,
  "games": [
    {
      "native_id": "1234",
      "title": "Example Game",
      "installed": true,
      "launch": {"kind": "uri", "uri": "example://run/1234"},
      "platform": "linux",
      "playtime_secs": 3600,
      "last_played_at": 1763500000,
      "artwork": {"cover": "/home/user/.local/share/example-launcher/covers/1234.jpg"}
    }
  ],
  "problems": ["Entry 17 in library.json has no title"]
}
```

| Field | Type | Maps to `ImportedGame` | Rule |
| --- | --- | --- | --- |
| `native_id` | string, required | `native_id` | Non-empty, at most 256 bytes, unique in the answer |
| `title` | string, required | `title` | Non-empty |
| `installed` | bool, required | `installed` | |
| `launch` | object, required | `launch_kind`, `launch_uri`, `launch_config` | See [Launches](#launches) |
| `developer`, `description` | string | `developer`, `description` | |
| `platform` | `"windows"`, `"linux"` | `platform` | Anything else is unknown, so the `platform` `CHECK` holds |
| `install_path` | string | `install_path` | Absolute |
| `size_bytes`, `playtime_secs` | integer | `size_bytes`, `playtime_secs` | Not negative |
| `last_played_at` | integer | `last_played_at` | Unix seconds, UTC; the helper converts local times with `utc_offset` |
| `genres` | list of strings | `genres` | Only names in Pindu's category set count, as for the built-in sources; others are ignored |
| `artwork` | object: `cover`, `hero`, `logo`, `icon`, `square` | `artwork` | Absolute paths Pindu can read; others dropped with a problem |
| `flags` | object: `hidden`, `favourite`, `categories` | `source_flags` | Applied once, when the game first enters the library; `categories` names the launcher's own categories that hold the game, which become collections, not the game's categories in `genres` |
| `service` | object: `service`, `service_id` | `service` | See [Grouping](#grouping) |
| `runner`, `year` | string, integer | `runner`, `year` | |

Pindu validates every entry. One that breaks a rule is left out with a problem naming it, and the rest import, as a
built-in importer skips an entry it cannot read. Everything a helper returns is data: no field is run, opened or
shown as markup.

### Launches

Only the kinds that already exist, never a shell string:

| `kind` | Fields | Becomes | Allowed from |
| --- | --- | --- | --- |
| `uri` | `uri` | `store-uri` with `launch_uri`, opened through the OpenURI portal | Every tier, for the schemes in `uri_schemes`; never `file`, `http` or `https` |
| `flatpak` | `app_id`, `args` (list), `command` (optional) | `host-command` running `flatpak run [--command=<command>] <app_id> <args…>`, as Faugus's Flatpak launch does | Every tier; needs host spawn in the Flatpak, like any host launch |
| `host` | `argv` (list, first entry absolute), `working_dir`, `env` | `host-command` with a `LaunchConfig` | `host` helpers, and every helper natively |

Proton and Steam Linux Runtime launches inside Pindu's sandbox (`proton`, `runtime-native`) are not offered: they would
let a helper point Pindu's own runner at any program in a folder Pindu can see.

### Versioning

- `protocol` is one integer, in the manifest, every request and every answer. Pindu speaks a set of versions and
  refuses a helper whose version it does not speak, saying which it needs. Within a version, fields are only added;
  unknown fields are ignored on both sides. Removing or changing a field is a new version.
- **No stability is promised until two integrations from outside exist** (the analysis's rollout). Until then the
  version is `0`, the documentation says it can change in any release, and Preferences marks helpers as experimental.
  The first stable version is `1`.

## Game IDs and the `Source` set

`Source` is a closed set and the prefix of every game ID (`model.rs`), which is the contract's main structural cost.

- **Outside helpers share one variant**, `Source::External` (`external`), with native IDs `<helper ID>/<native ID>`:
  `external:example-launcher/1234`. A helper ID is the manifest's `id`, lowercase letters, digits and `-`, so the ID
  splits unambiguously. Renaming a helper's `id` makes new games.
- **Per-helper state** (turned on, the probe's root and paths to watch, last sync, problems) does not fit
  `source_states`, keyed by source: it has a small table of its own, `external_importers`, added by migration `0013`.
  `games.source` has no `CHECK`, so the variant needs no migration. When a helper's folder is gone, its games get
  `missing_since`, as the games of a source no longer on the machine do, and its state is forgotten.
- **A built-in importer moved behind the protocol keeps its own variant.** The protocol is the transport; the source
  is assigned by Pindu, so `bottles:` IDs, play history and groups stay as they are.
- `DEFAULT_SOURCE_ORDER` (`game_groups.rs`) places `External` after `Manual`, since Pindu knows least about it.
- **Backups:** an `external:` ID is new text in the same format; a build without the variant cannot parse it and leaves
  the game out of the import.

### Grouping

[Game identity](game-identity.md) applies to helpers' games unchanged: one tile per group, the Copies section,
title suggestions between sources. One difference: a Lutris store link is grouped automatically (spike #501) because
Lutris is a built-in source Pindu reads itself; a helper's `service` is only a **high-confidence suggestion**, never an
automatic group, since a helper's claim is not checked. The `service` names it matches are Lutris's: `steam` and
`steamwindows` for a Steam game, `gog` for a Heroic GOG game and `egs` for a Heroic Epic game.

## Rollout

1. **Write the contract down** as this page, with protocol `0`.
2. **Move Bottles behind it** as a helper built in the workspace (`pindu-import-bottles`), shipped in every package,
   run in the sub-sandbox in the Flatpak, and keeping `Source::Bottles`. Bottles is the lowest-risk built-in: it reads
   one file (`library.yml`, plus `data.yml` for a custom path), launches only through `bottles:run/…` URIs, so a mistake
   cannot start a command on the host, and has few users. The built-in importer's tests (`tests/import/bottles.rs`)
   run against the helper unchanged, which proves the snapshot written down is the snapshot.
3. **Promise stability (protocol `1`)** once two integrations from outside exist and work unchanged across a release.
4. Only then consider moving more built-ins, starting with those Pindu's users use least.

## Flatpak, native and Pindu OS

- **No `finish-args` change.** The sub-sandbox goes through the Flatpak portal (`org.freedesktop.portal.Flatpak`), which
  every Flatpak app may talk to. A helper reads only folders Pindu's sandbox already sees, so a launcher outside the
  manifest's list is read from a folder the user chooses, as standalone Legendary and itch are
  (`location::sandbox_hides`).
- **Host helpers** need the host-spawn override, as host launches do; without it they show as needing host access and
  do not run.
- **Native packages and Pindu OS** have no sandbox to offer: every helper runs as a child process with the user's
  rights, and the Sources list says so. A sub-sandbox natively would need `bwrap`, a new requirement this page does not
  ask for.

## Out of scope

- A binary or in-process plugin interface.
- Distributing helpers: a catalogue, downloads, updates or signatures.
- `.zip` packages.
- Installing a helper from fullscreen mode.
- Helpers that write to Pindu's library beyond the snapshot: artwork providers, metadata, play sessions, collections.
- Helpers starting games inside Pindu's sandbox (`proton`, `runtime-native`).

## Issues

Filed in M9 Breadth after the decision, each labelled `hold` for the maintainer's CLI session:

| Issue | Covers | Depends on |
| --- | --- | --- |
| #831 XP-2 Run external importers through the protocol | Manifest discovery, probe and import over stdin and stdout, validation, the timeout and size limits, the sub-sandbox and host runners, the no-escalation rule, `Source::External`, the `external_importers` table | #547 |
| #832 XP-3 List external importers in both shells | Preferences and fullscreen mode's Sources: off by default, where it runs, what it reads, experimental | XP-2 |
| #833 XP-5 Install, replace and remove an external importer from Preferences | The ten rules under [Installing a helper from Preferences](#installing-a-helper-from-preferences), tested against fixture archives | XP-2, XP-3 |
| #834 XP-4 Move the Bottles importer behind the protocol | `pindu-import-bottles` as a bundled helper, packaging in the RPM, DEB and Flatpak, the existing Bottles tests against the helper | XP-2 |

Order: XP-2, then XP-3 and XP-5. XP-4 can start any time after XP-2. The compatibility policy that 1.0 asks for is
#826 (milestone M10 1.0).

## Recommendation

**Adopt the contract at protocol `0`, sub-sandboxed by default, and prove it on Bottles before anything else.**

- The core already has the shape: a snapshot, a probe and paths to watch. Writing it down costs one `Source` variant,
  one table and no Flatpak permission.
- The sub-sandbox makes "safe" true in the Flatpak, and the no-escalation rule keeps a helper from widening its own
  rights through Pindu. Natively, saying plainly that a helper runs with the user's rights is the honest version.
- It is also the cheaper way into [emulation](emulation.md): a ROM-folder helper returning `flatpak` and `host`
  launches would exercise the protocol as an outside integration without the core owning emulator support.
- What it costs is a second way to import, which stays small only while built-ins move behind it slowly and the
  protocol promises nothing until two outside helpers exist.

## Decisions

On 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)) the
maintainer left the decision to after this page. **Decided by the maintainer on 2026-11-23** (#547): the contract is
adopted as recommended above.

| Choice | Decision |
| --- | --- |
| Adopt the contract, and where helpers may run | **Yes**, at protocol `0`. Sub-sandboxed by default in the Flatpak. Host helpers are allowed as specified: the host-spawn override, and "Runs with your permissions" |
| No escalation | **Yes.** A sub-sandboxed helper may describe URI and Flatpak launches only |
| One `external` source | **Yes.** `Source::External`, IDs `external:<helper ID>/<native ID>`, and the `external_importers` state table |
| Bottles first, protocol `0` until two outside integrations exist | **Yes** |
| A helper's store link suggests a group, never makes one | **Yes** |

One addition: a helper is installed from Preferences, from a local `.tar.gz`, by the ten rules under
[Installing a helper from Preferences](#installing-a-helper-from-preferences).
