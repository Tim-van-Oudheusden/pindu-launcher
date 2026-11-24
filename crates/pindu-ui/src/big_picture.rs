//! Steam Big Picture from fullscreen mode's Home: the tile shows where Steam is present,
//! and activating it hands Big Picture to Steam (`launch_service::open_big_picture`). In a
//! `gamescope --steam` session Big Picture then takes the launcher's place
//! (`GamescopeService`): the tile says it is opening until Steam's window shows, and
//! activating it again gives up. Steam keeps the controller while its window is visible or
//! a game it started is still active. The launcher takes the controller back once Steam's
//! window is gone and no game it started is active.
//!
//! The set-up checklist's Sign In and Add a Store's Open come here for Steam too. A Steam
//! that has never run sets itself up first, behind the Setting Up Steam screen
//! (`steam_setup_service`); Quick Access, where Add a Store was, stays open under the
//! screen, so Cancel returns there, and closes once Steam is ready.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::launch::StoreClient;
use pindu_core::sources::location::HostDirs;
use pindu_core::steam_setup;

use crate::application::PinduApplication;
use crate::launch_service::{self, BigPictureRoute};
use crate::steam_setup_service::SteamSetupFlow;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Adds `win.steam-big-picture`, offers Big Picture on Home where Steam is present,
    /// and follows Big Picture in the launcher's place.
    pub(crate) fn setup_big_picture(&self, application: &PinduApplication) {
        let open = gio::ActionEntry::builder("steam-big-picture")
            .activate(|window: &Self, _, _| window.open_big_picture())
            .build();
        self.add_action_entries([open]);
        application
            .gamescope_service()
            .connect_big_picture_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.big_picture_changed()
            ));
        // A sync finds or loses the Steam folder, and the games change with it.
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.update_big_picture_offered()
        ));
        self.update_big_picture_offered();
    }

    /// Whether Steam Big Picture has the controller: Steam's window or a game it started
    /// shows in the launcher's place.
    pub(crate) fn big_picture_has_controller(&self) -> bool {
        self.application()
            .and_downcast::<PinduApplication>()
            .is_some_and(|application| application.gamescope_service().big_picture_has_controller())
    }

    /// Offers Big Picture on Home once it is known whether Steam is present.
    fn update_big_picture_offered(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let window = self.downgrade();
        glib::spawn_future_local(async move {
            let library = application.library_service().clone();
            let route = launch_service::big_picture_route(application.upcast_ref(), &library).await;
            if let Some(window) = window.upgrade() {
                window
                    .imp()
                    .home_page
                    .set_big_picture_offered(route.is_some());
            }
        });
    }

    /// Hands Big Picture to Steam, behind the Setting Up Steam screen on Steam's first
    /// start, or gives up on it while it is still opening.
    fn open_big_picture(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let gamescope = application.gamescope_service();
        if gamescope.in_big_picture() {
            gamescope.end_big_picture();
            return;
        }
        if self.imp().steam_setup.borrow().is_some() {
            return;
        }
        let window = self.downgrade();
        glib::spawn_future_local(async move {
            let library = application.library_service().clone();
            let route = launch_service::big_picture_route(application.upcast_ref(), &library).await;
            let (Some(window), Some(route)) = (window.upgrade(), route) else {
                return;
            };
            match route.first_start {
                Some(client) => window.set_up_steam(route, client),
                None => {
                    window.close_panel();
                    window.hand_big_picture(&route, |_| {});
                }
            }
        });
    }

    /// Hands Big Picture to Steam along `route`, then tells `opened` whether that worked;
    /// a toast says why it did not.
    fn hand_big_picture(&self, route: &BigPictureRoute, opened: impl FnOnce(bool) + 'static) {
        let Some(application) = self.application() else {
            return;
        };
        let window = self.downgrade();
        launch_service::open_big_picture(
            &application,
            self.upcast_ref(),
            route,
            move |toast| {
                if let Some(window) = window.upgrade() {
                    window.add_toast(toast);
                }
            },
            opened,
        );
    }

    /// Shows the Setting Up Steam screen while `client`, which has never run, sets itself
    /// up, and hands Big Picture to it: again on Try Again. Steam's sign-in, or its client
    /// starting where the launcher cannot see its windows, ends the screen; Cancel returns
    /// to where the user was.
    fn set_up_steam(&self, route: BigPictureRoute, client: StoreClient) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let follows_windows = application.gamescope_service().is_active();
        let paths = steam_setup::bootstrap_logs(&HostDirs::from_env(), client);
        let window = self.downgrade();
        let hand_off = move |failed: Box<dyn FnOnce()>| {
            if let Some(window) = window.upgrade() {
                window.hand_big_picture(&route, move |opened| {
                    if !opened {
                        failed();
                    }
                });
            }
        };
        let window = self.downgrade();
        let finished = move |ready: bool| {
            let Some(window) = window.upgrade() else {
                return;
            };
            window.imp().steam_setup.take();
            if ready {
                window.close_panel();
                return;
            }
            if let Some(application) = window.application().and_downcast::<PinduApplication>() {
                application.gamescope_service().end_big_picture();
            }
        };
        let flow = SteamSetupFlow::show(self, follows_windows, paths, hand_off, finished);
        self.imp().steam_setup.replace(Some(flow.clone()));
        flow.hand_off();
    }

    /// Follows Big Picture in the launcher's place: the tile says whether it is opening,
    /// the Setting Up Steam screen learns when Steam's sign-in shows, and once Big Picture
    /// ends the controller is the window's again.
    fn big_picture_changed(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let gamescope = application.gamescope_service();
        self.imp()
            .home_page
            .set_big_picture_opening(gamescope.big_picture_opening());
        let setup = self.imp().steam_setup.borrow().clone();
        if let Some(setup) = setup {
            setup.big_picture_changed(gamescope.in_big_picture(), gamescope.big_picture_opening());
        }
        if !gamescope.in_big_picture() {
            self.give_controller_back();
        }
    }
}
