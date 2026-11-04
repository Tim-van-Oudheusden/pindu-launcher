//! Turns controller input into app intents for the shells. Events from the backend are
//! queued through the main loop, so a modal dialog never stalls them, and held directions
//! repeat on a main-loop timer. Intents never become synthetic key events.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use gtk::glib;
use pindu_core::gamepad::{DeviceKind, GamepadBackend, GamepadEvent, Intent, Intents};

/// Something the shells do with an intent.
type Listener = Box<dyn Fn(Intent)>;

#[derive(Clone)]
pub struct GamepadService {
    inner: Rc<Inner>,
}

struct Inner {
    backend: RefCell<Box<dyn GamepadBackend>>,
    intents: RefCell<Intents>,
    listeners: RefCell<Vec<Listener>>,
    repeat: RefCell<Option<glib::SourceId>>,
}

impl GamepadService {
    pub fn new(backend: Box<dyn GamepadBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend: RefCell::new(backend),
                intents: RefCell::new(Intents::new()),
                listeners: RefCell::new(Vec::new()),
                repeat: RefCell::new(None),
            }),
        }
    }

    /// Calls `listener` with every intent. Controllers are only opened once the first
    /// listener connects, so nothing touches them until something wants their input.
    pub fn connect_intent(&self, listener: impl Fn(Intent) + 'static) {
        let first = self.inner.listeners.borrow().is_empty();
        self.inner.listeners.borrow_mut().push(Box::new(listener));
        if first {
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
        }
    }

    /// The kind of the controller last plugged in or used, for glyphs and the A/B order.
    pub fn active_kind(&self) -> Option<DeviceKind> {
        self.inner.intents.borrow().active_kind()
    }

    fn handle(&self, event: GamepadEvent) {
        match &event {
            GamepadEvent::Connected { name, kind, .. } => {
                glib::g_info!("pindu", "Controller connected: {name} ({kind:?})");
            }
            GamepadEvent::Disconnected { .. } => glib::g_info!("pindu", "Controller disconnected"),
            _ => {}
        }
        let intent = self.inner.intents.borrow_mut().handle(&event, now());
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
