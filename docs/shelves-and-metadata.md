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

   As built (#527), following #523's write-up: Steam's `appinfo.vdf` gives the release year (the UTC year of
   `original_release_date`, else `steam_release_date`; 0 means unknown), genres (the `genres` IDs named from a fixed
   table: 1 Action, 2 Strategy, 3 RPG, 4 Casual, 9 Racing, 18 Sports, 23 Indie, 25 Adventure, 28 Simulation, 29
   Massively Multiplayer; Free To Play, Early Access and unknown IDs are dropped) and controller support (`full` or
   `partial`; absent means not declared, not none). Lutris gives `year` and Heroic's Amazon library the year of
   `extra.releaseDate`. All of them become source values in `game_metadata` (fields `release-year` and
   `controller-support` join the three below, with no migration), and both game pages show them in the About group,
   Steam's genre names translated. The year also stays in the source details, where grouping copies compares it.
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
   user's value cannot turn it into the source's. Release year and controller support joined the table with #527.

   Backups (#619) carry the user's and providers' values in a `metadata` list, each with the game's per-source ID, its
   field, origin, value (genres as a list, a release year as a number, a Steam Deck rating as its object) and time; a
   source's values are left out, as the next sync writes them. A restore adds a value only where that origin has none
   for the field in this library, keeps the time it was set and counts it, so it never overwrites a value already
   there; a value for a game the library lacks is skipped and the game counted as missing. This extends backup format
   2 (#509), unreleased until then, so format 1 and 2 backups still import.

   As built (#530): the desktop **Edit…** dialog (a game page's ⋮ menu) has a Details group with Developer, Genres
   (separated by commas) and Description, filled with what the game shows; as #520 decided, fullscreen mode has no
   edit. Saving goes through `pindu_core::metadata::edit_by_user(store, game, shown, edited, now)`, which writes user
   values for every copy of the game's group: a field the user left as shown keeps each copy's own user value, a
   field cleared drops the user's value so the game falls back to a provider's and then the source's, and a value the
   game would fall back to anyway is not kept as the user's, so later syncs of it still show.

   As built (#641): the Details group also has a Release Year row, filled with the year the game shows and edited
   the same way; clearing it drops the user's year so the game falls back to a provider's and then the source's.
   `Metadata::typed_year` reads the row: four digits not starting with 0, trimmed, or nothing. Other text marks the
   row as an error (and invalid for assistive technologies) and keeps **Save** insensitive, so a typo is corrected
   rather than saved as a cleared year.
4. **An online provider, opt-in, after a spike.** The spike compares SteamGridDB, IGDB, PCGamingWiki, Wikidata and
   others on licence and terms, whether a key is needed, which store IDs it maps (which helps
   [game identity](game-identity.md)), the fields it has and its rate limits, and recommends one or none. If one is
   chosen it is off by default, a refresh is a cancellable task in Activity, nothing is fetched during an import, each
   value records its provider and time, any key lives in the keyring, and it uses the HTTP stack the app already uses
   for SteamGridDB.

   As built (#529, after spike #528 chose Wikidata): **Get Metadata From Wikidata** (GSettings `wikidata-metadata`,
   off by default) in Preferences, General, and in fullscreen Quick Access's settings shows **Refresh Metadata** in
   the desktop main menu and in Quick Access. Only that action fetches; a sync never does. `pindu_core::wikidata`
   looks games up by Steam app ID only, never by title: a Steam game by its own, another by the Steam ID its source
   links it to (Lutris's `steam` service) or by a grouped Steam copy's. Hidden games and games no longer in the
   library are left out. The IDs go out in SPARQL queries of up to 100 (`VALUES ?steam { … }`, Wikidata's `P1733`),
   one after another, to `query.wikidata.org` through the app's soup3 stack, with a User-Agent naming the project's
   address and the SteamGridDB client's 429 back-off (`steamgriddb::retry_delay`, following `Retry-After`). Each
   answer gives developers (`P178`) and genres (`P136`), labelled in the UI's languages, then `en`, then `mul`; a
   value with no label in those is left out. Developers become one value, joined with commas, genres a sorted list,
   both stored with origin `provider` through `Store::set_metadata`, timed when they were fetched (a refresh that
   finds the same value keeps its earlier time), so a user's value still wins and a sync leaves them alone. The
   origin `provider` means Wikidata, the only provider. A game Wikidata has no item for keeps what it had. The run
   is a task in Activity (and a toast with Cancel on the desktop); each batch is stored as it arrives, so a
   cancelled or failed run keeps what it got, and the outcome is a toast and a notification.

   Not done in #529: the item's QID and its cross-IDs (PCGamingWiki, IGDB, GOG, Epic, Lutris) and a link out per
   value would need a new table, and only #504, #524, #526 and #531 add migrations, so they wait for a later issue.
   Release year, publisher and series have no `game_metadata` field yet, and Lutris slugs are not kept, so Lutris
   games without a Steam link are not looked up.

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
