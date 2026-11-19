# Library scale: 10,000 games

How fast the core does the work behind the library views on a library of 10,000 games with artwork paths: loading it
from the store, sorting, filtering and searching. Measured with the `library_bench` example of `pindu-core` (#552); it
times the same calls the shells' list models make, without GTK, so it measures the core and not the widgets.

## How to run it

In the GNOME 51 SDK, from the repository root:

```sh
cargo run --release -p pindu-core --example library_bench [GAMES] [RUNS]
```

`GAMES` defaults to 10000 and `RUNS` to 15. The example fills a fresh store in a temporary directory (`$TMPDIR`) with a
generated library, the same on every run, then prints each step's minimum, median and maximum in milliseconds as a
Markdown table, and how many games each filter and search shows. Paste the table below when you re-measure, with the
machine and date.

The generated library has:

- 10,000 games spread over every source, with two- or three-word titles from a fixed word list, some with accents
  (`Pokémon`, `Ōkami`, `Æther`), a leading "The" or a sequel number;
- 55,024 artwork rows: each of the five layouts from the source, SteamGridDB, both or neither, as the art cache stores
  them (`library::load` picks the best per layout);
- a played date on 40 % of the games, with imported playtime for Steam, Epic and GOG games and one to three play
  sessions for the rest; 25 % installed, 5 % favourites, 2 % hidden;
- 20 collections holding 15 % of the games.

What each step times:

| Step | What it does |
| --- | --- |
| Load: open the store, `library::load` | Opens the SQLite store and reads every game, its art, playtime and collections into `LibraryEntry` values: the library service's load. |
| Load: sort and search keys | Builds a `SortKey` and `SearchKey` for every entry, as the list model's `GameObject`s do. |
| Sort | Sorts all 10,000 games with `library::compare` for each `SortMode`. |
| Filter | One `LibraryFilter::matches` pass over every game for a view or toggle set: a full refilter. |
| Search | Types a query one key at a time, each keystroke a full `LibraryFilter::matches` pass, so the time is for the whole query. |

## Results

Measured 2026-11-19 on an AMD Ryzen 7 7800X3D (8 cores, 16 threads), 32 GB RAM, NVMe SSD with btrfs, Fedora on Linux
7.3.4, in the GNOME 51 SDK with rustc 1.98.0, release build. Other builds were running on the machine at the time (load
average about 3), so treat the numbers as an upper bound for this machine.

Library: 10,000 games, 55,024 artwork rows, 20 collections; 15 runs per step. Generating and writing it took 552 ms.

| Step | Detail | Min (ms) | Median (ms) | Max (ms) |
| --- | --- | ---: | ---: | ---: |
| Load | open the store, `library::load` | 77.72 | 80.94 | 88.39 |
| Load | sort and search keys for every game | 4.48 | 4.54 | 6.57 |
| Sort | LastPlayed | 0.71 | 0.72 | 0.75 |
| Sort | NameAscending | 0.85 | 0.86 | 0.93 |
| Sort | NameDescending | 0.84 | 0.86 | 1.05 |
| Sort | RecentlyAdded | 0.37 | 0.38 | 0.44 |
| Sort | Playtime | 0.70 | 0.71 | 0.78 |
| Filter | view All | 0.05 | 0.05 | 0.12 |
| Filter | view Installed | 0.04 | 0.04 | 0.06 |
| Filter | view Favourites | 0.03 | 0.03 | 0.04 |
| Filter | view Play history | 0.04 | 0.04 | 0.07 |
| Filter | view Steam | 0.15 | 0.15 | 0.26 |
| Filter | view a collection | 0.03 | 0.03 | 0.05 |
| Filter | view Hidden | 0.02 | 0.02 | 0.03 |
| Filter | All, Installed + Windows toggles | 0.04 | 0.04 | 0.09 |
| Search | type "dragon", 6 keystrokes | 2.37 | 2.39 | 2.55 |
| Search | type "pokemon 3", 9 keystrokes | 3.73 | 3.77 | 4.02 |
| Search | type "the aether", 10 keystrokes | 3.76 | 3.79 | 3.83 |
| Search | type "okami", 5 keystrokes | 1.85 | 1.87 | 1.90 |
| Search | type "zzz", 3 keystrokes | 1.20 | 1.21 | 1.36 |

Games shown: view All 9,796; Installed 2,423; Favourites 481; Play history 3,916; Steam 963; a collection 60; Hidden
204; Installed + Windows toggles 1,422; search "dragon" 725, "pokemon 3" 69, "the aether" 746, "okami" 695, "zzz" 0.

## Reading the numbers

- Loading is the one step that costs noticeable time, about 80 ms, and the library service runs it off the main
  thread. Building the keys adds under 5 ms.
- A sort of the whole library takes under 1 ms and a full refilter well under 1 ms, so changing the sort order, the
  view or a toggle stays far inside one 16 ms frame on the core's side.
- A search keystroke costs about 0.4 ms, so search can refilter on every key without a delay.
- What the numbers leave out: GTK's `SortListModel` and `FilterListModel` overhead, widget creation for the visible
  tiles, and decoding the cover images, which the art cache and the grid's recycling handle.
