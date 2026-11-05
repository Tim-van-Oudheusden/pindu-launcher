//! Fullscreen mode's game page: its presentation, View opening the game's options
//! (`win.game-options`), the launch overlay over the page while Play starts the game, and
//! the return to the page, with focus on Play or Stop, once the game ends or the window
//! has focus again. What the overlay shows and when to return is
//! `pindu_core::launch_overlay`; this feeds it the window's focus, dialogs, the game's
//! Playing state and the launch's report (its toast), and does what it says.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::hints::Launching;
use pindu_core::launch_overlay::{Effect, Event, Launch};
use pindu_core::model::GameId;

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
        let options = gio::ActionEntry::builder("game-options")
            .activate(|window: &Self, _, _| window.show_game_options())
            .build();
        self.add_action_entries([options]);
    }

    /// Shows the game pages in fullscreen mode's presentation, or the desktop one; leaving
    /// fullscreen mode drops the launch overlay and the return.
    pub(crate) fn show_fullscreen_game(&self, fullscreen: bool) {
        for page in self.game_pages() {
            page.set_fullscreen(fullscreen);
        }
        if !fullscreen {
            self.drop_launch();
        }
    }

    /// Starts following a Play of `game` in fullscreen mode: the overlay goes up over its
    /// page until the launch is through.
    pub fn begin_launch(&self, game: &GameObject) {
        if !self.is_fullscreen() || permission_service::needs_permission(&game.game()) {
            return;
        }
        let Some(launch) = Launch::of(&game.game()) else {
            return;
        };
        self.drop_launch();
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
    }

    /// The launch reported back: handed off, started, or failed.
    pub fn launch_reported(&self) {
        self.follow_launch(Event::Reported);
    }

    /// What the launch overlay shows.
    pub(crate) fn launching(&self) -> Launching {
        self.imp().launch.borrow().shown()
    }

    /// B held down over the launch overlay: stops a game we start.
    pub(crate) fn cancel_launch(&self) {
        self.follow_launch(Event::CancelHeld);
    }

    /// View or X on a game page: the game's options.
    pub(crate) fn show_game_options(&self) {
        if self.visible_dialog().is_some() {
            return;
        }
        if let Some(page) = self.visible_game_page() {
            page.show_options();
        }
    }

    /// The shown page, when it is a game's.
    pub(crate) fn visible_game_page(&self) -> Option<GamePage> {
        self.imp()
            .navigation_view
            .visible_page()
            .and_downcast::<GamePage>()
    }

    fn follow_launch(&self, event: Event) {
        glib::g_debug!("pindu", "Launch overlay event: {event:?}");
        let effect = self.imp().launch.borrow_mut().handle(event);
        match effect {
            Some(Effect::Linger(after)) => self.linger(after),
            Some(Effect::Stop(id)) => {
                if let Some(application) = self.application() {
                    application.activate_action("stop", Some(&id.as_str().to_variant()));
                }
            }
            Some(Effect::Return(id)) => self.return_to_game(&id),
            None => {}
        }
        self.show_launch();
        self.update_hints();
    }

    /// Lets the overlay linger for `after`, then tells the launch it did.
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

    /// Shows the overlay over the launched game's page: the launcher's name for a store
    /// game, "Starting…" for a game we run.
    fn show_launch(&self) {
        let (game, shown) = {
            let launch = self.imp().launch.borrow();
            (launch.game().cloned(), launch.shown())
        };
        for page in self.game_pages() {
            let launched = page
                .game()
                .zip(game.as_ref())
                .is_some_and(|(object, id)| object.id() == *id);
            let status = match shown {
                _ if !launched => None,
                Launching::No => None,
                Launching::HandingOff => Some(
                    page.game()
                        .and_then(|object| launcher_name(object.game().source()))
                        .map_or_else(
                            || gettext("Opening the launcher…"),
                            |launcher| {
                                gettext("Opening {launcher}…").replace("{launcher}", launcher)
                            },
                        ),
                ),
                Launching::Starting => Some(gettext("Starting…")),
            };
            page.show_launch(status.as_deref());
        }
    }

    /// Back to the game's page, with focus on Play, or on Stop while the game runs.
    fn return_to_game(&self, id: &GameId) {
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

    /// Forgets the launch: no overlay, no return.
    fn drop_launch(&self) {
        let imp = self.imp();
        if let Some(source) = imp.launch_linger.take() {
            source.remove();
        }
        if let Some((game, handler)) = imp.launch_game.take() {
            game.disconnect(handler);
        }
        imp.launch.take();
        self.show_launch();
    }

    /// The page of the game with `id`, the one nearest the top if it is open twice.
    fn game_page(&self, id: &GameId) -> Option<GamePage> {
        self.game_pages()
            .into_iter()
            .rev()
            .find(|page| page.game().is_some_and(|object| object.id() == *id))
    }

    fn game_pages(&self) -> Vec<GamePage> {
        self.imp()
            .navigation_view
            .navigation_stack()
            .iter::<adw::NavigationPage>()
            .flatten()
            .filter_map(|page| page.downcast::<GamePage>().ok())
            .collect()
    }
}
