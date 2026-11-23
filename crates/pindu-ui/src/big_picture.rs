//! Steam Big Picture from fullscreen mode's Home: the tile shows where Steam is present,
//! installed or with a folder the library came from, and activating it hands Big Picture
//! to Steam (`launch_service::open_big_picture`). Add a Store's Open Steam and the Steam
//! Installed notification activate it too, so a Steam that has not run yet opens for its
//! first sign-in. In a `gamescope --steam` session Big Picture then takes the launcher's
//! place (`GamescopeService`): the tile says it is opening until Steam's window shows, and
//! activating it again gives up. Steam keeps the controller while its window is visible or
//! a game it started is still active. The launcher takes the controller back once Steam's
//! window is gone and no game it started is active.

use std::cell::Cell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gtk::{gio, glib};
use pindu_core::add_store::StoreApp;

use crate::application::PinduApplication;
use crate::launch_service;
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
        // Add a Store installs Steam, or finds it installed or gone.
        let steam_installed = Cell::new(false);
        application
            .add_store_service()
            .connect_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |state| {
                    let installed = state.installed.contains(&StoreApp::Steam);
                    if steam_installed.replace(installed) != installed {
                        window.update_big_picture_offered();
                    }
                }
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

    /// Hands Big Picture to Steam, or gives up on it while it is still opening. A panel
    /// it was activated from, such as Quick Access, closes first.
    fn open_big_picture(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        if self.is_fullscreen() {
            self.close_panel();
        }
        let gamescope = application.gamescope_service();
        if gamescope.in_big_picture() {
            gamescope.end_big_picture();
            return;
        }
        let window = self.downgrade();
        launch_service::open_big_picture(
            application.upcast_ref(),
            self.upcast_ref(),
            application.library_service(),
            move |toast| {
                if let Some(window) = window.upgrade() {
                    window.add_toast(toast);
                }
            },
        );
    }

    /// Follows Big Picture in the launcher's place: the tile says whether it is opening,
    /// and once it ends the controller is the window's again.
    fn big_picture_changed(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let gamescope = application.gamescope_service();
        self.imp()
            .home_page
            .set_big_picture_opening(gamescope.big_picture_opening());
        if !gamescope.in_big_picture() {
            self.give_controller_back();
        }
    }
}
