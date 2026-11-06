//! Fullscreen mode's Playing Now cards, in the menu drawer and at the top of Quick Access's
//! main level: one per game we started that runs, with how long its session has run,
//! Return to its page and Stop. Both panels hold the same cards, which follow the games
//! starting and ending; their times move on each minute while either panel shows.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use pindu_core::model::Timestamp;
use pindu_core::session_time::{SessionTimeWords, session_time_text, until_next_minute};

use crate::application::PinduApplication;
use crate::models::GameObject;
use crate::session_service;
use crate::window::PinduWindow;

impl PinduWindow {
    /// The cards' Return and Stop, and the cards themselves, filled now and again whenever
    /// a game we started begins or ends.
    pub(crate) fn setup_playing_cards(&self, application: &PinduApplication) {
        let imp = self.imp();
        self.add_game_action("playing-return", |window, id| {
            window.close_panel();
            window.return_to_game(id);
        });
        self.add_game_action("playing-stop", |window, id| {
            window.close_panel();
            if let Some(application) = window.application() {
                application.activate_action("stop", Some(&id.as_str().to_variant()));
            }
        });
        // Up from the top of a panel's list goes to the nearest card's Return (rather than
        // the Stop beside it).
        for (list, cards) in [
            (imp.drawer_list.get(), imp.playing_cards.get()),
            (
                imp.quick_access_list.get(),
                imp.quick_access_playing_cards.get(),
            ),
        ] {
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
        }
        for panel in [imp.drawer.get(), imp.quick_access.get()] {
            panel.connect_show_sidebar_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.update_session_times()
            ));
        }
        application
            .session_service()
            .connect_sessions_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.fill_playing_cards()
            ));
        self.fill_playing_cards();
    }

    /// Fills the drawer and Quick Access with a card per game we started that runs, the
    /// first started first, and hides them without one. Focus on a card that went moves
    /// to the open panel's first entry.
    pub(crate) fn fill_playing_cards(&self) {
        let imp = self.imp();
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let playing: Vec<(GameObject, Timestamp)> = application
            .session_service()
            .sessions()
            .into_iter()
            .filter_map(|(id, started_at)| Some((self.library().game_object(&id)?, started_at)))
            .collect();
        let focus = gtk::prelude::GtkWindowExt::focus(self);
        let boxes = [
            imp.playing_cards.get(),
            imp.quick_access_playing_cards.get(),
        ];
        let had_focus = boxes
            .iter()
            .any(|cards| focus.as_ref().is_some_and(|focus| focus.is_ancestor(cards)));
        let now = session_service::now();
        let mut times = Vec::new();
        for cards in &boxes {
            while let Some(card) = cards.first_child() {
                cards.remove(&card);
            }
            for (game, started_at) in &playing {
                let (card, time) = playing_card(game, session_time(*started_at, now));
                cards.append(&card);
                times.push((time, *started_at));
            }
            cards.set_visible(!playing.is_empty());
        }
        imp.session_times.replace(times);
        self.update_session_times();
        if had_focus && self.open_panel().is_some() {
            self.focus_panel(None);
        }
    }

    /// Moves the cards' times on each minute while the drawer or Quick Access shows, and
    /// stops otherwise.
    fn update_session_times(&self) {
        let imp = self.imp();
        if let Some(tick) = imp.session_tick.take() {
            tick.remove();
        }
        let shown = imp.drawer.shows_sidebar() || imp.quick_access.shows_sidebar();
        if shown && !imp.session_times.borrow().is_empty() {
            self.tick_session_times();
        }
    }

    /// Shows each card's time and comes back when the next one changes.
    fn tick_session_times(&self) {
        let imp = self.imp();
        let now = session_service::now();
        let mut next = None;
        for (label, started_at) in imp.session_times.borrow().iter() {
            label.set_label(&session_time(*started_at, now));
            let delay = until_next_minute(*started_at, now);
            next = Some(next.map_or(delay, |next: std::time::Duration| next.min(delay)));
        }
        let Some(delay) = next else {
            return;
        };
        let tick = glib::timeout_add_local_once(
            delay,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // This timeout is done: forget it rather than remove it.
                    window.imp().session_tick.take();
                    window.tick_session_times();
                }
            ),
        );
        imp.session_tick.replace(Some(tick));
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
