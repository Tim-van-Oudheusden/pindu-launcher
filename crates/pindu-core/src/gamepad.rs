//! Turns controller input into app intents, so the shells never see hardware. Devices come
//! from a [`GamepadBackend`]; [`Intents`] maps their mapped buttons and sticks to intents,
//! repeats held directions, notices a held Back, and keeps the connected pads. Each pad's
//! [`Layout`] (its glyphs and whether confirm and back swap) comes from its kind and the
//! user's [`ControllerSettings`], and serves the hints and the intents alike.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

/// A connected controller, as its backend numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);

/// A controller button, by its position on an Xbox-style pad (SDL's mapping): `South` is
/// A on Xbox, Cross on PlayStation and B on Nintendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    South,
    East,
    West,
    North,
    LeftShoulder,
    RightShoulder,
    /// LT, L2 or ZL: a trigger libmanette maps to a button whether the pad reports it as
    /// a button or as an axis.
    LeftTrigger,
    RightTrigger,
    /// View, Back, Share or Minus.
    Select,
    /// Menu, Options or Plus.
    Start,
    /// Xbox, PS or Home.
    Guide,
    /// R3: the right stick pressed in.
    RightStick,
    /// The Steam Deck's "…" button (libmanette's `misc1`).
    Misc1,
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
    /// Back held down for [`HOLD_DURATION`], as long presses ask: it cancels a launch.
    BackHeld,
    PreviousTab,
    NextTab,
    PreviousSort,
    NextSort,
    Search,
    Menu,
    Options,
    Home,
    /// Open or close the Quick Access panel.
    QuickAccess,
}

/// The kind of controller, which decides its glyphs and whether A and B swap, unless the
/// [`ControllerSettings`] say otherwise.
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
    pub const ALL: [Self; 5] = [
        Self::Xbox,
        Self::PlayStation,
        Self::Nintendo,
        Self::SteamDeck,
        Self::Generic,
    ];

    /// The kind of a device from its SDL GUID, which holds its USB vendor and product IDs
    /// (little-endian, at bytes 4 and 8), and from whether libmanette drives it as a Steam
    /// Deck.
    pub fn detect(guid: Option<&str>, steam_deck: bool) -> Self {
        if steam_deck {
            return Self::SteamDeck;
        }
        let Some(ids) = guid.and_then(HidIds::from_guid) else {
            return Self::Generic;
        };
        match (ids.vendor, ids.product) {
            (0x045e, _) => Self::Xbox,
            (0x054c, _) => Self::PlayStation,
            (0x057e, _) => Self::Nintendo,
            (0x28de, 0x1205) => Self::SteamDeck,
            // Steam Input's virtual pad, laid out like an Xbox 360 pad.
            (0x28de, 0x11ff) => Self::Xbox,
            _ => Self::Generic,
        }
    }

    /// The glyphs printed on this kind of pad. Unknown pads look like Xbox pads, as
    /// libmanette maps them.
    pub fn glyphs(self) -> Glyphs {
        match self {
            Self::Xbox | Self::Generic => Glyphs::Letters,
            Self::PlayStation => Glyphs::Shapes,
            Self::Nintendo => Glyphs::NintendoLetters,
            Self::SteamDeck => Glyphs::DeckLetters,
        }
    }
}

/// The set of labels the glyphs draw on the buttons, as a kind of pad prints them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyphs {
    /// A south, B east, X west, Y north; LB, RB, LT, RT and RS.
    Letters,
    /// The same letters with L1, R1, L2, R2 and R3, as a Steam Deck prints them.
    DeckLetters,
    /// Cross, circle, square and triangle; L1, R1, L2, R2 and R3.
    Shapes,
    /// A east, B south, X north, Y west; L, R, ZL, ZR and RS.
    NintendoLetters,
}

/// The Button Prompts setting: the glyphs to show whatever the pad is, or Auto for the
/// pad's own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Prompts {
    #[default]
    Auto,
    Letters,
    Shapes,
    /// Letters with A on the east and B on the south.
    NintendoLetters,
}

impl Prompts {
    /// Every choice, in the order the settings list them.
    pub const ALL: [Self; 4] = [
        Self::Auto,
        Self::Letters,
        Self::Shapes,
        Self::NintendoLetters,
    ];

    /// The value of the `controller-prompts` key.
    pub fn setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Letters => "letters",
            Self::Shapes => "shapes",
            Self::NintendoLetters => "nintendo-letters",
        }
    }

    /// The choice a `controller-prompts` value names; Auto for any other.
    pub fn from_setting(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|choice| choice.setting() == value)
            .unwrap_or_default()
    }
}

/// The Swap Confirm and Back setting: Auto swaps when the glyphs put A on the east.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Swap {
    #[default]
    Auto,
    Off,
    On,
}

impl Swap {
    /// Every choice, in the order the settings list them.
    pub const ALL: [Self; 3] = [Self::Auto, Self::Off, Self::On];

    /// The value of the `swap-confirm-back` key.
    pub fn setting(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Off => "off",
            Self::On => "on",
        }
    }

    /// The choice a `swap-confirm-back` value names; Auto for any other.
    pub fn from_setting(value: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|choice| choice.setting() == value)
            .unwrap_or_default()
    }
}

/// The GSettings key of the Button Prompts setting.
pub const PROMPTS_KEY: &str = "controller-prompts";
/// The GSettings key of the Swap Confirm and Back setting.
pub const SWAP_KEY: &str = "swap-confirm-back";

/// What the user chose for the glyphs and the confirm and back buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControllerSettings {
    pub prompts: Prompts,
    pub swap: Swap,
}

/// How a pad shows and reads its buttons: the glyphs the hints draw, and whether its east
/// button confirms and its south button goes back. One layout serves the hints and the
/// intents, so they always agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    pub glyphs: Glyphs,
    /// Whether confirm and back swap: east activates and south goes back.
    pub swapped: bool,
}

impl Layout {
    /// The layout of a pad of `kind` under `settings`. Prompts other than Auto replace
    /// the pad's glyphs, except that letters on a Steam Deck keep its own shoulder labels;
    /// Swap Auto swaps when the glyphs put A on the east.
    pub fn resolve(kind: DeviceKind, settings: ControllerSettings) -> Self {
        let glyphs = match (settings.prompts, kind) {
            (Prompts::Auto, _) | (Prompts::Letters, DeviceKind::SteamDeck) => kind.glyphs(),
            (Prompts::Letters, _) => Glyphs::Letters,
            (Prompts::Shapes, _) => Glyphs::Shapes,
            (Prompts::NintendoLetters, _) => Glyphs::NintendoLetters,
        };
        let swapped = match settings.swap {
            Swap::Auto => glyphs == Glyphs::NintendoLetters,
            Swap::Off => false,
            Swap::On => true,
        };
        Self { glyphs, swapped }
    }

    /// The layout of a pad of `kind` as the pad itself has it.
    pub fn detected(kind: DeviceKind) -> Self {
        Self::resolve(kind, ControllerSettings::default())
    }

    /// The button that asks for `intent`, for hints: with confirm and back swapped, the
    /// east button confirms.
    pub fn button(self, intent: Intent) -> Button {
        BUTTONS
            .into_iter()
            .find(|&button| button_intent(self.xbox_position(button)) == intent)
            .expect("every intent has a button")
    }

    /// Where an Xbox pad has the button that does what `button` does in this layout: with
    /// confirm and back swapped, east confirms and south goes back.
    fn xbox_position(self, button: Button) -> Button {
        match button {
            Button::East if self.swapped => Button::South,
            Button::South if self.swapped => Button::East,
            _ => button,
        }
    }
}

/// The bus and USB vendor and product IDs of a device, as its SDL GUID and the kernel's
/// HID device names (`0005:054C:0CE6.0007`) hold them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HidIds {
    /// The bus: 3 for USB, 5 for Bluetooth.
    pub bus: u16,
    pub vendor: u16,
    pub product: u16,
}

impl HidIds {
    /// The IDs in an SDL GUID: bus, vendor and product, little-endian, at bytes 0, 4 and 8.
    pub fn from_guid(guid: &str) -> Option<Self> {
        let word = |at: usize| {
            let bytes = guid.get(at..at + 4)?;
            let low = u8::from_str_radix(&bytes[..2], 16).ok()?;
            let high = u8::from_str_radix(&bytes[2..], 16).ok()?;
            Some(u16::from_le_bytes([low, high]))
        };
        if guid.len() != 32 {
            return None;
        }
        Some(Self {
            bus: word(0)?,
            vendor: word(8)?,
            product: word(16)?,
        })
    }
}

/// A connected controller, as its backend describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pad {
    /// The name the device gives, such as "DualSense Wireless Controller".
    pub name: String,
    pub kind: DeviceKind,
    /// Its bus, vendor and product, from its SDL GUID, to find its battery by.
    pub ids: Option<HidIds>,
    /// Whether it can rumble.
    pub rumble: bool,
}

/// Input from a backend, already mapped to buttons and axes.
#[derive(Debug, Clone, PartialEq)]
pub enum GamepadEvent {
    /// A device was plugged in, or was already there when the backend started.
    Connected {
        device: DeviceId,
        pad: Pad,
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
/// How long Back is held to cancel a launch.
pub const HOLD_DURATION: Duration = Duration::from_secs(1);
/// How far a stick must move before it navigates.
pub const DEADZONE: f64 = 0.5;

/// A rumble: how hard the strong (low-frequency) and weak (high-frequency) motors turn,
/// from 0 to 1, and for how long.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rumble {
    pub strong: f64,
    pub weak: f64,
    pub duration: Duration,
}

/// The short rumble that tells which pad a row of Quick Access's Controllers is.
pub const IDENTIFY_RUMBLE: Rumble = Rumble {
    strong: 0.5,
    weak: 0.5,
    duration: Duration::from_millis(300),
};

/// A source of controller events: libmanette 0.2 today, its 1.0 API or SDL3 later. All
/// device access sits behind this trait.
pub trait GamepadBackend {
    /// Starts reporting events to `sink` on the main thread: a `Connected` for each device
    /// already plugged in, then input and hotplug as they happen, until it is stopped.
    fn start(&mut self, sink: Box<dyn Fn(GamepadEvent)>);

    /// Stops reporting and lets go of every device, until the next `start`.
    fn stop(&mut self);

    /// Rumbles `device`, if it is connected and can rumble.
    fn rumble(&mut self, device: DeviceId, rumble: Rumble);
}

/// Maps controller input to intents.
#[derive(Debug, Default)]
pub struct Intents {
    /// The connected pads, by number.
    pads: BTreeMap<DeviceId, Pad>,
    sticks: HashMap<DeviceId, [f64; 2]>,
    /// The buttons held down, so a trigger that reports each move as a press asks once.
    pressed: HashSet<(DeviceId, Button)>,
    held: Option<Held>,
    /// Back held down, by which device and button, and when it counts as a hold.
    back_held: Option<(DeviceId, Button, Duration)>,
    /// The device last plugged in or used, whose layout the hints show.
    active: Option<DeviceId>,
    settings: ControllerSettings,
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
            GamepadEvent::Connected { device, ref pad } => {
                self.pads.insert(device, pad.clone());
                self.active = Some(device);
                None
            }
            GamepadEvent::Disconnected { device } => {
                self.pads.remove(&device);
                self.sticks.remove(&device);
                self.pressed.retain(|(holder, _)| *holder != device);
                if self.held.is_some_and(|held| held.device == device) {
                    self.held = None;
                }
                if self.back_held.is_some_and(|(holder, ..)| holder == device) {
                    self.back_held = None;
                }
                if self.active == Some(device) {
                    self.active = self.pads.keys().next().copied();
                }
                None
            }
            GamepadEvent::Button {
                device,
                button,
                pressed: true,
            } => {
                self.active = Some(device);
                if !self.pressed.insert((device, button)) {
                    return None;
                }
                let intent = button_intent(self.confirm_and_back_swapped(device, button));
                match intent {
                    Intent::Navigate(direction) => {
                        self.hold(device, Source::Button(button), direction, now);
                    }
                    Intent::Back => self.back_held = Some((device, button, now + HOLD_DURATION)),
                    _ => {}
                }
                Some(intent)
            }
            GamepadEvent::Button {
                device,
                button,
                pressed: false,
            } => {
                self.pressed.remove(&(device, button));
                self.release(device, Source::Button(button));
                if self
                    .back_held
                    .is_some_and(|(holder, held, _)| (holder, held) == (device, button))
                {
                    self.back_held = None;
                }
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

    /// The repeat of a held direction, or the hold of Back, when one is due at `now`.
    pub fn repeat(&mut self, now: Duration) -> Option<Intent> {
        let direction_due = self.held.map(|held| held.due).filter(|&due| due <= now);
        let back_due = self
            .back_held
            .map(|(.., due)| due)
            .filter(|&due| due <= now);
        match (direction_due, back_due) {
            (_, Some(back)) if direction_due.is_none_or(|direction| back < direction) => {
                self.back_held = None;
                Some(Intent::BackHeld)
            }
            (Some(_), _) => {
                let held = self.held.as_mut()?;
                held.due = now + REPEAT_INTERVAL;
                Some(Intent::Navigate(held.direction))
            }
            _ => None,
        }
    }

    /// When the held direction next repeats or Back counts as held, if either is held.
    pub fn next_repeat(&self) -> Option<Duration> {
        let back = self.back_held.map(|(.., due)| due);
        [self.held.map(|held| held.due), back]
            .into_iter()
            .flatten()
            .min()
    }

    /// The layout of the device last plugged in or used, for the hints; `None` without one.
    pub fn active_layout(&self) -> Option<Layout> {
        let device = self.active?;
        self.pads.contains_key(&device).then(|| self.layout(device))
    }

    /// The connected pads, in the order they were numbered.
    pub fn pads(&self) -> impl Iterator<Item = (DeviceId, &Pad)> {
        self.pads.iter().map(|(device, pad)| (*device, pad))
    }

    /// The glyphs and the confirm and back buttons the user chose, which every device's
    /// layout follows from now on.
    pub fn set_settings(&mut self, settings: ControllerSettings) {
        self.settings = settings;
    }

    /// The settings every device's layout follows.
    pub fn settings(&self) -> ControllerSettings {
        self.settings
    }

    /// The layout of `device` under the settings; one not yet connected reads as a generic
    /// pad.
    fn layout(&self, device: DeviceId) -> Layout {
        let kind = self
            .pads
            .get(&device)
            .map_or(DeviceKind::Generic, |pad| pad.kind);
        Layout::resolve(kind, self.settings)
    }

    /// The button as an Xbox pad would place it, after the device's confirm and back swap.
    fn confirm_and_back_swapped(&self, device: DeviceId, button: Button) -> Button {
        self.layout(device).xbox_position(button)
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

/// Every button, in the order hints prefer them when two do the same: options show on
/// View, which the spec names for it, rather than on the west face button.
pub(crate) const BUTTONS: [Button; 17] = [
    Button::South,
    Button::East,
    Button::North,
    Button::Select,
    Button::West,
    Button::Start,
    Button::Guide,
    Button::RightStick,
    Button::Misc1,
    Button::LeftShoulder,
    Button::RightShoulder,
    Button::LeftTrigger,
    Button::RightTrigger,
    Button::DpadUp,
    Button::DpadDown,
    Button::DpadLeft,
    Button::DpadRight,
];

fn button_intent(button: Button) -> Intent {
    match button {
        Button::South => Intent::Activate,
        Button::East => Intent::Back,
        Button::North => Intent::Search,
        Button::West | Button::Select => Intent::Options,
        Button::Start => Intent::Menu,
        Button::Guide => Intent::Home,
        Button::RightStick | Button::Misc1 => Intent::QuickAccess,
        Button::LeftShoulder => Intent::PreviousTab,
        Button::RightShoulder => Intent::NextTab,
        Button::LeftTrigger => Intent::PreviousSort,
        Button::RightTrigger => Intent::NextSort,
        Button::DpadUp => Intent::Navigate(Direction::Up),
        Button::DpadDown => Intent::Navigate(Direction::Down),
        Button::DpadLeft => Intent::Navigate(Direction::Left),
        Button::DpadRight => Intent::Navigate(Direction::Right),
    }
}

/// What a window knows about its focus, to decide whether controller input is its.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WindowFocus {
    /// The compositor reports the window as focused (`GDK_TOPLEVEL_STATE_FOCUSED`).
    pub compositor_focused: bool,
    /// The window has keyboard focus (`gtk_window_is_active`).
    pub keyboard_focused: bool,
}

impl WindowFocus {
    /// Whether the window acts on the controller. A game or another app in front of it
    /// has focus instead, and then the controller is theirs. Keyboard focus counts too: on
    /// X11 the compositor only says "focused" when the window manager supports
    /// `_NET_WM_STATE_FOCUSED`, which gamescope's doesn't.
    pub fn takes_controller(self) -> bool {
        self.compositor_focused || self.keyboard_focused
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
            (Button::LeftTrigger, Intent::PreviousSort),
            (Button::RightTrigger, Intent::NextSort),
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

    #[test]
    fn r3_and_the_deck_s_dots_button_open_quick_access() {
        let mut intents = Intents::new();
        for button in [Button::RightStick, Button::Misc1] {
            assert_eq!(
                intents.handle(&press(button), Duration::ZERO),
                Some(Intent::QuickAccess),
                "{button:?}"
            );
        }
    }

    #[test]
    fn a_trigger_pulled_further_asks_once_until_it_is_let_go() {
        // libmanette reports every move of an axis trigger past halfway as another press.
        let mut intents = Intents::new();
        let pull = press(Button::RightTrigger);

        assert_eq!(intents.handle(&pull, ms(0)), Some(Intent::NextSort));
        assert_eq!(intents.handle(&pull, ms(10)), None);
        assert_eq!(intents.handle(&pull, ms(20)), None);

        intents.handle(&release(Button::RightTrigger), ms(30));
        intents.handle(&release(Button::RightTrigger), ms(40));
        assert_eq!(intents.handle(&pull, ms(50)), Some(Intent::NextSort));
    }

    fn pad(kind: DeviceKind) -> Pad {
        Pad {
            name: "Pad".into(),
            kind,
            ids: None,
            rumble: false,
        }
    }

    fn connected(kind: DeviceKind) -> GamepadEvent {
        GamepadEvent::Connected {
            device: PAD,
            pad: pad(kind),
        }
    }

    #[test]
    fn the_pads_list_while_plugged_in_and_leave_when_unplugged() {
        let mut intents = Intents::new();
        let dualsense = Pad {
            name: "DualSense Wireless Controller".into(),
            kind: DeviceKind::PlayStation,
            ids: HidIds::from_guid("050000004c050000e60c000000810000"),
            rumble: true,
        };
        intents.handle(
            &GamepadEvent::Connected {
                device: DeviceId(2),
                pad: dualsense.clone(),
            },
            ms(0),
        );
        intents.handle(&connected(DeviceKind::Generic), ms(0));

        assert_eq!(
            intents.pads().collect::<Vec<_>>(),
            [(PAD, &pad(DeviceKind::Generic)), (DeviceId(2), &dualsense)]
        );

        intents.handle(&GamepadEvent::Disconnected { device: PAD }, ms(10));
        assert_eq!(
            intents.pads().collect::<Vec<_>>(),
            [(DeviceId(2), &dualsense)]
        );
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
        intents.handle(&release(Button::South), Duration::ZERO);

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

    /// Every kind, by a match that stops compiling when a kind is added, so the tables
    /// below have to cover it.
    fn every_kind() -> [DeviceKind; 5] {
        let kinds = DeviceKind::ALL;
        for kind in kinds {
            match kind {
                DeviceKind::Xbox
                | DeviceKind::PlayStation
                | DeviceKind::Nintendo
                | DeviceKind::SteamDeck
                | DeviceKind::Generic => {}
            }
        }
        kinds
    }

    #[test]
    fn every_prompt_and_swap_setting_resolves_with_every_kind() {
        use DeviceKind::{Generic, Nintendo, PlayStation, SteamDeck, Xbox};
        use Glyphs::{DeckLetters, Letters, NintendoLetters, Shapes};
        // Each kind with each prompt setting: the glyphs, and whether Auto swaps confirm
        // and back (when the glyphs put A on the east).
        let rows = [
            (Xbox, Prompts::Auto, Letters, false),
            (Xbox, Prompts::Letters, Letters, false),
            (Xbox, Prompts::Shapes, Shapes, false),
            (Xbox, Prompts::NintendoLetters, NintendoLetters, true),
            (PlayStation, Prompts::Auto, Shapes, false),
            (PlayStation, Prompts::Letters, Letters, false),
            (PlayStation, Prompts::Shapes, Shapes, false),
            (PlayStation, Prompts::NintendoLetters, NintendoLetters, true),
            (Nintendo, Prompts::Auto, NintendoLetters, true),
            (Nintendo, Prompts::Letters, Letters, false),
            (Nintendo, Prompts::Shapes, Shapes, false),
            (Nintendo, Prompts::NintendoLetters, NintendoLetters, true),
            // A Steam Deck's letters keep its L1, R1, L2, R2 and R3.
            (SteamDeck, Prompts::Auto, DeckLetters, false),
            (SteamDeck, Prompts::Letters, DeckLetters, false),
            (SteamDeck, Prompts::Shapes, Shapes, false),
            (SteamDeck, Prompts::NintendoLetters, NintendoLetters, true),
            (Generic, Prompts::Auto, Letters, false),
            (Generic, Prompts::Letters, Letters, false),
            (Generic, Prompts::Shapes, Shapes, false),
            (Generic, Prompts::NintendoLetters, NintendoLetters, true),
        ];
        for kind in every_kind() {
            for prompts in Prompts::ALL {
                assert!(
                    rows.iter().any(|row| (row.0, row.1) == (kind, prompts)),
                    "{kind:?} with {prompts:?} has no row"
                );
            }
        }

        for (kind, prompts, glyphs, auto_swap) in rows {
            for (swap, swapped) in [
                (Swap::Auto, auto_swap),
                (Swap::Off, false),
                (Swap::On, true),
            ] {
                assert_eq!(
                    Layout::resolve(kind, ControllerSettings { prompts, swap }),
                    Layout { glyphs, swapped },
                    "{kind:?} with {prompts:?} and {swap:?}"
                );
            }
        }
        assert_eq!(Swap::ALL, [Swap::Auto, Swap::Off, Swap::On]);
    }

    #[test]
    fn the_settings_read_and_write_their_gsettings_values() {
        let prompts = ["auto", "letters", "shapes", "nintendo-letters"];
        for (choice, value) in Prompts::ALL.into_iter().zip(prompts) {
            assert_eq!(choice.setting(), value);
            assert_eq!(Prompts::from_setting(value), choice);
        }
        for (choice, value) in Swap::ALL.into_iter().zip(["auto", "off", "on"]) {
            assert_eq!(choice.setting(), value);
            assert_eq!(Swap::from_setting(value), choice);
        }
        // A value an older or newer schema wrote reads as Auto.
        assert_eq!(Prompts::from_setting("icons"), Prompts::Auto);
        assert_eq!(Swap::from_setting(""), Swap::Auto);
    }

    #[test]
    fn glyphs_follow_the_pad_last_plugged_in_or_used() {
        let mut intents = Intents::new();
        let other = DeviceId(2);
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        assert_eq!(glyphs(&intents), Some(Glyphs::Letters));

        intents.handle(
            &GamepadEvent::Connected {
                device: other,
                pad: pad(DeviceKind::PlayStation),
            },
            ms(0),
        );
        assert_eq!(glyphs(&intents), Some(Glyphs::Shapes));

        intents.handle(&press(Button::South), ms(10));
        assert_eq!(glyphs(&intents), Some(Glyphs::Letters));

        intents.handle(&GamepadEvent::Disconnected { device: PAD }, ms(20));
        assert_eq!(glyphs(&intents), Some(Glyphs::Shapes));

        intents.handle(&GamepadEvent::Disconnected { device: other }, ms(30));
        assert_eq!(glyphs(&intents), None);
    }

    fn glyphs(intents: &Intents) -> Option<Glyphs> {
        intents.active_layout().map(|layout| layout.glyphs)
    }

    #[test]
    fn with_swap_on_the_east_button_activates_and_the_south_one_goes_back_and_holds() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        intents.set_settings(ControllerSettings {
            swap: Swap::On,
            ..ControllerSettings::default()
        });

        assert_eq!(
            intents.handle(&press(Button::East), ms(0)),
            Some(Intent::Activate)
        );
        assert_eq!(
            intents.handle(&press(Button::South), ms(0)),
            Some(Intent::Back)
        );
        assert_eq!(intents.repeat(HOLD_DURATION), Some(Intent::BackHeld));
    }

    #[test]
    fn with_swap_off_a_nintendo_pad_activates_with_its_south_button() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Nintendo), ms(0));
        intents.set_settings(ControllerSettings {
            swap: Swap::Off,
            ..ControllerSettings::default()
        });

        assert_eq!(
            intents.handle(&press(Button::South), ms(0)),
            Some(Intent::Activate)
        );
        assert_eq!(
            intents.active_layout(),
            Some(Layout {
                glyphs: Glyphs::NintendoLetters,
                swapped: false
            })
        );
    }

    #[test]
    fn button_prompts_change_the_active_layout_s_glyphs_at_once() {
        // Steam Input's virtual pad, which reads as an Xbox pad, holding a DualSense.
        let steam_pad = DeviceKind::detect(Some("03000000de280000ff11000001000000"), false);
        let mut intents = Intents::new();
        intents.handle(&connected(steam_pad), ms(0));

        intents.set_settings(ControllerSettings {
            prompts: Prompts::Shapes,
            ..ControllerSettings::default()
        });

        assert_eq!(glyphs(&intents), Some(Glyphs::Shapes));
    }

    #[test]
    fn holding_back_asks_once_more_after_the_hold_duration() {
        let mut intents = Intents::new();

        assert_eq!(
            intents.handle(&press(Button::East), ms(0)),
            Some(Intent::Back)
        );
        assert_eq!(intents.next_repeat(), Some(HOLD_DURATION));
        assert_eq!(intents.repeat(HOLD_DURATION - ms(1)), None);
        assert_eq!(intents.repeat(HOLD_DURATION), Some(Intent::BackHeld));
        assert_eq!(intents.repeat(HOLD_DURATION * 2), None);
        assert_eq!(intents.next_repeat(), None);
    }

    #[test]
    fn letting_go_of_back_early_is_no_hold() {
        let mut intents = Intents::new();

        intents.handle(&press(Button::East), ms(0));
        intents.handle(&release(Button::East), ms(500));

        assert_eq!(intents.next_repeat(), None);
        assert_eq!(intents.repeat(HOLD_DURATION), None);
    }

    #[test]
    fn a_nintendo_pad_holds_back_on_its_south_button() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Nintendo), ms(0));

        intents.handle(&press(Button::East), ms(0));
        assert_eq!(intents.repeat(HOLD_DURATION), None);
        intents.handle(&release(Button::East), ms(0));

        intents.handle(&press(Button::South), ms(0));
        assert_eq!(intents.repeat(HOLD_DURATION), Some(Intent::BackHeld));
    }

    #[test]
    fn a_held_direction_still_repeats_while_back_is_held() {
        let mut intents = Intents::new();
        let down = Intent::Navigate(Direction::Down);

        intents.handle(&press(Button::East), ms(0));
        intents.handle(&press(Button::DpadDown), ms(0));

        assert_eq!(intents.next_repeat(), Some(REPEAT_DELAY));
        assert_eq!(intents.repeat(REPEAT_DELAY), Some(down));
        intents.handle(&release(Button::DpadDown), ms(400));
        assert_eq!(intents.repeat(HOLD_DURATION), Some(Intent::BackHeld));
    }

    #[test]
    fn a_window_with_keyboard_focus_takes_the_controller_where_the_compositor_never_says_focused() {
        // Gamescope's X11 window manager lacks _NET_WM_STATE_FOCUSED.
        let focus = WindowFocus {
            compositor_focused: false,
            keyboard_focused: true,
        };

        assert!(focus.takes_controller());
    }

    #[test]
    fn a_window_behind_a_game_leaves_the_controller_to_it() {
        let focus = WindowFocus {
            compositor_focused: false,
            keyboard_focused: false,
        };

        assert!(!focus.takes_controller());
    }
}
