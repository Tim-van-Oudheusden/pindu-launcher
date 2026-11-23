//! The set-up checklist (`pindu_core::setup`) on fullscreen mode's Home: it starts when
//! fullscreen mode starts with no games and no store installed, shows in place of Home's
//! content until a game is in the library, and leads each step to the screen that does
//! it: Quick Access's Bluetooth or Controller section, Network and Design section, Add a
//! Store, and the installed store to sign in (`win.add-store-open`). Its state is kept in
//! GSettings (`fullscreen-setup`): hidden, Quick Access → Manage brings it back. The
//! desktop window shows nothing of it.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::add_store::StoreApp;
use pindu_core::library::View;
use pindu_core::setup::{SetupEntry, SetupFacts, SetupItem, SetupState, checklist};
use pindu_core::system::network::Availability;

use crate::add_store_service::store_name;
use crate::application::PinduApplication;
use crate::permission_service;
use crate::window::PinduWindow;

/// The GSettings key the checklist's state is kept under.
pub(crate) const STATE_KEY: &str = "fullscreen-setup";

/// The GSettings key of the fullscreen design, which counts as changed once the owner
/// picked one.
const DESIGN_KEY: &str = "fullscreen-design";

impl PinduWindow {
    /// Adds `win.setup-hide` and `win.setup-restore`, and shows the checklist as the
    /// library, the controllers, the network, the stores and the design change.
    pub(crate) fn setup_checklist(&self, application: &PinduApplication) {
        let hide = gio::ActionEntry::builder("setup-hide")
            .activate(|window: &Self, _, _| window.change_setup(SetupState::dismissed))
            .build();
        let restore = gio::ActionEntry::builder("setup-restore")
            .activate(|window: &Self, _, _| {
                window.close_panel();
                window.show_home();
                window.change_setup(SetupState::restored);
            })
            .build();
        self.add_action_entries([hide, restore]);
        let update = glib::clone!(
            #[weak(rename_to = window)]
            self,
            move || window.update_setup()
        );
        self.library().connect_counts_changed({
            let update = update.clone();
            move |_| update()
        });
        application
            .gamepad_service()
            .connect_pads_changed(update.clone());
        application.network_service().connect_changed({
            let update = update.clone();
            move |_| update()
        });
        application.bluetooth_service().connect_changed({
            let update = update.clone();
            move |_| update()
        });
        application.add_store_service().connect_changed({
            let update = update.clone();
            move |_| update()
        });
        application
            .style_service()
            .connect_design_changed(update.clone());
        self.settings().connect_changed(Some(STATE_KEY), {
            let update = update.clone();
            move |_, _| update()
        });
        gio::NetworkMonitor::default().connect_network_changed({
            let update = update.clone();
            move |_, _| update()
        });
        self.connect_fullscreened_notify(move |_| update());
        // The stores are looked for once, so the checklist knows whether one is there.
        application.add_store_service().refresh();
        self.update_setup();
    }

    /// Changes the checklist's kept state with `change`.
    fn change_setup(&self, change: fn(SetupState) -> SetupState) {
        let state = self.setup_state();
        self.keep_setup_state(state, change(state));
    }

    /// The checklist's kept state.
    fn setup_state(&self) -> SetupState {
        SetupState::from_name(&self.settings().string(STATE_KEY)).unwrap_or_default()
    }

    /// Keeps `state` when it is not `before`.
    fn keep_setup_state(&self, before: SetupState, state: SetupState) {
        if state != before
            && let Err(error) = self.settings().set_string(STATE_KEY, state.name())
        {
            glib::g_warning!(
                "pindu",
                "Could not keep the set-up checklist's state: {error}"
            );
        }
    }

    /// Brings the kept state up to date, offers it back in Quick Access while it is
    /// hidden, and shows it on Home while it is active in fullscreen mode.
    fn update_setup(&self) {
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let loaded = application.library_service().is_loaded();
        let games = self.library().counts().get(View::All) > 0;
        let stores = application.add_store_service().state();
        let before = self.setup_state();
        let mut state = before;
        if loaded {
            state = state.library_changed(games);
        }
        let imp = self.imp();
        if self.is_fullscreen() && loaded && stores.detected && !imp.setup_started.get() {
            imp.setup_started.set(true);
            state = state.started(games, !stores.installed.is_empty());
        }
        self.keep_setup_state(before, state);
        imp.quick_access_panel
            .update_shown(|shown| shown.setup = state.restorable());
        let items = (state.shows() && self.is_fullscreen()).then(|| {
            checklist(&SetupFacts {
                controller: !application.gamepad_service().pads().is_empty(),
                bluetooth: application.bluetooth_service().state().available,
                network: application.network_service().state().availability != Availability::Hidden,
                online: gio::NetworkMonitor::default().is_network_available(),
                design_changed: self.settings().user_value(DESIGN_KEY).is_some(),
                stores: stores.installed,
            })
        });
        if *imp.setup_items.borrow() == items {
            return;
        }
        let first = items
            .iter()
            .flatten()
            .position(|item| !item.done)
            .unwrap_or_default();
        let host_spawn = permission_service::host_spawn_allowed();
        let rows = items.as_ref().map(|items| {
            items
                .iter()
                .map(|item| setup_row(item, host_spawn))
                .collect()
        });
        imp.setup_items.replace(items);
        let home_shown = imp.navigation_view.visible_page().as_ref()
            == Some(imp.home_page.upcast_ref::<adw::NavigationPage>());
        if imp.home_page.set_setup(rows, first) && home_shown && self.open_panel().is_none() {
            // Once the rows are mapped, which they are not yet as the checklist shows.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || {
                    if window.open_panel().is_none() {
                        window.focus_page();
                    }
                }
            ));
        }
    }
}

/// The row for `item`: what to do and whether it is done, leading where it is done, with
/// `host_spawn` saying whether the launcher may open a store on the host.
fn setup_row(item: &SetupItem, host_spawn: bool) -> gtk::Widget {
    let (title, subtitle, icon) = text(item, host_spawn);
    let row = adw::ActionRow::builder()
        .title(title)
        .subtitle(subtitle)
        .use_markup(false)
        .build();
    let mark = gtk::Image::from_icon_name(if item.done {
        "object-select-symbolic"
    } else {
        icon
    });
    if item.done {
        mark.add_css_class("success");
        mark.update_property(&[gtk::accessible::Property::Label(&gettext("Done"))]);
    } else {
        mark.set_accessible_role(gtk::AccessibleRole::Presentation);
    }
    row.add_prefix(&mark);
    match action(item.entry, host_spawn) {
        Some((name, target)) => {
            row.set_activatable(true);
            row.set_action_name(Some(name));
            row.set_action_target_value(target.as_ref());
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
        }
        None => {
            row.set_activatable(false);
            row.set_sensitive(false);
        }
    }
    row.upcast()
}

/// The action a row runs and its target: the screen where its step is done.
fn action(entry: SetupEntry, host_spawn: bool) -> Option<(&'static str, Option<glib::Variant>)> {
    let section = |name: &str| Some(("win.quick-access-section", Some(name.to_variant())));
    match entry {
        SetupEntry::Controller { bluetooth: true } => Some(("win.quick-access-bluetooth", None)),
        SetupEntry::Controller { bluetooth: false } => section("controller"),
        SetupEntry::Network => Some(("win.quick-access-network", None)),
        SetupEntry::Design => section("design"),
        SetupEntry::Store => Some(("win.quick-access-add-store", None)),
        // Steam opens through Big Picture's route; Heroic and Lutris need host spawn.
        SetupEntry::SignIn(Some(store)) if store == StoreApp::Steam || host_spawn => {
            Some(("win.add-store-open", Some(store.app_id().to_variant())))
        }
        SetupEntry::SignIn(_) => None,
    }
}

/// A row's title, its line under it, and its icon while it is not done.
fn text(item: &SetupItem, host_spawn: bool) -> (String, String, &'static str) {
    let done = item.done;
    match item.entry {
        SetupEntry::Controller { bluetooth } => (
            gettext("Connect a Controller"),
            if done {
                gettext("A controller is connected")
            } else if bluetooth {
                gettext("Plug one in with a cable or use a keyboard, then pair a wireless one here")
            } else {
                gettext("Plug one in with a cable, or use a keyboard")
            },
            "input-gaming-symbolic",
        ),
        SetupEntry::Network => (
            gettext("Join a Network"),
            if done {
                gettext("Online")
            } else {
                gettext("Stores and games download from the internet")
            },
            "network-wireless-symbolic",
        ),
        SetupEntry::Design => (
            gettext("Choose a Design"),
            gettext("Pick how fullscreen mode looks"),
            "applications-graphics-symbolic",
        ),
        SetupEntry::Store => (
            gettext("Add a Store"),
            if done {
                gettext("A store is installed")
            } else {
                gettext("Install Steam, Heroic or Lutris")
            },
            "system-software-install-symbolic",
        ),
        SetupEntry::SignIn(store) => (
            match store {
                // Translators: {store} is a store's name, such as Steam.
                Some(store) => gettext("Sign In to {store}").replace("{store}", store_name(store)),
                None => gettext("Sign In to Your Store"),
            },
            match store {
                None => gettext("Add a store first"),
                // Translators: {store} is a store's name, such as Steam.
                Some(store) if action(item.entry, host_spawn).is_some() => {
                    gettext("Open {store} and sign in. Its games then show here.")
                        .replace("{store}", store_name(store))
                }
                // Translators: {store} is a store's name, such as Heroic.
                Some(store) => {
                    gettext("Open {store} on a desktop and sign in. Its games then show here.")
                        .replace("{store}", store_name(store))
                }
            },
            "avatar-default-symbolic",
        ),
    }
}
