//! Turns controller input into app intents for the shells. Events from the backend are
//! queued through the main loop, so a modal dialog never stalls them, and held directions
//! repeat on a main-loop timer. Intents never become synthetic key events. Follows the
//! Button Prompts, Swap Confirm and Back and button bindings settings, which decide every
//! pad's layout, keeps the connected pads for Quick Access's Controllers, and rumbles one
//! to identify it. While the menu drawer's Controls waits for a button for a function, the
//! next button that can be bound is bound to it and kept in the settings; any other input
//! ends the wait and acts as usual.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::time::Duration;

use gtk::gio::{self, prelude::*};
use gtk::glib;
use pindu_core::bindings::{BINDINGS_KEY, Bindings};
use pindu_core::gamepad::{
    ControllerSettings, DeviceId, GamepadBackend, GamepadEvent, IDENTIFY_RUMBLE, Intent, Intents,
    Layout, PROMPTS_KEY, Pad, Prompts, SWAP_KEY, Swap,
};

use crate::config::APP_ID;

/// Something the shells do with an intent.
type Listener = Box<dyn Fn(Intent)>;
/// Something the shells do when the active controller's layout changes.
type LayoutListener = Box<dyn Fn(Option<Layout>)>;
/// Something the shells do when a pad comes or goes, or the settings change how they show.
type PadsListener = Box<dyn Fn()>;
/// Something the shells do when a wait for a button to bind starts or ends.
type BindingListener = Box<dyn Fn(Option<Intent>)>;

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
    /// The function waiting for a button to be bound to, while the drawer's Controls waits.
    binding: Cell<Option<Intent>>,
    binding_listeners: RefCell<Vec<BindingListener>>,
    /// Holds the Button Prompts, Swap Confirm and Back and bindings settings, and their
    /// signals.
    settings: gio::Settings,
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
                binding: Cell::new(None),
                binding_listeners: RefCell::new(Vec::new()),
                settings,
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
            self.stop_binding();
            if let Some(source) = self.inner.repeat.take() {
                source.remove();
            }
            let mut intents = Intents::new();
            intents.set_settings(self.controller_settings());
            self.inner.intents.replace(intents);
            glib::g_info!("pindu", "Controllers released");
            self.notify_layout();
            self.notify_pads();
        }
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

    /// Waits for the next press of a button that can be bound, to bind `function` to it
    /// (see `pindu_core::bindings`), until that press or any other input.
    pub fn start_binding(&self, function: Intent) {
        self.set_binding(Some(function));
    }

    /// Stops waiting for a button to bind: the Controls level was left.
    pub fn stop_binding(&self) {
        self.set_binding(None);
    }

    /// The function waiting for a button to be bound to, if any.
    pub fn binding(&self) -> Option<Intent> {
        self.inner.binding.get()
    }

    /// Calls `listener` whenever a wait for a button to bind starts or ends, with the
    /// function that waits.
    pub fn connect_binding_notify(&self, listener: impl Fn(Option<Intent>) + 'static) {
        self.inner
            .binding_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Gives every function its default button again.
    pub fn reset_bindings(&self) {
        self.stop_binding();
        self.inner.settings.reset(BINDINGS_KEY);
    }

    fn set_binding(&self, function: Option<Intent>) {
        if self.inner.binding.replace(function) == function {
            return;
        }
        for listener in self.inner.binding_listeners.borrow().iter() {
            listener(function);
        }
    }

    /// Keeps `bindings` in the settings, which apply them to the intents and the layouts.
    fn save_bindings(&self, bindings: Bindings) {
        let value: BTreeMap<&str, &str> = bindings.setting().into_iter().collect();
        if let Err(error) = self
            .inner
            .settings
            .set_value(BINDINGS_KEY, &value.to_variant())
        {
            glib::g_warning!("pindu", "Could not keep the button bindings: {error}");
        }
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
        if let Some(function) = self.inner.binding.get() {
            let bound = self.inner.intents.borrow_mut().bind_press(&event, function);
            if let Some(bindings) = bound {
                glib::g_info!("pindu", "Bound {function:?}");
                self.set_binding(None);
                self.save_bindings(bindings);
                self.notify_layout();
                self.notify_pads();
                return;
            }
        }
        let intent = self.inner.intents.borrow_mut().handle(&event, now());
        self.notify_layout();
        if plugged {
            self.notify_pads();
        }
        if let Some(intent) = intent {
            // Guide, the D-pad or the stick: the wait ends, and they act as usual.
            self.set_binding(None);
            self.emit(intent);
        }
        self.schedule_repeat();
    }

    fn emit(&self, intent: Intent) {
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

/// The Button Prompts, Swap Confirm and Back and bindings settings as `settings` hold them.
fn controller_settings(settings: &gio::Settings) -> ControllerSettings {
    let bindings = settings
        .value(BINDINGS_KEY)
        .get::<HashMap<String, String>>()
        .unwrap_or_default();
    ControllerSettings {
        prompts: Prompts::from_setting(&settings.string(PROMPTS_KEY)),
        swap: Swap::from_setting(&settings.string(SWAP_KEY)),
        bindings: Bindings::from_setting(
            bindings
                .iter()
                .map(|(button, function)| (button.as_str(), function.as_str())),
        ),
    }
}
