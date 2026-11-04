//! Turns controller input into app intents for the shells. Events from the backend are
//! queued through the main loop, so a modal dialog never stalls them, and held directions
//! repeat on a main-loop timer. Intents never become synthetic key events.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use pindu_core::gamepad::{DeviceKind, GamepadBackend, GamepadEvent, Intent, Intents};

/// Something the shells do with an intent.
type Listener = Box<dyn Fn(Intent)>;
/// Something the shells do when the active controller's kind changes.
type KindListener = Box<dyn Fn(Option<DeviceKind>)>;

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
    kind_listeners: RefCell<Vec<KindListener>>,
    /// The active kind the kind listeners last heard of.
    kind: Cell<Option<DeviceKind>>,
}

impl std::fmt::Debug for GamepadService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GamepadService")
            .field("listening", &self.inner.listening.get())
            .finish_non_exhaustive()
    }
}

impl GamepadService {
    pub fn new(backend: Box<dyn GamepadBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend: RefCell::new(backend),
                intents: RefCell::new(Intents::new()),
                listeners: RefCell::new(Vec::new()),
                repeat: RefCell::new(None),
                listening: Cell::new(false),
                kind_listeners: RefCell::new(Vec::new()),
                kind: Cell::new(None),
            }),
        }
    }

    /// Calls `listener` with every intent while the service listens.
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
            self.inner.intents.replace(Intents::new());
            glib::g_info!("pindu", "Controllers released");
            self.notify_kind();
        }
    }

    /// The kind of the controller last plugged in or used, for glyphs and the A/B order.
    pub fn active_kind(&self) -> Option<DeviceKind> {
        self.inner.intents.borrow().active_kind()
    }

    /// Calls `listener` whenever the active controller's kind changes, with `None` once no
    /// controller is left or the service stops listening.
    pub fn connect_active_kind_notify(&self, listener: impl Fn(Option<DeviceKind>) + 'static) {
        self.inner
            .kind_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    fn notify_kind(&self) {
        let kind = self.active_kind();
        if self.inner.kind.replace(kind) == kind {
            return;
        }
        for listener in self.inner.kind_listeners.borrow().iter() {
            listener(kind);
        }
    }

    fn handle(&self, event: GamepadEvent) {
        if !self.inner.listening.get() {
            return;
        }
        match &event {
            GamepadEvent::Connected { name, kind, .. } => {
                glib::g_info!("pindu", "Controller connected: {name} ({kind:?})");
            }
            GamepadEvent::Disconnected { .. } => glib::g_info!("pindu", "Controller disconnected"),
            _ => {}
        }
        let intent = self.inner.intents.borrow_mut().handle(&event, now());
        self.notify_kind();
        if let Some(intent) = intent {
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
