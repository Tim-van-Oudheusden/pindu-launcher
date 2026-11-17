//! Fullscreen mode's Playing Now cards, in the menu drawer and at the top of Quick Access's
//! main level: one per game we started that runs, with how long its session has run,
//! Return to its page and Stop. Both panels hold the same cards, each a `PlayingCards`,
//! which follow the games starting and ending; their times move on each minute while their
//! panel shows. The window has the cards' `win.playing-return` and `win.playing-stop`. In
//! a `--steam` gamescope, Return also brings the game back to the front, and Guide over a
//! game opens the drawer on the cards.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::gamescope::Front;
use pindu_core::model::Timestamp;
use pindu_core::panels::{DrawerLevel, Panel};
use pindu_core::session_time::{SessionTimeWords, session_time_text, until_next_minute};

use crate::application::PinduApplication;
use crate::models::GameObject;
use crate::quick_access::PinduQuickAccess;
use crate::session_service;
use crate::window::PinduWindow;

/// A panel's Playing Now cards: a card per game we started that runs, in a box over the
/// panel's list, with their times moving while the panel shows.
#[derive(Debug, Clone)]
pub(crate) struct PlayingCards(Rc<Inner>);

#[derive(Debug)]
struct Inner {
    cards: gtk::Box,
    /// The cards' time labels, each with when its session began.
    times: RefCell<Vec<(gtk::Label, Timestamp)>>,
    /// The next tick of the times while the panel shows.
    tick: RefCell<Option<glib::SourceId>>,
    shown: Cell<bool>,
}

impl PlayingCards {
    /// The cards in `cards`, over `list`: Up from the top of the list goes to the nearest
    /// card's Return (rather than the Stop beside it).
    pub(crate) fn new(cards: &gtk::Box, list: &gtk::ListBox) -> Self {
        list.connect_keynav_failed(glib::clone!(
            #[weak]
            cards,
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
        Self(Rc::new(Inner {
            cards: cards.clone(),
            times: RefCell::new(Vec::new()),
            tick: RefCell::new(None),
            shown: Cell::new(false),
        }))
    }

    /// Shows a card for each of `playing`, the first started first, and hides the cards
    /// without one. Says whether a card had focus, which the panel then takes back.
    pub(crate) fn fill(&self, playing: &[(GameObject, Timestamp)]) -> bool {
        let cards = &self.0.cards;
        let had_focus = cards
            .root()
            .and_then(|root| root.focus())
            .is_some_and(|focus| focus.is_ancestor(cards));
        while let Some(card) = cards.first_child() {
            cards.remove(&card);
        }
        let now = session_service::now();
        let mut times = Vec::new();
        for (game, started_at) in playing {
            let (card, time) = playing_card(game, session_time(*started_at, now));
            cards.append(&card);
            times.push((time, *started_at));
        }
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

/// The games we started that run, the first started first, each with when it began.
fn playing_games(application: &PinduApplication) -> Vec<(GameObject, Timestamp)> {
    let library = application.library_service().model();
    application
        .session_service()
        .sessions()
        .into_iter()
        .filter_map(|(id, started_at)| Some((library.game_object(&id)?, started_at)))
        .collect()
}

impl PinduWindow {
    /// The cards' Return and Stop, and the drawer's cards, filled now and again whenever a
    /// game we started begins or ends.
    pub(crate) fn setup_playing_cards(&self, application: &PinduApplication) {
        let imp = self.imp();
        self.add_game_action("playing-return", |window, id| {
            window.close_panel();
            window.return_to_game(id);
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
        let cards = PlayingCards::new(&imp.playing_cards, &imp.drawer_list);
        imp.drawer.connect_show_sidebar_notify(glib::clone!(
            #[strong]
            cards,
            move |drawer| cards.set_shown(drawer.shows_sidebar())
        ));
        let _ = imp.drawer_cards.set(cards);
        application
            .session_service()
            .connect_sessions_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.fill_playing_cards()
            ));
        self.fill_playing_cards();
        application
            .gamescope_service()
            .connect_front_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |front| window.front_changed(front)
            ));
    }

    /// Guide over a game brought the launcher to the front in a `--steam` gamescope: in
    /// fullscreen mode the drawer opens on the Playing Now cards, so Return is a press
    /// away. A game that ends gives the front back too, and so does a link in Quick Access
    /// over a game, which shows what it leads to instead.
    fn front_changed(&self, front: Front) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let by_guide =
            front == Front::Launcher && application.gamescope_service().launcher_by_guide();
        if !by_guide || !self.is_fullscreen() || !application.session_service().any_playing() {
            return;
        }
        self.show_panel(Panel::Drawer(DrawerLevel::Main));
        if let Some(button) = self
            .imp()
            .playing_cards
            .last_child()
            .as_ref()
            .and_then(card_return)
        {
            button.grab_focus();
        }
    }

    /// Fills the drawer's cards. Focus on a card that went moves to the open panel's first
    /// entry.
    fn fill_playing_cards(&self) {
        let (Some(application), Some(cards)) = (
            self.application().and_downcast::<PinduApplication>(),
            self.imp().drawer_cards.get(),
        ) else {
            return;
        };
        if cards.fill(&playing_games(&application)) && self.open_panel().is_some() {
            self.focus_panel(None);
        }
    }
}

impl PinduQuickAccess {
    /// Fills the panel's cards now and again whenever a game we started begins or ends.
    pub(crate) fn setup_playing_cards(&self, application: &PinduApplication) {
        application
            .session_service()
            .connect_sessions_changed(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move || quick_access.fill_playing_cards()
            ));
        self.fill_playing_cards();
    }

    /// Fills the panel's cards. Focus on a card that went moves to the level's first entry.
    fn fill_playing_cards(&self) {
        let (Some(application), Some(cards)) = (self.application(), self.imp().playing.get())
        else {
            return;
        };
        if cards.fill(&playing_games(&application)) && self.level().is_some() {
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

/// A Playing Now card for a game we started that runs: its title, `time` it has run, and
/// Return to its page or Stop it. Also gives the label of its time, to move it on.
fn playing_card(game: &GameObject, time: String) -> (gtk::Box, gtk::Label) {
    let card = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(12)
        .css_classes(["card", "playing-card"])
        .build();
    let heading = gtk::Box::builder().spacing(12).build();
    heading.append(
        &gtk::Label::builder()
            .label(gettext("Playing Now"))
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
            .label(game.game().title.as_str())
            .xalign(0.0)
            .wrap(true)
            .css_classes(["title-3"])
            .build(),
    );
    let buttons = gtk::Box::builder().spacing(12).homogeneous(true).build();
    let target = game.id().as_str().to_variant();
    for (label, action, style) in [
        (gettext("_Return"), "win.playing-return", "suggested-action"),
        (gettext("_Stop"), "win.playing-stop", "destructive-action"),
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

/// A Playing Now card's Return button: a card holds its labels, then its buttons.
pub(crate) fn card_return(card: &gtk::Widget) -> Option<gtk::Widget> {
    card.last_child()?.first_child()
}
