//! Speaks gamescope's `--steam` focus protocol, where the launcher takes Steam's place: its
//! main window is app 769, it writes the base layer list on the root, tags the windows of
//! the games it started with their app IDs, and reads back who is in front. With a game in
//! front it opens Quick Access over the game in its overlay window, which it marks
//! `STEAM_OVERLAY` and opens and closes by opacity and `STEAM_INPUT_FOCUS`. The decisions
//! are `pindu_core::gamescope`'s; this is the only module that touches X11, on GDK's own
//! connection.
//!
//! The service is inert unless the display is X11 and gamescope runs with `--steam`: on a
//! Wayland desktop and in a plain gamescope it writes nothing.
//!
//! In such a gamescope with a writable `MANGOHUD_CONFIGFILE`, it also drives the session's
//! MangoApp from the Performance Overlay switch of the game in front
//! (`pindu_core::gamescope::mangoapp`).

use std::cell::{Cell, RefCell};
use std::ffi::{c_int, c_long, c_uchar, c_uint, c_ulong};
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Instant;

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamescope::{
    self, BaseLayerClaim, ExpectedGame, Front, InGameIntent, Reclaims, STEAM_APP_ID, StoreLaunch,
    WindowFacts, Xid, mangoapp, overlay,
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
/// Something the overlay does when Quick Access opens or closes over the game.
type QuickAccessListener = Box<dyn Fn(bool)>;
/// Something the shells do when the store-launched game that plays changes.
type StorePlayingListener = Box<dyn Fn()>;

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
    /// The timer that looks at the store launch again at its deadline.
    store_timer: RefCell<Option<glib::SourceId>>,
    /// The store-launched game that plays, and since when the launcher has seen it.
    store_playing: RefCell<Option<(GameId, Timestamp)>>,
    store_playing_listeners: RefCell<Vec<StorePlayingListener>>,
    /// Who the launcher asked gamescope to show.
    wanted: Cell<Front>,
    /// Who gamescope shows, as `GAMESCOPE_FOCUSED_APP_GFX` says.
    front: Cell<Front>,
    front_listeners: RefCell<Vec<FrontListener>>,
    /// Whether Guide asked for the launcher, rather than a link or a game that ended.
    by_guide: Cell<bool>,
    /// The window Quick Access opens in over a game, once there is one.
    overlay: glib::WeakRef<gtk::Window>,
    /// Whether Quick Access is open over the game.
    quick_access_open: Cell<bool>,
    quick_access_listeners: RefCell<Vec<QuickAccessListener>>,
    /// The session's MangoApp config file, when the launcher drives it.
    mangoapp: Option<PathBuf>,
    /// Counts the Performance Overlay writes asked for, so a game's switch read late
    /// never overwrites a newer one.
    overlay_writes: Cell<u64>,
    /// The launcher's main window, which the claim pins, once it is realized.
    main_window: Cell<Option<Xid>>,
    /// The app IDs of the windows gamescope could show (`GAMESCOPE_FOCUSABLE_APPS`).
    focusable: RefCell<Vec<u32>>,
    /// When the launcher wrote its claim back after another client changed it.
    reclaims: RefCell<Reclaims>,
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
                store_playing: RefCell::default(),
                store_playing_listeners: RefCell::default(),
                wanted: Cell::new(Front::Launcher),
                front: Cell::new(Front::Launcher),
                front_listeners: RefCell::default(),
                by_guide: Cell::new(false),
                overlay: glib::WeakRef::new(),
                quick_access_open: Cell::new(false),
                quick_access_listeners: RefCell::default(),
                mangoapp,
                overlay_writes: Cell::default(),
                main_window: Cell::default(),
                focusable: RefCell::default(),
                reclaims: RefCell::default(),
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
            let Some(intent) = gamescope::in_game_intent(intent) else {
                return;
            };
            let service = GamescopeService { inner };
            if intent == InGameIntent::ShowLauncher && service.steam_window().is_some() {
                // The user leaves the Steam client's window: the launch is given up on.
                service.end_store_launch();
                return;
            }
            if !matches!(service.inner.front.get(), Front::Game(_)) {
                return;
            }
            service.set_quick_access_open(overlay::open_after(service.quick_access_open(), intent));
            if intent == InGameIntent::ShowLauncher {
                service.ask_for(Front::Launcher, true);
            }
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

    /// Makes `window` the overlay Quick Access opens in over a game: once it is realized
    /// it is marked `STEAM_OVERLAY` and closed, see-through and leaving input to the game,
    /// and Guide + A or the Steam Deck's "…" opens it while a game is in front. It never
    /// gets an app ID, so the game stays the base layer while it is open.
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

    /// Whether Quick Access is open over the game in front.
    pub fn quick_access_open(&self) -> bool {
        self.inner.quick_access_open.get()
    }

    /// Opens Quick Access over the game in front, or closes it. It opens only while a game
    /// we started is in front and the overlay window is there.
    pub fn set_quick_access_open(&self, open: bool) {
        let open = open
            && matches!(self.inner.front.get(), Front::Game(_))
            && self.inner.overlay.upgrade().is_some();
        if self.inner.quick_access_open.replace(open) == open {
            return;
        }
        glib::g_debug!(
            "pindu",
            "gamescope: Quick Access open over the game: {open}"
        );
        self.write_overlay_window();
        for listener in self.inner.quick_access_listeners.borrow().iter() {
            listener(open);
        }
    }

    /// Calls `listener` whenever Quick Access opens or closes over the game.
    pub fn connect_quick_access_changed(&self, listener: impl Fn(bool) + 'static) {
        self.inner
            .quick_access_listeners
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

    /// Brings the launcher to the front for what a link in Quick Access over the game
    /// leads to, closing Quick Access there.
    pub fn show_launcher(&self) {
        self.set_quick_access_open(false);
        self.ask_for(Front::Launcher, false);
    }

    /// Whether Guide over a game asked for the launcher, which the launcher shows in front
    /// now or soon: then the drawer opens on the Playing Now cards.
    pub fn launcher_by_guide(&self) -> bool {
        self.inner.by_guide.get() && self.inner.wanted.get() == Front::Launcher
    }

    /// Who gamescope shows: the launcher, or a game we started or handed to its store.
    pub fn front(&self) -> Front {
        self.inner.front.get()
    }

    /// Who the launcher last asked gamescope to show: a game it just started, the launcher
    /// after Guide or once the game in front ends, or a game after its Return.
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
                    .filter(|launch| launch.is_playing() && launch.game() == id)
                    .map(StoreLaunch::app_id)
            });
        if let Some(app_id) = app_id {
            self.ask_for(Front::Game(app_id), false);
        }
    }

    /// The store-launched game that plays, with when the launcher first saw its window:
    /// its store launcher runs it, so the launcher can bring it back but not stop it.
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

    /// Follows the store launch into the store-launched game that plays: its game's
    /// Playing state, and the listeners, when it changed.
    fn update_store_playing(&self) {
        let playing = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .filter(|launch| launch.is_playing())
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

    /// The Performance Overlay switch of the game with `id` turned `on`: MangoApp shows or
    /// hides it now when that game is in front.
    pub fn performance_overlay_switched(&self, id: &GameId, on: bool) {
        if self.inner.mangoapp.is_some()
            && mangoapp::writes_for(self.inner.front.get(), gamescope::app_id(id))
        {
            self.write_overlay(on);
        }
    }

    /// Writes the Performance Overlay switch of the game that came to the front for
    /// MangoApp, once it is read; nothing with the launcher in front.
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
        let (weak, library) = (
            Rc::downgrade(&self.inner),
            self.inner.sessions.library().clone(),
        );
        glib::spawn_future_local(async move {
            let on = library
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
    /// open windows show meanwhile. It replaces an earlier store launch.
    pub fn store_launch_started(&self, id: &GameId) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        self.forget_store_launch();
        let now = Instant::now();
        let mut launch = StoreLaunch::new(id, now);
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        launch.focusable_apps_changed(&apps, now);
        // A Steam client that already runs asks inside the window it has open.
        let own = own_windows();
        for xid in x11.viewable_children() {
            if let Some(facts) = x11.window_facts(xid) {
                launch.window_mapped(&facts, &own);
            }
        }
        let app_id = launch.app_id();
        glib::g_info!("pindu", "gamescope: expecting {id} as app {app_id}");
        self.inner.store_launch.replace(Some(launch));
        self.schedule_store_check();
        self.ask_for(Front::Game(app_id), false);
        self.update_store_playing();
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
        self.set_wanted(wanted);
        self.write_base_layer();
    }

    /// Takes `wanted` as the front the launcher wants after a game came or went: Guide no
    /// longer asked for it once it changes.
    fn set_wanted(&self, wanted: Front) {
        if self.inner.wanted.replace(wanted) != wanted {
            self.inner.by_guide.set(false);
        }
    }

    /// Follows the root's `GAMESCOPE_FOCUSABLE_APPS`: for the store launch, and for the
    /// claim, which pins the launcher while the game it wants in front has no window.
    fn focusable_apps_changed(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let apps = x11.cardinals(x11.root, FOCUSABLE_APPS).unwrap_or_default();
        if let Some(launch) = self.inner.store_launch.borrow_mut().as_mut() {
            launch.focusable_apps_changed(&apps, Instant::now());
        }
        // gamescope rewrites the property, unchanged, after every base layer write: only a
        // changed claim is written, or the two would loop.
        let before = self.claim();
        self.inner.focusable.replace(apps);
        self.check_store_launch();
        if self.claim() != before {
            self.write_base_layer();
        }
        self.update_store_playing();
    }

    /// Ends the store launch once it is over, or looks again at its next deadline.
    fn check_store_launch(&self) {
        let over = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .map(|launch| launch.is_over(Instant::now()));
        match over {
            Some(true) => self.end_store_launch(),
            Some(false) => self.schedule_store_check(),
            None => {}
        }
    }

    fn schedule_store_check(&self) {
        if let Some(timer) = self.inner.store_timer.take() {
            timer.remove();
        }
        let deadline = self
            .inner
            .store_launch
            .borrow()
            .as_ref()
            .and_then(StoreLaunch::deadline);
        let Some(deadline) = deadline else {
            return;
        };
        let weak = Rc::downgrade(&self.inner);
        let wait = deadline.saturating_duration_since(Instant::now());
        let timer = glib::timeout_add_local_once(wait, move || {
            if let Some(inner) = weak.upgrade() {
                // The timer has fired, so there is nothing left to remove.
                inner.store_timer.take();
                GamescopeService { inner }.check_store_launch();
            }
        });
        self.inner.store_timer.replace(Some(timer));
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
        self.set_wanted(wanted);
        self.write_base_layer();
        self.read_front();
    }

    fn forget_store_launch(&self) {
        self.inner.store_launch.take();
        if let Some(timer) = self.inner.store_timer.take() {
            timer.remove();
        }
    }

    /// Asks gamescope to show `front`, for Guide (`by_guide`) or otherwise.
    fn ask_for(&self, front: Front, by_guide: bool) {
        glib::g_debug!("pindu", "gamescope: asking for {front:?} in front");
        self.inner.wanted.set(front);
        self.inner.by_guide.set(by_guide);
        self.write_base_layer();
    }

    /// Writes the overlay window's properties for Quick Access open or closed.
    fn write_overlay_window(&self) {
        let (Some(x11), Some(window)) = (&self.inner.x11, self.inner.overlay.upgrade()) else {
            return;
        };
        let Some(surface) = window.surface().and_downcast::<X11Surface>() else {
            return;
        };
        let xid = surface.xid();
        let props = overlay::overlay_props(self.inner.quick_access_open.get());
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
    /// store launch.
    fn front_ids(&self) -> Vec<u32> {
        let mut ids = self.running_ids();
        ids.extend(
            self.inner
                .store_launch
                .borrow()
                .as_ref()
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

    /// What the launcher claims on the root for the front it wants: the Steam client's
    /// window the store launch waits on in the launcher's place, when there is one.
    fn claim(&self) -> BaseLayerClaim {
        BaseLayerClaim::new(
            self.inner.wanted.get(),
            &self.front_ids(),
            &self.inner.focusable.borrow(),
            self.inner.main_window.get(),
        )
        .showing(self.steam_window())
    }

    /// The Steam client's window the store launch waits on, which gamescope shows.
    fn steam_window(&self) -> Option<Xid> {
        self.inner
            .store_launch
            .borrow()
            .as_ref()
            .and_then(|launch| launch.steam_window(self.inner.wanted.get()))
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
    /// the session, changed the root's base layer properties.
    fn base_layer_changed(&self) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
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
        // Quick Access belongs to the game it opened over.
        self.set_quick_access_open(false);
        glib::g_debug!("pindu", "gamescope: {front:?} in front");
        for listener in self.inner.front_listeners.borrow().iter() {
            listener(front);
        }
        self.write_overlay_for_front();
    }

    /// Gives a window another client just mapped its game's app ID, when it is one, and
    /// shows it while the store launch waits on it, when it is the Steam client's.
    fn window_mapped(&self, xid: Xid) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let tracked = self.tracked();
        let launch = self.inner.store_launch.borrow().clone();
        if tracked.is_empty() && launch.is_none() {
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
            self.follow_store_window(|launch| launch.window_mapped(&facts, &own));
        }
    }

    /// Lets the store launch know another client's top-level window was unmapped.
    fn window_unmapped(&self, xid: Xid) {
        self.follow_store_window(|launch| launch.window_unmapped(xid, Instant::now()));
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
