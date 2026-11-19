//! Times the library's load, sort, filter and search at 10,000 games with artwork paths:
//! the work the shells' list models do on a big library (#552).
//!
//! It fills a fresh store in a temporary directory with a generated library, then runs
//! each step several times and prints the minimum, median and maximum in milliseconds as
//! a Markdown table. The library is the same on every run, so runs on one machine compare.
//!
//! ```sh
//! cargo run --release -p pindu-core --example library_bench [GAMES] [RUNS]
//! ```
//!
//! `GAMES` defaults to 10000 and `RUNS` to 15. `docs/metrics/library-scale.md` records the
//! results.

use std::error::Error;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use pindu_core::game_groups;
use pindu_core::library::{
    self, Filters, LibraryEntry, LibraryFilter, SearchKey, SortKey, SortMode, View,
};
use pindu_core::model::{
    ArtLayout, ArtOrigin, Artwork, Game, GameId, ImportedPlaytime, LaunchKind, Platform, Source,
};
use pindu_core::store::Store;

/// Collections in the generated library.
const COLLECTIONS: u64 = 20;

/// Words the generated titles are made of, accents and a leading article included so
/// search normalising and name sorting have real work to do.
const WORDS: [&str; 32] = [
    "Shadow",
    "Legend",
    "Kingdom",
    "Pokémon",
    "Starfield",
    "Rogue",
    "Dungeon",
    "Crystal",
    "Dragon",
    "Ōkami",
    "Tactics",
    "Odyssey",
    "Frontier",
    "Neon",
    "Echoes",
    "Hollow",
    "Citadel",
    "Velocity",
    "Arcadia",
    "Wanderer",
    "Rebellion",
    "Æther",
    "Harbour",
    "Lantern",
    "Monolith",
    "Nocturne",
    "Paradox",
    "Quarry",
    "Riftbreaker",
    "Solstice",
    "Tempest",
    "Zenith",
];

/// A small deterministic generator (SplitMix64), so every run builds the same library.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Whether an event of probability `percent` in 100 happens.
    fn chance(&mut self, percent: u64) -> bool {
        self.next() % 100 < percent
    }

    fn below(&mut self, bound: u64) -> u64 {
        self.next() % bound
    }
}

fn index(value: u64) -> usize {
    usize::try_from(value).expect("index fits in usize")
}

fn timestamp(value: u64) -> i64 {
    i64::try_from(value).expect("timestamp fits in i64")
}

/// Generated game `n`: a title of two or three words, a source, flags and dates.
fn game(rng: &mut Rng, n: u64) -> Game {
    let source = Source::ALL[index(rng.below(Source::ALL.len() as u64))];
    let words = 2 + rng.below(2);
    let mut title = if rng.chance(10) {
        "The ".to_owned()
    } else {
        String::new()
    };
    for word in 0..words {
        if word > 0 {
            title.push(' ');
        }
        title.push_str(WORDS[index(rng.below(WORDS.len() as u64))]);
    }
    if rng.chance(30) {
        title.push_str(&format!(" {}", 2 + rng.below(4)));
    }
    let launch_kind = match source {
        Source::Manual => LaunchKind::Proton,
        Source::Desktop | Source::Bottles => LaunchKind::DesktopEntry,
        _ => LaunchKind::StoreUri,
    };
    // Spread over the past five years, ending 2026-11-01.
    const NOW: u64 = 1_793_491_200;
    let mut game = Game::new(
        GameId::new(source, &n.to_string()),
        title,
        launch_kind,
        timestamp(NOW - rng.below(5 * 365 * 86_400)),
    );
    game.platform = Some(if rng.chance(60) {
        Platform::Windows
    } else {
        Platform::Linux
    });
    game.installed = rng.chance(25);
    if game.installed {
        game.install_path = Some(PathBuf::from(format!("/home/user/Games/game-{n}")));
        game.size_bytes = Some(timestamp(rng.below(100_000_000_000)));
    }
    if rng.chance(40) {
        game.last_played_at = Some(timestamp(NOW - rng.below(2 * 365 * 86_400)));
    }
    game.favourite = rng.chance(5);
    game.hidden = rng.chance(2);
    game.genres = vec![WORDS[index(rng.below(WORDS.len() as u64))].to_owned()];
    game
}

/// The artwork rows of a game: source covers, heroes, logos and icons for most games,
/// SteamGridDB copies for some, as the art cache stores them.
fn artwork(rng: &mut Rng, art_dir: &Path, game: &Game) -> Vec<Artwork> {
    let mut rows = Vec::new();
    let file = game.id.as_str().replace(':', "-");
    for &layout in ArtLayout::ALL {
        let origins: &[ArtOrigin] = match rng.below(10) {
            0 => &[],
            1..=6 => &[ArtOrigin::Source],
            7 | 8 => &[ArtOrigin::Source, ArtOrigin::SteamGridDb],
            _ => &[ArtOrigin::SteamGridDb],
        };
        for &origin in origins {
            rows.push(Artwork {
                game_id: game.id.clone(),
                layout,
                origin,
                path: art_dir.join(format!("{file}-{layout}-{origin}.png")),
            });
        }
    }
    rows
}

/// Fills `store` with `games` generated games, their artwork, playtime and collections.
/// Returns the number of artwork rows.
fn populate(store: &Store, art_dir: &Path, games: u64) -> Result<usize, Box<dyn Error>> {
    let mut rng = Rng(552);
    let mut art_rows = 0;
    store.in_transaction(|store| {
        let collections = (0..COLLECTIONS)
            .map(|n| store.create_collection(&format!("Collection {n}")))
            .collect::<Result<Vec<_>, _>>()?;
        for n in 0..games {
            let game = game(&mut rng, n);
            store.insert_game(&game)?;
            for art in artwork(&mut rng, art_dir, &game) {
                store.set_artwork(&art)?;
                art_rows += 1;
            }
            if let Some(last_played) = game.last_played_at {
                if matches!(
                    game.source(),
                    Source::Steam | Source::HeroicEpic | Source::HeroicGog
                ) {
                    store.set_imported_playtime(&ImportedPlaytime {
                        game_id: game.id.clone(),
                        source: game.source(),
                        total_secs: timestamp(rng.below(500 * 3_600)),
                        last_played_at: Some(last_played),
                    })?;
                } else {
                    for session in 0..1 + rng.below(3) {
                        let start = last_played - timestamp(session * 86_400);
                        store.add_play_session(
                            &game.id,
                            start,
                            start + timestamp(rng.below(4 * 3_600)),
                        )?;
                    }
                }
            }
            if rng.chance(15) {
                let collection = &collections[index(rng.below(COLLECTIONS))];
                store.add_to_collection(collection.id, &game.id)?;
            }
        }
        Ok(())
    })?;
    Ok(art_rows)
}

/// A game as the shells' list models hold it: the entry and its precomputed keys.
struct Row {
    entry: LibraryEntry,
    sort_key: SortKey,
    search_key: SearchKey,
}

fn rows(entries: Vec<LibraryEntry>) -> Vec<Row> {
    entries
        .into_iter()
        .map(|entry| Row {
            sort_key: SortKey::new(&entry.game, entry.playtime_secs),
            search_key: SearchKey::new(&entry.game.title),
            entry,
        })
        .collect()
}

/// How many rows `filter` shows: one full pass, as a filter model's refilter makes.
fn shown(rows: &[Row], filter: &LibraryFilter) -> usize {
    rows.iter()
        .filter(|row| filter.matches(&row.entry.game, &row.entry.collections, &row.search_key))
        .count()
}

/// Runs `step` `runs` times and returns the durations, sorted.
fn time(runs: usize, mut step: impl FnMut()) -> Vec<Duration> {
    let mut times: Vec<_> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            step();
            start.elapsed()
        })
        .collect();
    times.sort();
    times
}

fn report(step: &str, detail: &str, times: &[Duration]) {
    let ms = |duration: Duration| duration.as_secs_f64() * 1_000.0;
    println!(
        "| {step} | {detail} | {:.2} | {:.2} | {:.2} |",
        ms(times[0]),
        ms(times[times.len() / 2]),
        ms(times[times.len() - 1]),
    );
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let games: u64 = args.next().map_or(Ok(10_000), |arg| arg.parse())?;
    let runs: usize = args.next().map_or(Ok(15), |arg| arg.parse())?;
    if runs == 0 {
        return Err("RUNS must be at least 1".into());
    }

    let dir = tempfile::tempdir()?;
    let db = dir.path().join("library.db");
    let art_dir = dir.path().join("art");
    let start = Instant::now();
    let art_rows = populate(&Store::open(&db)?, &art_dir, games)?;
    let populate_time = start.elapsed();

    println!(
        "Library: {games} games, {art_rows} artwork rows, {COLLECTIONS} collections; {runs} runs per step."
    );
    println!(
        "Generating and writing it took {:.0} ms.",
        populate_time.as_secs_f64() * 1_000.0
    );
    println!();
    println!("| Step | Detail | Min (ms) | Median (ms) | Max (ms) |");
    println!("| --- | --- | ---: | ---: | ---: |");

    report(
        "Load",
        "open the store, `library::load`",
        &time(runs, || {
            let store = Store::open(&db).expect("store opens");
            black_box(
                library::load(&store, true, game_groups::DEFAULT_SOURCE_ORDER)
                    .expect("library loads"),
            );
        }),
    );
    let entries = library::load(&Store::open(&db)?, true, game_groups::DEFAULT_SOURCE_ORDER)?;
    report(
        "Load",
        "sort and search keys for every game",
        &time(runs, || {
            black_box(rows(entries.clone()));
        }),
    );
    let rows = rows(entries);

    for mode in SortMode::ALL {
        let times = time(runs, || {
            let mut order: Vec<&Row> = rows.iter().collect();
            order.sort_by(|a, b| library::compare(mode, &a.sort_key, &b.sort_key));
            black_box(order);
        });
        report("Sort", &format!("{mode:?}"), &times);
    }

    let collection = rows
        .iter()
        .find_map(|row| row.entry.collections.first().copied())
        .unwrap_or_default();
    let toggles = Filters {
        installed: true,
        windows: true,
        ..Filters::default()
    };
    let filters = [
        ("view All", View::All, Filters::default()),
        ("view Installed", View::Installed, Filters::default()),
        ("view Favourites", View::Favourites, Filters::default()),
        ("view Play history", View::PlayHistory, Filters::default()),
        (
            "view Steam",
            View::Source(Source::Steam),
            Filters::default(),
        ),
        (
            "view a collection",
            View::Collection(collection),
            Filters::default(),
        ),
        ("view Hidden", View::Hidden, Filters::default()),
        ("All, Installed + Windows toggles", View::All, toggles),
    ];
    let mut shown_counts = Vec::new();
    for (detail, view, toggles) in filters {
        let mut filter = LibraryFilter::default();
        filter.set_view(view);
        filter.set_filters(toggles);
        let times = time(runs, || {
            black_box(shown(&rows, &filter));
        });
        shown_counts.push(format!("{detail}: {}", shown(&rows, &filter)));
        report("Filter", detail, &times);
    }

    // Typing a query one key at a time, each keystroke a full pass over the library.
    for query in ["dragon", "pokemon 3", "the aether", "okami", "zzz"] {
        let mut filter = LibraryFilter::default();
        let times = time(runs, || {
            for end in 1..=query.len() {
                filter.set_search(&query[..end]);
                black_box(shown(&rows, &filter));
            }
        });
        filter.set_search(query);
        shown_counts.push(format!("search {query:?}: {}", shown(&rows, &filter)));
        report(
            "Search",
            &format!("type {query:?}, {} keystrokes", query.len()),
            &times,
        );
    }

    println!();
    println!("Games shown: {}.", shown_counts.join("; "));
    Ok(())
}
