//! Get Games in fullscreen mode (#887; `docs/get-games.md` section 8.1): Home's Get Games
//! tile opens the catalogue page above Home, an app's card opens its page above that, and
//! B goes back down. The pages follow `GetGamesService`: the catalogue as it loads and
//! refreshes, the installed marks, and the job each app has. Y searches with the search
//! keyboard, the shoulder buttons jump between shelves or turn an app's screenshots, the
//! main button installs, cancels, plays or opens, and Remove asks before it removes. A
//! library game Get Games can remove (#888) offers Uninstall on its page, which asks the
//! same. In the desktop window (#889, section 8.2) the sidebar's Get Games item shows the
//! same catalogue in the library's place, for the mouse. The native build only: the
//! Flatpak build has no Get Games tile or item.

use std::collections::HashSet;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamepad::Intent;
use pindu_core::get_games::{Refusal, removable_app};
use pindu_core::hints::Shoulders;
use pindu_core::model::{GameId, Source};

use crate::application::PinduApplication;
use crate::get_games_app_page::{AppView, GetGamesAppPage, MainAction};
use crate::get_games_page::{GetGamesPage, View};
use crate::get_games_service::{GetGamesService, store};
use crate::launch_service;
use crate::window::PinduWindow;

impl PinduWindow {
    /// Home's Get Games tile where the build offers it, the pages' actions, and the pages
    /// kept up to date with the service.
    pub(crate) fn setup_get_games(&self, application: &PinduApplication) {
        let imp = self.imp();
        let available = GetGamesService::available();
        imp.home_page.set_get_games_offered(available);
        // The desktop sidebar's item, in the native build only.
        imp.get_games_item.set_visible(available);
        let open_app = gio::ActionEntry::builder("get-games-app")
            .parameter_type(Some(glib::VariantTy::STRING))
            .activate(|window: &Self, _, target| {
                if let Some(id) = target.and_then(|target| target.str()) {
                    window.show_get_games_app(id);
                }
            })
            .build();
        let main = gio::ActionEntry::builder("get-games-main")
            .activate(|window: &Self, _, _| window.get_games_main())
            .build();
        let remove = gio::ActionEntry::builder("get-games-remove")
            .activate(|window: &Self, _, _| window.ask_get_games_remove())
            .build();
        let refresh = gio::ActionEntry::builder("get-games-refresh")
            .activate(|window: &Self, _, _| {
                if let Some(service) = window.get_games_service() {
                    service.refresh();
                }
            })
            .build();
        let screenshot = gio::ActionEntry::builder("get-games-screenshot")
            .parameter_type(Some(glib::VariantTy::INT32))
            .activate(|window: &Self, _, target| {
                let by = target.and_then(glib::Variant::get::<i32>).unwrap_or(1);
                if let Some(page) = window.visible_get_games_app_page() {
                    page.turn_screenshot(by);
                }
            })
            .build();
        self.add_action_entries([open_app, main, remove, refresh, screenshot]);
        self.add_game_action("uninstall-game", Self::ask_uninstall_game);
        application
            .get_games_service()
            .connect_changed(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.refresh_get_games()
            ));
        // Play follows the library, which a sync fills, and Open follows Apps.
        self.library().model().connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.refresh_get_games_app()
        ));
        application.apps_service().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.refresh_get_games_app()
        ));
        // The pages show their header bars, and their back buttons, out of fullscreen mode.
        self.connect_fullscreened_notify(|window| {
            let fullscreen = window.is_fullscreen();
            for page in window
                .imp()
                .navigation_view
                .navigation_stack()
                .iter::<adw::NavigationPage>()
                .flatten()
            {
                if let Some(page) = page.downcast_ref::<GetGamesPage>() {
                    page.set_fullscreen(fullscreen);
                } else if let Some(page) = page.downcast_ref::<GetGamesAppPage>() {
                    page.set_fullscreen(fullscreen);
                }
            }
        });
    }

    fn get_games_service(&self) -> Option<GetGamesService> {
        self.application()
            .and_downcast::<PinduApplication>()
            .map(|application| application.get_games_service().clone())
            .filter(|_| GetGamesService::available())
    }

    /// The catalogue page, made the first time it is asked for.
    fn get_games_page(&self) -> GetGamesPage {
        let imp = self.imp();
        if let Some(page) = imp.get_games_page.borrow().as_ref() {
            return page.clone();
        }
        let page = GetGamesPage::new();
        page.search_entry().connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.refresh_get_games()
        ));
        // B's hint turns from Delete into Close as the field empties, however it does.
        page.search_entry().connect_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                if window.imp().search_keyboard.get().is_some() {
                    window.update_hints();
                }
            }
        ));
        // On the desktop, its header bar's toggle shows and hides the sidebar.
        imp.split_view
            .bind_property("show-sidebar", &page.sidebar_toggle(), "active")
            .bidirectional()
            .sync_create()
            .build();
        imp.get_games_page.replace(Some(page.clone()));
        page
    }

    /// Shows the catalogue, reading it the first time: in fullscreen mode above Home,
    /// focused on its first card; on the desktop in the library's place, as the sidebar's
    /// Get Games item does.
    pub(crate) fn show_get_games(&self) {
        let Some(service) = self.get_games_service() else {
            return;
        };
        service.open();
        let imp = self.imp();
        let page = self.get_games_page();
        page.set_fullscreen(self.is_fullscreen());
        if self.is_fullscreen() {
            imp.navigation_view.pop_to_page(&*imp.home_page);
            imp.navigation_view.push(&page);
            self.refresh_get_games();
            page.focus_first_when_ready();
            return;
        }
        imp.navigation_view
            .replace(&[page.upcast::<adw::NavigationPage>()]);
        imp.sidebar.set_selected(imp.get_games_item.index());
        if imp.split_view.is_collapsed() {
            imp.split_view.set_show_sidebar(false);
        }
        self.refresh_get_games();
    }

    /// Shows the page of the app `id` above the catalogue, and asks Flathub for its sizes.
    fn show_get_games_app(&self, id: &str) {
        let Some(service) = self.get_games_service() else {
            return;
        };
        if service.catalogue().offer.get(id).is_none() {
            return;
        }
        if let Some(catalogue) = self.imp().get_games_page.borrow().as_ref() {
            catalogue.remember_app(id);
        }
        let page = GetGamesAppPage::new(id);
        page.set_fullscreen(self.is_fullscreen());
        // Filled first, so the page has its title as it goes in.
        self.fill_get_games_app(&page);
        self.imp().navigation_view.push(&page);
        page.focus_main();
        service.fetch_sizes(id);
    }

    fn visible_get_games_page(&self) -> Option<GetGamesPage> {
        self.imp()
            .navigation_view
            .visible_page()
            .and_downcast::<GetGamesPage>()
    }

    fn visible_get_games_app_page(&self) -> Option<GetGamesAppPage> {
        self.imp()
            .navigation_view
            .visible_page()
            .and_downcast::<GetGamesAppPage>()
    }

    /// The catalogue page and the app page that shows, as the service has them now.
    fn refresh_get_games(&self) {
        let Some(service) = self.get_games_service() else {
            return;
        };
        let catalogue = service.catalogue();
        let installed = service.installed();
        let busy = busy(&service);
        if let Some(page) = self.imp().get_games_page.borrow().as_ref() {
            page.show(View {
                status: service.status(),
                broken: catalogue.broken.is_some(),
                loading: catalogue.loading,
                offer: &catalogue.offer,
                folder: catalogue.folder.as_deref(),
                installed: &installed,
                busy: &busy,
            });
        }
        self.refresh_get_games_app();
        self.update_hints();
    }

    /// The app page that shows, as the service, the library and Apps have it now.
    fn refresh_get_games_app(&self) {
        if let Some(page) = self.visible_get_games_app_page() {
            self.fill_get_games_app(&page);
        }
    }

    /// Shows on `page` its app as the service, the library and Apps have it now.
    fn fill_get_games_app(&self, page: &GetGamesAppPage) {
        let Some(service) = self.get_games_service() else {
            return;
        };
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let id = page.app_id();
        let catalogue = service.catalogue();
        let Some(app) = catalogue.offer.get(&id) else {
            return;
        };
        let jobs = service.jobs_state().unwrap_or_default();
        let game = app
            .desktop_id
            .as_deref()
            .map(|desktop_id| GameId::new(Source::Desktop, desktop_id))
            .filter(|game| self.library().game_object(game).is_some());
        let opens = app
            .desktop_id
            .as_deref()
            .filter(|desktop_id| application.apps_service().app(desktop_id).is_some());
        page.show(AppView {
            app,
            folder: catalogue.folder.as_deref(),
            installed: service.installed().contains(&id),
            extra: catalogue.offer.is_extra(&id),
            store: store(&id),
            game: game.as_ref(),
            opens,
            running: jobs
                .running
                .as_ref()
                .filter(|running| running.job.app_id() == Some(id.as_str())),
            queued: jobs
                .queue
                .iter()
                .any(|job| job.app_id() == Some(id.as_str())),
            outcome: jobs.outcomes.get(&id),
            sizes: jobs.sizes.get(&id).copied(),
        });
    }

    /// The main button of the app page that shows: install, cancel, play or open.
    fn get_games_main(&self) {
        let (Some(page), Some(service)) =
            (self.visible_get_games_app_page(), self.get_games_service())
        else {
            return;
        };
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let id = page.app_id();
        match page.main_action() {
            MainAction::Install => match service.install(&id) {
                Ok(()) | Err(Refusal::Busy) => {}
                Err(Refusal::Store(store)) => application.add_store_service().choose(store),
                Err(refusal) => {
                    glib::g_warning!("pindu", "Get Games did not install {id}: {refusal:?}");
                }
            },
            MainAction::AddStore(store) => application.add_store_service().choose(store),
            MainAction::Cancel => service.cancel(&id),
            MainAction::Play(game) => {
                WidgetExt::activate_action(self, "app.play", Some(&game.to_string().to_variant()))
                    .ok();
            }
            MainAction::Open(desktop_id) => {
                if let Some(entry) = application.apps_service().app(&desktop_id) {
                    let window = self.downgrade();
                    launch_service::open_app(application.upcast_ref(), &entry, move |toast| {
                        if let Some(window) = window.upgrade() {
                            window.add_toast(toast);
                        }
                    });
                }
            }
            MainAction::Nothing => {}
        }
        self.refresh_get_games_app();
    }

    /// Asks before removing the app the page shows, offering to delete its data too.
    fn ask_get_games_remove(&self) {
        let (Some(page), Some(service)) =
            (self.visible_get_games_app_page(), self.get_games_service())
        else {
            return;
        };
        let id = page.app_id();
        let Some(app) = service.catalogue().offer.get(&id).cloned() else {
            return;
        };
        self.ask_remove_app(&id, &app.name, page.upcast_ref());
    }

    /// Asks before removing the app `id`, called `name`, with a switch for its data, over
    /// `parent`; removes it when the user says so.
    pub(crate) fn ask_remove_app(&self, id: &str, name: &str, parent: &gtk::Widget) {
        let delete_data = adw::SwitchRow::builder()
            .title(gettext("Delete Its Data"))
            .subtitle(gettext("Saves and settings in your home folder go too"))
            .build();
        let list = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["boxed-list"])
            .build();
        list.append(&delete_data);
        let dialog = adw::AlertDialog::builder()
            // Translators: {name} is an app's name, such as SuperTuxKart.
            .heading(gettext("Remove {name}?").replace("{name}", name))
            .body(gettext(
                "It goes from this console. Get Games can install it again.",
            ))
            .extra_child(&list)
            .close_response("cancel")
            .default_response("cancel")
            .build();
        dialog.add_responses(&[
            ("cancel", &gettext("Cancel")),
            ("remove", &gettext("Remove")),
        ]);
        dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
        let id = id.to_owned();
        let name = name.to_owned();
        dialog.connect_response(
            Some("remove"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| {
                    if let Some(service) = window.get_games_service()
                        && let Err(refusal) = service.remove(&id, &name, delete_data.is_active())
                    {
                        glib::g_warning!("pindu", "Get Games did not remove {id}: {refusal:?}");
                    }
                    window.refresh_get_games_app();
                }
            ),
        );
        dialog.present(Some(parent));
    }

    /// Hands the shoulder buttons to Get Games' pages: they jump between the catalogue's
    /// shelves, or turn an app's screenshots. `false` for any other intent or page.
    pub(crate) fn get_games_intent(&self, intent: Intent) -> bool {
        let forward = match intent {
            Intent::NextTab => true,
            Intent::PreviousTab => false,
            _ => return false,
        };
        if self.visible_dialog().is_some() || self.open_panel().is_some() {
            return false;
        }
        if let Some(page) = self.visible_get_games_page() {
            page.jump_shelf(forward);
            return true;
        }
        if let Some(page) = self.visible_get_games_app_page() {
            page.turn_screenshot(if forward { 1 } else { -1 });
            return true;
        }
        false
    }

    /// What the shoulder buttons do on the page that shows, for the hints.
    pub(crate) fn get_games_shoulders(&self) -> Option<Shoulders> {
        if self
            .visible_get_games_page()
            .is_some_and(|page| page.has_shelves())
        {
            return Some(Shoulders::Shelves);
        }
        self.visible_get_games_app_page()
            .filter(GetGamesAppPage::has_screenshots)
            .map(|_| Shoulders::Screenshots)
    }

    /// The search field Y types into on the page that shows: the catalogue's, or `None`
    /// on any other page.
    pub(crate) fn get_games_search_entry(&self) -> Option<gtk::SearchEntry> {
        self.visible_get_games_page()
            .map(|page| page.search_entry())
    }

    /// Focuses the page that shows, when it is one of Get Games'; `false` otherwise.
    pub(crate) fn focus_get_games(&self) -> bool {
        if let Some(page) = self.visible_get_games_page() {
            page.focus_first_when_ready();
            return true;
        }
        if let Some(page) = self.visible_get_games_app_page() {
            return page.focus_main();
        }
        false
    }

    /// The app Get Games can remove for the library game `id`: the Flatpak, in the
    /// installation Get Games uses, whose desktop entry it is.
    pub(crate) fn removable_app(&self, id: &GameId) -> Option<String> {
        let service = self.get_games_service()?;
        removable_app(id, &service.installed()).map(str::to_owned)
    }

    /// Asks before removing the Flatpak of the library game `id`, as Get Games' Remove does.
    fn ask_uninstall_game(&self, id: &GameId) {
        let (Some(app), Some(service)) = (self.removable_app(id), self.get_games_service()) else {
            return;
        };
        let name = service
            .catalogue()
            .offer
            .get(&app)
            .map(|offered| offered.name.clone())
            .or_else(|| {
                self.library()
                    .game_object(id)
                    .map(|game| game.game().title.clone())
            })
            .unwrap_or_else(|| app.clone());
        self.ask_remove_app(&app, &name, self.upcast_ref());
    }
}

/// The apps a job is queued or runs for.
fn busy(service: &GetGamesService) -> HashSet<String> {
    service
        .jobs_state()
        .map(|state| {
            state
                .running
                .iter()
                .map(|running| &running.job)
                .chain(&state.queue)
                .filter_map(|job| job.app_id().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}
