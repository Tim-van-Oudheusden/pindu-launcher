//! Fullscreen mode's game page: its presentation, a controller scrolling through its
//! description, the starting screen over the page while Play starts the game, and the
//! return to the page, with focus on Play or Stop, once the game ends or the window has
//! focus again. What the screen shows and when to return is
//! `pindu_core::launch_overlay`; this feeds it the window's focus (or in a `--steam`
//! gamescope, who gamescope shows and the store launch), dialogs, the game's Playing state,
//! the launch's report (its toast) and Cancel, and does what it says.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamepad::Direction;
use pindu_core::gamescope::{self, Front};
use pindu_core::hints::Launching;
use pindu_core::launch_overlay::{CANCEL_AFTER, Effect, Event, Launch, LaunchOverlay};
use pindu_core::model::GameId;

use crate::application::PinduApplication;
use crate::game_page::GamePage;
use crate::launch_service::launcher_name;
use crate::models::GameObject;
use crate::permission_service;
use crate::window::PinduWindow;

impl PinduWindow {
    pub(crate) fn setup_fullscreen_game(&self) {
        self.connect_is_active_notify(|window| {
            window.follow_launch(Event::Focus(window.is_active()));
        });
        self.connect_notify_local(Some("visible-dialog"), |window, _| {
            if window.visible_dialog().is_some() {
                window.follow_launch(Event::Interrupted);
            }
        });
        let cancel = gio::ActionEntry::builder("cancel-launch")
            .activate(|window: &Self, _, _| window.activate_cancel())
            .build();
        self.add_action_entries([cancel]);
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let gamescope = application.gamescope_service();
        gamescope.connect_front_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |front| window.follow_launch(Event::Front(front))
        ));
        gamescope.connect_store_launch_ended(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.check_store_launch()
        ));
    }

    /// Shows the game pages in fullscreen mode's presentation, or the desktop one; leaving
    /// fullscreen mode drops the starting screen and the return.
    pub(crate) fn show_fullscreen_game(&self, fullscreen: bool) {
        for page in self.game_pages() {
            page.set_fullscreen(fullscreen);
        }
        if !fullscreen {
            self.drop_launch();
        }
    }

    /// Starts following a Play of `game` in fullscreen mode: the starting screen goes up
    /// over its page until the launch is through, and offers Cancel after a while.
    pub fn begin_launch(&self, game: &GameObject) {
        if !self.is_fullscreen() || permission_service::needs_permission(&game.game()) {
            return;
        }
        let Some(launch) = Launch::of(&game.game()) else {
            return;
        };
        self.drop_launch();
        let in_gamescope = self
            .application()
            .and_downcast::<PinduApplication>()
            .is_some_and(|application| application.gamescope_service().is_active());
        if in_gamescope {
            self.imp().launch.replace(LaunchOverlay::in_gamescope());
        }
        let id = game.id();
        if self.game_page(&id).is_none() {
            self.show_details(&id);
        }
        let handler = game.connect_notify_local(
            Some("playing"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |game, _| window.follow_launch(Event::Playing(game.id(), game.is_playing()))
            ),
        );
        self.imp()
            .launch_game
            .replace(Some((game.clone(), handler)));
        self.follow_launch(Event::Play(id, launch));
        let source = glib::timeout_add_local_once(
            CANCEL_AFTER,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // This timer has fired.
                    window.imp().launch_cancel.take();
                    window.follow_launch(Event::CancelDue);
                }
            ),
        );
        self.imp().launch_cancel.replace(Some(source));
    }

    /// The launch reported back: handed off, started, or failed.
    pub fn launch_reported(&self) {
        self.follow_launch(Event::Reported);
        self.check_store_launch();
    }

    /// What the starting screen shows.
    pub(crate) fn launching(&self) -> Launching {
        self.imp().launch.borrow().shown()
    }

    /// B held down over the starting screen: stops a game we start, or does what its
    /// Cancel does once that shows.
    pub(crate) fn cancel_launch(&self) {
        self.follow_launch(Event::CancelHeld);
    }

    /// The starting screen's Cancel: stops a game we start, or gives up on a store launch.
    pub(crate) fn activate_cancel(&self) {
        self.follow_launch(Event::Cancel);
    }

    /// The shown page, when it is a game's.
    pub(crate) fn visible_game_page(&self) -> Option<GamePage> {
        self.imp()
            .navigation_view
            .visible_page()
            .and_downcast::<GamePage>()
    }

    /// Up or Down while the shown game page's description has focus, in fullscreen mode:
    /// scrolls through it rather than move focus, while more of it lies that way; says
    /// whether it did.
    pub(crate) fn scroll_description(&self, direction: Direction) -> bool {
        self.is_fullscreen()
            && self
                .visible_game_page()
                .is_some_and(|page| page.scroll_description(direction))
    }

    fn follow_launch(&self, event: Event) {
        glib::g_debug!("pindu", "Launch overlay event: {event:?}");
        let effect = self.imp().launch.borrow_mut().handle(event);
        let application = self.application().and_downcast::<PinduApplication>();
        match effect {
            Some(Effect::Linger(after)) => self.linger(after),
            Some(Effect::Stop(id)) => {
                if let Some(application) = application {
                    application.activate_action("stop", Some(&id.as_str().to_variant()));
                }
            }
            Some(Effect::Return(id)) => self.return_to_game(&id),
            Some(Effect::GiveUp(id)) => {
                if let Some(application) = application {
                    application.gamescope_service().give_up_store_launch(&id);
                }
                self.return_to_game(&id);
            }
            None => {}
        }
        self.show_launch();
        self.update_hints();
        // A game page turns Stop back into Play in its own `playing` handler, which may
        // run after this one: the hints follow once it has.
        glib::idle_add_local_once(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.update_hints()
        ));
    }

    /// In a `--steam` gamescope, tells the launch its store launch is over once the
    /// launcher no longer wants the game in front: the launch ended, or the hand-off failed.
    fn check_store_launch(&self) {
        let game = {
            let launch = self.imp().launch.borrow();
            launch
                .follows_gamescope()
                .then(|| launch.game().cloned())
                .flatten()
        };
        let (Some(id), Some(application)) =
            (game, self.application().and_downcast::<PinduApplication>())
        else {
            return;
        };
        let wanted = application.gamescope_service().wanted_front();
        if wanted != Front::Game(gamescope::app_id(&id)) {
            self.follow_launch(Event::StoreLaunchOver);
        }
    }

    /// Lets the starting screen linger for `after`, then tells the launch it did.
    fn linger(&self, after: std::time::Duration) {
        if let Some(source) = self.imp().launch_linger.take() {
            source.remove();
        }
        let source = glib::timeout_add_local_once(
            after,
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    // This timer has fired.
                    window.imp().launch_linger.take();
                    window.follow_launch(Event::Lingered);
                }
            ),
        );
        self.imp().launch_linger.replace(Some(source));
    }

    /// Shows the starting screen over the launched game's page: the launcher's name for a
    /// store game, "Starting…" for a game we run, and Cancel once it is offered.
    fn show_launch(&self) {
        let (game, launch, cancel) = {
            let launch = self.imp().launch.borrow();
            (
                launch.game().cloned(),
                launch.launch(),
                launch.shown() == Launching::Cancel,
            )
        };
        for page in self.game_pages() {
            let launched = page
                .game()
                .zip(game.as_ref())
                .is_some_and(|(object, id)| object.id() == *id);
            let status = match launch {
                _ if !launched => None,
                None => None,
                Some(Launch::HandOff) => Some(
                    page.game()
                        .and_then(|object| launcher_name(object.game().source()))
                        .map_or_else(
                            || gettext("Opening the launcher…"),
                            |launcher| {
                                gettext("Opening {launcher}…").replace("{launcher}", launcher)
                            },
                        ),
                ),
                Some(Launch::Run) => Some(gettext("Starting…")),
            };
            page.show_launch(status.as_deref(), launched && cancel);
        }
    }

    /// Back to the game's page, with focus on Play, or on Stop while the game runs.
    pub(crate) fn return_to_game(&self, id: &GameId) {
        let navigation_view = &self.imp().navigation_view;
        match self.game_page(id) {
            Some(page) if self.visible_game_page().as_ref() == Some(&page) => page.focus_play(),
            // Showing the page gives Play the focus.
            Some(page) => {
                navigation_view.pop_to_page(&page);
            }
            None => self.show_details(id),
        }
    }

    /// Forgets the launch: no starting screen, no return.
    fn drop_launch(&self) {
        let imp = self.imp();
        for timer in [&imp.launch_linger, &imp.launch_cancel] {
            if let Some(source) = timer.take() {
                source.remove();
            }
        }
        if let Some((game, handler)) = imp.launch_game.take() {
            game.disconnect(handler);
        }
        imp.launch.take();
        self.show_launch();
    }

    /// The page of the game with `id`, if it is open.
    fn game_page(&self, id: &GameId) -> Option<GamePage> {
        self.game_pages()
            .into_iter()
            .find(|page| page.game().is_some_and(|object| object.id() == *id))
    }

    pub(crate) fn game_pages(&self) -> Vec<GamePage> {
        self.imp()
            .navigation_view
            .navigation_stack()
            .iter::<adw::NavigationPage>()
            .flatten()
            .filter_map(|page| page.downcast::<GamePage>().ok())
            .collect()
    }
}
