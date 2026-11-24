//! Speaks gamescope's `--steam` focus protocol, where the launcher takes Steam's place: its
//! main window is app 769, it writes the base layer list on the root, tags the windows of
//! the games it started with their app IDs, and reads back who is in front. With a game in
//! front it opens Quick Access over the game in its overlay window, which it marks
//! `STEAM_OVERLAY` and opens and closes by opacity and `STEAM_INPUT_FOCUS`. Steam Big
//! Picture opened from Home takes the launcher's place until its window closes, or until
//! the user leaves it with Guide + Y or the `leave-big-picture` action, and the overlay
//! window says how once it shows (`pindu_core::gamescope::BigPicture`). The decisions are
//! `pindu_core::gamescope`'s; this is the only module that touches X11, on GDK's own
//! connection.
//!
//! The service is inert unless the display is X11 and gamescope runs with `--steam`: on a
//! Wayland desktop and in a plain gamescope it writes nothing.
//!
//! In such a gamescope with a writable `MANGOHUD_CONFIGFILE`, it also drives the session's
//! MangoApp from the Performance Overlay switch of the game in front
//! (`pindu_core::gamescope::mangoapp`).

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ffi::{c_int, c_long, c_uchar, c_uint, c_ulong};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Instant;

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::Intent;
use pindu_core::gamescope::overlay::{self, OverGame};
use pindu_core::gamescope::{
    self, BaseLayerClaim, BigPicture, EXIT_GRACE, ExpectedGame, Front, InGameIntent, Leave,
    PickedWindow, Reclaims, STEAM_APP_ID, StoreExit, StoreLaunch, WindowFacts, Xid, mangoapp,
};
use pindu_core::model::{GameId, LaunchConfig, Timestamp};

use crate::gamepad_service::GamepadService;
use crate::session_service::{self, SessionService};

/// gamescope's property that says it runs with `--steam`; the app ID with input focus.
const FOCUSED_APP: &str = "GAMESCOPE_FOCUSED_APP";
/// gamescope's property with the app ID of the base layer: who is in front.
const FOCUSED_APP_GFX: &str = "GAMESCOPE_FOCUSED_APP_GFX";
/// gamescope's property with the app IDs of the windows it could show.
const FOCUSABLE_APPS: &str = "GAMESCOPE_FOCUSABLE_APPS";
/// gamescope's property with the windows it could show: window, app ID and process.
const FOCUSABLE_WINDOWS: &str = "GAMESCOPE_FOCUSABLE_WINDOWS";
/// The root's focus priority list, which Steam writes and the launcher writes in its place.
const BASELAYER_APPID: &str = "GAMESCOPECTRL_BASELAYER_APPID";
/// The root's pinned base layer window, which takes precedence over the list.
const BASELAYER_WINDOW: &str = "GAMESCOPECTRL_BASELAYER_WINDOW";
/// A window's app ID.
const STEAM_GAME: &str = "STEAM_GAME";
/// The process ID a client gives its window.
const NET_WM_PID: &str = "_NET_WM_PID";
/// A window's instance and class names.
const WM_CLASS: &str = "WM_CLASS";
/// A window's title in UTF-8.
const NET_WM_NAME: &str = "_NET_WM_NAME";
/// A window's title, the older way.
const WM_NAME: &str = "WM_NAME";
/// The protocols a window takes part in, such as being asked to close.
const WM_PROTOCOLS: &str = "WM_PROTOCOLS";
/// The request to close a window, as its close button would.
const WM_DELETE_WINDOW: &str = "WM_DELETE_WINDOW";
/// Marks a window as an overlay, which is never a base layer.
const STEAM_OVERLAY: &str = "STEAM_OVERLAY";
/// Whether an overlay holds the keyboard and pointer.
const STEAM_INPUT_FOCUS: &str = "STEAM_INPUT_FOCUS";
/// A window's opacity; gamescope leaves an overlay at 0 out of composition.
const NET_WM_WINDOW_OPACITY: &str = "_NET_WM_WINDOW_OPACITY";

/// How many items a property read takes at most: a focus list or a window class is far
/// shorter.
const MAX_ITEMS: c_long = 1024;

/// Something the shells do when who is in front changes.
type FrontListener = Box<dyn Fn(Front)>;
/// Something the overlay and its panels do when the menu drawer or Quick Access opens or
/// closes over the game.
type OverGameListener = Box<dyn Fn(Option<OverGame>)>;
/// Something the shells do when the store-launched game that plays changes.
type StorePlayingListener = Box<dyn Fn()>;
/// Something the shells do when Steam Big Picture opens, shows or ends.
type BigPictureListener = Box<dyn Fn()>;
/// Something the shells do when the store launch ends.
type StoreLaunchListener = Box<dyn Fn()>;

/// gamescope's `--steam` protocol for the launcher, or nothing outside such a gamescope.
#[derive(Clone)]
pub struct GamescopeService {
    inner: Rc<Inner>,
}

impl std::fmt::Debug for GamescopeService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamescopeService")
            .field("active", &self.inner.x11.is_some())
            .field("front", &self.inner.front.get())
            .finish_non_exhaustive()
    }
}

struct Inner {
    /// The X11 side; `None` keeps the service inert.
    x11: Option<X11>,
    sessions: SessionService,
    /// The games we started that run, the first started first, with their app IDs.
    running: RefCell<Vec<(GameId, u32)>>,
    /// The store launch expected in front, when there is one.
    store_launch: RefCell<Option<StoreLaunch>>,
    /// The timer that looks at the store launch again at its deadline, or when a Steam
    /// client's window it waits on shows.
    store_timer: RefCell<Option<glib::SourceId>>,
    /// Called when the store launch ends.
    store_launch_listeners: RefCell<Vec<StoreLaunchListener>>,
    /// The store-launched game that plays, and since when the launcher has seen it.
    store_playing: RefCell<Option<(GameId, Timestamp)>>,
    store_playing_listeners: RefCell<Vec<StorePlayingListener>>,
    /// Who the launcher asked gamescope to show.
    wanted: Cell<Front>,
    /// Who gamescope shows, as `GAMESCOPE_FOCUSED_APP_GFX` says.
    front: Cell<Front>,
    front_listeners: RefCell<Vec<FrontListener>>,
    /// The window the menu drawer and Quick Access open in over a game, once there is one.
    overlay: glib::WeakRef<gtk::Window>,
    /// What the overlay window shows over the game.
    over_game: Cell<Option<OverGame>>,
    over_game_listeners: RefCell<Vec<OverGameListener>>,
    /// The session's MangoApp config file, when the launcher drives it.
    mangoapp: Option<PathBuf>,
    /// Counts the Performance Overlay writes asked for, so a game's switch read late
    /// never overwrites a newer one.
    overlay_writes: Cell<u64>,
    /// The games whose last start was a safe one, which MangoApp shows no Performance
    /// Overlay over, whatever their switch says.
    started_safely: RefCell<HashSet<GameId>>,
    /// The games whose launch is recorded, each with whether a window of it has appeared
    /// since (see [`GamescopeService::watch_windows`]).
    windows_seen: RefCell<HashMap<GameId, bool>>,
    /// The launcher's main window, which the claim pins, once it is realized.
    main_window: Cell<Option<Xid>>,
    /// The app IDs of the windows gamescope could show (`GAMESCOPE_FOCUSABLE_APPS`).
    focusable: RefCell<Vec<u32>>,
    /// When the launcher wrote its claim back after another client changed it.
    reclaims: RefCell<Reclaims>,
    /// The window of the game in front the user picked from the menu drawer over it.
    picked: Cell<Option<PickedWindow>>,
    /// Steam Big Picture, opened from Home, while it is in the launcher's place.
    big_picture: RefCell<Option<BigPicture>>,
    /// The timer that looks at Big Picture again at its deadline.
    big_picture_timer: RefCell<Option<glib::SourceId>>,
    big_picture_listeners: RefCell<Vec<BigPictureListener>>,
    /// Whether the overlay window says how to leave Steam Big Picture.
    notice: Cell<bool>,
    /// The timer that hides the notice of how to leave Steam Big Picture.
    notice_timer: RefCell<Option<glib::SourceId>>,
}

impl GamescopeService {
    /// A service for the default display that follows the games `sessions` runs and reads
    /// Guide and the Quick Access of Guide + A or Misc1 from `gamepad`'s intents; inert
    /// unless the display is X11 and gamescope runs with `--steam`.
    pub fn new(sessions: &SessionService, gamepad: &GamepadService) -> Self {
        let x11 = X11::steam_mode_display();
        let mangoapp = mangoapp::session_config_file(
            x11.is_some(),
            std::env::var_os("MANGOHUD_CONFIGFILE").as_deref(),
        );
        let service = Self {
            inner: Rc::new(Inner {
                x11,
                sessions: sessions.clone(),
                running: RefCell::default(),
                store_launch: RefCell::default(),
                store_timer: RefCell::default(),
                store_launch_listeners: RefCell::default(),
                store_playing: RefCell::default(),
                store_playing_listeners: RefCell::default(),
                wanted: Cell::new(Front::Launcher),
                front: Cell::new(Front::Launcher),
                front_listeners: RefCell::default(),
                overlay: glib::WeakRef::new(),
                over_game: Cell::default(),
                over_game_listeners: RefCell::default(),
                mangoapp,
                overlay_writes: Cell::default(),
                started_safely: RefCell::default(),
                windows_seen: RefCell::default(),
                main_window: Cell::default(),
                focusable: RefCell::default(),
                reclaims: RefCell::default(),
                picked: Cell::default(),
                big_picture: RefCell::default(),
                big_picture_timer: RefCell::default(),
                big_picture_listeners: RefCell::default(),
                notice: Cell::default(),
                notice_timer: RefCell::default(),
            }),
        };
        let Some(x11) = &service.inner.x11 else {
            return service;
        };
        glib::g_info!(
            "pindu",
            "gamescope runs with --steam: the launcher takes Steam's place"
        );
        if let Some(path) = &service.inner.mangoapp {
            glib::g_info!(
                "pindu",
                "gamescope: the Performance Overlay drives MangoApp through {}",
                path.display()
            );
        }
        let focusable = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        service.inner.focusable.replace(focusable);
        x11.watch_root(Rc::downgrade(&service.inner));
        let weak = Rc::downgrade(&service.inner);
        sessions.connect_sessions_changed(move || {
            if let Some(inner) = weak.upgrade() {
                GamescopeService { inner }.sessions_changed();
            }
        });
        let weak = Rc::downgrade(&service.inner);
        // Intents rather than buttons, so Guide acts once let go alone and Guide + A
        // toggles Quick Access, as in the launcher's own window.
        gamepad.connect_intent(move |intent| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let service = GamescopeService { inner };
            // Guide + Y, which Steam leaves alone, is the way back while Steam has it.
            if intent == Intent::LeaveBigPicture {
                service.leave_big_picture();
                return;
            }
            let Some(intent) = gamescope::in_game_intent(intent) else {
                return;
            };
            // Steam Big Picture has the controller, Guide included.
            if service.big_picture_has_controller() {
                return;
            }
            if intent == InGameIntent::ToggleMenu && service.steam_window().is_some() {
                // The user leaves the Steam client's window: the launch is given up on.
                service.end_store_launch();
                return;
            }
            if !matches!(service.inner.front.get(), Front::Game(_)) {
                return;
            }
            service.set_over_game(overlay::shown_after(service.over_game(), intent));
        });
        service.write_base_layer();
        service.read_front();
        service
    }

    /// Tags `window`, the launcher's main window, as the launcher (app 769) once it is
    /// realized, so gamescope shows it, and pins it whenever the launcher should be in
    /// front, so a Steam client's own 769 windows do not cover it.
    pub fn attach_window(&self, window: &impl IsA<gtk::Window>) {
        if self.inner.x11.is_none() {
            return;
        }
        let weak = Rc::downgrade(&self.inner);
        let tag = move |window: &gtk::Window| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let surface = window.surface().and_downcast::<X11Surface>();
            let xid = match (&inner.x11, surface) {
                (Some(x11), Some(surface)) => {
                    x11.write_cardinals(surface.xid(), STEAM_GAME, &[STEAM_APP_ID]);
                    surface.xid()
                }
                _ => return,
            };
            inner.main_window.set(Some(xid));
            GamescopeService { inner }.write_base_layer();
        };
        let window = window.upcast_ref::<gtk::Window>();
        if window.is_realized() {
            tag(window);
        }
        window.connect_realize(move |window| tag(window));
    }

    /// Whether gamescope runs with `--steam`, so the service speaks its protocol.
    pub fn is_active(&self) -> bool {
        self.inner.x11.is_some()
    }

    /// Makes `window` the overlay the menu drawer and Quick Access open in over a game:
    /// once it is realized it is marked `STEAM_OVERLAY` and closed, see-through and leaving
    /// input to the game, and Guide, Guide + A or the Steam Deck's "…" opens it while a
    /// game is in front. It never gets an app ID, so the game stays the base layer while it
    /// is open.
    pub fn attach_overlay(&self, window: &impl IsA<gtk::Window>) {
        if self.inner.x11.is_none() {
            return;
        }
        let window = window.upcast_ref::<gtk::Window>();
        self.inner.overlay.set(Some(window));
        let weak = Rc::downgrade(&self.inner);
        window.connect_realize(move |_| {
            if let Some(inner) = weak.upgrade() {
                GamescopeService { inner }.write_overlay_window();
            }
        });
        if window.is_realized() {
            self.write_overlay_window();
        }
    }

    /// What the overlay window shows over the game in front: the menu drawer, Quick Access,
    /// or nothing.
    pub fn over_game(&self) -> Option<OverGame> {
        self.inner.over_game.get()
    }

    /// Whether Quick Access is open over the game in front.
    pub fn quick_access_open(&self) -> bool {
        self.over_game() == Some(OverGame::QuickAccess)
    }

    /// Opens the menu drawer or Quick Access over the game in front, or closes the one
    /// open (`None`). Either opens only while a game is in front and the overlay window is
    /// there.
    pub fn set_over_game(&self, shown: Option<OverGame>) {
        let shown = shown.filter(|_| {
            matches!(self.inner.front.get(), Front::Game(_))
                && self.inner.overlay.upgrade().is_some()
        });
        if self.inner.over_game.replace(shown) == shown {
            return;
        }
        glib::g_debug!("pindu", "gamescope: over the game: {shown:?}");
        self.write_overlay_window();
        for listener in self.inner.over_game_listeners.borrow().iter() {
            listener(shown);
        }
    }

    /// Calls `listener` whenever the menu drawer or Quick Access opens or closes over the
    /// game.
    pub fn connect_over_game_changed(&self, listener: impl Fn(Option<OverGame>) + 'static) {
        self.inner
            .over_game_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// The game we started that gamescope shows, while one is in front.
    pub fn game_in_front(&self) -> Option<GameId> {
        let Front::Game(app_id) = self.inner.front.get() else {
            return None;
        };
        self.inner
            .running
            .borrow()
            .iter()
            .find(|&&(_, id)| id == app_id)
            .map(|(game, _)| game.clone())
    }

    /// Brings the launcher to the front for what an entry of the menu drawer or a link in
    /// Quick Access over the game leads to, closing the panel there.
    pub fn show_launcher(&self) {
        self.set_over_game(None);
        self.ask_for(Front::Launcher);
    }

    /// Who gamescope shows: the launcher, or a game we started or handed to its store.
    pub fn front(&self) -> Front {
        self.inner.front.get()
    }

    /// Who the launcher last asked gamescope to show: a game it just started, the launcher
    /// once the game in front ends or the drawer over the game leads to it, or a game after
    /// its Return.
    pub fn wanted_front(&self) -> Front {
        self.inner.wanted.get()
    }

    /// Calls `listener` whenever who gamescope shows changes.
    pub fn connect_front_changed(&self, listener: impl Fn(Front) + 'static) {
        self.inner
            .front_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Brings the game with `id` to the front, as Return in Playing Now does, while it
    /// runs: a game we started, or the store-launched game that plays.
    pub fn bring_to_front(&self, id: &GameId) {
        let app_id = self
            .inner
            .running
            .borrow()
            .iter()
            .find(|(game, _)| game == id)
            .map(|&(_, app_id)| app_id)
            .or_else(|| {
                self.inner
                    .store_launch
                    .borrow()
                    .as_ref()
                    .filter(|launch| launch.is_playing(Instant::now()) && launch.game() == id)
                    .map(StoreLaunch::app_id)
            });
        if let Some(app_id) = app_id {
            self.ask_for(Front::Game(app_id));
        }
    }

    /// The store-launched game that plays, with when the launcher first saw its window:
    /// its store launcher runs it, so Stop closes its windows (see
    /// [`Self::exit_store_game`]).
    pub fn store_playing(&self) -> Option<(GameId, Timestamp)> {
        self.inner.store_playing.borrow().clone()
    }

    /// Calls `listener` whenever the store-launched game that plays changes.
    pub fn connect_store_playing_changed(&self, listener: impl Fn() + 'static) {
        self.inner
            .store_playing_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Calls `listener` whenever the store launch ends: over, or left by the user from the
    /// Steam client's window it waits on.
    pub fn connect_store_launch_ended(&self, listener: impl Fn() + 'static) {
        self.inner
            .store_launch_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Gives up on the store launch of the game with `id` while it waits for the game, as
    /// Cancel on the starting screen: the launcher comes back to the front and the Steam
    /// client's windows stay behind it. The launch itself stays until its time-out, as the
    /// store launcher may still start the game: its window then plays, behind the
    /// launcher, with Return and Stop. A game its store launcher already shows is left
    /// alone; Stop exits it.
    pub fn give_up_store_launch(&self, id: &GameId) {
        let now = Instant::now();
        let given_up = self
            .inner
            .store_launch
            .borrow_mut()
            .as_mut()
            .filter(|launch| launch.game() == id && !launch.is_playing(now))
            .map(|launch| launch.give_up(now))
            .is_some();
        if given_up {
            glib::g_info!("pindu", "gamescope: giving up on the store launch of {id}");
            self.schedule_store_check();
            self.ask_for(Front::Launcher);
            self.read_front();
        }
    }

    /// Follows the store launch into the store-launched game that plays: its game's
    /// Playing state, and the listeners, when it changed.
    fn update_store_playing(&self) {
        let playing = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .filter(|launch| launch.is_playing(Instant::now()))
            .map(|launch| launch.game().clone());
        let before = self.inner.store_playing.borrow().clone();
        if before.as_ref().map(|(id, _)| id) == playing.as_ref() {
            return;
        }
        let model = self.inner.sessions.library().model();
        if let Some(object) = before.and_then(|(id, _)| model.game_object(&id)) {
            object.set_playing_in_store(false);
        }
        if let Some(object) = playing.as_ref().and_then(|id| model.game_object(id)) {
            object.set_playing_in_store(true);
        }
        glib::g_info!(
            "pindu",
            "gamescope: store-launched game playing: {playing:?}"
        );
        self.inner
            .store_playing
            .replace(playing.map(|id| (id, session_service::now())));
        for listener in self.inner.store_playing_listeners.borrow().iter() {
            listener();
        }
    }

    /// Whether the session's MangoApp shows the Performance Overlay, rather than MangoHud
    /// of the game's own.
    pub fn drives_mangoapp(&self) -> bool {
        self.inner.mangoapp.is_some()
    }

    /// The launch config a game starts with: without MangoHud of its own while the
    /// session's MangoApp shows the Performance Overlay, otherwise `config`.
    pub fn launch_config(&self, config: LaunchConfig) -> LaunchConfig {
        if self.drives_mangoapp() {
            mangoapp::launch_config(&config)
        } else {
            config
        }
    }

    /// Notes whether the game with `id` starts safely this time, so MangoApp shows no
    /// Performance Overlay over it while it runs.
    pub fn set_started_safely(&self, id: &GameId, safely: bool) {
        let mut started_safely = self.inner.started_safely.borrow_mut();
        if safely {
            started_safely.insert(id.clone());
        } else {
            started_safely.remove(id);
        }
    }

    /// Starts noting whether a window of the game with `id`, which is about to start,
    /// appears; nothing outside a `--steam` gamescope, which cannot tell.
    pub fn watch_windows(&self, id: &GameId) {
        if self.inner.x11.is_some() {
            self.inner
                .windows_seen
                .borrow_mut()
                .insert(id.clone(), false);
        }
    }

    /// Whether a window of the game with `id` appeared since [`Self::watch_windows`], and
    /// stops noting it; `None` where the session cannot tell.
    pub fn window_seen(&self, id: &GameId) -> Option<bool> {
        self.inner.windows_seen.borrow_mut().remove(id)
    }

    /// Notes a window of each watched game whose app ID is among `apps`.
    fn note_windows(&self, apps: &[u32]) {
        for (id, seen) in self.inner.windows_seen.borrow_mut().iter_mut() {
            *seen |= apps.contains(&gamescope::app_id(id));
        }
    }

    /// The Performance Overlay switch of the game with `id` turned `on`: MangoApp shows or
    /// hides it now when that game is in front, unless the game started safely.
    pub fn performance_overlay_switched(&self, id: &GameId, on: bool) {
        if self.inner.mangoapp.is_some()
            && mangoapp::writes_for(self.inner.front.get(), gamescope::app_id(id))
            && !self.inner.started_safely.borrow().contains(id)
        {
            self.write_overlay(on);
        }
    }

    /// Writes the Performance Overlay switch of the game that came to the front for
    /// MangoApp, once it is read, or off for a game that started safely; nothing with the
    /// launcher in front.
    fn write_overlay_for_front(&self) {
        if self.inner.mangoapp.is_none() {
            return;
        }
        let front = self.inner.front.get();
        let id = self
            .inner
            .running
            .borrow()
            .iter()
            .find(|&&(_, app_id)| mangoapp::writes_for(front, app_id))
            .map(|(id, _)| id.clone());
        let Some(id) = id else {
            return;
        };
        let asked = self.next_overlay_write();
        let safely = self.inner.started_safely.borrow().contains(&id);
        let (weak, library) = (
            Rc::downgrade(&self.inner),
            self.inner.sessions.library().clone(),
        );
        glib::spawn_future_local(async move {
            let on = !safely
                && library
                    .launch_config(&id)
                    .await
                    .is_some_and(|config| config.mangohud);
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let service = GamescopeService { inner };
            if service.inner.overlay_writes.get() == asked && service.inner.front.get() == front {
                service.write_overlay_file(on);
            }
        });
    }

    fn next_overlay_write(&self) -> u64 {
        let asked = self.inner.overlay_writes.get() + 1;
        self.inner.overlay_writes.set(asked);
        asked
    }

    fn write_overlay(&self, on: bool) {
        self.next_overlay_write();
        self.write_overlay_file(on);
    }

    /// Rewrites MangoApp's config file in place for a switch that is `on`.
    fn write_overlay_file(&self, on: bool) {
        let Some(path) = &self.inner.mangoapp else {
            return;
        };
        glib::g_debug!("pindu", "gamescope: Performance Overlay {on} for MangoApp");
        if let Err(error) = mangoapp::write_config(path, on) {
            glib::g_warning!(
                "pindu",
                "Could not write MangoApp's config {}: {error}",
                path.display()
            );
        }
    }

    /// Expects the game with `id`, which the launcher just handed to its store launcher, in
    /// front: its windows, and the store launcher's untagged ones, get its app ID until
    /// they have come and gone, or for a while when none comes, and the Steam client's
    /// windows that stay open show meanwhile. It replaces an earlier store launch.
    pub fn store_launch_started(&self, id: &GameId) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        self.forget_store_launch();
        if self.forget_big_picture() {
            self.big_picture_changed();
        }
        let now = Instant::now();
        let mut launch = StoreLaunch::new(id, now);
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        launch.focusable_apps_changed(&apps, now);
        // A Steam client that already runs asks inside the window it has open.
        let own = own_windows();
        for xid in x11.viewable_children() {
            if let Some(facts) = x11.window_facts(xid) {
                launch.window_mapped(&facts, &own, now);
            }
        }
        let app_id = launch.app_id();
        glib::g_info!("pindu", "gamescope: expecting {id} as app {app_id}");
        self.inner.store_launch.replace(Some(launch));
        self.schedule_store_check();
        self.ask_for(Front::Game(app_id));
        self.update_store_playing();
    }

    /// Puts Steam Big Picture, just handed to Steam, in the launcher's place: the Steam
    /// client's window shows instead of the launcher's once it is there, a game it starts
    /// comes to the front, and the launcher leaves the base layer and the controller to
    /// Steam until Big Picture's window has closed, or gives up when none comes for a
    /// while, longer on Steam's `first_start`. It ends a store launch.
    pub fn big_picture_started(&self, first_start: bool) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        self.set_over_game(None);
        // Games that run already, ours or a store's, are never the game Steam starts.
        let mut before = self.front_ids();
        self.forget_store_launch();
        self.update_store_playing();
        let now = Instant::now();
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        before.extend_from_slice(&apps);
        let mut session = BigPicture::new(now, &before);
        if first_start {
            session = session.first_start();
        }
        session.focusable_apps_changed(&apps, now);
        // A Steam client that already runs may show Big Picture in the window it has open.
        let own = own_windows();
        for xid in x11.viewable_children() {
            if let Some(facts) = x11.window_facts(xid) {
                session.window_mapped(&facts, &own, now);
            }
        }
        glib::g_info!("pindu", "gamescope: Steam Big Picture is opening");
        self.inner.big_picture.replace(Some(session));
        self.schedule_big_picture_check();
        self.ask_for(Front::Launcher);
        self.big_picture_changed();
    }

    /// Whether Steam Big Picture, opened from Home, is in the launcher's place, opening
    /// or shown.
    pub fn in_big_picture(&self) -> bool {
        self.inner.big_picture.borrow().is_some()
    }

    /// Whether Steam Big Picture is opening: the Steam client has shown no window yet.
    pub fn big_picture_opening(&self) -> bool {
        self.inner
            .big_picture
            .borrow()
            .as_ref()
            .is_some_and(BigPicture::is_opening)
    }

    /// Whether Steam Big Picture has the controller: Steam's window or a game it started is
    /// in the launcher's place (`BigPicture::has_controller`).
    pub fn big_picture_has_controller(&self) -> bool {
        self.inner
            .big_picture
            .borrow()
            .as_ref()
            .is_some_and(BigPicture::has_controller)
    }

    /// Calls `listener` whenever Steam Big Picture opens, shows or ends.
    pub fn connect_big_picture_changed(&self, listener: impl Fn() + 'static) {
        self.inner
            .big_picture_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Ends Steam Big Picture's place in front: the launcher comes back to the front with
    /// its controller.
    pub fn end_big_picture(&self) {
        if !self.forget_big_picture() {
            return;
        }
        glib::g_info!("pindu", "gamescope: Steam Big Picture is over");
        self.ask_for(Front::Launcher);
        self.read_front();
        self.big_picture_changed();
    }

    /// Leaves Steam Big Picture on request, from the `leave-big-picture` action or Guide + Y
    /// (`gamescope::leave_big_picture`): the launcher comes back to the front with its
    /// controller and the Steam client keeps running behind it, with a Steam game it
    /// started kept playing behind the launcher as a store-launched game. Outside a session
    /// nothing changes.
    pub fn leave_big_picture(&self) {
        let leave = gamescope::leave_big_picture(self.inner.big_picture.borrow().as_ref());
        glib::g_info!(
            "pindu",
            "gamescope: asked to leave Steam Big Picture: {leave:?}"
        );
        match leave {
            Leave::Nothing => {}
            Leave::ToLauncher => self.end_big_picture(),
            Leave::ToLauncherWithGame(id) => {
                self.end_big_picture();
                self.keep_playing(&id);
            }
        }
    }

    /// Follows the game with `id`, which Steam started and which plays, as a store launch
    /// behind the launcher: it gets a Playing Now card with Return and Stop, and the
    /// launcher stays in front when it ends.
    fn keep_playing(&self, id: &GameId) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let now = Instant::now();
        let mut launch = StoreLaunch::new(id, now);
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        launch.focusable_apps_changed(&apps, now);
        glib::g_info!("pindu", "gamescope: {id} plays behind the launcher");
        self.inner.store_launch.replace(Some(launch));
        self.schedule_store_check();
        self.write_base_layer();
        self.update_store_playing();
    }

    /// Whether the overlay says how to leave Steam Big Picture, which it does once a
    /// session for a while after Big Picture's window shows (`BigPicture::shows_notice`).
    pub fn big_picture_notice(&self) -> bool {
        self.inner.notice.get()
    }

    /// Forgets Steam Big Picture; says whether it was there.
    fn forget_big_picture(&self) -> bool {
        if let Some(timer) = self.inner.big_picture_timer.take() {
            timer.remove();
        }
        self.inner.big_picture.take().is_some()
    }

    /// Tells the listeners that Steam Big Picture opened, showed or ended, or that the
    /// notice of how to leave it came or went, which it follows first.
    fn big_picture_changed(&self) {
        self.update_notice();
        for listener in self.inner.big_picture_listeners.borrow().iter() {
            listener();
        }
    }

    /// Shows or hides the notice of how to leave Steam Big Picture in the overlay window,
    /// and sets the timer that hides it.
    fn update_notice(&self) {
        if let Some(timer) = self.inner.notice_timer.take() {
            timer.remove();
        }
        let now = Instant::now();
        let (shown, until) = self
            .inner
            .big_picture
            .borrow()
            .as_ref()
            .map_or((false, None), |session| {
                (session.shows_notice(now), session.notice_until())
            });
        if let Some(until) = until.filter(|_| shown) {
            let weak = Rc::downgrade(&self.inner);
            let timer = glib::timeout_add_local_once(until - now, move || {
                if let Some(inner) = weak.upgrade() {
                    // The timer has fired, so there is nothing left to remove.
                    inner.notice_timer.take();
                    GamescopeService { inner }.big_picture_changed();
                }
            });
            self.inner.notice_timer.replace(Some(timer));
        }
        if self.inner.notice.replace(shown) != shown {
            glib::g_debug!(
                "pindu",
                "gamescope: how to leave Big Picture shown: {shown}"
            );
            self.write_overlay_window();
        }
    }

    /// Ends Steam Big Picture once it is over, or looks again at its next deadline.
    fn check_big_picture(&self) {
        let over = self
            .inner
            .big_picture
            .borrow()
            .as_ref()
            .map(|session| session.is_over(Instant::now()));
        match over {
            Some(true) => self.end_big_picture(),
            Some(false) => self.schedule_big_picture_check(),
            None => {}
        }
    }

    fn schedule_big_picture_check(&self) {
        if let Some(timer) = self.inner.big_picture_timer.take() {
            timer.remove();
        }
        let deadline = self
            .inner
            .big_picture
            .borrow()
            .as_ref()
            .and_then(BigPicture::deadline);
        let Some(deadline) = deadline else {
            return;
        };
        let weak = Rc::downgrade(&self.inner);
        let wait = deadline.saturating_duration_since(Instant::now());
        let timer = glib::timeout_add_local_once(wait, move || {
            if let Some(inner) = weak.upgrade() {
                // The timer has fired, so there is nothing left to remove.
                inner.big_picture_timer.take();
                GamescopeService { inner }.check_big_picture();
            }
        });
        self.inner.big_picture_timer.replace(Some(timer));
    }

    /// Lets Steam Big Picture follow another client's window with `follow`, then writes
    /// the claim when it changed, tells the listeners when Big Picture showed, and looks
    /// at it again at its new deadline.
    fn follow_big_picture_window(&self, follow: impl FnOnce(&mut BigPicture)) {
        let (claim, opening) = (self.claim(), self.big_picture_opening());
        match self.inner.big_picture.borrow_mut().as_mut() {
            Some(session) => follow(session),
            None => return,
        }
        self.check_big_picture();
        if !self.in_big_picture() {
            return;
        }
        if self.claim() != claim {
            self.write_base_layer();
        }
        if self.big_picture_opening() != opening {
            glib::g_info!("pindu", "gamescope: Steam Big Picture shows");
            self.big_picture_changed();
        }
    }

    /// Follows the games we started: one that starts comes to the front, and the launcher
    /// once the game in front ends.
    fn sessions_changed(&self) {
        let running: Vec<(GameId, u32)> = self
            .inner
            .sessions
            .sessions()
            .into_iter()
            .map(|(id, _)| {
                let app_id = gamescope::app_id(&id);
                (id, app_id)
            })
            .collect();
        let before = self.front_ids();
        let tracked_before = self.running_ids();
        self.inner.running.replace(running);
        // A game we start takes the place of Steam Big Picture that is still opening.
        let started = self
            .running_ids()
            .iter()
            .any(|id| !tracked_before.contains(id));
        if started && self.forget_big_picture() {
            self.big_picture_changed();
        }
        let replaced = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .is_some_and(|launch| {
                launch.replaced_by_sessions(&tracked_before, &self.running_ids())
            });
        if replaced {
            self.forget_store_launch();
            self.update_store_playing();
        }
        let wanted =
            gamescope::front_after_sessions(self.inner.wanted.get(), &before, &self.front_ids());
        self.inner.wanted.set(wanted);
        self.write_base_layer();
    }

    /// Follows the root's `GAMESCOPE_FOCUSABLE_APPS`: for the store launch, and for the
    /// claim, which pins the launcher while the game it wants in front has no window. A
    /// store-launched game whose window shows again after it exited comes back to the front.
    fn focusable_apps_changed(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        let back = self
            .inner
            .store_launch
            .borrow_mut()
            .as_mut()
            .and_then(|launch| {
                let back = launch.focusable_apps_changed(&apps, Instant::now());
                back.then_some(launch.app_id())
            });
        // gamescope rewrites the property, unchanged, after every base layer write: only a
        // changed claim is written, or the two would loop.
        let before = self.claim();
        if let Some(session) = self.inner.big_picture.borrow_mut().as_mut() {
            session.focusable_apps_changed(&apps, Instant::now());
        }
        self.note_windows(&apps);
        self.inner.focusable.replace(apps);
        self.check_store_launch();
        self.schedule_big_picture_check();
        if self.claim() != before {
            self.write_base_layer();
        }
        self.update_store_playing();
        if let Some(app_id) = back {
            glib::g_info!("pindu", "gamescope: the store-launched game came back");
            self.ask_for(Front::Game(app_id));
        }
    }

    /// Ends the store launch once it is over, or looks again at its next check: the
    /// launcher comes back to the front once its game exited.
    fn check_store_launch(&self) {
        let over = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .map(|launch| launch.is_over(Instant::now()));
        match over {
            Some(true) => self.end_store_launch(),
            Some(false) => {
                self.update_store_playing();
                self.leave_exited_store_game();
                self.schedule_store_check();
            }
            None => {}
        }
    }

    /// Brings the launcher back to the front when the store-launched game it wanted there
    /// has exited, while the launch still waits for a window that comes back.
    fn leave_exited_store_game(&self) {
        let ids = self.front_ids();
        let wanted = gamescope::front_after_sessions(self.inner.wanted.get(), &ids, &ids);
        if wanted != self.inner.wanted.get() {
            glib::g_info!("pindu", "gamescope: the store-launched game exited");
            self.ask_for(wanted);
        }
    }

    /// Sets the timer that looks at the store launch again at its deadline, or when a Steam
    /// client's window it waits on shows, whichever comes first.
    fn schedule_store_check(&self) {
        if let Some(timer) = self.inner.store_timer.take() {
            timer.remove();
        }
        let now = Instant::now();
        let next = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .and_then(|launch| {
                launch
                    .next_check(now)
                    .into_iter()
                    .chain(launch.reveal_at(now))
                    .min()
            });
        let Some(next) = next else {
            return;
        };
        let weak = Rc::downgrade(&self.inner);
        let wait = next.saturating_duration_since(now);
        let timer = glib::timeout_add_local_once(wait, move || {
            if let Some(inner) = weak.upgrade() {
                // The timer has fired, so there is nothing left to remove.
                inner.store_timer.take();
                let service = GamescopeService { inner };
                service.check_store_launch();
                service.hold_claim();
            }
        });
        self.inner.store_timer.replace(Some(timer));
    }

    /// Writes the claim when the one on the root is no longer the launcher's: a Steam
    /// client's window the store launch waits on has just shown.
    fn hold_claim(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let app_ids = x11.cardinals(x11.root, BASELAYER_APPID);
        let window = x11
            .cardinals(x11.root, BASELAYER_WINDOW)
            .and_then(|ids| ids.first().copied())
            .map(Xid::from);
        if !self.claim().is_held(app_ids.as_deref(), window) {
            glib::g_info!(
                "pindu",
                "gamescope: showing the Steam client's window the launch waits on"
            );
            self.write_base_layer();
        }
    }

    /// The store launch is over: the launcher comes back to the front if it was the one
    /// wanted there.
    fn end_store_launch(&self) {
        let before = self.front_ids();
        self.forget_store_launch();
        self.update_store_playing();
        glib::g_info!("pindu", "gamescope: the store launch is over");
        let wanted =
            gamescope::front_after_sessions(self.inner.wanted.get(), &before, &self.front_ids());
        self.inner.wanted.set(wanted);
        self.write_base_layer();
        self.read_front();
        for listener in self.inner.store_launch_listeners.borrow().iter() {
            listener();
        }
    }

    fn forget_store_launch(&self) {
        self.inner.store_launch.take();
        if let Some(timer) = self.inner.store_timer.take() {
            timer.remove();
        }
    }

    /// Asks gamescope to show `front`; a window picked of another game is forgotten.
    fn ask_for(&self, front: Front) {
        glib::g_debug!("pindu", "gamescope: asking for {front:?} in front");
        self.inner.wanted.set(front);
        let picked = self.inner.picked.get();
        if picked.is_some_and(|picked| front != Front::Game(picked.app_id)) {
            self.inner.picked.set(None);
        }
        self.write_base_layer();
    }

    /// The windows gamescope could show (`GAMESCOPE_FOCUSABLE_WINDOWS`).
    fn focusable_windows(&self) -> Vec<gamescope::FocusableWindow> {
        let Some(x11) = &self.inner.x11 else {
            return Vec::new();
        };
        let property = x11
            .cardinals(x11.root, FOCUSABLE_WINDOWS)
            .unwrap_or_default();
        gamescope::focusable_windows(&property)
    }

    /// The windows of the game in front, each with its title, in gamescope's order; none
    /// while the launcher is in front.
    pub fn front_windows(&self) -> Vec<(Xid, Option<String>)> {
        let (Some(x11), Front::Game(app_id)) = (&self.inner.x11, self.inner.front.get()) else {
            return Vec::new();
        };
        gamescope::game_windows(&self.focusable_windows(), app_id)
            .into_iter()
            .map(|xid| (xid, x11.title(xid)))
            .collect()
    }

    /// Exits the store-launched game with `id` while it plays, as Stop: its windows, its
    /// store launcher's among them, are asked to close, and the clients of those still
    /// there after [`EXIT_GRACE`] are ended. The launcher cannot signal a process outside
    /// its sandbox, so this goes through X11.
    pub fn exit_store_game(&self, id: &GameId) {
        let app_id = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .filter(|launch| launch.is_playing(Instant::now()) && launch.game() == id)
            .map(StoreLaunch::app_id);
        let (Some(x11), Some(app_id)) = (&self.inner.x11, app_id) else {
            return;
        };
        let exit = StoreExit::new(app_id, Instant::now());
        for xid in exit.to_ask(&self.focusable_windows()) {
            glib::g_info!(
                "pindu",
                "gamescope: asking window {xid:#x} of {id} to close"
            );
            x11.close_window(xid);
        }
        let weak = Rc::downgrade(&self.inner);
        glib::timeout_add_local_once(EXIT_GRACE, move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let service = GamescopeService { inner };
            let Some(x11) = &service.inner.x11 else {
                return;
            };
            for xid in exit.to_end(&service.focusable_windows(), Instant::now()) {
                glib::g_info!("pindu", "gamescope: ending the client of window {xid:#x}");
                x11.kill_client(xid);
            }
        });
    }

    /// Shows `xid`, a window of the game in front the user picked, instead of the one
    /// gamescope shows: it stays pinned while the game is wanted in front and the window
    /// is there.
    pub fn show_window(&self, xid: Xid) {
        let Front::Game(app_id) = self.inner.front.get() else {
            return;
        };
        glib::g_info!(
            "pindu",
            "gamescope: showing window {xid:#x} of app {app_id}"
        );
        self.inner.picked.set(Some(PickedWindow { app_id, xid }));
        self.write_base_layer();
    }

    /// Writes the overlay window's properties for it open or closed over the game, or
    /// showing the notice of how to leave Steam Big Picture.
    fn write_overlay_window(&self) {
        let (Some(x11), Some(window)) = (&self.inner.x11, self.inner.overlay.upgrade()) else {
            return;
        };
        let Some(surface) = window.surface().and_downcast::<X11Surface>() else {
            return;
        };
        let xid = surface.xid();
        let open = self.inner.over_game.get().is_some();
        let props = if !open && self.inner.notice.get() {
            overlay::notice_props()
        } else {
            overlay::overlay_props(open)
        };
        x11.write_cardinals(xid, STEAM_OVERLAY, &[props.steam_overlay]);
        x11.write_cardinals(xid, NET_WM_WINDOW_OPACITY, &[props.opacity]);
        x11.write_cardinals(xid, STEAM_INPUT_FOCUS, &[props.input_focus]);
    }

    fn running_ids(&self) -> Vec<u32> {
        self.inner
            .running
            .borrow()
            .iter()
            .map(|&(_, app_id)| app_id)
            .collect()
    }

    /// The app IDs the launcher may want in front: the games we started that run, and the
    /// store launch until its game exited.
    fn front_ids(&self) -> Vec<u32> {
        let mut ids = self.running_ids();
        ids.extend(
            self.inner
                .store_launch
                .borrow()
                .as_ref()
                .filter(|launch| !launch.has_exited(Instant::now()))
                .map(StoreLaunch::app_id),
        );
        ids
    }

    /// The games we started that run, as their windows are matched.
    fn tracked(&self) -> Vec<ExpectedGame> {
        self.inner
            .running
            .borrow()
            .iter()
            .map(|(id, _)| ExpectedGame::new(id, self.inner.sessions.pid(id)))
            .collect()
    }

    /// The games the launcher expects in front: the tracked ones and the store launch.
    fn expected(&self) -> Vec<ExpectedGame> {
        let mut expected = self.tracked();
        if let Some(launch) = self.inner.store_launch.borrow().as_ref() {
            expected.push(launch.expected().clone());
        }
        expected
    }

    /// What the launcher claims on the root for the front it wants: Steam Big Picture's
    /// while it is in the launcher's place; otherwise the Steam client's window the store
    /// launch waits on in the launcher's place, when there is one, and the window the user
    /// picked of the game in front.
    fn claim(&self) -> BaseLayerClaim {
        if let Some(session) = self.inner.big_picture.borrow().as_ref() {
            return session.claim(self.inner.main_window.get());
        }
        let claim = BaseLayerClaim::new(
            self.inner.wanted.get(),
            &self.front_ids(),
            &self.inner.focusable.borrow(),
            self.inner.main_window.get(),
        )
        .showing(self.steam_window());
        match self.inner.picked.get() {
            Some(picked) => claim.picking(Some(picked), &self.focusable_windows()),
            None => claim,
        }
    }

    /// The Steam client's window the store launch waits on, which gamescope shows.
    fn steam_window(&self) -> Option<Xid> {
        self.inner
            .store_launch
            .borrow()
            .as_ref()
            .and_then(|launch| launch.steam_window(self.inner.wanted.get(), Instant::now()))
    }

    /// Writes `GAMESCOPECTRL_BASELAYER_APPID` and `GAMESCOPECTRL_BASELAYER_WINDOW` for the
    /// front the launcher wants.
    fn write_base_layer(&self) {
        if let Some(x11) = &self.inner.x11 {
            let claim = self.claim();
            x11.write_cardinals(x11.root, BASELAYER_APPID, &claim.app_ids);
            // XIDs have 29 bits; 0 is no pin, as gamescope and Steam read it.
            let window = claim
                .window
                .and_then(|xid| u32::try_from(xid).ok())
                .unwrap_or(0);
            x11.write_cardinals(x11.root, BASELAYER_WINDOW, &[window]);
        }
    }

    /// Writes the claim back when another client, such as a Steam client started inside
    /// the session, changed the root's base layer properties; not while Steam Big Picture
    /// is in the launcher's place, where the Steam client decides.
    fn base_layer_changed(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        if self.in_big_picture() {
            return;
        }
        let app_ids = x11.cardinals(x11.root, BASELAYER_APPID);
        let window = x11
            .cardinals(x11.root, BASELAYER_WINDOW)
            .and_then(|ids| ids.first().copied())
            .map(Xid::from);
        if self.claim().is_held(app_ids.as_deref(), window) {
            return;
        }
        if self.inner.reclaims.borrow_mut().allow(Instant::now()) {
            glib::g_info!(
                "pindu",
                "gamescope: another client changed the base layer ({app_ids:?}, window {window:?}); writing the launcher's back"
            );
            self.write_base_layer();
        } else {
            glib::g_warning!(
                "pindu",
                "gamescope: another client keeps changing the base layer; leaving it for now"
            );
        }
    }

    /// Reads who gamescope shows and tells the listeners when it changed.
    fn read_front(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let gfx = x11
            .cardinals(x11.root, FOCUSED_APP_GFX)
            .and_then(|ids| ids.first().copied());
        let front = gamescope::front(gfx, &self.expected());
        if self.inner.front.replace(front) == front {
            return;
        }
        if let Front::Game(app_id) = front {
            self.note_windows(&[app_id]);
        }
        // The panel over the game belongs to the game it opened over.
        self.set_over_game(None);
        glib::g_debug!("pindu", "gamescope: {front:?} in front");
        for listener in self.inner.front_listeners.borrow().iter() {
            listener(front);
        }
        self.write_overlay_for_front();
    }

    /// Gives a window another client just mapped its game's app ID, when it is one, and
    /// shows it while the store launch waits on it, or Steam Big Picture is in the
    /// launcher's place, when it is the Steam client's.
    fn window_mapped(&self, xid: Xid) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let tracked = self.tracked();
        let launch = self.inner.store_launch.borrow().clone();
        if tracked.is_empty() && launch.is_none() && !self.in_big_picture() {
            return;
        }
        let own = own_windows();
        let Some(facts) = x11.window_facts(xid) else {
            return;
        };
        let app_id = match &launch {
            Some(launch) => launch.tag_for_window(&facts, &tracked, &own),
            None => gamescope::tag_for_window(&facts, &tracked, &own),
        };
        if let Some(app_id) = app_id {
            glib::g_info!("pindu", "gamescope: window {xid:#x} is app {app_id}");
            x11.write_cardinals_trapped(xid, STEAM_GAME, &[app_id]);
        }
        if launch.is_some() {
            self.follow_store_window(|launch| launch.window_mapped(&facts, &own, Instant::now()));
        }
        self.follow_big_picture_window(|session| {
            session.window_mapped(&facts, &own, Instant::now())
        });
    }

    /// Lets the store launch and Steam Big Picture know another client's top-level window
    /// was unmapped, and drops the pin of a picked window that went.
    fn window_unmapped(&self, xid: Xid) {
        if self
            .inner
            .picked
            .get()
            .is_some_and(|picked| picked.xid == xid)
        {
            self.inner.picked.set(None);
            self.write_base_layer();
        }
        self.follow_store_window(|launch| launch.window_unmapped(xid, Instant::now()));
        self.follow_big_picture_window(|session| session.window_unmapped(xid, Instant::now()));
    }

    /// Lets the store launch follow another client's window with `follow`, then writes the
    /// claim when the Steam client's window it waits on changed, and looks at the launch
    /// again at its new deadline.
    fn follow_store_window(&self, follow: impl FnOnce(&mut StoreLaunch)) {
        let before = self.steam_window();
        match self.inner.store_launch.borrow_mut().as_mut() {
            Some(launch) => follow(launch),
            None => return,
        }
        self.schedule_store_check();
        let after = self.steam_window();
        if after == before {
            return;
        }
        match after {
            Some(xid) => glib::g_info!(
                "pindu",
                "gamescope: showing the Steam client's window {xid:#x} the launch waits on"
            ),
            None => glib::g_info!("pindu", "gamescope: the Steam client's window is gone"),
        }
        self.write_base_layer();
    }
}

/// The X11 windows of the launcher's own top-levels.
fn own_windows() -> Vec<Xid> {
    gtk::Window::list_toplevels()
        .iter()
        .filter_map(|widget| widget.downcast_ref::<gtk::Window>()?.surface())
        .filter_map(|surface| surface.downcast::<X11Surface>().ok())
        .map(|surface| surface.xid())
        .collect()
}

/// GDK's X11 connection, and Xlib to call on it.
struct X11 {
    display: X11Display,
    xlib: xlib::Xlib,
    root: xlib::Window,
}

impl X11 {
    /// The default display when it is X11 and gamescope's root says `--steam`.
    fn steam_mode_display() -> Option<Self> {
        let display = gtk::gdk::Display::default()?
            .downcast::<X11Display>()
            .ok()?;
        let xlib = match xlib::Xlib::open() {
            Ok(xlib) => xlib,
            Err(error) => {
                glib::g_warning!("pindu", "Could not load Xlib: {error}");
                return None;
            }
        };
        let root = display.xrootwindow();
        let x11 = Self {
            display,
            xlib,
            root,
        };
        let focused_app = x11.cardinals(root, FOCUSED_APP);
        gamescope::steam_mode(focused_app.as_deref()).then_some(x11)
    }

    fn xdisplay(&self) -> *mut xlib::Display {
        // SAFETY: the display is an open X11 display, whose Xlib connection lives as long
        // as it does; `self` holds a reference to it.
        unsafe { self.display.xdisplay() }
    }

    fn atom(&self, name: &str) -> xlib::Atom {
        gdk4_x11::x11_get_xatom_by_name_for_display(&self.display, name)
    }

    /// Adds `PropertyChangeMask` to the events GDK selects on the root, which already has
    /// `SubstructureNotifyMask`, and hands the root's property changes and the top-level
    /// windows other clients map to the service.
    fn watch_root(&self, service: Weak<Inner>) {
        let display = self.xdisplay();
        // SAFETY: a zeroed `XWindowAttributes` is a valid value (integers and null
        // pointers) for Xlib to fill in.
        let mut attributes: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
        // SAFETY: `display` is GDK's open connection, the root exists as long as it does,
        // and `attributes` outlives the call.
        let read = unsafe { (self.xlib.XGetWindowAttributes)(display, self.root, &mut attributes) };
        if read == 0 {
            glib::g_warning!("pindu", "gamescope: could not read the root's event mask");
            return;
        }
        let mask =
            attributes.your_event_mask | xlib::PropertyChangeMask | xlib::SubstructureNotifyMask;
        // SAFETY: as above; selecting events on the root takes no pointer.
        unsafe { (self.xlib.XSelectInput)(display, self.root, mask) };
        let root = self.root;
        let gfx = self.atom(FOCUSED_APP_GFX);
        let focusable = self.atom(FOCUSABLE_APPS);
        let base_layer = [self.atom(BASELAYER_APPID), self.atom(BASELAYER_WINDOW)];
        let handler = move |_: &X11Display, event: *mut xlib::XEvent| {
            // SAFETY: GDK hands the handler the event it is processing, valid for the
            // call; `type_` is the first field of every event in the union.
            let kind = unsafe { (*event).type_ };
            let work = match kind {
                xlib::PropertyNotify => {
                    // SAFETY: a `PropertyNotify` event is an `XPropertyEvent`.
                    let property = unsafe { (*event).property };
                    match property.atom {
                        _ if property.window != root => None,
                        atom if atom == gfx => Some(Work::ReadFront),
                        atom if atom == focusable => Some(Work::ReadFocusable),
                        atom if base_layer.contains(&atom) => Some(Work::CheckClaim),
                        _ => None,
                    }
                }
                xlib::MapNotify => {
                    // SAFETY: a `MapNotify` event is an `XMapEvent`.
                    let map = unsafe { (*event).map };
                    // The root's own `MapNotify` for its children: top-level windows.
                    (map.event == root && map.override_redirect == 0)
                        .then_some(Work::Mapped(map.window))
                }
                xlib::UnmapNotify => {
                    // SAFETY: an `UnmapNotify` event is an `XUnmapEvent`.
                    let unmap = unsafe { (*event).unmap };
                    (unmap.event == root).then_some(Work::Unmapped(unmap.window))
                }
                _ => None,
            };
            if let Some(work) = work {
                let service = service.clone();
                // Out of GDK's event processing, which the X11 calls below should not
                // run inside.
                glib::idle_add_local_once(move || {
                    if let Some(inner) = service.upgrade() {
                        let service = GamescopeService { inner };
                        match work {
                            Work::ReadFront => service.read_front(),
                            Work::ReadFocusable => service.focusable_apps_changed(),
                            Work::Mapped(xid) => service.window_mapped(xid),
                            Work::Unmapped(xid) => service.window_unmapped(xid),
                            Work::CheckClaim => service.base_layer_changed(),
                        }
                    }
                });
            }
            glib::Propagation::Proceed
        };
        // SAFETY: the handler only reads the event GDK passes it, during the call, and
        // leaves the event to GDK (`Proceed`).
        unsafe { self.display.connect_xevent(handler) };
    }

    /// The CARDINALs of a window's property: `None` without the property, empty when it
    /// holds no 32-bit items.
    fn cardinals(&self, window: xlib::Window, property: &str) -> Option<Vec<u32>> {
        match self.read(window, property)? {
            Items::Format32(longs) => Some(
                longs
                    .into_iter()
                    .map(gamescope::cardinal_from_long)
                    .collect(),
            ),
            Items::Other(_) => Some(Vec::new()),
        }
    }

    /// The bytes of a window's 8-bit property, such as `WM_CLASS`.
    fn bytes(&self, window: xlib::Window, property: &str) -> Option<Vec<u8>> {
        match self.read(window, property)? {
            Items::Other(bytes) => Some(bytes),
            Items::Format32(_) => None,
        }
    }

    fn read(&self, window: xlib::Window, property: &str) -> Option<Items> {
        let mut kind: xlib::Atom = 0;
        let mut format: c_int = 0;
        let mut count: c_ulong = 0;
        let mut after: c_ulong = 0;
        let mut data: *mut c_uchar = std::ptr::null_mut();
        // SAFETY: `xdisplay` is GDK's open connection and every out-pointer points to a
        // local that outlives the call. A missing window is caught by GDK's error trap
        // around the callers that read other clients' windows.
        let status = unsafe {
            (self.xlib.XGetWindowProperty)(
                self.xdisplay(),
                window,
                self.atom(property),
                0,
                MAX_ITEMS,
                xlib::False,
                // AnyPropertyType.
                0,
                &mut kind,
                &mut format,
                &mut count,
                &mut after,
                &mut data,
            )
        };
        if data.is_null() {
            return None;
        }
        let count = usize::try_from(count).unwrap_or(0);
        let items = match (status == c_int::from(xlib::Success), kind, format) {
            (false, _, _) | (_, 0, _) => None,
            (true, _, 32) => {
                // SAFETY: with format 32 Xlib returns `count` C longs at `data`, valid
                // until the `XFree` below.
                let longs = unsafe { std::slice::from_raw_parts(data.cast::<c_long>(), count) };
                Some(Items::Format32(longs.to_vec()))
            }
            (true, _, 8) => {
                // SAFETY: with format 8 Xlib returns `count` bytes at `data`, valid until
                // the `XFree` below.
                let bytes = unsafe { std::slice::from_raw_parts(data, count) };
                Some(Items::Other(bytes.to_vec()))
            }
            (true, _, _) => Some(Items::Other(Vec::new())),
        };
        // SAFETY: Xlib allocated `data` for this call, and it is freed once, after the
        // items were copied out.
        unsafe { (self.xlib.XFree)(data.cast()) };
        items
    }

    /// Replaces a window's CARDINAL property with `values`.
    fn write_cardinals(&self, window: xlib::Window, property: &str, values: &[u32]) {
        let longs: Vec<c_long> = values
            .iter()
            .map(|&value| gamescope::cardinal_to_long(value))
            .collect();
        let Ok(count) = c_int::try_from(longs.len()) else {
            return;
        };
        // SAFETY: `xdisplay` is GDK's open connection; format-32 data is an array of C
        // longs, and `longs` holds `count` of them for the length of the call.
        unsafe {
            (self.xlib.XChangeProperty)(
                self.xdisplay(),
                window,
                self.atom(property),
                xlib::XA_CARDINAL,
                32,
                xlib::PropModeReplace,
                longs.as_ptr().cast::<c_uchar>(),
                count,
            )
        };
        self.display.flush();
    }

    /// [`Self::write_cardinals`] on another client's window, which may already be gone:
    /// GDK's error trap catches the `BadWindow`.
    fn write_cardinals_trapped(&self, window: Xid, property: &str, values: &[u32]) {
        self.display.error_trap_push();
        self.write_cardinals(window, property, values);
        let error = self.display.error_trap_pop();
        if error != 0 {
            glib::g_debug!(
                "pindu",
                "gamescope: window {window:#x} went (X error {error})"
            );
        }
    }

    /// Asks another client's window to close (`WM_DELETE_WINDOW`), as its close button
    /// would; nothing once it is gone.
    fn close_window(&self, xid: Xid) {
        // SAFETY: a zeroed `XEvent` is a valid value; the client message is filled in
        // through its own member below.
        let mut event: xlib::XEvent = unsafe { std::mem::zeroed() };
        let message: &mut xlib::XClientMessageEvent = event.as_mut();
        message.type_ = xlib::ClientMessage;
        message.window = xid;
        message.message_type = self.atom(WM_PROTOCOLS);
        message.format = 32;
        let delete = c_long::try_from(self.atom(WM_DELETE_WINDOW)).unwrap_or_default();
        message.data.set_long(0, delete);
        // CurrentTime.
        message.data.set_long(1, 0);
        self.display.error_trap_push();
        // SAFETY: `xdisplay` is GDK's open connection and `event` outlives the call; a
        // window that is gone is caught by the error trap.
        unsafe {
            (self.xlib.XSendEvent)(
                self.xdisplay(),
                xid,
                xlib::False,
                xlib::NoEventMask,
                &mut event,
            )
        };
        self.display.flush();
        self.display.error_trap_pop();
    }

    /// Ends the client of another client's window, which closes its connection and, for
    /// nearly every program, the program; nothing once it is gone.
    fn kill_client(&self, xid: Xid) {
        self.display.error_trap_push();
        // SAFETY: `xdisplay` is GDK's open connection; a window that is gone is caught by
        // the error trap.
        unsafe { (self.xlib.XKillClient)(self.xdisplay(), xid) };
        self.display.flush();
        self.display.error_trap_pop();
    }

    /// Another client's window's title, or `None` when it has none or is gone.
    fn title(&self, xid: Xid) -> Option<String> {
        self.display.error_trap_push();
        let net_wm_name = self.bytes(xid, NET_WM_NAME);
        let wm_name = self.bytes(xid, WM_NAME);
        self.display.error_trap_pop();
        gamescope::window_title(net_wm_name.as_deref(), wm_name.as_deref())
    }

    /// What decides a window's app ID, or `None` once it is gone.
    fn window_facts(&self, xid: Xid) -> Option<WindowFacts> {
        let display = self.xdisplay();
        self.display.error_trap_push();
        // SAFETY: a zeroed `XWindowAttributes` is a valid value for Xlib to fill in.
        let mut attributes: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
        // SAFETY: `display` is GDK's open connection and `attributes` outlives the call;
        // a window that is gone is caught by the error trap.
        let read = unsafe { (self.xlib.XGetWindowAttributes)(display, xid, &mut attributes) };
        let facts = WindowFacts {
            xid,
            wm_class: self
                .bytes(xid, WM_CLASS)
                .map(|class| gamescope::wm_class_names(&class))
                .unwrap_or_default(),
            pid: self
                .cardinals(xid, NET_WM_PID)
                .and_then(|pid| pid.first().copied()),
            steam_game: self
                .cardinals(xid, STEAM_GAME)
                .and_then(|id| id.first().copied()),
            override_redirect: attributes.override_redirect != 0,
            width: u32::try_from(attributes.width).unwrap_or(0),
            height: u32::try_from(attributes.height).unwrap_or(0),
        };
        let error = self.display.error_trap_pop();
        (read != 0 && error == 0).then_some(facts)
    }

    /// The root's children that are mapped and viewable, the top-level windows on screen,
    /// bottom first.
    fn viewable_children(&self) -> Vec<Xid> {
        let display = self.xdisplay();
        let mut root: xlib::Window = 0;
        let mut parent: xlib::Window = 0;
        let mut children: *mut xlib::Window = std::ptr::null_mut();
        let mut count: c_uint = 0;
        // SAFETY: `display` is GDK's open connection, the root exists as long as it does,
        // and every out-pointer points to a local that outlives the call.
        let status = unsafe {
            (self.xlib.XQueryTree)(
                display,
                self.root,
                &mut root,
                &mut parent,
                &mut children,
                &mut count,
            )
        };
        if children.is_null() {
            return Vec::new();
        }
        let count = usize::try_from(count).unwrap_or(0);
        let windows = if status == 0 {
            Vec::new()
        } else {
            // SAFETY: on success Xlib returns `count` windows at `children`, valid until
            // the `XFree` below.
            unsafe { std::slice::from_raw_parts(children, count) }.to_vec()
        };
        // SAFETY: Xlib allocated `children` for this call, and it is freed once, after the
        // windows were copied out.
        unsafe { (self.xlib.XFree)(children.cast()) };
        windows
            .into_iter()
            .filter(|&xid| self.is_viewable(xid))
            .collect()
    }

    /// Whether `xid` is mapped and viewable; `false` once it is gone.
    fn is_viewable(&self, xid: Xid) -> bool {
        self.display.error_trap_push();
        // SAFETY: a zeroed `XWindowAttributes` is a valid value for Xlib to fill in.
        let mut attributes: xlib::XWindowAttributes = unsafe { std::mem::zeroed() };
        // SAFETY: `xdisplay` is GDK's open connection and `attributes` outlives the call; a
        // window that is gone is caught by the error trap.
        let read =
            unsafe { (self.xlib.XGetWindowAttributes)(self.xdisplay(), xid, &mut attributes) };
        let error = self.display.error_trap_pop();
        read != 0 && error == 0 && attributes.map_state == xlib::IsViewable
    }
}

/// A property's items as read.
enum Items {
    /// Format 32: CARDINALs, as Xlib hands them, C longs.
    Format32(Vec<c_long>),
    /// Format 8 as bytes, or no items for format 16, which the launcher never reads.
    Other(Vec<u8>),
}

/// What a root event asks of the service, once GDK is done with it.
#[derive(Debug, Clone, Copy)]
enum Work {
    /// `GAMESCOPE_FOCUSED_APP_GFX` changed.
    ReadFront,
    /// `GAMESCOPE_FOCUSABLE_APPS` changed.
    ReadFocusable,
    /// Another client mapped this top-level window.
    Mapped(Xid),
    /// A top-level window was unmapped.
    Unmapped(Xid),
    /// `GAMESCOPECTRL_BASELAYER_APPID` or `GAMESCOPECTRL_BASELAYER_WINDOW` changed.
    CheckClaim,
}
