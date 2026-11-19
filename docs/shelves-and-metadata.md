# Shelves and metadata

A large library needs more ways to slice it than source, install state and platform, and the metadata Pindu has is
thin and uneven across sources: Heroic fills developer and, for some stores, genres and a description; Steam fills
developer; and each sync overwrites those columns, so a value from anywhere else cannot live there. This workstream
saves filters as shelves, reads the metadata the launchers already hold, keeps every value with its origin, and only
then considers an online provider. It is workstream **MD** of the
[product analysis](product-analysis.md#85-shelves-and-metadata-md), milestones M6 One library (#524, #525), M7 Launch
confidence (#523, #526, #527, #530) and M8 Resilience (#528, #529).

This page is the workstream's spec as issue #550 asks for it. Where this page and an issue disagree, the issue wins.

## What it does

In this order, local data first:

1. **Saved filters.** The current filter, and search if any, is saved as a dynamic collection whose members are
   computed, not stored. Collections already appear in the desktop sidebar, as fullscreen tabs and as Home sections,
   so a saved filter becomes a shelf there, renamed and deleted like a collection. Useful shelves that need no new
   data (installed and never played, recently added, native, Windows, no longer listed by its source, needs a
   permission) are offered as suggestions the user can save, not hard-coded sections. A genre filter and a
   never-played filter join both shells and can be saved.

   As built (#524): a saved filter is a `collections` row with its view, filter toggles and normalised search in
   `saved_filters` (migration 0007); `pindu_core::saved_filters` computes its members with the library views' own
   matching each time the library loads, so games cannot be added to it by hand and a source's categories never land
   in it. Saving is offered while a toggle or a search narrows a view other than Hidden or another saved filter:
   **Save Filter…** in the desktop filter row and in fullscreen Quick Access's Filters, which name it after what it
   shows. The suggestions offered are **Native Games**, **Windows Games** and, since #525, **Installed and Never
   Played**; the others need a filter that does not exist yet (added recently, no longer listed, needs a permission).

   As built (#525): the filters gain **Never Played** (games with no last played time) and a genre, matched ignoring
   case against the game's resolved genres. The desktop filter row has a **Never Played** toggle and a **Genre** menu
   (Any Genre, then each genre of the library); fullscreen Quick Access's Filters has a **Never Played** switch and a
   **Genre** list with a check on the one chosen, reached with the D-pad like every row there. `pindu_core::library`
   matches them (`Filters::never_played`, `Filters::genre`) and lists the genres to offer (`genres`); saved filters
   keep both in `saved_filters` (migration 0010), and backups carry them as `never_played` and `genre` beside the other
   toggles.

   Backups (#600) carry each saved filter as a collection entry with no games and a `filter` beside it: its view,
   toggles and search, with a view of a collection naming that collection, since IDs differ between libraries.
   Importing makes it a saved filter again, first making the plain collection it views if the library lacks one. A
   name the library already has keeps what it is here: a plain collection does not become a saved filter, and a saved
   filter gains no stored games. A filter whose view this version does not know imports as an empty plain collection.
   This extends backup format 2 (#509), unreleased until then, so format 1 and 2 backups still import.
2. **Metadata the launchers already hold.** A spike lists, per source, the fields available offline and how reliable
   they are in real libraries: release year, genres, controller support and Steam Deck compatibility from Steam's
   binary `appinfo.vdf`, which the Steam importer already parses, and `year` from Lutris's `pga.db`. The fields it
   finds reliable are imported offline, with fixture tests.
3. **Metadata with its origin.** A table holds values by field with their origin (source, provider, user) and time.
   Reading prefers the user's value, then a provider's, then the source's; a sync changes source values only. The user
   can edit developer, genres, description and release year from the desktop game page (and from fullscreen mode if
   #520 says a console needs it), and that value survives syncs and refreshes.

   As built (#526): `game_metadata` (migration 0008) holds one row per game, field (`developer`, `genres`,
   `description`, and since #534 `deck-compatibility`, Steam's Steam Deck rating) and origin (`source`, `provider`,
   `user`) with the value and when that origin last changed it. The migration moved the `games` columns of the first
   three names into source rows and dropped the columns, so nothing reads a
   stale copy. `pindu_core::metadata` resolves the rows (`shown` picks one field's winning value with its origin and
   time, `resolve` all of them), and the store hands every `Game` out with resolved `developer`, `genres` and
   `description`, so both shells and the filters show the winning value without knowing about origins. Writes
   go through `Store::set_metadata(game, origin, metadata, now)`, which replaces one origin's values, keeps the time
   of a value that did not change and drops a field left empty. A sync calls it for the source only; `insert_game`
   records a new game's values as its source's; `update_game` never touches metadata, so saving a game read with a
   user's value cannot turn it into the source's. Release year stays in the source details for now; #527 decides
   whether it joins this table.

   Backups (#619) carry the user's and providers' values in a `metadata` list, each with the game's per-source ID, its
   field, origin, value (genres as a list, a Steam Deck rating as its object) and time; a source's values are left out,
   as the next sync writes them. A
   restore adds a value only where that origin has none for the field in this library, keeps the time it was set and
   counts it, so it never overwrites a value already there; a value for a game the library lacks is skipped and the
   game counted as missing. This extends backup format 2 (#509), unreleased until then, so format 1 and 2 backups
   still import.
4. **An online provider, opt-in, after a spike.** The spike compares SteamGridDB, IGDB, PCGamingWiki, Wikidata and
   others on licence and terms, whether a key is needed, which store IDs it maps (which helps
   [game identity](game-identity.md)), the fields it has and its rate limits, and recommends one or none. If one is
   chosen it is off by default, a refresh is a cancellable task in Activity, nothing is fetched during an import, each
   value records its provider and time, any key lives in the keyring, and it uses the HTTP stack the app already uses
   for SteamGridDB.

## Flatpak, native and Pindu OS

Saved filters and the offline metadata need no permission: the importers read the launchers' files read-only in their
native and Flatpak layouts, as they do today. An online provider uses the network access the app already has for
SteamGridDB, and its key, if it needs one, is stored in the keyring (the Secret portal inside the Flatpak). On a
console a key is typed with fullscreen mode's on-screen keyboard and needs a working keyring, as the SteamGridDB key
does ([controller parity](controller-parity.md)). Shelves show as fullscreen tabs and Home sections, so they work with
a controller alone.

## Out of scope

- Ratings, prices and store pages as content: a link out is enough.
- Completion-time data until a provider decision exists.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #523 Find what metadata the launchers' own files hold | Spike: fields available offline per source, and which to import | |
| #524 Save a filter as a dynamic collection | Saved filters in the sidebar, fullscreen tabs and Home; suggested shelves | #504, for migration order |
| #525 Filter the library by genre and by never played | Both filters in both shells, savable | #524 |
| #526 Keep metadata apart from source fields, with its origin | The metadata table and the user, provider, source order | #524, for migration order |
| #527 Import release year, genres and controller support from Steam and Lutris | The fields #523 found reliable, offline | #523, #526 |
| #528 Choose an online metadata provider | Spike: candidates compared, one or none recommended | |
| #529 Fetch metadata on request and cache it | The recommended provider, off by default, as a task in Activity | #526, #528 |
| #530 Edit a game's details by hand | User values that win over syncs and refreshes | #526 |

#524 and #526 each add a migration, after #504's and in that order, so the chain stays linear.

## Decisions

Decided by the maintainer on 2026-11-19 (analysis [section 14](product-analysis.md#14-decisions-to-take-before-issues-are-filed)):

- Local data first; whether there is an online provider, and which, is settled by #528 (MD-6)'s write-up, whose
  recommendation is adopted.
