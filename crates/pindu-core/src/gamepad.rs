//! Turns controller input into app intents, so the shells never see hardware. Devices come
//! from a [`GamepadBackend`]; [`Intents`] maps their mapped buttons and sticks to intents,
//! repeats held directions, notices a held Back, reads Guide on its release unless
//! another button went down while it was held (Guide + A, [`QUICK_ACCESS_CHORD`], asks for
//! Quick Access), and keeps the connected pads. Each pad's [`Layout`] (its glyphs, whether
//! confirm and back swap, and which button asks for which function, [`Bindings`]) comes
//! from its kind and the user's [`ControllerSettings`], and serves the hints and the
//! intents alike. While the menu drawer's Controls waits for a button to bind a function
//! to, [`Intents::bind_press`] takes the next press for it.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Duration;

use crate::bindings::{BINDABLE, Bindings};

/// A connected controller, as its backend numbers it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DeviceId(pub u64);

/// A controller button, by its position on an Xbox-style pad (SDL's mapping): `South` is
/// A on Xbox, Cross on PlayStation and B on Nintendo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    /// A on Xbox, Cross on PlayStation, B on Nintendo.
    South,
    /// B on Xbox, Circle on PlayStation, A on Nintendo.
    East,
    /// X on Xbox, Square on PlayStation, Y on Nintendo.
    West,
    /// Y on Xbox, Triangle on PlayStation, X on Nintendo.
    North,
    /// LB, L1 or L.
    LeftShoulder,
    /// RB, R1 or R.
    RightShoulder,
    /// LT, L2 or ZL: a trigger libmanette maps to a button whether the pad reports it as
    /// a button or as an axis.
    LeftTrigger,
    /// RT, R2 or ZR, mapped to a button like the left trigger.
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
    /// The D-pad's up.
    DpadUp,
    /// The D-pad's down.
    DpadDown,
    /// The D-pad's left.
    DpadLeft,
    /// The D-pad's right.
    DpadRight,
}

/// A stick axis: -1 is left or up, 1 right or down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    /// The left stick's horizontal axis.
    LeftX,
    /// The left stick's vertical axis.
    LeftY,
}

/// A direction focus moves in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Up.
    Up,
    /// Down.
    Down,
    /// Left.
    Left,
    /// Right.
    Right,
}

/// What the user asks the app to do. Intents never become synthetic key events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Move focus in a direction.
    Navigate(Direction),
    /// Select the focused item.
    Activate,
    /// Go back, or close what is open.
    Back,
    /// Back held down for [`HOLD_DURATION`], as long presses ask: it cancels a launch.
    BackHeld,
    /// Switch to the previous tab.
    PreviousTab,
    /// Switch to the next tab.
    NextTab,
    /// Switch to the previous sort order.
    PreviousSort,
    /// Switch to the next sort order.
    NextSort,
    /// Start searching.
    Search,
    /// Open or close the menu drawer.
    Menu,
    /// Open the focused game's options.
    Options,
    /// Guide, once let go without another button pressed while it was held: open or close
    /// the menu drawer, or from the desktop open fullscreen mode. It reaches the launcher
    /// while a game has the controller.
    Home,
    /// Open or close the Quick Access panel: R3, which games use too, or its key.
    QuickAccess,
    /// Open or close the Quick Access panel from a button no game uses: Guide + South
    /// ([`QUICK_ACCESS_CHORD`]) or the Steam Deck's "…". Unlike R3's, it reaches the
    /// launcher while a game has the controller, and opens Quick Access over the game.
    SystemQuickAccess,
}

/// The intents a button asks for when held down for [`HOLD_DURATION`] rather than pressed:
/// the pressed intent, then the held one.
const HOLDS: [(Intent, Intent); 1] = [(Intent::Back, Intent::BackHeld)];

/// The chord that asks for [`Intent::SystemQuickAccess`], as Steam has it: the first button
/// held down and the second pressed while it is. Both go by position whatever the layout,
/// so the second is the bottom face button even where confirm and back swap.
pub const QUICK_ACCESS_CHORD: (Button, Button) = (Button::Guide, Button::South);

impl Intent {
    /// The intent whose button, held down, asks for this one: Back for BackHeld.
    pub fn held_from(self) -> Option<Intent> {
        HOLDS
            .into_iter()
            .find(|&(_, held)| held == self)
            .map(|(pressed, _)| pressed)
    }
}

/// The kind of controller, which decides its glyphs and whether A and B swap, unless the
/// [`ControllerSettings`] say otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    /// An Xbox pad, or Steam Input's virtual pad.
    Xbox,
    /// A PlayStation pad.
    PlayStation,
    /// A on the east, B on the south: confirm and back swap.
    Nintendo,
    /// A Steam Deck's built-in controls.
    SteamDeck,
    /// Anything else, shown with Xbox-style glyphs.
    Generic,
}

impl DeviceKind {
    /// Every kind.
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
    /// The pad's own glyphs.
    #[default]
    Auto,
    /// Letters with A on the south, as on Xbox pads.
    Letters,
    /// PlayStation's cross, circle, square and triangle.
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
    /// Swap when the glyphs put A on the east.
    #[default]
    Auto,
    /// Never swap.
    Off,
    /// Always swap.
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

/// What the user chose for the glyphs, the confirm and back buttons and the buttons of the
/// other functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ControllerSettings {
    /// The Button Prompts setting.
    pub prompts: Prompts,
    /// The Swap Confirm and Back setting.
    pub swap: Swap,
    /// The button each function is bound to (see [`crate::bindings::BINDINGS_KEY`]).
    pub bindings: Bindings,
}

/// How a pad shows and reads its buttons: the glyphs the hints draw, whether its east
/// button confirms and its south button goes back, and the function each button asks for.
/// One layout serves the hints and the intents, so they always agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layout {
    /// The glyphs the hints draw.
    pub glyphs: Glyphs,
    /// Whether confirm and back swap: east activates and south goes back.
    pub swapped: bool,
    /// The function each button asks for, by its position on an Xbox pad, before the swap.
    pub bindings: Bindings,
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
        Self {
            glyphs,
            swapped,
            bindings: settings.bindings,
        }
    }

    /// The layout of a pad of `kind` as the pad itself has it.
    pub fn detected(kind: DeviceKind) -> Self {
        Self::resolve(kind, ControllerSettings::default())
    }

    /// The button that asks for `intent`, for hints: with confirm and back swapped, the
    /// east button does what the bindings give the south one. A held intent's button is the
    /// one held down.
    pub fn button(self, intent: Intent) -> Button {
        let pressed = intent.held_from().unwrap_or(intent);
        let bound = self
            .bindings
            .button(pressed)
            .expect("every intent has a button");
        self.xbox_position(bound)
    }

    /// The intent `button` asks for on this layout.
    pub fn intent(self, button: Button) -> Intent {
        self.bindings.intent(self.xbox_position(button))
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
    /// The USB vendor ID.
    pub vendor: u16,
    /// The USB product ID.
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
    /// The kind of pad, from its IDs.
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
        /// The device's number.
        device: DeviceId,
        /// The device as the backend describes it.
        pad: Pad,
    },
    /// A device was unplugged.
    Disconnected {
        /// The device's number.
        device: DeviceId,
    },
    /// A button was pressed or released.
    Button {
        /// The device's number.
        device: DeviceId,
        /// The button.
        button: Button,
        /// Whether it went down rather than up.
        pressed: bool,
    },
    /// A stick axis moved, from -1 to 1.
    Axis {
        /// The device's number.
        device: DeviceId,
        /// The axis.
        axis: Axis,
        /// Where the axis is now, from -1 to 1.
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
    /// How hard the strong motor turns, from 0 to 1.
    pub strong: f64,
    /// How hard the weak motor turns, from 0 to 1.
    pub weak: f64,
    /// How long it lasts.
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
    /// A button held down that asks for an intent once held (Back's, to cancel a launch):
    /// by which device and button, that intent, and when it counts as a hold.
    back_held: Option<(DeviceId, Button, Intent, Duration)>,
    /// The first button of [`QUICK_ACCESS_CHORD`], Guide, held down: by which device, and
    /// whether another button was pressed while it was, so letting go asks for nothing.
    guide: Option<(DeviceId, bool)>,
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
    /// No pads, nothing held and the default settings.
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
                if self.guide.is_some_and(|(holder, _)| holder == device) {
                    self.guide = None;
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
                let (first, second) = QUICK_ACCESS_CHORD;
                if button == first {
                    self.guide = Some((device, false));
                    return None;
                }
                if let Some((holder, chorded)) = &mut self.guide
                    && *holder == device
                {
                    *chorded = true;
                    if button == second {
                        return Some(Intent::SystemQuickAccess);
                    }
                }
                let intent = self.layout(device).intent(button);
                match intent {
                    Intent::Navigate(direction) => {
                        self.hold(device, Source::Button(button), direction, now);
                    }
                    _ => {
                        if let Some((_, held)) = HOLDS.into_iter().find(|&(on, _)| on == intent) {
                            self.back_held = Some((device, button, held, now + HOLD_DURATION));
                        }
                    }
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
                    .is_some_and(|(holder, held, ..)| (holder, held) == (device, button))
                {
                    self.back_held = None;
                }
                if button == QUICK_ACCESS_CHORD.0
                    && let Some((holder, chorded)) = self.guide
                    && holder == device
                {
                    self.guide = None;
                    return (!chorded).then_some(Intent::Home);
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
                self.back_held.take().map(|(_, _, held, _)| held)
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

    /// The glyphs, the confirm and back buttons and the bindings the user chose, which
    /// every device's layout follows from now on.
    pub fn set_settings(&mut self, settings: ControllerSettings) {
        self.settings = settings;
    }

    /// The settings every device's layout follows.
    pub fn settings(&self) -> ControllerSettings {
        self.settings
    }

    /// While the menu drawer's Controls waits for a button for `function`: when `event`
    /// presses a button that can be bound ([`BINDABLE`], by where the device's layout puts
    /// it after any confirm and back swap), binds `function` to it (swapping, see
    /// [`Bindings::bind`]) and returns the bindings now in force. The press asks for
    /// nothing else, and neither does its release. Any other event returns `None` and is
    /// left to [`Self::handle`]: Guide, the D-pad and the stick go on working.
    pub fn bind_press(&mut self, event: &GamepadEvent, function: Intent) -> Option<Bindings> {
        let GamepadEvent::Button {
            device,
            button,
            pressed: true,
        } = *event
        else {
            return None;
        };
        let position = self.layout(device).xbox_position(button);
        if !BINDABLE.contains(&position) || self.pressed.contains(&(device, button)) {
            return None;
        }
        self.pressed.insert((device, button));
        self.active = Some(device);
        // Guide let go after this asks for nothing, as after Guide + A.
        if let Some((holder, chorded)) = &mut self.guide
            && *holder == device
        {
            *chorded = true;
        }
        self.settings.bindings = self.settings.bindings.bind(function, position);
        Some(self.settings.bindings)
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

/// Every intent a controller asks for: each button's, in the order hints prefer the
/// buttons, then each held one.
pub fn intents() -> Vec<Intent> {
    let mut intents = Vec::new();
    let held = HOLDS.map(|(_, held)| held);
    let bindings = Bindings::default();
    let buttons = BUTTONS.into_iter().map(|button| bindings.intent(button));
    for intent in buttons.chain(held) {
        if !intents.contains(&intent) {
            intents.push(intent);
        }
    }
    intents
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
    fn r3_opens_quick_access_and_the_deck_s_dots_button_the_one_games_leave_alone() {
        let mut intents = Intents::new();
        assert_eq!(
            intents.handle(&press(Button::RightStick), Duration::ZERO),
            Some(Intent::QuickAccess)
        );
        assert_eq!(
            intents.handle(&press(Button::Misc1), Duration::ZERO),
            Some(Intent::SystemQuickAccess)
        );
    }

    #[test]
    fn guide_asks_for_home_once_released_rather_than_when_pressed() {
        let mut intents = Intents::new();

        assert_eq!(intents.handle(&press(Button::Guide), ms(0)), None);
        assert_eq!(
            intents.handle(&release(Button::Guide), ms(100)),
            Some(Intent::Home)
        );
    }

    #[test]
    fn guide_and_the_south_button_open_quick_access_and_nothing_else() {
        let mut intents = Intents::new();

        intents.handle(&press(Button::Guide), ms(0));
        assert_eq!(
            intents.handle(&press(Button::South), ms(50)),
            Some(Intent::SystemQuickAccess)
        );
        assert_eq!(intents.handle(&release(Button::South), ms(100)), None);
        assert_eq!(intents.handle(&release(Button::Guide), ms(150)), None);
    }

    #[test]
    fn another_button_pressed_while_guide_is_held_keeps_guide_from_asking_for_home() {
        let mut intents = Intents::new();

        intents.handle(&press(Button::Guide), ms(0));
        assert_eq!(
            intents.handle(&press(Button::DpadDown), ms(50)),
            Some(Intent::Navigate(Direction::Down))
        );
        intents.handle(&release(Button::DpadDown), ms(100));
        assert_eq!(intents.handle(&release(Button::Guide), ms(150)), None);
    }

    #[test]
    fn the_south_button_pressed_before_guide_is_no_chord() {
        let mut intents = Intents::new();

        assert_eq!(
            intents.handle(&press(Button::South), ms(0)),
            Some(Intent::Activate)
        );
        assert_eq!(intents.handle(&press(Button::Guide), ms(50)), None);
        assert_eq!(intents.handle(&release(Button::South), ms(100)), None);
        assert_eq!(
            intents.handle(&release(Button::Guide), ms(150)),
            Some(Intent::Home)
        );
    }

    #[test]
    fn letting_go_of_guide_before_the_south_button_still_asks_for_nothing_more() {
        let mut intents = Intents::new();

        intents.handle(&press(Button::Guide), ms(0));
        intents.handle(&press(Button::South), ms(50));
        assert_eq!(intents.handle(&release(Button::Guide), ms(100)), None);
        assert_eq!(intents.handle(&release(Button::South), ms(150)), None);
        // The next press of A alone is A's again.
        assert_eq!(
            intents.handle(&press(Button::South), ms(200)),
            Some(Intent::Activate)
        );
    }

    #[test]
    fn the_chord_takes_the_bottom_face_button_where_confirm_and_back_swap() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Nintendo), ms(0));

        intents.handle(&press(Button::Guide), ms(0));
        assert_eq!(
            intents.handle(&press(Button::South), ms(50)),
            Some(Intent::SystemQuickAccess)
        );
        // The south button goes back on this pad, but the chord's press is no hold of Back.
        assert_eq!(intents.repeat(HOLD_DURATION), None);
        intents.handle(&release(Button::South), ms(100));
        intents.handle(&release(Button::Guide), ms(100));

        intents.handle(&press(Button::Guide), ms(200));
        assert_eq!(
            intents.handle(&press(Button::East), ms(250)),
            Some(Intent::Activate)
        );
    }

    #[test]
    fn the_south_button_of_another_pad_is_no_chord() {
        let mut intents = Intents::new();
        let other = DeviceId(2);

        intents.handle(&press(Button::Guide), ms(0));
        let south = GamepadEvent::Button {
            device: other,
            button: Button::South,
            pressed: true,
        };
        assert_eq!(intents.handle(&south, ms(50)), Some(Intent::Activate));
        assert_eq!(
            intents.handle(&release(Button::Guide), ms(100)),
            Some(Intent::Home)
        );
    }

    #[test]
    fn unplugging_a_pad_while_guide_is_held_asks_for_nothing() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        intents.handle(&press(Button::Guide), ms(0));

        intents.handle(&GamepadEvent::Disconnected { device: PAD }, ms(50));
        intents.handle(&connected(DeviceKind::Xbox), ms(100));

        assert_eq!(intents.handle(&release(Button::Guide), ms(150)), None);
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
                let settings = ControllerSettings {
                    prompts,
                    swap,
                    ..ControllerSettings::default()
                };
                assert_eq!(
                    Layout::resolve(kind, settings),
                    Layout {
                        glyphs,
                        swapped,
                        bindings: Bindings::default(),
                    },
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
                swapped: false,
                bindings: Bindings::default(),
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

    fn bound(function: Intent, button: Button) -> ControllerSettings {
        ControllerSettings {
            bindings: Bindings::default().bind(function, button),
            ..ControllerSettings::default()
        }
    }

    #[test]
    fn a_rebound_function_asks_from_its_new_button_and_shows_there() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        intents.set_settings(bound(Intent::Search, Button::South));

        assert_eq!(
            intents.handle(&press(Button::South), ms(0)),
            Some(Intent::Search)
        );
        assert_eq!(
            intents.handle(&press(Button::North), ms(0)),
            Some(Intent::Activate)
        );
        let layout = intents.active_layout().expect("a pad");
        assert_eq!(layout.button(Intent::Search), Button::South);
        assert_eq!(layout.button(Intent::Activate), Button::North);
    }

    #[test]
    fn holding_back_s_new_button_cancels_a_launch() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        intents.set_settings(bound(Intent::Back, Button::RightShoulder));

        assert_eq!(
            intents.handle(&press(Button::RightShoulder), ms(0)),
            Some(Intent::Back)
        );
        assert_eq!(intents.repeat(HOLD_DURATION), Some(Intent::BackHeld));
        let layout = intents.active_layout().expect("a pad");
        assert_eq!(layout.button(Intent::BackHeld), Button::RightShoulder);
    }

    #[test]
    fn a_press_while_binding_binds_the_function_to_that_button_and_asks_for_nothing() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));

        let bindings = intents.bind_press(&press(Button::LeftTrigger), Intent::Menu);

        let expected = Bindings::default().bind(Intent::Menu, Button::LeftTrigger);
        assert_eq!(bindings, Some(expected));
        assert_eq!(intents.settings().bindings, expected);
        // A trigger that reports each move as a press binds nothing more, nor asks.
        assert_eq!(
            intents.bind_press(&press(Button::LeftTrigger), Intent::Search),
            None
        );
        assert_eq!(intents.handle(&press(Button::LeftTrigger), ms(10)), None);
        assert_eq!(intents.handle(&release(Button::LeftTrigger), ms(20)), None);
        assert_eq!(
            intents.handle(&press(Button::LeftTrigger), ms(30)),
            Some(Intent::Menu)
        );
        assert_eq!(
            intents.handle(&press(Button::Start), ms(30)),
            Some(Intent::PreviousSort)
        );
    }

    #[test]
    fn binding_goes_by_where_the_swap_puts_the_button() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Nintendo), ms(0));

        // A Nintendo pad's east button confirms: it is the south one of an Xbox pad.
        let bindings = intents
            .bind_press(&press(Button::East), Intent::Search)
            .expect("bound");

        assert_eq!(bindings.intent(Button::South), Intent::Search);
        intents.handle(&release(Button::East), ms(0));
        assert_eq!(
            intents.handle(&press(Button::East), ms(0)),
            Some(Intent::Search)
        );
        assert_eq!(
            intents.handle(&press(Button::North), ms(0)),
            Some(Intent::Activate)
        );
        let layout = intents.active_layout().expect("a pad");
        assert_eq!(layout.button(Intent::Search), Button::East);
    }

    #[test]
    fn guide_the_d_pad_and_the_stick_are_not_taken_while_binding() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));
        let stick = GamepadEvent::Axis {
            device: PAD,
            axis: Axis::LeftX,
            value: 1.0,
        };

        for event in [
            press(Button::Guide),
            press(Button::DpadDown),
            press(Button::Misc1),
            release(Button::South),
            stick,
        ] {
            assert_eq!(
                intents.bind_press(&event, Intent::Search),
                None,
                "{event:?}"
            );
        }
        assert_eq!(intents.settings().bindings, Bindings::default());
    }

    #[test]
    fn guide_held_while_binding_asks_for_nothing_when_let_go() {
        let mut intents = Intents::new();
        intents.handle(&connected(DeviceKind::Xbox), ms(0));

        intents.handle(&press(Button::Guide), ms(0));
        assert!(
            intents
                .bind_press(&press(Button::South), Intent::Search)
                .is_some()
        );
        assert_eq!(intents.handle(&release(Button::Guide), ms(10)), None);
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
    fn a_held_intent_is_asked_for_by_holding_its_pressed_intent_s_button() {
        let nintendo = Layout::detected(DeviceKind::Nintendo);
        let xbox = Layout::detected(DeviceKind::Xbox);

        assert_eq!(Intent::BackHeld.held_from(), Some(Intent::Back));
        assert_eq!(Intent::Back.held_from(), None);
        assert_eq!(xbox.button(Intent::BackHeld), Button::East);
        assert_eq!(nintendo.button(Intent::BackHeld), Button::South);
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
