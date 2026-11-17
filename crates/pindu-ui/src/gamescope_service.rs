//! Speaks gamescope's `--steam` focus protocol, where the launcher takes Steam's place: its
//! main window is app 769, it writes the base layer list on the root, tags the windows of
//! the games it started with their app IDs, and reads back who is in front. The decisions
//! are `pindu_core::gamescope`'s; this is the only module that touches X11, on GDK's own
//! connection.
//!
//! The service is inert unless the display is X11 and gamescope runs with `--steam`: on a
//! Wayland desktop and in a plain gamescope it writes nothing.

use std::cell::{Cell, RefCell};
use std::ffi::{c_int, c_long, c_uchar, c_ulong};
use std::rc::{Rc, Weak};

use gdk4_x11::x11::xlib;
use gdk4_x11::{X11Display, X11Surface};
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamescope::{
    self, ExpectedGame, Front, InGameIntent, STEAM_APP_ID, WindowFacts, Xid,
};
use pindu_core::model::GameId;

use crate::gamepad_service::GamepadService;
use crate::session_service::SessionService;

/// gamescope's property that says it runs with `--steam`; the app ID with input focus.
const FOCUSED_APP: &str = "GAMESCOPE_FOCUSED_APP";
/// gamescope's property with the app ID of the base layer: who is in front.
const FOCUSED_APP_GFX: &str = "GAMESCOPE_FOCUSED_APP_GFX";
/// The root's focus priority list, which Steam writes and the launcher writes in its place.
const BASELAYER_APPID: &str = "GAMESCOPECTRL_BASELAYER_APPID";
/// A window's app ID.
const STEAM_GAME: &str = "STEAM_GAME";
/// The process ID a client gives its window.
const NET_WM_PID: &str = "_NET_WM_PID";
/// A window's instance and class names.
const WM_CLASS: &str = "WM_CLASS";

/// How many items a property read takes at most: a focus list or a window class is far
/// shorter.
const MAX_ITEMS: c_long = 1024;

/// Something the shells do when who is in front changes.
type FrontListener = Box<dyn Fn(Front)>;

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
    /// Who the launcher asked gamescope to show.
    wanted: Cell<Front>,
    /// Who gamescope shows, as `GAMESCOPE_FOCUSED_APP_GFX` says.
    front: Cell<Front>,
    front_listeners: RefCell<Vec<FrontListener>>,
}

impl GamescopeService {
    /// A service for the default display that follows the games `sessions` runs and reads
    /// Guide from `gamepad`; inert unless the display is X11 and gamescope runs with
    /// `--steam`.
    pub fn new(sessions: &SessionService, gamepad: &GamepadService) -> Self {
        let x11 = X11::steam_mode_display();
        let service = Self {
            inner: Rc::new(Inner {
                x11,
                sessions: sessions.clone(),
                running: RefCell::default(),
                wanted: Cell::new(Front::Launcher),
                front: Cell::new(Front::Launcher),
                front_listeners: RefCell::default(),
            }),
        };
        let Some(x11) = &service.inner.x11 else {
            return service;
        };
        glib::g_info!(
            "pindu",
            "gamescope runs with --steam: the launcher takes Steam's place"
        );
        x11.watch_root(Rc::downgrade(&service.inner));
        let weak = Rc::downgrade(&service.inner);
        sessions.connect_sessions_changed(move || {
            if let Some(inner) = weak.upgrade() {
                GamescopeService { inner }.sessions_changed();
            }
        });
        let weak = Rc::downgrade(&service.inner);
        gamepad.connect_button_pressed(move |button| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let in_game = matches!(inner.front.get(), Front::Game(_));
            if in_game && gamescope::in_game_intent(button) == Some(InGameIntent::ShowLauncher) {
                GamescopeService { inner }.ask_for(Front::Launcher);
            }
        });
        service.write_base_layer();
        service.read_front();
        service
    }

    /// Tags `window` as the launcher (app 769) once it is realized, so gamescope shows it.
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
            if let (Some(x11), Some(surface)) = (&inner.x11, surface) {
                x11.write_cardinals(surface.xid(), STEAM_GAME, &[STEAM_APP_ID]);
            }
        };
        let window = window.upcast_ref::<gtk::Window>();
        if window.is_realized() {
            tag(window);
        }
        window.connect_realize(move |window| tag(window));
    }

    /// Who gamescope shows: the launcher, or a game we started.
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
    /// runs.
    pub fn bring_to_front(&self, id: &GameId) {
        let app_id = self
            .inner
            .running
            .borrow()
            .iter()
            .find(|(game, _)| game == id)
            .map(|&(_, app_id)| app_id);
        if let Some(app_id) = app_id {
            self.ask_for(Front::Game(app_id));
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
        let before = self.running_ids();
        self.inner.running.replace(running);
        let wanted =
            gamescope::front_after_sessions(self.inner.wanted.get(), &before, &self.running_ids());
        self.inner.wanted.set(wanted);
        self.write_base_layer();
    }

    fn ask_for(&self, front: Front) {
        glib::g_debug!("pindu", "gamescope: asking for {front:?} in front");
        self.inner.wanted.set(front);
        self.write_base_layer();
    }

    fn running_ids(&self) -> Vec<u32> {
        self.inner
            .running
            .borrow()
            .iter()
            .map(|&(_, app_id)| app_id)
            .collect()
    }

    fn expected(&self) -> Vec<ExpectedGame> {
        self.inner
            .running
            .borrow()
            .iter()
            .map(|(id, _)| ExpectedGame::new(id, self.inner.sessions.pid(id)))
            .collect()
    }

    /// Writes `GAMESCOPECTRL_BASELAYER_APPID` for the front the launcher wants.
    fn write_base_layer(&self) {
        if let Some(x11) = &self.inner.x11 {
            let list = gamescope::base_layer(self.inner.wanted.get(), &self.running_ids());
            x11.write_cardinals(x11.root, BASELAYER_APPID, &list);
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
        glib::g_debug!("pindu", "gamescope: {front:?} in front");
        for listener in self.inner.front_listeners.borrow().iter() {
            listener(front);
        }
    }

    /// Gives a window another client just mapped its game's app ID, when it is one.
    fn window_mapped(&self, xid: Xid) {
        let Some(x11) = &self.inner.x11 else {
            return;
        };
        let expected = self.expected();
        if expected.is_empty() {
            return;
        }
        let own: Vec<Xid> = gtk::Window::list_toplevels()
            .iter()
            .filter_map(|widget| widget.downcast_ref::<gtk::Window>()?.surface())
            .filter_map(|surface| surface.downcast::<X11Surface>().ok())
            .map(|surface| surface.xid())
            .collect();
        let Some(facts) = x11.window_facts(xid) else {
            return;
        };
        if let Some(app_id) = gamescope::tag_for_window(&facts, &expected, &own) {
            glib::g_info!("pindu", "gamescope: window {xid:#x} is app {app_id}");
            x11.write_cardinals_trapped(xid, STEAM_GAME, &[app_id]);
        }
    }
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
        let handler = move |_: &X11Display, event: *mut xlib::XEvent| {
            // SAFETY: GDK hands the handler the event it is processing, valid for the
            // call; `type_` is the first field of every event in the union.
            let kind = unsafe { (*event).type_ };
            let work = match kind {
                xlib::PropertyNotify => {
                    // SAFETY: a `PropertyNotify` event is an `XPropertyEvent`.
                    let property = unsafe { (*event).property };
                    (property.window == root && property.atom == gfx).then_some(Work::ReadFront)
                }
                xlib::MapNotify => {
                    // SAFETY: a `MapNotify` event is an `XMapEvent`.
                    let map = unsafe { (*event).map };
                    // The root's own `MapNotify` for its children: top-level windows.
                    (map.event == root && map.override_redirect == 0)
                        .then_some(Work::Mapped(map.window))
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
                            Work::Mapped(xid) => service.window_mapped(xid),
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
    /// Another client mapped this top-level window.
    Mapped(Xid),
}
