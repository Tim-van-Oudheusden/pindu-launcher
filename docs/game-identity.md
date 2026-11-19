# Game identity

The same game often arrives from several sources: one title owned in two stores, a Lutris entry that wraps a store
game, a copy added by hand beside a store copy, native and Windows builds of one title. Today each shows as a tile of
its own, and Heroic lists owned games that are not installed, so Steam plus a Heroic store soon shows a game twice.
Game identity groups those copies into one game with a preferred copy, without touching the records the launchers
own. It is workstream **GI** of the [product analysis](product-analysis.md#81-game-identity-and-duplicates-gi),
milestone M6 One library.

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

- **A group beside `games`.** A group holds two or more source records and names a preferred copy. `games` rows and
  their IDs (`<source>:<native id>`) do not change, so play sessions, backups, the GNOME Shell search provider and the
  per-game window IDs in gamescope keep working, and a split restores exactly what was there before. A game in no
  group behaves exactly as today.
- **A view, not an owner.** Favourite, hidden, pin and collection membership stay on the source records; a change on
  the group's tile applies to every member.
- **One tile per group.** Both shells show one tile per group. The game page lists the copies with their source,
  install state and playtime. Play starts the preferred copy, or an installed one when the preferred copy is not
  installed. GNOME Shell search gives one result per group.
- **Conservative, staged matching.**
  1. Exact cross-references, such as a Lutris record's store ID (`service`, `service_id`) against the Steam, Heroic
     GOG or Heroic Epic record with that ID, are grouped automatically, with an undo.
  2. Equal normalised titles (`search::normalize`) of records from different sources are a suggestion only, with its
     reason and a confidence: high when the developer or the year agrees on both records, low otherwise.
  3. Merge and split by hand, from the desktop game menu and from fullscreen mode's options sheet. A split is
     remembered ("not the same game"), so the suggestion does not come back.

  Nothing is merged on title alone without the user.
- **Review.** Both shells list the suggestions with accept and dismiss; a dismissed pair never returns, and automatic
  groups show with an undo.
- **A copy before every migration.** Opening an older database first writes a copy beside it, named for its schema
  version (for example `library.db.v5.bak`); the newest two copies are kept, and a failed copy stops the migration.
- **Backups.** Backup format 2 carries groups, preferred copies and remembered splits, naming games by their
  per-source IDs. Format 1 still imports; a group whose games are absent is skipped and counted.

## How groups are stored and read

Schema migration 6 (#504) adds the grouping layer; the rules follow spike #501.

- **Stored:** each group's games (a game is in at most one group), the preferred copy only when the user chose one,
  whether the group was made automatically, and the remembered "not the same game" pairs. Beside them, every sync
  stores what a source says beyond the game's own fields: Lutris's runner, the store record it links the game to
  (`service`, `service_id`) and the release year.
- **Untouched by a sync.** A sync never changes a group, a preferred copy or a remembered pair. A copy gone from its
  source stays in its group and keeps its playtime; only deleting a game takes it out, and a group left with one game
  ends.
- **Ungrouping** removes only the group, so the games are exactly as they were. Splitting a group, by hand or as the
  undo of an automatic group, also remembers each pair, so neither the automatic grouping nor a suggestion brings it
  back. An automatic group is never made for a game already in a group or for a remembered pair.
- **Default preferred copy** (computed, never stored): installed before not installed, then Linux, unknown, Windows,
  then the source order (default Steam, Heroic GOG, Heroic Epic, Heroic Amazon, Lutris, Faugus, Bottles, Heroic
  sideload, manual, desktop), then most recently played, then game ID. Play starts the preferred copy, or the first
  installed copy in that order when the user's choice is not installed.
- **Playtime** (computed, never stored): the Steam copy and every Lutris copy whose runner is `steam` were timed
  together, so they count once, by the largest figure among them; every other copy was played apart and adds up. Steam
  10 h with Lutris (runner `steam`) 2 h is 10 h; Steam 10 h with Heroic GOG 5 h is 15 h. A group was last played when
  any copy was.

## How a group shows

- **One tile, one list row, one search result.** Both shells, the desktop list layout and GNOME Shell search show a
  group once: under its preferred copy's title and art (a kind of art the preferred copy lacks comes from the next
  copy that has it), standing for the copy Play starts. A `--launch` or search result naming any copy starts the
  group's Play copy.
- **The game page lists the copies**, the preferred one first, each with its source, whether it is installed and its
  own playtime with where that comes from; the copy Play starts says so.
- **Changes on a group reach every copy.** Favourite, hide, remove and collection changes made on a group's tile or
  page are written to each copy. A group is a favourite, or in a collection, when any copy is; it is hidden or removed
  only when every copy is. A "Jump back in" place holds one game, so pinning a group pins the copy Play starts, and
  unpinning it unpins every copy.
- **Playtime and last played** are the group's (above): sorting by playtime or last played, Play history, "Jump back
  in" and the game page use them. When two or more copies played apart have playtime, the total reads "across N
  copies"; otherwise it names where the one figure comes from.
- A group sits in the source view of the copy Play starts.

## Merging and splitting by hand

- **Merge With…** sits in a game's menus on the desktop (the tile's and the page's) and in its options sheet in
  fullscreen mode. The desktop asks with a search over the other games; fullscreen mode lists them in the sheet, which
  a controller moves through. Both list games with the same normalised title first, as the likely copies, then the
  rest by title (`game_groups::merge_candidates`). Selection mode's **Merge** merges two or more selected games.
- **Merging** (`Store::merge_games`) takes every copy of the groups involved: the first game's group stays, with its
  preferred copy, the others end, and the group becomes the user's own. A remembered split does not stop the user.
- **Preferred Copy** chooses the copy a group shows and Play starts: a submenu in the desktop menus, a list in the
  options sheet, and the radio buttons of the game page's Copies section in both shells.
- **Split Into Separate Games** is `Store::split_group`: the games show on their own again and every pair is
  remembered as not the same game, so neither the automatic grouping nor a suggestion brings them back.

## How duplicates are found

`duplicates::suggestions` (#505) lists the pairs of library records that look like copies of one game, each with a
reason and a confidence, and writes nothing. It follows spike #501:

- **Exact cross-references come first.** Only a Lutris record's store link counts, compared after trimming
  whitespace, against a record in the library (installed or not): `steam` and `steamwindows` name `steam:<id>`, `gog`
  names `heroic-gog:<id>`, `egs` names `heroic-epic:<id>`. Amazon does not count, because Lutris stores the
  entitlement ID and Heroic the product ID; other services have no Pindu source to match.
- **Then equal normalised titles**, only between records of different sources, never on a prefix or substring. The
  confidence is high when both records name a developer and the names agree (ignoring `inc`, `ltd`, `llc`, `gmbh`,
  `corp`, `co`) or both know the year and the years are at most one apart; otherwise it is low. A disagreement only
  lowers the confidence.
- **Left out:** records gone from their source or removed by the user, pairs already in one group, and pairs
  remembered as not the same game, so a dismissed or split pair never returns.

After every sync, `duplicates::group_exact_matches` groups each exact pair automatically, unless either copy is in a
group already; the sync reports the new groups. Undoing one is `Store::split_group`, which remembers the pair, so the
next sync leaves it apart. Title matches are never grouped without the user.

## How groups are backed up

Backup format 2 (#509) adds two lists to the backup file, both naming games by their per-source IDs:
`groups`, each with its games, the preferred copy when the user chose one and whether it was made automatically, and
`not_the_same`, the remembered pairs. Importing adds and never removes:

- **A group is made again** from those of its games that are in the library, when at least two are and none of them
  is in a group here already. A preferred copy that is not in the library falls back to the default order. A group
  this library already has is left as it is and not counted.
- **Skipped and counted** is every other group: fewer than two of its games are in the library, one of them is in
  another group here, or an automatic group's pair is remembered here as not the same game. The import result says
  how many groups were skipped, beside how many games the library lacks.
- **Remembered pairs** are added when both games are in the library.
- **Format 1** backups, which have neither list, still import and change no group. An older build refuses a format 2
  backup rather than drop its groups. Format 2 also carries each saved filter's filter (#600; see
  `docs/shelves-and-metadata.md`).

## Flatpak, native and Pindu OS

The same everywhere: grouping needs no permission and no network. The Lutris fields are read from `pga.db` in its
native and Flatpak layouts, read-only, like the rest of the Lutris importer. Merging, splitting and reviewing
suggestions work in fullscreen mode with a controller alone, because on Pindu OS fullscreen mode is the whole
interface.

## Out of scope

- Changing any launcher's data: a group is Pindu's own note about records it does not own.
- Online lookups to find matches. A provider that maps store IDs ([shelves and metadata](shelves-and-metadata.md),
  #528, #529) can add a matching stage later.
- Regional and revision variants of emulated games (#546).

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #501 Measure duplicates in real libraries and settle the grouping model | Spike: duplicates by class in real libraries, the exact cross-references, the playtime rule | This page |
| #502 Keep a copy of the library before a schema migration | `library.db.v<N>.bak` beside the database, newest two kept | |
| #503 Read Lutris service IDs, runners and years | `service`, `service_id`, `runner` and `year` from `pga.db` | |
| #504 Add game groups to the library store | Migration 0006, the store functions, the default preferred copy, the playtime rule | #501, #502, #548 |
| #505 Suggest duplicate games | Core suggestions with reason and confidence; writes nothing | #503, #504 |
| #506 Show a group as one game with a preferred copy | One tile per group in both shells, the copies on the game page, Play, search | #504 |
| #507 Merge and split games by hand | Desktop menu and fullscreen options sheet; remembered splits; choosing the preferred copy | #506 |
| #508 Review duplicate suggestions | Accept and dismiss in both shells; undo for automatic groups | #505, #506 |
| #509 Carry game groups in backups | Backup format 2; format 1 still imports | #504 |

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- A group is a **view** over its source records: favourite, hidden, pin and collection membership stay on the source
  records, and a change made on the group's tile applies to every member. Old backups stay valid.
- Exact cross-references (for example a Lutris record's store ID matching a Steam, Heroic GOG or Heroic Epic record)
  are **grouped automatically, with an undo**.
- Playtime across copies: settled by #501's findings, recorded on #501. #504 and #506 implement the rule it states.
- Default preferred copy: installed before not installed, then native before Windows, then a source order the user
  can change.
- Database migrations and the backup format are high risk (#548).
