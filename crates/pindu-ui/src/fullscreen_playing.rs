//! Fullscreen mode's Playing Now cards, in the menu drawer and at the top of Quick Access's
//! main level: one per game we started that runs, with how long its session has run,
//! Return to its page and Stop. Both panels hold the same cards, each a `PlayingCards`,
//! which follow the games starting and ending; their times move on each minute while their
//! panel shows. The window has the cards' `win.playing-return` and `win.playing-stop`. In
//! a `--steam` gamescope, Return also brings the game back to the front, the drawer and
//! Quick Access over a game hold the same cards, and a store-launched game that plays gets
//! a card too, whose Stop closes its windows, as does an app opened from Apps
//! (`pindu_core::apps`), whose card offers Return and Exit. A design draws the cards
//! compact or as banners of the game's art (`pindu_core::themes::PlayingCard`), in both
//! panels and over a game.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::{GameId, Source, Timestamp};
use pindu_core::session_time::{SessionTimeWords, session_time_text, until_next_minute};
use pindu_core::themes::PlayingCard;

use crate::application::PinduApplication;
use crate::home_page::{hero_art, load_picture};
use crate::menu_drawer::PinduMenuDrawer;
use crate::models::GameObject;
use crate::quick_access::PinduQuickAccess;
use crate::session_service;
use crate::window::PinduWindow;

/// What a Playing Now card stands for: a game that runs, or an app opened from Apps
/// (`pindu_core::apps`), whose windows a `--steam` gamescope follows as a store's.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Running {
    /// A game of the library.
    Game(GameObject),
    /// An installed app, by the ID gamescope follows it under (`apps::opened_id`) and its
    /// name.
    App { id: GameId, name: String },
}

impl Running {
    /// The ID the card's buttons act on.
    fn id(&self) -> GameId {
        match self {
            Self::Game(game) => game.id(),
            Self::App { id, .. } => id.clone(),
        }
    }

    /// The game's title, or the app's name.
    fn title(&self) -> String {
        match self {
            Self::Game(game) => game.game().title.clone(),
            Self::App { name, .. } => name.clone(),
        }
    }
}

/// A panel's Playing Now cards: a card per game we started that runs, in a box at the top
/// of the panel, with their times moving while the panel shows.
#[derive(Debug, Clone)]
pub(crate) struct PlayingCards(Rc<Inner>);

#[derive(Debug)]
struct Inner {
    cards: gtk::Box,
    /// The games and apps the cards show, each with when its session began.
    playing: RefCell<Vec<(Running, Timestamp)>>,
    /// How the design draws the cards.
    arrangement: Cell<PlayingCard>,
    /// The cards' time labels, each with when its session began.
    times: RefCell<Vec<(gtk::Label, Timestamp)>>,
    /// The next tick of the times while the panel shows.
    tick: RefCell<Option<glib::SourceId>>,
    shown: Cell<bool>,
}

impl PlayingCards {
    /// The cards in `cards`.
    pub(crate) fn new(cards: &gtk::Box) -> Self {
        Self(Rc::new(Inner {
            cards: cards.clone(),
            playing: RefCell::new(Vec::new()),
            arrangement: Cell::new(PlayingCard::default()),
            times: RefCell::new(Vec::new()),
            tick: RefCell::new(None),
            shown: Cell::new(false),
        }))
    }

    /// Draws the cards as `arrangement`, the design's, says, at once; a card that had focus
    /// gives it to the first card's Return.
    pub(crate) fn set_arrangement(&self, arrangement: PlayingCard) {
        if self.0.arrangement.replace(arrangement) == arrangement {
            return;
        }
        let playing = self.0.playing.borrow().clone();
        if self.fill(&playing)
            && let Some(button) = self.0.cards.first_child().as_ref().and_then(card_return)
        {
            button.grab_focus();
        }
    }

    /// Up from the top of `list`, under the cards, goes to the nearest card's Return
    /// (rather than the Stop beside it).
    pub(crate) fn lead_up_from(&self, list: &gtk::ListBox) {
        list.connect_keynav_failed(glib::clone!(
            #[weak(rename_to = cards)]
            self.0.cards,
            #[upgrade_or]
            glib::Propagation::Stop,
            move |_, direction| {
                if direction == gtk::DirectionType::Up
                    && cards.is_visible()
                    && let Some(button) = cards.last_child().as_ref().and_then(card_return)
                {
                    button.grab_focus();
                }
                glib::Propagation::Stop
            }
        ));
    }

    /// Shows a card for each of `playing`, the first started first, drawn as the design
    /// says, and hides the cards without one. Says whether a card had focus, which the
    /// panel then takes back.
    pub(crate) fn fill(&self, playing: &[(Running, Timestamp)]) -> bool {
        let cards = &self.0.cards;
        let had_focus = cards
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus.is_ancestor(cards));
        while let Some(card) = cards.first_child() {
            cards.remove(&card);
        }
        let now = session_service::now();
        let arrangement = self.0.arrangement.get();
        let mut times = Vec::new();
        for (running, started_at) in playing {
            let time = session_time(*started_at, now);
            let (card, time) = match arrangement {
                PlayingCard::Compact => playing_card(running, time),
                PlayingCard::Banner => playing_banner(running, time),
            };
            cards.append(&card);
            times.push((time, *started_at));
        }
        self.0.playing.replace(playing.to_vec());
        cards.set_visible(!playing.is_empty());
        self.0.times.replace(times);
        self.update_times();
        had_focus
    }

    /// The panel shows, or no longer: the times move only while it shows.
    pub(crate) fn set_shown(&self, shown: bool) {
        self.0.shown.set(shown);
        self.update_times();
    }

    /// Moves the times on each minute while the panel shows, and stops otherwise.
    fn update_times(&self) {
        if let Some(tick) = self.0.tick.take() {
            tick.remove();
        }
        if self.0.shown.get() && !self.0.times.borrow().is_empty() {
            tick_times(&self.0);
        }
    }
}

/// Shows each card's time and comes back when the next one changes.
fn tick_times(inner: &Rc<Inner>) {
    let now = session_service::now();
    let mut next = None;
    for (label, started_at) in inner.times.borrow().iter() {
        label.set_label(&session_time(*started_at, now));
        let delay = until_next_minute(*started_at, now);
        next = Some(next.map_or(delay, |next: std::time::Duration| next.min(delay)));
    }
    let Some(delay) = next else {
        return;
    };
    let weak = Rc::downgrade(inner);
    let tick = glib::timeout_add_local_once(delay, move || {
        if let Some(inner) = weak.upgrade() {
            // This timeout is done: forget it rather than remove it.
            inner.tick.take();
            tick_times(&inner);
        }
    });
    inner.tick.replace(Some(tick));
}

/// The games that run, each with when it began: the games we started, the first started
/// first, then the store-launched game that plays, or the app opened from Apps that is
/// open.
fn playing_games(application: &PinduApplication) -> Vec<(Running, Timestamp)> {
    let library = application.library_service().model();
    let apps = application.apps_service();
    application
        .session_service()
        .sessions()
        .into_iter()
        .chain(application.gamescope_service().store_playing())
        .filter_map(|(id, started_at)| {
            let running = match library.game_object(&id) {
                Some(game) => Running::Game(game),
                None if id.source() == Source::Desktop => {
                    let app = apps.app(id.native_id())?;
                    Running::App { id, name: app.name }
                }
                None => return None,
            };
            Some((running, started_at))
        })
        .collect()
}

impl PinduWindow {
    /// The cards' Return and Stop, and the drawer's cards, filled now and again whenever a
    /// game begins or ends.
    pub(crate) fn setup_playing_cards(&self, application: &PinduApplication) {
        let imp = self.imp();
        self.add_game_action("playing-return", |window, id| {
            window.close_panel();
            // An app has no page to return to.
            if window.library().game_object(id).is_some() {
                window.return_to_game(id);
            }
            if let Some(application) = window.application().and_downcast::<PinduApplication>() {
                application.gamescope_service().bring_to_front(id);
            }
        });
        self.add_game_action("playing-stop", |window, id| {
            window.close_panel();
            if let Some(application) = window.application() {
                application.activate_action("stop", Some(&id.as_str().to_variant()));
            }
        });
        imp.drawer.connect_show_sidebar_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |drawer| {
                window
                    .imp()
                    .drawer_panel
                    .cards()
                    .set_shown(drawer.shows_sidebar());
            }
        ));
        // Focus on a card that went moves to the open panel's first entry.
        imp.drawer_panel.follow_playing(
            application,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    if window.open_panel().is_some() {
                        window.focus_panel(None);
                    }
                }
            ),
        );
    }
}

impl PinduMenuDrawer {
    /// Fills the drawer's cards now and again whenever a game begins or ends; `refocus`
    /// runs when a card that had focus went.
    pub(crate) fn follow_playing(
        &self,
        application: &PinduApplication,
        refocus: impl Fn() + 'static,
    ) {
        let fill: Rc<dyn Fn()> = Rc::new(glib::clone!(
            #[weak(rename_to = drawer)]
            self,
            #[weak]
            application,
            move || {
                if drawer.cards().fill(&playing_games(&application)) {
                    refocus();
                }
            }
        ));
        application.session_service().connect_sessions_changed({
            let fill = fill.clone();
            move || fill()
        });
        application
            .gamescope_service()
            .connect_store_playing_changed({
                let fill = fill.clone();
                move || fill()
            });
        fill();
    }
}

impl PinduQuickAccess {
    /// Fills the panel's cards now and again whenever a game begins or ends.
    pub(crate) fn setup_playing_cards(&self, application: &PinduApplication) {
        application
            .session_service()
            .connect_sessions_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || quick_access.fill_playing_cards()
            ));
        application
            .gamescope_service()
            .connect_store_playing_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || quick_access.fill_playing_cards()
            ));
        self.fill_playing_cards();
    }

    /// Fills the panel's cards, which Quick shows while a game runs. Focus on a card that
    /// went moves to the level's first item.
    fn fill_playing_cards(&self) {
        let (Some(application), Some(cards)) = (self.application(), self.imp().playing.get())
        else {
            return;
        };
        let had_focus = cards.fill(&playing_games(&application));
        let playing = self.imp().playing_cards.is_visible();
        self.update_shown(|shown| shown.playing = playing);
        if had_focus && self.level().is_some() {
            self.focus_level(None);
        }
    }
}

/// How long a session that began at `started_at` has run at `now`.
fn session_time(started_at: Timestamp, now: Timestamp) -> String {
    let under_a_minute = gettext("Under a minute");
    // Translators: how long a game has run, under an hour; {minutes} is a number from 1
    // to 59.
    let minutes = gettext("{minutes} min");
    // Translators: how long a game has run, an hour or more; {hours} is a number and
    // {minutes} two digits from 00 to 59.
    let hours = gettext("{hours} h {minutes} min");
    session_time_text(
        started_at,
        now,
        SessionTimeWords {
            under_a_minute: &under_a_minute,
            minutes: &minutes,
            hours: &hours,
        },
    )
}

/// A compact Playing Now card for a game that runs (`.playing-card`): its title, `time` it
/// has run, and Return to its page, or Stop it when we started it. Also gives the label of
/// its time, to move it on.
fn playing_card(running: &Running, time: String) -> (gtk::Widget, gtk::Label) {
    let (card, time) = card_content(running, time);
    card.add_css_class("card");
    card.add_css_class("playing-card");
    (card.upcast(), time)
}

/// A Playing Now card drawn as a banner, in a panel that is `.playing-card-banner`: the
/// game's art (`.playing-card-art`), its hero or else its cover, loaded as Home loads a
/// game's hero, filling the card, with what the compact card holds over a scrim at its foot
/// (`.playing-card-scrim`). Without art, or until it loads, the card is `.no-art`, in the
/// card colour, as an app's always is. The card grows with its text at a larger text size.
fn playing_banner(running: &Running, time: String) -> (gtk::Widget, gtk::Label) {
    let art = gtk::Picture::builder()
        .content_fit(gtk::ContentFit::Cover)
        .can_shrink(true)
        .height_request(BANNER_HEIGHT)
        .accessible_role(gtk::AccessibleRole::Presentation)
        .css_classes(["playing-card-art"])
        .build();
    let card = gtk::Overlay::builder()
        .child(&art)
        .overflow(gtk::Overflow::Hidden)
        .css_classes(["card", "playing-card", "no-art"])
        .build();
    let (scrim, time) = card_content(running, time);
    scrim.set_valign(gtk::Align::End);
    scrim.add_css_class("playing-card-scrim");
    card.add_overlay(&scrim);
    card.set_measure_overlay(&scrim, true);
    if let Running::Game(game) = running
        && let Some(path) = hero_art(game)
    {
        let card = card.downgrade();
        load_picture(&art, path, move |_| {
            if let Some(card) = card.upgrade() {
                card.remove_css_class("no-art");
            }
        });
    }
    (card.upcast(), time)
}

/// The least height, in logical pixels, of a Playing Now card drawn as a banner.
const BANNER_HEIGHT: i32 = 168;

/// What a Playing Now card holds, top to bottom: "Playing Now" and `time` the game has run,
/// its title, then Return and Stop; for an app, "Open Now", its name, Return and Exit.
/// Also gives the label of its time.
fn card_content(running: &Running, time: String) -> (gtk::Box, gtk::Label) {
    let app = matches!(running, Running::App { .. });
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .build();
    let heading = gtk::Box::builder().spacing(12).build();
    heading.append(
        &gtk::Label::builder()
            .label(if app {
                gettext("Open Now")
            } else {
                gettext("Playing Now")
            })
            .xalign(0.0)
            .hexpand(true)
            .css_classes(["heading", "accent"])
            .build(),
    );
    let time = gtk::Label::builder()
        .label(time)
        .xalign(1.0)
        .css_classes(["session-time", "numeric", "dim-label"])
        .build();
    heading.append(&time);
    card.append(&heading);
    card.append(
        &gtk::Label::builder()
            .label(running.title())
            .xalign(0.0)
            .wrap(true)
            .css_classes(["title-3"])
            .build(),
    );
    let buttons = gtk::Box::builder().spacing(12).homogeneous(true).build();
    let target = running.id().as_str().to_variant();
    let stop = if app {
        gettext("_Exit")
    } else {
        gettext("_Stop")
    };
    for (label, action, style) in [
        (gettext("_Return"), "win.playing-return", "suggested-action"),
        (stop, "win.playing-stop", "destructive-action"),
    ] {
        let button = gtk::Button::builder()
            .label(label)
            .use_underline(true)
            .action_name(action)
            .action_target(&target)
            .css_classes(["pill", style])
            .build();
        buttons.append(&button);
    }
    card.append(&buttons);
    (card, time)
}

/// A Playing Now card's Return button: a card holds its labels, then its buttons; a banner
/// holds them over its art.
pub(crate) fn card_return(card: &gtk::Widget) -> Option<gtk::Widget> {
    let content = if card.is::<gtk::Overlay>() {
        card.last_child()?
    } else {
        card.clone()
    };
    content.last_child()?.first_child()
}
