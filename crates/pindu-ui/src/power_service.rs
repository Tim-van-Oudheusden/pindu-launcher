//! Keeps the batteries the kernel reports (see `pindu_core::power`) for the shells: read
//! off the main thread at start-up, about once a minute after, and whenever a shell asks,
//! such as when Quick Access opens.

use std::cell::{Cell, RefCell};
use std::path::Path;
use std::rc::Rc;

use gtk::{gio, glib};
use pindu_core::power::{self, Batteries, REFRESH_INTERVAL, SYSFS_POWER_SUPPLY};

/// Something the shells do when the batteries change.
type Listener = Box<dyn Fn(&Batteries)>;

#[derive(Clone)]
pub struct PowerService {
    inner: Rc<Inner>,
}

#[derive(Default)]
struct Inner {
    batteries: RefCell<Batteries>,
    listeners: RefCell<Vec<Listener>>,
    /// Whether a read is running; a refresh asked for meanwhile is left to it.
    reading: Cell<bool>,
}

impl std::fmt::Debug for PowerService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PowerService")
            .field("batteries", &self.inner.batteries.borrow())
            .finish_non_exhaustive()
    }
}

impl PowerService {
    /// Reads the batteries now and then every [`REFRESH_INTERVAL`].
    pub fn new() -> Self {
        let service = Self {
            inner: Rc::default(),
        };
        let weak = Rc::downgrade(&service.inner);
        glib::timeout_add_local(REFRESH_INTERVAL, move || match weak.upgrade() {
            Some(inner) => {
                PowerService { inner }.refresh();
                glib::ControlFlow::Continue
            }
            None => glib::ControlFlow::Break,
        });
        service.refresh();
        service
    }

    /// The batteries as last read.
    pub fn batteries(&self) -> Batteries {
        self.inner.batteries.borrow().clone()
    }

    /// Calls `listener` whenever a read finds the batteries changed.
    pub fn connect_changed(&self, listener: impl Fn(&Batteries) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Reads the batteries again, off the main thread.
    pub fn refresh(&self) {
        if self.inner.reading.replace(true) {
            return;
        }
        let service = self.clone();
        glib::spawn_future_local(async move {
            let read =
                gio::spawn_blocking(|| power::read_batteries(Path::new(SYSFS_POWER_SUPPLY))).await;
            service.inner.reading.set(false);
            let Ok(batteries) = read else {
                return;
            };
            if *service.inner.batteries.borrow() == batteries {
                return;
            }
            service.inner.batteries.replace(batteries);
            let batteries = service.batteries();
            for listener in service.inner.listeners.borrow().iter() {
                listener(&batteries);
            }
        });
    }
}

impl Default for PowerService {
    fn default() -> Self {
        Self::new()
    }
}
