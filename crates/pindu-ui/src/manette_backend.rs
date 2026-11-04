//! The libmanette 0.2 gamepad backend: the only code that touches controllers. It reports
//! libmanette's mapped buttons and axes (SDL's mapping database), so every pad shows up as
//! the same positions; pads without a mapping send their D-pad as a hat, which becomes
//! D-pad buttons here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pindu_core::gamepad::{Axis, Button, DeviceId, DeviceKind, GamepadBackend, GamepadEvent};

/// Linux input codes libmanette maps buttons and axes to.
mod code {
    pub const BTN_SOUTH: u16 = 0x130;
    pub const BTN_EAST: u16 = 0x131;
    pub const BTN_NORTH: u16 = 0x133;
    pub const BTN_WEST: u16 = 0x134;
    pub const BTN_TL: u16 = 0x136;
    pub const BTN_TR: u16 = 0x137;
    pub const BTN_SELECT: u16 = 0x13a;
    pub const BTN_START: u16 = 0x13b;
    pub const BTN_MODE: u16 = 0x13c;
    pub const BTN_DPAD_UP: u16 = 0x220;
    pub const BTN_DPAD_DOWN: u16 = 0x221;
    pub const BTN_DPAD_LEFT: u16 = 0x222;
    pub const BTN_DPAD_RIGHT: u16 = 0x223;
    pub const ABS_X: u16 = 0x00;
    pub const ABS_Y: u16 = 0x01;
    pub const ABS_HAT0X: u16 = 0x10;
    pub const ABS_HAT0Y: u16 = 0x11;
}

/// Controllers through libmanette's monitor. Nothing is opened until [`start`]: libmanette
/// takes over a Steam Deck's controls (its "lizard mode") as soon as it opens them.
///
/// [`start`]: GamepadBackend::start
#[derive(Default)]
pub struct ManetteBackend {
    monitor: Option<manette::Monitor>,
}

impl ManetteBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl GamepadBackend for ManetteBackend {
    fn start(&mut self, sink: Box<dyn Fn(GamepadEvent)>) {
        let monitor = manette::Monitor::new();
        let devices = Rc::new(Devices {
            sink,
            ids: RefCell::new(Vec::new()),
            next_id: Cell::new(1),
        });
        for device in monitor.devices() {
            devices.attach(&device);
        }
        monitor.connect_device_connected({
            let devices = devices.clone();
            move |_, device| devices.attach(device)
        });
        monitor.connect_device_disconnected(move |_, device| devices.detach(device));
        self.monitor = Some(monitor);
    }
}

/// The devices seen so far, numbered as they appear.
struct Devices {
    sink: Box<dyn Fn(GamepadEvent)>,
    ids: RefCell<Vec<(manette::Device, DeviceId)>>,
    next_id: Cell<u64>,
}

impl Devices {
    fn attach(self: &Rc<Self>, device: &manette::Device) {
        let id = DeviceId(self.next_id.replace(self.next_id.get() + 1));
        self.ids.borrow_mut().push((device.clone(), id));
        let steam_deck = device.device_type() == manette::DeviceType::SteamDeck;
        (self.sink)(GamepadEvent::Connected {
            device: id,
            name: device.name().map(Into::into).unwrap_or_default(),
            kind: DeviceKind::detect(device.guid().as_deref(), steam_deck),
        });
        device.connect_button_press_event({
            let devices = self.clone();
            move |_, event| devices.button(id, event, true)
        });
        device.connect_button_release_event({
            let devices = self.clone();
            move |_, event| devices.button(id, event, false)
        });
        device.connect_absolute_axis_event({
            let devices = self.clone();
            move |_, event| devices.axis(id, event)
        });
        device.connect_hat_axis_event({
            let devices = self.clone();
            move |_, event| devices.hat(id, event)
        });
    }

    fn detach(&self, device: &manette::Device) {
        let mut ids = self.ids.borrow_mut();
        if let Some(index) = ids.iter().position(|(known, _)| known == device) {
            let (_, id) = ids.remove(index);
            drop(ids);
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
        code::BTN_SELECT => Button::Select,
        code::BTN_START => Button::Start,
        code::BTN_MODE => Button::Guide,
        code::BTN_DPAD_UP => Button::DpadUp,
        code::BTN_DPAD_DOWN => Button::DpadDown,
        code::BTN_DPAD_LEFT => Button::DpadLeft,
        code::BTN_DPAD_RIGHT => Button::DpadRight,
        _ => return None,
    })
}
