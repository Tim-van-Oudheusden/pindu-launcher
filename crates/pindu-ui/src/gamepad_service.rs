//! Turns controller input into app intents for the shells. Events from the backend are
//! queued through the main loop, so a modal dialog never stalls them, and held directions
//! repeat on a main-loop timer. Intents never become synthetic key events. Follows the
//! Button Prompts and Swap Confirm and Back settings and the buttons the player gave other
//! functions, which decide every pad's layout, keeps the connected pads for Quick Access's
//! Controllers, rumbles one to identify it, and waits for the button to give a function in
//! the menu drawer's Controls.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio::{self, prelude::*};
use gtk::glib;
use pindu_core::gamepad::{
    BINDINGS_KEY, Bindings, Button, CAPTURE_TIMEOUT, Capture, ControllerSettings, DeviceId,
    GamepadBackend, GamepadEvent, IDENTIFY_RUMBLE, Intent, Intents, Layout, PROMPTS_KEY, Pad,
    Prompts, SWAP_KEY, Swap,
};

use crate::config::APP_ID;

/// Something the shells do with an intent.
type Listener = Box<dyn Fn(Intent)>;
/// Something the shells do when the active controller's layout changes.
type LayoutListener = Box<dyn Fn(Option<Layout>)>;
/// Something the shells do when a pad comes or goes, or the settings change how they show.
type PadsListener = Box<dyn Fn()>;

/// Turns controller input into intents and hands them to the shells' listeners.
#[derive(Clone)]
pub struct GamepadService {
    inner: Rc<Inner>,
}

struct Inner {
    backend: RefCell<Box<dyn GamepadBackend>>,
    intents: RefCell<Intents>,
    listeners: RefCell<Vec<Listener>>,
    repeat: RefCell<Option<glib::SourceId>>,
    /// Whether controllers are open; events still queued after a stop are dropped.
    listening: Cell<bool>,
    layout_listeners: RefCell<Vec<LayoutListener>>,
    /// The active layout the layout listeners last heard of.
    layout: Cell<Option<Layout>>,
    pads_listeners: RefCell<Vec<PadsListener>>,
    /// Holds the Button Prompts, Swap Confirm and Back and bindings settings, and their
    /// signals.
    settings: gio::Settings,
    /// The wait for the button to give a function, while the menu drawer's Controls waits.
    capture: RefCell<Option<Capturing>>,
    /// The buttons pressed while waiting, by device: their presses go to the wait rather
    /// than to an intent until they are let go, as a trigger reports each move as a press.
    swallowed: RefCell<HashSet<(DeviceId, Button)>>,
}

/// A wait for the button to give `intent`.
struct Capturing {
    intent: Intent,
    /// Hears whether a button was given.
    done: Box<dyn FnOnce(bool)>,
    /// The time-out that ends the wait.
    timeout: glib::SourceId,
}

impl std::fmt::Debug for GamepadService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadService")
            .field("listening", &self.inner.listening.get())
            .finish_non_exhaustive()
    }
}

impl GamepadService {
    /// A service reading events from `backend`, with the controller settings applied.
    pub fn new(backend: Box<dyn GamepadBackend>) -> Self {
        let settings = gio::Settings::new(APP_ID);
        let mut intents = Intents::new();
        intents.set_settings(controller_settings(&settings));
        let service = Self {
            inner: Rc::new(Inner {
                backend: RefCell::new(backend),
                intents: RefCell::new(intents),
                listeners: RefCell::new(Vec::new()),
                repeat: RefCell::new(None),
                listening: Cell::new(false),
                layout_listeners: RefCell::new(Vec::new()),
                layout: Cell::new(None),
                pads_listeners: RefCell::new(Vec::new()),
                settings,
                capture: RefCell::new(None),
                swallowed: RefCell::new(HashSet::new()),
            }),
        };
        for key in [PROMPTS_KEY, SWAP_KEY, BINDINGS_KEY] {
            let weak = Rc::downgrade(&service.inner);
            service
                .inner
                .settings
                .connect_changed(Some(key), move |settings, _| {
                    if let Some(inner) = weak.upgrade() {
                        let service = GamepadService { inner };
                        service
                            .inner
                            .intents
                            .borrow_mut()
                            .set_settings(controller_settings(settings));
                        service.notify_layout();
                        service.notify_pads();
                    }
                });
        }
        service
    }

    /// Calls `listener` with every intent while the service listens, whichever window has
    /// focus: the shells act on those of their focused window, and the gamescope service
    /// reads Guide and Guide + A while a game is in front, where the shells take no intent.
    pub fn connect_intent(&self, listener: impl Fn(Intent) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Opens the controllers, or lets go of them. Nothing touches a controller until
    /// something asks to listen: libmanette takes over a Steam Deck's controls once it
    /// opens them.
    pub fn set_listening(&self, listening: bool) {
        if self.inner.listening.replace(listening) == listening {
            return;
        }
        if listening {
            let service = Rc::downgrade(&self.inner);
            self.inner
                .backend
                .borrow_mut()
                .start(Box::new(move |event| {
                    let service = service.clone();
                    glib::idle_add_local_once(move || {
                        if let Some(inner) = service.upgrade() {
                            GamepadService { inner }.handle(event);
                        }
                    });
                }));
        } else {
            self.inner.backend.borrow_mut().stop();
            if let Some(source) = self.inner.repeat.take() {
                source.remove();
            }
            self.finish_capture(None);
            self.inner.swallowed.borrow_mut().clear();
            let mut intents = Intents::new();
            intents.set_settings(self.controller_settings());
            self.inner.intents.replace(intents);
            glib::g_info!("pindu", "Controllers released");
            self.notify_layout();
            self.notify_pads();
        }
    }

    /// How long the button asking for `intent` has been held down, while one is: fullscreen
    /// mode's hold to confirm follows the confirm button with it.
    pub fn held_for(&self, intent: Intent) -> Option<Duration> {
        self.inner.intents.borrow().held_for(intent, now())
    }

    /// The layout of the controller last plugged in or used, for the hints' glyphs.
    pub fn active_layout(&self) -> Option<Layout> {
        self.inner.intents.borrow().active_layout()
    }

    /// Calls `listener` whenever the active controller's layout changes, with `None` once
    /// no controller is left or the service stops listening.
    pub fn connect_active_layout_notify(&self, listener: impl Fn(Option<Layout>) + 'static) {
        self.inner
            .layout_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// The connected pads, in the order they were plugged in.
    pub fn pads(&self) -> Vec<(DeviceId, Pad)> {
        self.inner
            .intents
            .borrow()
            .pads()
            .map(|(device, pad)| (device, pad.clone()))
            .collect()
    }

    /// The Button Prompts, Swap Confirm and Back and bindings settings in force.
    pub fn controller_settings(&self) -> ControllerSettings {
        self.inner.intents.borrow().settings()
    }

    /// Calls `listener` whenever a pad is plugged in or out, or the settings change the
    /// glyphs the pads show.
    pub fn connect_pads_changed(&self, listener: impl Fn() + 'static) {
        self.inner
            .pads_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Rumbles `device` briefly, so the user can tell which pad it is; a pad that cannot
    /// rumble stays still.
    pub fn identify(&self, device: DeviceId) {
        let rumble = self
            .inner
            .intents
            .borrow()
            .pads()
            .any(|(id, pad)| id == device && pad.rumble);
        if rumble {
            self.inner
                .backend
                .borrow_mut()
                .rumble(device, IDENTIFY_RUMBLE);
        }
    }

    /// Waits for the next press of a rebindable button on any controller and gives it to
    /// `intent`, swapping with the function that had it ([`Layout::rebind`]); the setting
    /// keeps it and every layout follows at once. Guide, or [`CAPTURE_TIMEOUT`] without a
    /// press, ends the wait with nothing changed, and the D-pad and the Steam Deck's "…"
    /// do nothing meanwhile: no press becomes an intent while it waits. `done` hears
    /// whether a button was given. A wait already running ends first, giving nothing.
    pub fn rebind_next_press(&self, intent: Intent, done: impl FnOnce(bool) + 'static) {
        self.finish_capture(None);
        let service = Rc::downgrade(&self.inner);
        let timeout = glib::timeout_add_local_once(CAPTURE_TIMEOUT, move || {
            let Some(inner) = service.upgrade() else {
                return;
            };
            // This timer has fired, so the wait is taken without removing it.
            let capture = inner.capture.take();
            if let Some(Capturing { done, .. }) = capture {
                done(false);
            }
        });
        self.inner.capture.replace(Some(Capturing {
            intent,
            done: Box::new(done),
            timeout,
        }));
    }

    /// The function waiting for a button, if one is.
    pub fn rebinding(&self) -> Option<Intent> {
        self.inner
            .capture
            .borrow()
            .as_ref()
            .map(|capture| capture.intent)
    }

    /// Ends a wait for a button, giving nothing.
    pub fn cancel_rebind(&self) {
        self.finish_capture(None);
    }

    /// Gives every button its default function again.
    pub fn reset_bindings(&self) {
        self.finish_capture(None);
        self.write_bindings(Bindings::default());
    }

    /// Ends the wait, giving `pressed` (a button, as pressed on a pad with that layout) to
    /// the function waiting for one.
    fn finish_capture(&self, pressed: Option<(Layout, Button)>) {
        let Some(capture) = self.inner.capture.take() else {
            return;
        };
        capture.timeout.remove();
        if let Some((layout, button)) = pressed {
            self.write_bindings(layout.rebind(capture.intent, button));
        }
        (capture.done)(pressed.is_some());
    }

    /// Keeps `bindings` in the setting, whose change applies them.
    fn write_bindings(&self, bindings: Bindings) {
        let overrides: HashMap<String, String> = bindings
            .overrides()
            .into_iter()
            .map(|(button, intent)| (button.to_owned(), intent.to_owned()))
            .collect();
        if let Err(error) = self
            .inner
            .settings
            .set_value(BINDINGS_KEY, &overrides.to_variant())
        {
            glib::g_warning!("pindu", "Could not keep the controller bindings: {error}");
        }
    }

    /// Hands a button press or release to the wait for a button, and says whether the wait
    /// took it, so that no intent comes of it. A button already held when the wait began
    /// (the one that chose the function, or a trigger reporting a further move as a press)
    /// is no new press. A release always goes on, so the intents know the button is up.
    fn capture_button(&self, device: DeviceId, button: Button, pressed: bool) -> bool {
        let inner = &self.inner;
        if !pressed {
            inner.swallowed.borrow_mut().remove(&(device, button));
            return false;
        }
        if inner.swallowed.borrow().contains(&(device, button)) {
            return true;
        }
        if inner.capture.borrow().is_none() {
            return false;
        }
        inner.swallowed.borrow_mut().insert((device, button));
        let capture = inner.intents.borrow().capture(device, button);
        match capture {
            Capture::Rebind(button) => {
                let layout = inner.intents.borrow().layout(device);
                self.finish_capture(Some((layout, button)));
            }
            Capture::Cancel => self.finish_capture(None),
            Capture::Ignore => {}
        }
        true
    }

    fn notify_layout(&self) {
        let layout = self.active_layout();
        if self.inner.layout.replace(layout) == layout {
            return;
        }
        for listener in self.inner.layout_listeners.borrow().iter() {
            listener(layout);
        }
    }

    fn notify_pads(&self) {
        for listener in self.inner.pads_listeners.borrow().iter() {
            listener();
        }
    }

    fn handle(&self, event: GamepadEvent) {
        if !self.inner.listening.get() {
            return;
        }
        let plugged = match &event {
            GamepadEvent::Connected { pad, .. } => {
                glib::g_info!(
                    "pindu",
                    "Controller connected: {} ({:?})",
                    pad.name,
                    pad.kind
                );
                true
            }
            GamepadEvent::Disconnected { .. } => {
                glib::g_info!("pindu", "Controller disconnected");
                true
            }
            _ => false,
        };
        if let GamepadEvent::Button {
            device,
            button,
            pressed,
        } = event
            && self.capture_button(device, button, pressed)
        {
            return;
        }
        let intent = self.inner.intents.borrow_mut().handle(&event, now());
        self.notify_layout();
        if plugged {
            self.notify_pads();
        }
        if let Some(intent) = intent {
            self.emit(intent);
        }
        self.schedule_repeat();
    }

    /// Hands `intent` to the listeners, unless a wait for a button holds the controller:
    /// the left stick still moves then, but moves nothing.
    fn emit(&self, intent: Intent) {
        if self.inner.capture.borrow().is_some() {
            return;
        }
        glib::g_debug!("pindu", "Gamepad intent: {intent:?}");
        for listener in self.inner.listeners.borrow().iter() {
            listener(intent);
        }
    }

    /// Sets the repeat timer to the held direction's next repeat, or clears it.
    fn schedule_repeat(&self) {
        if let Some(source) = self.inner.repeat.take() {
            source.remove();
        }
        let Some(due) = self.inner.intents.borrow().next_repeat() else {
            return;
        };
        let service = Rc::downgrade(&self.inner);
        let source = glib::timeout_add_local_once(due.saturating_sub(now()), move || {
            let Some(inner) = service.upgrade() else {
                return;
            };
            // This timer has fired; the next one is set below.
            inner.repeat.take();
            let service = GamepadService { inner };
            let intent = service.inner.intents.borrow_mut().repeat(now());
            if let Some(intent) = intent {
                service.emit(intent);
            }
            service.schedule_repeat();
        });
        *self.inner.repeat.borrow_mut() = Some(source);
    }
}

/// The monotonic clock the repeat timing runs on.
fn now() -> Duration {
    Duration::from_micros(glib::monotonic_time().max(0) as u64)
}

/// The Button Prompts, Swap Confirm and Back and bindings settings as `settings` hold
/// them.
fn controller_settings(settings: &gio::Settings) -> ControllerSettings {
    let overrides: HashMap<String, String> = settings.value(BINDINGS_KEY).get().unwrap_or_default();
    ControllerSettings {
        prompts: Prompts::from_setting(&settings.string(PROMPTS_KEY)),
        swap: Swap::from_setting(&settings.string(SWAP_KEY)),
        bindings: Bindings::from_overrides(overrides),
    }
}
