//! Turns controller input into app intents, so the shells never see hardware. Devices come
//! from a [`GamepadBackend`]; [`Intents`] maps their mapped buttons and sticks to intents,
//! repeats held directions, and tracks each device's kind for glyphs and the A/B swap.

use std::collections::HashMap;
use std::time::Duration;

/// A connected controller, as its backend numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);

/// A controller button, by its position on an Xbox-style pad (SDL's mapping): `South` is
/// A on Xbox, Cross on PlayStation and B on Nintendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    /// View, Back, Share or Minus.
    Select,
    /// Menu, Options or Plus.
    Start,
    /// Xbox, PS or Home.
    Guide,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
}

/// A stick axis: -1 is left or up, 1 right or down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    LeftX,
    LeftY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// What the user asks the app to do. Intents never become synthetic key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Navigate(Direction),
    Activate,
    Back,
    PreviousTab,
    NextTab,
    Search,
    Menu,
    Options,
    Home,
}

/// The kind of controller, which decides its glyphs and whether A and B swap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Xbox,
    PlayStation,
    /// A on the east, B on the south: confirm and back swap.
    Nintendo,
    SteamDeck,
    /// Anything else, shown with Xbox-style glyphs.
    Generic,
}

impl DeviceKind {
    /// The kind of a device from its SDL GUID, which holds its USB vendor and product IDs
    /// (little-endian, at bytes 4 and 8), and from whether libmanette drives it as a Steam
    /// Deck.
    pub fn detect(guid: Option<&str>, steam_deck: bool) -> Self {
        if steam_deck {
            return Self::SteamDeck;
        }
        let Some((vendor, product)) = guid.and_then(usb_ids) else {
            return Self::Generic;
        };
        match (vendor, product) {
            (0x045e, _) => Self::Xbox,
            (0x054c, _) => Self::PlayStation,
            (0x057e, _) => Self::Nintendo,
            (0x28de, 0x1205) => Self::SteamDeck,
            // Steam Input's virtual pad, laid out like an Xbox 360 pad.
            (0x28de, 0x11ff) => Self::Xbox,
            _ => Self::Generic,
        }
    }
}

/// The USB vendor and product IDs in an SDL GUID.
fn usb_ids(guid: &str) -> Option<(u16, u16)> {
    let word = |at: usize| {
        let bytes = guid.get(at..at + 4)?;
        let low = u8::from_str_radix(&bytes[..2], 16).ok()?;
        let high = u8::from_str_radix(&bytes[2..], 16).ok()?;
        Some(u16::from_le_bytes([low, high]))
    };
    if guid.len() != 32 {
        return None;
    }
    Some((word(8)?, word(16)?))
}

/// Input from a backend, already mapped to buttons and axes.
#[derive(Debug, Clone, PartialEq)]
pub enum GamepadEvent {
    /// A device was plugged in, or was already there when the backend started.
    Connected {
        device: DeviceId,
        name: String,
        kind: DeviceKind,
    },
    Disconnected {
        device: DeviceId,
    },
    Button {
        device: DeviceId,
        button: Button,
        pressed: bool,
    },
    /// A stick axis moved, from -1 to 1.
    Axis {
        device: DeviceId,
        axis: Axis,
        value: f64,
    },
}

/// How long a direction is held before it repeats (Heroic's default).
pub const REPEAT_DELAY: Duration = Duration::from_millis(300);
/// How often a held direction repeats after that.
pub const REPEAT_INTERVAL: Duration = Duration::from_millis(50);
/// How far a stick must move before it navigates.
pub const DEADZONE: f64 = 0.5;

/// A source of controller events: libmanette 0.2 today, its 1.0 API or SDL3 later. All
/// device access sits behind this trait.
pub trait GamepadBackend {
    /// Starts reporting events to `sink` on the main thread: a `Connected` for each device
    /// already plugged in, then input and hotplug as they happen, until it is dropped.
    fn start(&mut self, sink: Box<dyn Fn(GamepadEvent)>);
}

/// Maps controller input to intents.
#[derive(Debug, Default)]
pub struct Intents {
    kinds: HashMap<DeviceId, DeviceKind>,
    sticks: HashMap<DeviceId, [f64; 2]>,
    held: Option<Held>,
    /// The device last plugged in or used, whose kind the glyphs show.
    active: Option<DeviceId>,
}

/// The direction being held, and when it next repeats.
#[derive(Debug, Clone, Copy)]
struct Held {
    device: DeviceId,
    source: Source,
    direction: Direction,
    due: Duration,
}

/// What holds a direction: a D-pad button or the left stick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Source {
    Button(Button),
    Stick,
}

impl Intents {
    pub fn new() -> Self {
        Self::default()
    }

    /// The intent an event asks for now, if any. `now` is a monotonic time, used to repeat
    /// held directions.
    pub fn handle(&mut self, event: &GamepadEvent, now: Duration) -> Option<Intent> {
        match *event {
            GamepadEvent::Connected { device, kind, .. } => {
                self.kinds.insert(device, kind);
                self.active = Some(device);
                None
            }
            GamepadEvent::Disconnected { device } => {
                self.kinds.remove(&device);
                self.sticks.remove(&device);
                if self.held.is_some_and(|held| held.device == device) {
                    self.held = None;
                }
                if self.active == Some(device) {
                    self.active = self.kinds.keys().min().copied();
                }
                None
            }
            GamepadEvent::Button {
                device,
                button,
                pressed: true,
            } => {
                self.active = Some(device);
                let intent = button_intent(self.confirm_and_back_swapped(device, button));
                if let Intent::Navigate(direction) = intent {
                    self.hold(device, Source::Button(button), direction, now);
                }
                Some(intent)
            }
            GamepadEvent::Button {
                device,
                button,
                pressed: false,
            } => {
                self.release(device, Source::Button(button));
                None
            }
            GamepadEvent::Axis {
                device,
                axis,
                value,
            } => {
                let stick = self.sticks.entry(device).or_default();
                stick[axis as usize] = value;
                let direction = stick_direction(*stick);
                let held = self
                    .held
                    .filter(|held| held.device == device && held.source == Source::Stick)
                    .map(|held| held.direction);
                match direction {
                    Some(direction) if held != Some(direction) => {
                        self.hold(device, Source::Stick, direction, now);
                        Some(Intent::Navigate(direction))
                    }
                    Some(_) => None,
                    None => {
                        self.release(device, Source::Stick);
                        None
                    }
                }
            }
        }
    }

    fn hold(&mut self, device: DeviceId, source: Source, direction: Direction, now: Duration) {
        self.held = Some(Held {
            device,
            source,
            direction,
            due: now + REPEAT_DELAY,
        });
    }

    /// Stops the repeat when this is what holds the direction.
    fn release(&mut self, device: DeviceId, source: Source) {
        if self
            .held
            .is_some_and(|held| held.device == device && held.source == source)
        {
            self.held = None;
        }
    }

    /// The repeat of a held direction, when one is due at `now`.
    pub fn repeat(&mut self, now: Duration) -> Option<Intent> {
        let held = self.held.as_mut().filter(|held| held.due <= now)?;
        held.due = now + REPEAT_INTERVAL;
        Some(Intent::Navigate(held.direction))
    }

    /// When the held direction next repeats, if one is held.
    pub fn next_repeat(&self) -> Option<Duration> {
        self.held.map(|held| held.due)
    }

    /// The kind of the device last plugged in or used, for glyphs; `None` without one.
    pub fn active_kind(&self) -> Option<DeviceKind> {
        self.kinds.get(&self.active?).copied()
    }

    /// The button as an Xbox pad would place it: Nintendo pads' A (east) confirms and their
    /// B (south) goes back.
    fn confirm_and_back_swapped(&self, device: DeviceId, button: Button) -> Button {
        match (self.kinds.get(&device), button) {
            (Some(DeviceKind::Nintendo), Button::East) => Button::South,
            (Some(DeviceKind::Nintendo), Button::South) => Button::East,
            _ => button,
        }
    }
}

/// The direction a stick points, once it is past the deadzone: the larger deflection wins.
fn stick_direction([x, y]: [f64; 2]) -> Option<Direction> {
    if x.abs().max(y.abs()) <= DEADZONE {
        return None;
    }
    Some(if x.abs() >= y.abs() {
        if x > 0.0 {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if y > 0.0 {
        Direction::Down
    } else {
        Direction::Up
    })
}

fn button_intent(button: Button) -> Intent {
    match button {
        Button::South => Intent::Activate,
        Button::East => Intent::Back,
        Button::North => Intent::Search,
        Button::West | Button::Select => Intent::Options,
        Button::Start => Intent::Menu,
        Button::Guide => Intent::Home,
        Button::LeftShoulder => Intent::PreviousTab,
        Button::RightShoulder => Intent::NextTab,
        Button::DpadUp => Intent::Navigate(Direction::Up),
        Button::DpadDown => Intent::Navigate(Direction::Down),
        Button::DpadLeft => Intent::Navigate(Direction::Left),
        Button::DpadRight => Intent::Navigate(Direction::Right),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    const PAD: DeviceId = DeviceId(1);

    fn press(button: Button) -> GamepadEvent {
        GamepadEvent::Button {
            device: PAD,
            button,
            pressed: true,
        }
    }

    fn release(button: Button) -> GamepadEvent {
        GamepadEvent::Button {
            device: PAD,
            button,
            pressed: false,
        }
    }

    #[test]
    fn buttons_map_to_intents_by_position() {
        let mut intents = Intents::new();
        let cases = [
            (Button::South, Intent::Activate),
            (Button::East, Intent::Back),
            (Button::North, Intent::Search),
            (Button::West, Intent::Options),
            (Button::Select, Intent::Options),
            (Button::Start, Intent::Menu),
            (Button::Guide, Intent::Home),
            (Button::LeftShoulder, Intent::PreviousTab),
            (Button::RightShoulder, Intent::NextTab),
            (Button::DpadUp, Intent::Navigate(Direction::Up)),
            (Button::DpadDown, Intent::Navigate(Direction::Down)),
            (Button::DpadLeft, Intent::Navigate(Direction::Left)),
            (Button::DpadRight, Intent::Navigate(Direction::Right)),
        ];

        for (button, intent) in cases {
            assert_eq!(
                intents.handle(&press(button), Duration::ZERO),
                Some(intent),
                "{button:?}"
            );
            assert_eq!(
                intents.handle(&release(button), Duration::ZERO),
                None,
                "{button:?}"
            );
        }
    }

    fn connected(kind: DeviceKind) -> GamepadEvent {
        GamepadEvent::Connected {
            device: PAD,
            name: "Pad".into(),
            kind,
        }
    }

    #[test]
    fn nintendo_pads_confirm_with_their_a_button_on_the_east() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Nintendo), Duration::ZERO);

        assert_eq!(
            intents.handle(&press(Button::East), Duration::ZERO),
            Some(Intent::Activate)
        );
        assert_eq!(
            intents.handle(&press(Button::South), Duration::ZERO),
            Some(Intent::Back)
        );

        intents.handle(&connected(DeviceKind::Xbox), Duration::ZERO);
        assert_eq!(
            intents.handle(&press(Button::South), Duration::ZERO),
            Some(Intent::Activate)
        );
    }

    fn ms(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn a_held_direction_repeats_after_300_ms_then_every_50_ms_until_released() {
        let mut intents = Intents::new();
        let down = Intent::Navigate(Direction::Down);

        assert_eq!(
            intents.handle(&press(Button::DpadDown), ms(1000)),
            Some(down)
        );
        assert_eq!(intents.next_repeat(), Some(ms(1300)));
        assert_eq!(intents.repeat(ms(1299)), None);
        assert_eq!(intents.repeat(ms(1300)), Some(down));
        assert_eq!(intents.next_repeat(), Some(ms(1350)));
        assert_eq!(intents.repeat(ms(1350)), Some(down));

        intents.handle(&release(Button::DpadDown), ms(1360));

        assert_eq!(intents.next_repeat(), None);
        assert_eq!(intents.repeat(ms(2000)), None);
    }

    fn stick(axis: Axis, value: f64) -> GamepadEvent {
        GamepadEvent::Axis {
            device: PAD,
            axis,
            value,
        }
    }

    #[test]
    fn the_left_stick_navigates_past_half_deflection_and_repeats_while_held() {
        let mut intents = Intents::new();
        let (right, up) = (
            Intent::Navigate(Direction::Right),
            Intent::Navigate(Direction::Up),
        );

        assert_eq!(intents.handle(&stick(Axis::LeftX, 0.5), ms(0)), None);
        assert_eq!(
            intents.handle(&stick(Axis::LeftX, 0.6), ms(10)),
            Some(right)
        );
        // Pushing further the same way is the same push.
        assert_eq!(intents.handle(&stick(Axis::LeftX, 0.9), ms(20)), None);
        assert_eq!(intents.repeat(ms(310)), Some(right));
        // The larger deflection decides the direction.
        assert_eq!(
            intents.handle(&stick(Axis::LeftY, -0.95), ms(320)),
            Some(up)
        );

        intents.handle(&stick(Axis::LeftX, 0.1), ms(330));
        assert_eq!(intents.handle(&stick(Axis::LeftY, -0.2), ms(340)), None);

        assert_eq!(intents.next_repeat(), None);
    }

    #[test]
    fn unplugging_a_pad_stops_the_direction_it_holds() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        intents.handle(&press(Button::DpadLeft), ms(0));

        intents.handle(&GamepadEvent::Disconnected { device: PAD }, ms(100));

        assert_eq!(intents.next_repeat(), None);
        assert_eq!(intents.repeat(ms(400)), None);
    }

    #[test]
    fn the_kind_comes_from_the_vendor_and_product_in_the_sdl_guid() {
        let cases = [
            // Xbox 360 pad, 045e:028e
            ("030000005e0400008e02000010010000", DeviceKind::Xbox),
            // DualSense, 054c:0ce6
            ("030000004c050000e60c000011810000", DeviceKind::PlayStation),
            // Switch Pro Controller, 057e:2009
            ("030000007e0500000920000011810000", DeviceKind::Nintendo),
            // Steam's virtual pad (Steam Input), 28de:11ff, laid out like an Xbox 360 pad
            ("03000000de280000ff11000001000000", DeviceKind::Xbox),
            // Steam Deck, 28de:1205
            ("03000000de2800000512000011010000", DeviceKind::SteamDeck),
            // 8BitDo, 2dc8:6100
            ("05000000c82d00000061000000010000", DeviceKind::Generic),
            ("not a guid", DeviceKind::Generic),
        ];

        for (guid, kind) in cases {
            assert_eq!(DeviceKind::detect(Some(guid), false), kind, "{guid}");
        }
        assert_eq!(DeviceKind::detect(None, false), DeviceKind::Generic);
        assert_eq!(DeviceKind::detect(None, true), DeviceKind::SteamDeck);
    }

    #[test]
    fn glyphs_follow_the_pad_last_plugged_in_or_used() {
        let mut intents = Intents::new();
        let other = DeviceId(2);
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        assert_eq!(intents.active_kind(), Some(DeviceKind::Xbox));

        intents.handle(
            &GamepadEvent::Connected {
                device: other,
                name: "DualSense".into(),
                kind: DeviceKind::PlayStation,
            },
            ms(0),
        );
        assert_eq!(intents.active_kind(), Some(DeviceKind::PlayStation));

        intents.handle(&press(Button::South), ms(10));
        assert_eq!(intents.active_kind(), Some(DeviceKind::Xbox));

        intents.handle(&GamepadEvent::Disconnected { device: PAD }, ms(20));
        assert_eq!(intents.active_kind(), Some(DeviceKind::PlayStation));

        intents.handle(&GamepadEvent::Disconnected { device: other }, ms(30));
        assert_eq!(intents.active_kind(), None);
    }
}
