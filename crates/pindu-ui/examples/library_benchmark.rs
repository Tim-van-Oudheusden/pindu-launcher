//! Measures the library pipeline with 5,000 generated games: frame times while a grid over
//! the model scrolls from top to bottom, and how long searches take to settle.
//!
//! Run inside the GNOME SDK with a display, for example:
//! `cargo run --release -p pindu-ui --example library_benchmark`

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::library::LibraryEntry;
use pindu_core::model::{Game, GameId, LaunchKind, Source};
use pindu_ui::models::{GameObject, LibraryModel};

const GAMES: usize = 5_000;
const WORDS: &[&str] = &[
    "Ring", "Elden", "Dark", "Star", "Hollow", "Knight", "Witcher", "Cyber", "Punk", "Stellar",
    "Rogue", "Legacy", "Tower", "Crown", "Shadow", "Ember", "Frost", "River", "Ocean", "Dream",
];

fn generated_games() -> Vec<LibraryEntry> {
    let mut seed: u64 = 0x2545_f491_4f6c_dd1d;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    (0..GAMES)
        .map(|n| {
            let title = (0..3)
                .map(|_| WORDS[next() as usize % WORDS.len()])
                .collect::<Vec<_>>()
                .join(" ");
            let mut game = Game::new(
                GameId::new(Source::Steam, &n.to_string()),
                format!("{title} {n}"),
                LaunchKind::StoreUri,
                next() as i64 % 1_700_000_000,
            );
            game.last_played_at = (n % 3 == 0).then(|| next() as i64 % 1_700_000_000);
            game.installed = n % 2 == 0;
            LibraryEntry {
                game,
                art: Default::default(),
                playtime_secs: 0,
                playtime_origin: None,
                collections: Vec::new(),
            }
        })
        .collect()
}

/// Waits on the main loop until the incremental filter and sort have finished.
async fn settled(library: &LibraryModel) {
    while library.filter_model().pending() > 0 || library.sort_model().pending() > 0 {
        glib::timeout_future(Duration::from_millis(1)).await;
    }
}

fn percentile(sorted: &[Duration], fraction: f64) -> Duration {
    sorted[((sorted.len() - 1) as f64 * fraction).round() as usize]
}

fn main() -> glib::ExitCode {
    let app = gtk::Application::builder()
        .application_id("io.github.tim_van_oudheusden.pindu-launcher.Benchmark")
        .flags(gio::ApplicationFlags::NON_UNIQUE)
        .build();
    app.connect_activate(|app| {
        let library = LibraryModel::new();
        let started = Instant::now();
        library.set_games(generated_games());
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let label = gtk::Label::builder()
                .width_request(200)
                .height_request(300)
                .build();
            item.set_child(Some(&label));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let game = item.item().and_downcast::<GameObject>().expect("a game");
            let label = item.child().and_downcast::<gtk::Label>().expect("a label");
            label.set_label(&game.game().title);
        });
        let grid = gtk::GridView::builder()
            .model(&gtk::NoSelection::new(Some(library.model())))
            .factory(&factory)
            .max_columns(12)
            .build();
        let scrolled = gtk::ScrolledWindow::builder().child(&grid).build();
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .default_width(1280)
            .default_height(800)
            .child(&scrolled)
            .build();
        window.present();

        glib::spawn_future_local(glib::clone!(
            #[strong]
            app,
            async move {
                settled(&library).await;
                println!(
                    "load {GAMES} games, filter and sort: {:?}",
                    started.elapsed()
                );

                // Scroll from top to bottom, timing each frame's work.
                let frames: Rc<RefCell<Vec<Duration>>> = Rc::default();
                let frame_start: Rc<Cell<Option<Instant>>> = Rc::default();
                let clock = window.frame_clock().expect("a mapped window");
                let update = clock.connect_update(glib::clone!(
                    #[strong]
                    frame_start,
                    move |_| frame_start.set(Some(Instant::now()))
                ));
                let paint = clock.connect_after_paint(glib::clone!(
                    #[strong]
                    frames,
                    #[strong]
                    frame_start,
                    move |_| {
                        if let Some(start) = frame_start.take() {
                            frames.borrow_mut().push(start.elapsed());
                        }
                    }
                ));
                let adjustment = scrolled.vadjustment();
                let done = Rc::new(Cell::new(false));
                scrolled.add_tick_callback(glib::clone!(
                    #[strong]
                    done,
                    move |_, _| {
                        let bottom = adjustment.upper() - adjustment.page_size();
                        adjustment.set_value((adjustment.value() + 120.0).min(bottom));
                        if adjustment.value() >= bottom {
                            done.set(true);
                            return glib::ControlFlow::Break;
                        }
                        glib::ControlFlow::Continue
                    }
                ));
                while !done.get() {
                    glib::timeout_future(Duration::from_millis(5)).await;
                }
                clock.disconnect(update);
                clock.disconnect(paint);
                let mut frames = frames.take();
                frames.sort();
                println!(
                    "scroll: {} frames, median {:?}, p95 {:?}, max {:?}, over 16 ms: {}",
                    frames.len(),
                    percentile(&frames, 0.5),
                    percentile(&frames, 0.95),
                    frames.last().copied().unwrap_or_default(),
                    frames
                        .iter()
                        .filter(|frame| **frame > Duration::from_millis(16))
                        .count()
                );

                for query in ["r", "ri", "ring", "ring 4", "", "shadow knight", ""] {
                    let started = Instant::now();
                    library.set_search(query);
                    settled(&library).await;
                    println!(
                        "search {query:?}: {:?}, {} games",
                        started.elapsed(),
                        library.model().n_items()
                    );
                }
                app.quit();
            }
        ));
    });
    app.run_with_args::<&str>(&[])
}
