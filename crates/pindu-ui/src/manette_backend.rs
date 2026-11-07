//! The libmanette 0.2 gamepad backend: the only code that touches controllers. It reports
//! libmanette's mapped buttons and axes (SDL's mapping database), so every pad shows up as
//! the same positions; pads without a mapping send their D-pad as a hat, which becomes
//! D-pad buttons here. Mapped triggers arrive as buttons, even on pads that report them as
//! axes.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gtk::glib::{self, prelude::*};
use pindu_core::gamepad::{
    Axis, Button, DeviceId, DeviceKind, GamepadBackend, GamepadEvent, HidIds, Pad, Rumble,
};

/// Linux input codes libmanette maps buttons and axes to.
mod code {
    pub(crate) const BTN_SOUTH: u16 = 0x130;
    pub(crate) const BTN_EAST: u16 = 0x131;
    pub(crate) const BTN_NORTH: u16 = 0x133;
    pub(crate) const BTN_WEST: u16 = 0x134;
    pub(crate) const BTN_TL: u16 = 0x136;
    pub(crate) const BTN_TR: u16 = 0x137;
    pub(crate) const BTN_TL2: u16 = 0x138;
    pub(crate) const BTN_TR2: u16 = 0x139;
    pub(crate) const BTN_SELECT: u16 = 0x13a;
    pub(crate) const BTN_START: u16 = 0x13b;
    pub(crate) const BTN_MODE: u16 = 0x13c;
    pub(crate) const BTN_THUMBR: u16 = 0x13e;
    pub(crate) const BTN_DPAD_UP: u16 = 0x220;
    pub(crate) const BTN_DPAD_DOWN: u16 = 0x221;
    pub(crate) const BTN_DPAD_LEFT: u16 = 0x222;
    pub(crate) const BTN_DPAD_RIGHT: u16 = 0x223;
    /// What libmanette maps `misc1` to, and its Steam Deck driver the "…" button.
    pub(crate) const BTN_TRIGGER_HAPPY5: u16 = 0x2c4;
    pub(crate) const ABS_X: u16 = 0x00;
    pub(crate) const ABS_Y: u16 = 0x01;
    pub(crate) const ABS_HAT0X: u16 = 0x10;
    pub(crate) const ABS_HAT0Y: u16 = 0x11;
}

/// Controllers through libmanette's monitor. Nothing is opened until [`start`]: libmanette
/// takes over a Steam Deck's controls (its "lizard mode") as soon as it opens them, and
/// [`stop`] lets go of them again.
///
/// [`start`]: GamepadBackend::start
/// [`stop`]: GamepadBackend::stop
#[derive(Default)]
pub struct ManetteBackend {
    running: Option<(manette::Monitor, Rc<Devices>)>,
}

impl ManetteBackend {
    /// A backend with no controllers open.
    pub fn new() -> Self {
        Self::default()
    }
}

impl GamepadBackend for ManetteBackend {
    fn start(&mut self, sink: Box<dyn Fn(GamepadEvent)>) {
        self.stop();
        let monitor = manette::Monitor::new();
        let devices = Rc::new(Devices {
            sink,
            attached: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
        });
        for device in monitor.devices() {
            devices.attach(&device);
        }
        monitor.connect_device_connected({
            let devices = devices.clone();
            move |_, device| devices.attach(device)
        });
        monitor.connect_device_disconnected({
            let devices = devices.clone();
            move |_, device| devices.detach(device)
        });
        self.running = Some((monitor, devices));
    }

    fn stop(&mut self) {
        if let Some((monitor, devices)) = self.running.take() {
            // The devices' handlers hold `devices`; without them the monitor is the last
            // owner of each device, and dropping it closes them.
            for attached in devices.attached.take() {
                attached.disconnect();
            }
            drop(monitor);
        }
    }

    fn rumble(&mut self, device: DeviceId, rumble: Rumble) {
        let Some((_, devices)) = &self.running else {
            return;
        };
        let attached = devices.attached.borrow();
        let Some(known) = attached.iter().find(|known| known.id == device) else {
            return;
        };
        if !known.device.has_rumble() {
            return;
        }
        let magnitude = |strength: f64| (strength.clamp(0.0, 1.0) * f64::from(u16::MAX)) as u16;
        let milliseconds = u16::try_from(rumble.duration.as_millis()).unwrap_or(u16::MAX);
        if !known.device.rumble(
            magnitude(rumble.strong),
            magnitude(rumble.weak),
            milliseconds,
        ) {
            glib::g_info!("pindu", "Controller {} did not rumble", device.0);
        }
    }
}

/// The devices seen so far, numbered as they appear.
struct Devices {
    sink: Box<dyn Fn(GamepadEvent)>,
    attached: RefCell<Vec<Attached>>,
    next_id: Cell<u64>,
}

/// A device we listen to, with its number and our handlers on it.
struct Attached {
    device: manette::Device,
    id: DeviceId,
    handlers: Vec<glib::SignalHandlerId>,
}

impl Attached {
    fn disconnect(self) {
        for handler in self.handlers {
            self.device.disconnect(handler);
        }
    }
}

impl Devices {
    fn attach(self: &Rc<Self>, device: &manette::Device) {
        let id = DeviceId(self.next_id.replace(self.next_id.get() + 1));
        let steam_deck = device.device_type() == manette::DeviceType::SteamDeck;
        let guid = device.guid();
        (self.sink)(GamepadEvent::Connected {
            device: id,
            pad: Pad {
                name: device.name().map(Into::into).unwrap_or_default(),
                kind: DeviceKind::detect(guid.as_deref(), steam_deck),
                ids: guid.as_deref().and_then(HidIds::from_guid),
                rumble: device.has_rumble(),
            },
        });
        let handlers = vec![
            device.connect_button_press_event({
                let devices = self.clone();
                move |_, event| devices.button(id, event, true)
            }),
            device.connect_button_release_event({
                let devices = self.clone();
                move |_, event| devices.button(id, event, false)
            }),
            device.connect_absolute_axis_event({
                let devices = self.clone();
                move |_, event| devices.axis(id, event)
            }),
            device.connect_hat_axis_event({
                let devices = self.clone();
                move |_, event| devices.hat(id, event)
            }),
        ];
        self.attached.borrow_mut().push(Attached {
            device: device.clone(),
            id,
            handlers,
        });
    }

    fn detach(&self, device: &manette::Device) {
        let mut attached = self.attached.borrow_mut();
        if let Some(index) = attached.iter().position(|known| known.device == *device) {
            let known = attached.remove(index);
            drop(attached);
            let id = known.id;
            known.disconnect();
            (self.sink)(GamepadEvent::Disconnected { device: id });
        }
    }

    fn button(&self, device: DeviceId, event: &manette::Event, pressed: bool) {
        if let Some(button) = event.button().and_then(button) {
            (self.sink)(GamepadEvent::Button {
                device,
                button,
                pressed,
            });
        }
    }

    fn axis(&self, device: DeviceId, event: &manette::Event) {
        let axis = match event.absolute() {
            Some((code::ABS_X, value)) => (Axis::LeftX, value),
            Some((code::ABS_Y, value)) => (Axis::LeftY, value),
            _ => return,
        };
        (self.sink)(GamepadEvent::Axis {
            device,
            axis: axis.0,
            value: axis.1,
        });
    }

    /// An unmapped pad's D-pad: each hat axis presses one of its two buttons, or releases
    /// both when centred.
    fn hat(&self, device: DeviceId, event: &manette::Event) {
        let (negative, positive, value) = match event.hat() {
            Some((code::ABS_HAT0X, value)) => (Button::DpadLeft, Button::DpadRight, value),
            Some((code::ABS_HAT0Y, value)) => (Button::DpadUp, Button::DpadDown, value),
            _ => return,
        };
        for (button, pressed) in [(negative, value < 0), (positive, value > 0)] {
            (self.sink)(GamepadEvent::Button {
                device,
                button,
                pressed,
            });
        }
    }
}

fn button(code: u16) -> Option<Button> {
    Some(match code {
        code::BTN_SOUTH => Button::South,
        code::BTN_EAST => Button::East,
        code::BTN_NORTH => Button::North,
        code::BTN_WEST => Button::West,
        code::BTN_TL => Button::LeftShoulder,
        code::BTN_TR => Button::RightShoulder,
        code::BTN_TL2 => Button::LeftTrigger,
        code::BTN_TR2 => Button::RightTrigger,
        code::BTN_SELECT => Button::Select,
        code::BTN_START => Button::Start,
        code::BTN_MODE => Button::Guide,
        code::BTN_THUMBR => Button::RightStick,
        code::BTN_TRIGGER_HAPPY5 => Button::Misc1,
        code::BTN_DPAD_UP => Button::DpadUp,
        code::BTN_DPAD_DOWN => Button::DpadDown,
        code::BTN_DPAD_LEFT => Button::DpadLeft,
        code::BTN_DPAD_RIGHT => Button::DpadRight,
        _ => return None,
    })
}
