//! Fullscreen mode's panels over its pages: the menu drawer on the left (where to go,
//! leaving, and the machine's power), the Quick Access panel on the right, in six sections
//! LB and RB switch between (Quick, Notifications, Design, Controller, System and Manage;
//! see [`section_entries`]), some of whose entries lead to a level, and the options sheet
//! at the bottom (what to do with one game).
//! This decides which one is open, at which level and section, what Start, Guide, R3,
//! View or X, LB, RB and B do to them, and what each lists.

use crate::gamepad::{Direction, Intent};
use crate::grid;
use crate::launch::{self, WebPage};
use crate::model::Game;
use crate::saves;
use crate::themes::Theme;

/// A panel over the page, at one of its levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// The menu drawer on the left.
    Drawer(DrawerLevel),
    /// The options sheet for one game at the bottom.
    Options(OptionsLevel),
    /// The Quick Access panel on the right.
    QuickAccess(QuickAccessLevel),
}

/// What the drawer shows: its entries, or the list one of them leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerLevel {
    /// The drawer's entries.
    Main,
    /// The collections, each opening its tab in the library; View or X on one leads to
    /// [`DrawerLevel::Collection`].
    Collections,
    /// One collection's actions: rename, move up or down, and delete (see
    /// [`collection_actions`]).
    Collection,
    /// Suspend, Restart and Power Off, as the system allows them (see
    /// [`crate::system::power`]), and Exit Pindu Launcher.
    Power,
    /// The functions a controller button can be given, each with its button, and Reset to
    /// Defaults (see [`crate::gamepad::Bindings`]).
    Controls,
}

/// What the options sheet shows: the game's actions, or one of the lists an action leads
/// to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsLevel {
    /// The game's actions.
    Main,
    /// The collections to add the game to or take it out of.
    Collections,
    /// The other games to merge the game with, likely copies first (see
    /// [`crate::game_groups::merge_candidates`]).
    Merge,
    /// A group's copies, to choose the one Play starts.
    Copies,
}

/// A section of the Quick Access panel, which LB and RB switch between while the panel
/// shows one ([`QuickAccessLevel::Main`]). Each lists its entries (see
/// [`section_entries`]), some of which lead to a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum QuickAccessSection {
    /// The Playing Now cards, the entries for the page under the panel and the things to
    /// reach in one press. Quick Access opens on it.
    #[default]
    Quick,
    /// What runs in the background, then the session's notifications, newest first.
    Notifications,
    /// The designs to pick from and their settings.
    Design,
    /// The connected controllers and their settings, and what each button does.
    Controller,
    /// The machine's network, Bluetooth, sound output and updates, the navigation sounds
    /// and Exit After Launching, and the diagnostics report.
    System,
    /// The sources, duplicates, Library Health, Proton and stores, adding and refreshing
    /// games, and their artwork and metadata.
    Manage,
}

impl QuickAccessSection {
    /// Every section, in the order the strip shows them and LB and RB step through them.
    pub const ALL: [Self; 6] = [
        Self::Quick,
        Self::Notifications,
        Self::Design,
        Self::Controller,
        Self::System,
        Self::Manage,
    ];

    /// The name the section goes by: its page of the panel's section stack, and what
    /// `win.quick-access-section` takes to show it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Quick => "quick",
            Self::Notifications => "notifications",
            Self::Design => "design",
            Self::Controller => "controller",
            Self::System => "system",
            Self::Manage => "manage",
        }
    }

    /// The section that goes by `name` (see [`Self::name`]), if one does.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|section| section.name() == name)
    }

    /// Whether one of the section's entries leads to `level`: the section that holds it
    /// ([`QuickAccessLevel::section`]), and Quick for Sound Output, Bluetooth and Network,
    /// which it lists too.
    pub fn leads_to(self, level: QuickAccessLevel) -> bool {
        let quick = matches!(
            level,
            QuickAccessLevel::SoundOutput | QuickAccessLevel::Bluetooth | QuickAccessLevel::Network
        );
        level != QuickAccessLevel::Main
            && (level.section() == self || (self == Self::Quick && quick))
    }
}

/// An entry of one of Quick Access's sections: a row, a list or cards that act, or lead
/// to a level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAccessEntry {
    /// The Playing Now cards, one for each game we started that runs, with Return and Stop.
    PlayingNow,
    /// Leads to the library filters ([`QuickAccessLevel::Filters`]).
    Filters,
    /// Leads to the focused game's performance switches
    /// ([`QuickAccessLevel::Performance`]).
    Performance,
    /// Leads to the sound outputs ([`QuickAccessLevel::SoundOutput`]).
    SoundOutput,
    /// Leads to Bluetooth ([`QuickAccessLevel::Bluetooth`]).
    Bluetooth,
    /// Leads to the network ([`QuickAccessLevel::Network`]).
    Network,
    /// Shows a random installed game.
    SurpriseMe,
    /// Imports the sources again.
    RefreshLibrary,
    /// Opens Add a Game.
    AddGame,
    /// A Running heading and what runs in the background, each with its progress and
    /// Cancel.
    Running,
    /// The session's notifications, newest first, or the line that says there are none.
    Notifications,
    /// The designs, the variant, Home's background and its blur and dim, Cover Size, Text
    /// Size, Reduce Motion and the user's own stylesheet.
    Designs,
    /// The connected controllers, the button prompts, the confirm and back swap, Hold to
    /// Confirm, the repeat delay and rate and the stick's dead zone.
    Controllers,
    /// Leads to what each button and key does ([`QuickAccessLevel::Controls`]).
    Controls,
    /// Leads to the OS updates on Pindu OS ([`QuickAccessLevel::SystemUpdate`]).
    SystemUpdate,
    /// A list of its own: Navigation Sounds, their Volume under it, insensitive while
    /// they are off, and Exit After Launching.
    NavigationSounds,
    /// Leads to the diagnostics report ([`QuickAccessLevel::DiagnosticsReport`]).
    DiagnosticsReport,
    /// Leads to the sources ([`QuickAccessLevel::Sources`]).
    Sources,
    /// Leads to the duplicates ([`QuickAccessLevel::Duplicates`]).
    Duplicates,
    /// Leads to Library Health ([`QuickAccessLevel::LibraryHealth`]).
    LibraryHealth,
    /// Leads to the Proton builds ([`QuickAccessLevel::Proton`]).
    Proton,
    /// Leads to the stores to install ([`QuickAccessLevel::AddStore`]).
    AddStore,
    /// The Artwork heading: the SteamGridDB key and Prefer SteamGridDB Art.
    Artwork,
    /// The Metadata heading: the Get Metadata From Wikidata switch, and Refresh Metadata
    /// right under it while the switch is on.
    Metadata,
}

/// The facts that decide which of Quick Access's entries show (see [`section_entries`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Shown {
    /// A game we started runs: the Playing Now cards show.
    pub playing: bool,
    /// The page under the panel offers Filters ([`page_entries`]).
    pub filters: bool,
    /// The page under the panel offers Performance ([`page_entries`]).
    pub performance: bool,
    /// Something runs in the background: a sync, an art fetch or a Proton download.
    pub running: bool,
    /// The system offers the network.
    pub network: bool,
    /// The system offers Bluetooth.
    pub bluetooth: bool,
    /// The system offers the sound outputs.
    pub sound_output: bool,
    /// The system offers OS updates (Pindu OS).
    pub system_update: bool,
    /// The panel is over a running game (`game_overlay`), which leaves out what needs the
    /// main window: Surprise Me, Add a Store, Add a Game and Diagnostics Report.
    pub over_game: bool,
}

/// What `section` lists while `shown` holds, top to bottom. Quick's tiles
/// ([`quick_tiles`]) Sound Output, Bluetooth, Network, Refresh Library and Add a Game are
/// listed twice, once in Quick and once where they belong.
pub fn section_entries(section: QuickAccessSection, shown: Shown) -> Vec<QuickAccessEntry> {
    use QuickAccessEntry::*;
    let main_window = !shown.over_game;
    let entries: &[(QuickAccessEntry, bool)] = match section {
        QuickAccessSection::Quick => &[
            (PlayingNow, shown.playing),
            (Filters, shown.filters),
            (Performance, shown.performance),
        ],
        QuickAccessSection::Notifications => &[(Running, shown.running), (Notifications, true)],
        QuickAccessSection::Design => &[(Designs, true)],
        QuickAccessSection::Controller => &[(Controllers, true), (Controls, true)],
        QuickAccessSection::System => &[
            (Network, shown.network),
            (Bluetooth, shown.bluetooth),
            (SoundOutput, shown.sound_output),
            (SystemUpdate, shown.system_update),
            (NavigationSounds, true),
            (DiagnosticsReport, main_window),
        ],
        QuickAccessSection::Manage => &[
            (Sources, true),
            (Duplicates, true),
            (LibraryHealth, true),
            (Proton, true),
            (AddStore, main_window),
            (AddGame, main_window),
            (RefreshLibrary, true),
            (Artwork, true),
            (Metadata, true),
        ],
    };
    let tiles = if section == QuickAccessSection::Quick {
        quick_tiles(shown)
    } else {
        Vec::new()
    };
    entries
        .iter()
        .filter_map(|&(entry, show)| show.then_some(entry))
        .chain(tiles.into_iter().map(QuickTile::entry))
        .collect()
}

/// A tile of Quick's grid, under its cards: one leads to a level, the others act.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickTile {
    /// Leads to the sound outputs, naming the one in use.
    SoundOutput,
    /// Leads to Bluetooth, naming the connected devices or how the radio is.
    Bluetooth,
    /// Leads to the network, naming the connection in use.
    Network,
    /// Shows a random installed game (`win.surprise-me`).
    SurpriseMe,
    /// Imports the sources again (`app.import`).
    RefreshLibrary,
    /// Opens Add a Game (`win.add-game-fullscreen`).
    AddGame,
}

impl QuickTile {
    /// The entry the tile stands for, which acts or leads where the tile does.
    pub fn entry(self) -> QuickAccessEntry {
        match self {
            Self::SoundOutput => QuickAccessEntry::SoundOutput,
            Self::Bluetooth => QuickAccessEntry::Bluetooth,
            Self::Network => QuickAccessEntry::Network,
            Self::SurpriseMe => QuickAccessEntry::SurpriseMe,
            Self::RefreshLibrary => QuickAccessEntry::RefreshLibrary,
            Self::AddGame => QuickAccessEntry::AddGame,
        }
    }

    /// The level the tile leads to, so it carries an arrow; `None` for a tile that acts.
    pub fn level(self) -> Option<QuickAccessLevel> {
        match self {
            Self::SoundOutput => Some(QuickAccessLevel::SoundOutput),
            Self::Bluetooth => Some(QuickAccessLevel::Bluetooth),
            Self::Network => Some(QuickAccessLevel::Network),
            Self::SurpriseMe | Self::RefreshLibrary | Self::AddGame => None,
        }
    }
}

/// Quick's tiles while `shown` holds, in the grid's order: Sound Output, Bluetooth and
/// Network where the system offers them, then Surprise Me, Refresh Library and Add a Game,
/// Surprise Me and Add a Game only in the main window. The tiles that show fill the grid
/// from its start.
pub fn quick_tiles(shown: Shown) -> Vec<QuickTile> {
    let main_window = !shown.over_game;
    [
        (QuickTile::SoundOutput, shown.sound_output),
        (QuickTile::Bluetooth, shown.bluetooth),
        (QuickTile::Network, shown.network),
        (QuickTile::SurpriseMe, main_window),
        (QuickTile::RefreshLibrary, true),
        (QuickTile::AddGame, main_window),
    ]
    .into_iter()
    .filter_map(|(tile, show)| show.then_some(tile))
    .collect()
}

/// Where focus goes first as Quick shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickFocus {
    /// The first page card: what the player most likely opened the panel for.
    PageCard,
    /// The first Playing Now card's Return.
    PlayingNowReturn,
    /// The first tile.
    Tile,
}

/// Where focus goes first as Quick shows while `shown` holds: the first page card, without
/// one the first Playing Now card's Return, without one the first tile (Refresh Library
/// always shows).
pub fn quick_first_focus(shown: Shown) -> QuickFocus {
    if shown.filters || shown.performance {
        QuickFocus::PageCard
    } else if shown.playing {
        QuickFocus::PlayingNowReturn
    } else {
        QuickFocus::Tile
    }
}

/// The columns of Quick's tile grid.
pub const QUICK_TILE_COLUMNS: u32 = 3;

/// Where a direction moves focus from a tile of Quick's grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileStep {
    /// To the tile at this index, the same one where a row ends.
    Tile(u32),
    /// Out of the grid above its first row: to the last page card, or the last Playing
    /// Now card.
    Above,
    /// Out of the grid below its last row: to the activity strip.
    Below,
}

/// Where `direction` moves focus from the tile at `index` of Quick's `count` tiles, laid
/// out [`QUICK_TILE_COLUMNS`] to a row: Left and Right along the row, stopping at its ends;
/// Up and Down to the row above or below in the same column, into a shorter row its
/// nearest tile, and out of the grid past its first or last row.
pub fn step_tile(index: u32, direction: Direction, count: u32) -> TileStep {
    match (
        grid::step(index, direction, count, QUICK_TILE_COLUMNS),
        direction,
    ) {
        (Some(tile), _) => TileStep::Tile(tile),
        (None, Direction::Left | Direction::Right) => TileStep::Tile(index),
        (None, Direction::Up) => TileStep::Above,
        (None, Direction::Down) => TileStep::Below,
    }
}

/// The sections that have anything to show while `shown` holds, in their order.
pub fn sections(shown: Shown) -> Vec<QuickAccessSection> {
    QuickAccessSection::ALL
        .into_iter()
        .filter(|&section| !section_entries(section, shown).is_empty())
        .collect()
}

/// What the Quick Access panel shows: one of its sections, or the list one of their
/// entries leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAccessLevel {
    /// The section [`Panels::section`] names, under the section strip.
    Main,
    /// What each button and key does: every intent in groups (see [`crate::controls`]).
    Controls,
    /// The library filters, while the library is under the panel (see [`page_entries`]).
    Filters,
    /// The Performance Overlay (MangoHud) and GameMode switches of the focused game, one
    /// we run ourselves (see [`page_entries`]).
    Performance,
    /// The wired and Wi-Fi devices and the Wi-Fi networks in range, to join or forget (see
    /// [`crate::system::network`]).
    Network,
    /// The Bluetooth adapter's switch, the paired devices and the ones found nearby (see
    /// [`crate::system::bluetooth`]).
    Bluetooth,
    /// The audio outputs, with a check on the one in use; choosing one makes it the
    /// default (see [`crate::system::audio`]).
    SoundOutput,
    /// The installed OS version, an update waiting for the next start and the version to
    /// roll back to, on Pindu OS (see [`crate::system::update`]).
    SystemUpdate,
    /// The stores to install from Flathub, and the install that runs (see
    /// [`crate::add_store`]).
    AddStore,
    /// The Library Health report (see [`crate::health`]): the sources, starting games,
    /// artwork and the library file, each problem with its next step.
    LibraryHealth,
    /// The diagnostics report in pages to photograph, a copy of which is saved as it opens
    /// (see [`crate::report_screen`]).
    DiagnosticsReport,
    /// The games with a log from Play with Logs, the game played with logs last first, to
    /// add one's newest log to the diagnostics report (see
    /// [`crate::diagnostics::log_choices`]); B leads back to the report.
    ReportLog,
    /// The Proton builds downloaded into our tools folder, the GE-Proton and
    /// Proton-CachyOS releases to download (see [`crate::proton_downloads`]), and the
    /// defaults for new Windows games: the Proton build, MangoHud and GameMode, the same
    /// settings as Preferences'.
    Proton,
    /// Each imported source, whether it was found on this computer and its Import switch,
    /// the same setting as Preferences' (see [`crate::sources::sync::imported_source_states`]),
    /// then the source order a game's copies follow, each source with Move Up and Move Down
    /// (see [`crate::game_groups::source_order`]).
    Sources,
    /// The duplicate suggestions to accept or dismiss, and the groups made automatically,
    /// to undo (see [`crate::duplicates::review`]).
    Duplicates,
}

impl QuickAccessLevel {
    /// Every level, the sections first.
    pub const ALL: [Self; 15] = [
        Self::Main,
        Self::Controls,
        Self::Filters,
        Self::Performance,
        Self::Network,
        Self::Bluetooth,
        Self::SoundOutput,
        Self::SystemUpdate,
        Self::AddStore,
        Self::LibraryHealth,
        Self::DiagnosticsReport,
        Self::ReportLog,
        Self::Proton,
        Self::Sources,
        Self::Duplicates,
    ];

    /// The name the level goes by: its page of the panel's stack, and what a row asks for
    /// to lead to it.
    pub fn name(self) -> &'static str {
        match self {
            Self::Main => "main",
            Self::Controls => "controls",
            Self::Filters => "filters",
            Self::Performance => "performance",
            Self::Network => "network",
            Self::Bluetooth => "bluetooth",
            Self::SoundOutput => "sound-output",
            Self::SystemUpdate => "system-update",
            Self::AddStore => "add-store",
            Self::LibraryHealth => "library-health",
            Self::DiagnosticsReport => "diagnostics-report",
            Self::ReportLog => "report-log",
            Self::Proton => "proton",
            Self::Sources => "sources",
            Self::Duplicates => "duplicates",
        }
    }

    /// The level that goes by `name` (see [`Self::name`]), if one does.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|level| level.name() == name)
    }

    /// The section that holds the entry leading to the level, which B leads back to when
    /// the level was not reached from another section that leads to it (see
    /// [`QuickAccessSection::leads_to`]). The sections themselves ([`Self::Main`]) are
    /// Quick's, where the panel opens.
    pub fn section(self) -> QuickAccessSection {
        match self {
            Self::Main | Self::Filters | Self::Performance => QuickAccessSection::Quick,
            Self::Controls => QuickAccessSection::Controller,
            Self::Network
            | Self::Bluetooth
            | Self::SoundOutput
            | Self::SystemUpdate
            | Self::DiagnosticsReport
            | Self::ReportLog => QuickAccessSection::System,
            Self::AddStore
            | Self::LibraryHealth
            | Self::Proton
            | Self::Sources
            | Self::Duplicates => QuickAccessSection::Manage,
        }
    }
}

/// What lies under the panels when an intent comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Under {
    /// A game has focus, or the page is a game's: View or X has a game to show options for.
    pub game: bool,
    /// That game is one we run ourselves ([`launch::offers_performance`]): Quick Access
    /// offers its performance switches.
    pub own_game: bool,
    /// A dialog is open over everything, and takes the buttons first.
    pub dialog: bool,
    /// A notification is unread: Quick Access opens on its Notifications section.
    pub unread: bool,
    /// The page is fullscreen mode's library: Quick Access offers its filters.
    pub library: bool,
    /// A collection in the drawer's Collections level has focus: View or X has a
    /// collection to show actions for.
    pub collection: bool,
    /// What Quick Access's sections show, which decides the sections LB and RB step
    /// through (see [`sections`]).
    pub shown: Shown,
}

/// The panel that is open, if any, and the section Quick Access shows at its main level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Panels {
    open: Option<Panel>,
    section: QuickAccessSection,
}

impl Panels {
    /// No panel open.
    pub fn new() -> Self {
        Self::default()
    }

    /// The panel that is open, at its level.
    pub fn open(&self) -> Option<Panel> {
        self.open
    }

    /// The section Quick Access shows while it is at [`QuickAccessLevel::Main`], or the
    /// one B leads back to from its level.
    pub fn section(&self) -> QuickAccessSection {
        self.section
    }

    /// Acts on a controller intent and says whether the panels took it, so the page does
    /// not act on it too. Start and Guide toggle the drawer, R3, Guide + A and the Steam
    /// Deck's "…" toggle Quick Access (on its Notifications section while one is unread,
    /// as Steam's does, else on Quick), LB and RB step through Quick Access's sections
    /// while it shows one, View or X opens the options for a game, or in the drawer's
    /// Collections leads to the focused collection's actions, and B steps back a level or
    /// closes. Opening one panel closes the other. While a panel is open the page's own
    /// buttons (search, tabs, sort) wait; directions and A move and act inside it.
    pub fn handle(&mut self, intent: Intent, under: Under) -> bool {
        if under.dialog {
            return false;
        }
        match (intent, self.open) {
            (Intent::Menu | Intent::Home, Some(Panel::Drawer(_))) => self.open = None,
            (Intent::Menu | Intent::Home, _) => self.open = Some(Panel::Drawer(DrawerLevel::Main)),
            (Intent::QuickAccess | Intent::SystemQuickAccess, Some(Panel::QuickAccess(_))) => {
                self.open = None;
            }
            (Intent::QuickAccess | Intent::SystemQuickAccess, _) => {
                self.show_section(if under.unread {
                    QuickAccessSection::Notifications
                } else {
                    QuickAccessSection::Quick
                });
            }
            (
                Intent::PreviousTab | Intent::NextTab,
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
            ) => {
                let shown = sections(under.shown);
                let at = shown.iter().position(|&section| section == self.section);
                let step = match (at, intent) {
                    (Some(at), Intent::NextTab) => (at + 1) % shown.len(),
                    (Some(at), _) => (at + shown.len() - 1) % shown.len(),
                    (None, _) => 0,
                };
                if let Some(&section) = shown.get(step) {
                    self.section = section;
                }
            }
            (Intent::Options, None) if under.game => {
                self.open = Some(Panel::Options(OptionsLevel::Main));
            }
            (Intent::Options, Some(Panel::Drawer(DrawerLevel::Collections)))
                if under.collection =>
            {
                self.open = Some(Panel::Drawer(DrawerLevel::Collection));
            }
            (
                Intent::Back,
                Some(
                    Panel::Drawer(DrawerLevel::Main)
                    | Panel::Options(OptionsLevel::Main)
                    | Panel::QuickAccess(QuickAccessLevel::Main),
                ),
            ) => {
                self.open = None;
            }
            (Intent::Back, Some(Panel::Drawer(DrawerLevel::Collection))) => {
                self.open = Some(Panel::Drawer(DrawerLevel::Collections));
            }
            (Intent::Back, Some(Panel::Drawer(_))) => {
                self.open = Some(Panel::Drawer(DrawerLevel::Main));
            }
            (Intent::Back, Some(Panel::Options(_))) => {
                self.open = Some(Panel::Options(OptionsLevel::Main));
            }
            (Intent::Back, Some(Panel::QuickAccess(QuickAccessLevel::ReportLog))) => {
                self.open = Some(Panel::QuickAccess(QuickAccessLevel::DiagnosticsReport));
            }
            (Intent::Back, Some(Panel::QuickAccess(_))) => {
                self.open = Some(Panel::QuickAccess(QuickAccessLevel::Main));
            }
            (Intent::Navigate(_) | Intent::Activate, _) | (_, None) => return false,
            (_, Some(_)) => {}
        }
        true
    }

    /// Shows `panel` at its level: opened by a button rather than a controller, or one of
    /// its entries leading to a list. A Quick Access level reached from a section that
    /// leads to it keeps that section for B to lead back to; any other takes its own
    /// ([`QuickAccessLevel::section`]).
    pub fn show(&mut self, panel: Panel) {
        if let Panel::QuickAccess(level) = panel
            && level != QuickAccessLevel::Main
        {
            let from_section = self.open == Some(Panel::QuickAccess(QuickAccessLevel::Main))
                && self.section.leads_to(level);
            if !from_section {
                self.section = level.section();
            }
        }
        self.open = Some(panel);
    }

    /// Shows Quick Access's `section`: opened by a button or the pointer on the section
    /// strip.
    pub fn show_section(&mut self, section: QuickAccessSection) {
        self.section = section;
        self.open = Some(Panel::QuickAccess(QuickAccessLevel::Main));
    }

    /// Closes the open panel: an entry did its work, the pointer clicked outside it, or
    /// the window left fullscreen mode.
    pub fn close(&mut self) {
        self.open = None;
    }
}

/// An entry of the menu drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerEntry {
    /// A card for a game we started that is running, with Return and Stop.
    PlayingNow,
    /// Go to Home.
    Home,
    /// Go to the library.
    Library,
    /// Lead to the collections ([`DrawerLevel::Collections`]).
    Collections,
    /// Leave fullscreen mode for the desktop window.
    ExitFullscreen,
    /// Lead to suspending, restarting and powering off the machine, and to quitting
    /// ([`DrawerLevel::Power`]).
    Power,
}

/// The drawer's entries, top to bottom: a Playing Now card while a game we started runs,
/// Collections when there are any, and Power last while its level lists anything
/// ([`crate::gamescope::session::power_level`]), which it does except in the launcher's
/// own session on a system that offers none of its actions.
pub fn drawer_entries(playing: bool, collections: bool, power: bool) -> Vec<DrawerEntry> {
    use DrawerEntry::*;
    let playing = playing.then_some(PlayingNow);
    let collections = collections.then_some(Collections);
    let power = power.then_some(Power);
    [
        playing,
        Some(Home),
        Some(Library),
        collections,
        Some(ExitFullscreen),
        power,
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// An action of the drawer's level for one collection ([`DrawerLevel::Collection`]), as
/// the desktop sidebar's collection menu has them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollectionAction {
    /// Give the collection another name, typed with the on-screen keyboard.
    Rename,
    /// Move the collection one place up, before the one above it.
    MoveUp,
    /// Move the collection one place down, after the one below it.
    MoveDown,
    /// Delete the collection, after asking; its games stay in the library.
    Delete,
}

/// The actions for the collection at `index` of `count` collections, top to bottom: Move
/// Up unless it is first, Move Down unless it is last.
pub fn collection_actions(index: usize, count: usize) -> Vec<CollectionAction> {
    use CollectionAction::*;
    [
        Some(Rename),
        (index > 0).then_some(MoveUp),
        (index + 1 < count).then_some(MoveDown),
        Some(Delete),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// An entry Quick Access adds for the page under it, besides the ones it always has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PageEntry {
    /// The library filters, leading to [`QuickAccessLevel::Filters`].
    Filters,
    /// The focused game's performance switches, leading to
    /// [`QuickAccessLevel::Performance`].
    Performance,
}

/// The entries Quick Access adds for the page `under` it, top to bottom: the library's
/// Filters, then Performance for a focused game we run ourselves (on its page, its cover
/// or its Home card).
pub fn page_entries(under: Under) -> Vec<PageEntry> {
    [
        under.library.then_some(PageEntry::Filters),
        (under.game && under.own_game).then_some(PageEntry::Performance),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// Whether a game runs, as the launcher knows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Playing {
    /// It does not run.
    #[default]
    No,
    /// We started it and it runs: the launcher can stop it.
    Started,
    /// Its store launcher runs it, in a `--steam` gamescope where the launcher sees its
    /// windows: the launcher can bring it back to the front, and stop it by closing them.
    InStore,
}

/// An action of the options sheet for one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOption {
    /// Start the game.
    Play,
    /// Start a game we run once without its wrappers, MangoHud, GameMode or its own
    /// environment variables (see [`launch::Start::Safely`]).
    StartSafely,
    /// Stop a game that runs: end one we started, or close the windows of one its store
    /// launcher runs.
    Stop,
    /// Bring back a game its store launcher runs.
    Return,
    /// Mark the game a favourite.
    AddToFavourites,
    /// Take the game out of the favourites.
    RemoveFromFavourites,
    /// Lead to the collections, to add the game to or take it out of.
    Collections,
    /// Change the game's artwork, from SteamGridDB or a file.
    ChangeArtwork,
    /// Lead to the other games, to merge the game with one as copies of one game
    /// ([`OptionsLevel::Merge`]).
    MergeWith,
    /// Lead to a group's copies, to choose the one Play starts ([`OptionsLevel::Copies`]).
    PreferredCopy,
    /// Split a group into its games again, remembering that they are not the same game.
    Split,
    /// Hide the game.
    Hide,
    /// Show the hidden game again.
    Unhide,
    /// Open the game in its store launcher, when it has one.
    OpenInLauncher,
    /// Open the game's ProtonDB page in the browser, for a Steam game.
    ProtonDb,
    /// Open the game's PCGamingWiki page in the browser, for a Steam game.
    PcGamingWiki,
    /// List the save snapshots of a Windows game we run, or of a game the user added save
    /// folders for, to restore one.
    SaveSnapshots,
}

/// The options sheet's actions for `game`, top to bottom, as it runs or not (`playing`).
pub fn game_options(game: &Game, playing: Playing) -> Vec<GameOption> {
    use GameOption::*;
    [
        Some(match playing {
            Playing::No => Play,
            Playing::Started => Stop,
            Playing::InStore => Return,
        }),
        (playing == Playing::No && launch::launch_settings(game).is_some()).then_some(StartSafely),
        (playing == Playing::InStore).then_some(Stop),
        Some(if game.favourite {
            RemoveFromFavourites
        } else {
            AddToFavourites
        }),
        Some(Collections),
        Some(ChangeArtwork),
        saves::offers_snapshots(game).then_some(SaveSnapshots),
        Some(if game.hidden { Unhide } else { Hide }),
        launch::launcher_uri(game).map(|_| OpenInLauncher),
        launch::web_page_uri(game, WebPage::ProtonDb).map(|_| ProtonDb),
        launch::web_page_uri(game, WebPage::PcGamingWiki).map(|_| PcGamingWiki),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The options sheet's actions for `game`, as [`game_options`] lists them with Merge With
/// after Change Artwork, and, for a group of `copies` copies (0 for a game in no group),
/// Preferred Copy and Split after it.
pub fn sheet_options(game: &Game, playing: Playing, copies: usize) -> Vec<GameOption> {
    let mut options = game_options(game, playing);
    let at = options
        .iter()
        .position(|option| *option == GameOption::ChangeArtwork)
        .map_or(options.len(), |index| index + 1);
    let grouped = copies > 1;
    let grouping = [
        Some(GameOption::MergeWith),
        grouped.then_some(GameOption::PreferredCopy),
        grouped.then_some(GameOption::Split),
    ];
    options.splice(at..at, grouping.into_iter().flatten());
    options
}

/// The options sheet's actions `options` with Save Snapshots, for a game the user added
/// save folders for, where a Windows game we run has it: before Hide or Unhide. Options
/// that hold it already stay as they are.
pub fn with_save_snapshots(mut options: Vec<GameOption>) -> Vec<GameOption> {
    if !options.contains(&GameOption::SaveSnapshots) {
        let at = options
            .iter()
            .position(|option| matches!(option, GameOption::Hide | GameOption::Unhide))
            .unwrap_or(options.len());
        options.insert(at, GameOption::SaveSnapshots);
    }
    options
}

/// A design the Quick Access panel offers, or Adwaita (no design, `id` empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignChoice {
    /// What the `fullscreen-design` setting holds for it; empty for Adwaita.
    pub id: String,
    /// The theme's name; `None` for Adwaita, which the shell names.
    pub name: Option<String>,
    /// The theme's description; empty for Adwaita.
    pub description: String,
    /// The design in force, which the panel checks.
    pub active: bool,
    /// Whether this Pindu launcher can apply it.
    pub available: bool,
    /// The theme's picture of its Home (`Theme::preview`); `None` for a design without
    /// one, and for Adwaita, whose preview the shell has built in.
    pub preview: Option<std::path::PathBuf>,
}

/// What the Quick Access panel's Designs level lists: Adwaita first, then `themes` in
/// their order, with the one in force checked. `active` is the setting's id; Adwaita is
/// in force when it names no design this Pindu launcher can apply.
pub fn design_choices(themes: &[Theme], active: &str) -> Vec<DesignChoice> {
    let applies = themes
        .iter()
        .any(|theme| theme.id == active && theme.supported());
    let default = DesignChoice {
        id: String::new(),
        name: None,
        description: String::new(),
        active: !applies,
        available: true,
        preview: None,
    };
    let designs = themes.iter().map(|theme| DesignChoice {
        id: theme.id.clone(),
        name: Some(theme.name.clone()),
        description: theme.description.clone(),
        active: applies && theme.id == active,
        available: theme.supported(),
        preview: theme.preview.clone(),
    });
    std::iter::once(default).chain(designs).collect()
}

/// A variant of the design in force the Quick Access panel offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantChoice {
    /// What the `fullscreen-design-variant` setting holds for it.
    pub id: String,
    /// The variant's name.
    pub name: String,
    /// The variant in force, which the panel checks.
    pub active: bool,
}

/// What the Quick Access panel's Variant list offers: the variants of `design` while it is
/// in force, with the one `variant` names checked, or else the first
/// (`Theme::variant`). Empty, so no Variant list shows, for Adwaita, a design this Pindu
/// launcher cannot apply and one without variants.
pub fn variant_choices(themes: &[Theme], design: &str, variant: &str) -> Vec<VariantChoice> {
    let Some(theme) = themes
        .iter()
        .find(|theme| theme.id == design && theme.supported())
    else {
        return Vec::new();
    };
    let active = theme.variant(variant).map(|variant| variant.id.as_str());
    theme
        .variants
        .iter()
        .map(|choice| VariantChoice {
            id: choice.id.clone(),
            name: choice.name.clone(),
            active: Some(choice.id.as_str()) == active,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::Direction;
    use crate::model::{GameId, LaunchKind, Source};
    use crate::themes::Variant;

    const PAGE: Under = Under {
        game: false,
        own_game: false,
        dialog: false,
        unread: false,
        library: false,
        collection: false,
        shown: BARE,
    };
    const GAME: Under = Under {
        game: true,
        own_game: false,
        dialog: false,
        unread: false,
        library: false,
        collection: false,
        shown: BARE,
    };
    const DIALOG: Under = Under {
        game: true,
        own_game: false,
        dialog: true,
        unread: false,
        library: false,
        collection: false,
        shown: BARE,
    };

    fn drawer() -> Option<Panel> {
        Some(Panel::Drawer(DrawerLevel::Main))
    }

    fn options() -> Option<Panel> {
        Some(Panel::Options(OptionsLevel::Main))
    }

    fn quick_access() -> Option<Panel> {
        Some(Panel::QuickAccess(QuickAccessLevel::Main))
    }

    #[test]
    fn quick_access_has_six_sections_in_order_each_with_its_name() {
        use QuickAccessSection::*;
        assert_eq!(
            QuickAccessSection::ALL,
            [Quick, Notifications, Design, Controller, System, Manage]
        );
        let names = QuickAccessSection::ALL.map(QuickAccessSection::name);
        assert_eq!(
            names,
            [
                "quick",
                "notifications",
                "design",
                "controller",
                "system",
                "manage"
            ]
        );
        for section in QuickAccessSection::ALL {
            assert_eq!(QuickAccessSection::from_name(section.name()), Some(section));
        }
        assert_eq!(QuickAccessSection::from_name("Quick"), None);
        assert_eq!(QuickAccessSection::from_name("main"), None);
    }

    /// Nothing optional shows: no game runs, nothing in the background, no page entry,
    /// no system entry, in the main window.
    const BARE: Shown = Shown {
        playing: false,
        filters: false,
        performance: false,
        running: false,
        network: false,
        bluetooth: false,
        sound_output: false,
        system_update: false,
        over_game: false,
    };
    /// Everything optional shows.
    const FULL: Shown = Shown {
        playing: true,
        filters: true,
        performance: true,
        running: true,
        network: true,
        bluetooth: true,
        sound_output: true,
        system_update: true,
        over_game: false,
    };

    #[test]
    fn quick_lists_the_cards_the_page_s_entries_the_system_s_and_the_quick_actions() {
        use QuickAccessEntry::*;
        assert_eq!(
            section_entries(QuickAccessSection::Quick, FULL),
            [
                PlayingNow,
                Filters,
                Performance,
                SoundOutput,
                Bluetooth,
                Network,
                SurpriseMe,
                RefreshLibrary,
                AddGame
            ]
        );
        assert_eq!(
            section_entries(QuickAccessSection::Quick, BARE),
            [SurpriseMe, RefreshLibrary, AddGame]
        );
        let some = Shown {
            filters: true,
            bluetooth: true,
            ..BARE
        };
        assert_eq!(
            section_entries(QuickAccessSection::Quick, some),
            [Filters, Bluetooth, SurpriseMe, RefreshLibrary, AddGame]
        );
        let over_game = Shown {
            over_game: true,
            ..FULL
        };
        assert_eq!(
            section_entries(QuickAccessSection::Quick, over_game),
            [
                PlayingNow,
                Filters,
                Performance,
                SoundOutput,
                Bluetooth,
                Network,
                RefreshLibrary
            ],
            "Surprise Me and Add a Game need the main window"
        );
    }

    #[test]
    fn quick_s_tiles_are_the_system_s_entries_then_the_quick_actions_as_they_show() {
        use QuickTile::*;
        assert_eq!(
            quick_tiles(FULL),
            [
                SoundOutput,
                Bluetooth,
                Network,
                SurpriseMe,
                RefreshLibrary,
                AddGame
            ]
        );
        assert_eq!(
            quick_tiles(BARE),
            [SurpriseMe, RefreshLibrary, AddGame],
            "the Flatpak, with no system entries, shows one row of three"
        );
        let some = Shown {
            network: true,
            ..BARE
        };
        assert_eq!(
            quick_tiles(some),
            [Network, SurpriseMe, RefreshLibrary, AddGame]
        );
        let over_game = Shown {
            over_game: true,
            ..FULL
        };
        assert_eq!(
            quick_tiles(over_game),
            [SoundOutput, Bluetooth, Network, RefreshLibrary],
            "Surprise Me and Add a Game need the main window"
        );
        let over_game_bare = Shown {
            over_game: true,
            ..BARE
        };
        assert_eq!(quick_tiles(over_game_bare), [RefreshLibrary]);
    }

    #[test]
    fn a_tile_names_its_entry_and_the_level_it_leads_to() {
        use QuickTile::*;
        let tiles = [
            (
                SoundOutput,
                QuickAccessEntry::SoundOutput,
                Some(QuickAccessLevel::SoundOutput),
            ),
            (
                Bluetooth,
                QuickAccessEntry::Bluetooth,
                Some(QuickAccessLevel::Bluetooth),
            ),
            (
                Network,
                QuickAccessEntry::Network,
                Some(QuickAccessLevel::Network),
            ),
            (SurpriseMe, QuickAccessEntry::SurpriseMe, None),
            (RefreshLibrary, QuickAccessEntry::RefreshLibrary, None),
            (AddGame, QuickAccessEntry::AddGame, None),
        ];
        for (tile, entry, level) in tiles {
            assert_eq!(tile.entry(), entry, "{tile:?}");
            assert_eq!(tile.level(), level, "{tile:?}");
        }
    }

    #[test]
    fn quick_s_focus_starts_on_the_page_card_then_return_then_the_first_tile() {
        let page_card = Shown {
            filters: true,
            playing: true,
            ..BARE
        };
        assert_eq!(quick_first_focus(page_card), QuickFocus::PageCard);
        let performance = Shown {
            performance: true,
            ..BARE
        };
        assert_eq!(quick_first_focus(performance), QuickFocus::PageCard);
        let playing = Shown {
            playing: true,
            sound_output: true,
            ..BARE
        };
        assert_eq!(quick_first_focus(playing), QuickFocus::PlayingNowReturn);
        assert_eq!(quick_first_focus(BARE), QuickFocus::Tile);
        let over_game = Shown {
            over_game: true,
            ..BARE
        };
        assert_eq!(quick_first_focus(over_game), QuickFocus::Tile);
    }

    #[test]
    fn left_and_right_move_along_a_row_of_tiles_and_stop_at_its_ends() {
        use Direction::{Left, Right};
        // Two rows: 0 1 2 / 3 4 5.
        assert_eq!(step_tile(0, Right, 6), TileStep::Tile(1));
        assert_eq!(step_tile(1, Left, 6), TileStep::Tile(0));
        assert_eq!(step_tile(0, Left, 6), TileStep::Tile(0));
        assert_eq!(
            step_tile(2, Right, 6),
            TileStep::Tile(2),
            "not on to the next row"
        );
        assert_eq!(
            step_tile(3, Left, 6),
            TileStep::Tile(3),
            "not back to the row above"
        );
        // A short last row ends at its last tile: 0 1 2 / 3.
        assert_eq!(step_tile(3, Right, 4), TileStep::Tile(3));
    }

    #[test]
    fn up_and_down_keep_the_column_and_leave_the_grid_past_its_rows() {
        use Direction::{Down, Up};
        assert_eq!(step_tile(1, Down, 6), TileStep::Tile(4));
        assert_eq!(step_tile(5, Up, 6), TileStep::Tile(2));
        assert_eq!(step_tile(1, Up, 6), TileStep::Above);
        assert_eq!(step_tile(4, Down, 6), TileStep::Below);
        // One row: both leave.
        assert_eq!(step_tile(0, Up, 3), TileStep::Above);
        assert_eq!(step_tile(2, Down, 3), TileStep::Below);
        // Into a shorter row, the nearest tile: 0 1 2 / 3 4.
        assert_eq!(step_tile(0, Down, 5), TileStep::Tile(3));
        assert_eq!(step_tile(2, Down, 5), TileStep::Tile(4));
        assert_eq!(step_tile(2, Down, 4), TileStep::Tile(3));
        assert_eq!(step_tile(4, Up, 5), TileStep::Tile(1));
    }

    #[test]
    fn notifications_lists_what_runs_first_while_something_does() {
        use QuickAccessEntry::*;
        assert_eq!(
            section_entries(QuickAccessSection::Notifications, FULL),
            [Running, Notifications]
        );
        assert_eq!(
            section_entries(QuickAccessSection::Notifications, BARE),
            [Notifications],
            "the notifications, or the line that says there are none"
        );
    }

    #[test]
    fn design_and_controller_hold_what_their_levels_held() {
        use QuickAccessEntry::*;
        for shown in [BARE, FULL] {
            assert_eq!(
                section_entries(QuickAccessSection::Design, shown),
                [Designs]
            );
            assert_eq!(
                section_entries(QuickAccessSection::Controller, shown),
                [Controllers, Controls]
            );
        }
    }

    #[test]
    fn system_lists_the_available_system_entries_then_the_navigation_sounds_and_the_report() {
        use QuickAccessEntry::*;
        assert_eq!(
            section_entries(QuickAccessSection::System, FULL),
            [
                Network,
                Bluetooth,
                SoundOutput,
                SystemUpdate,
                NavigationSounds,
                DiagnosticsReport
            ]
        );
        assert_eq!(
            section_entries(QuickAccessSection::System, BARE),
            [NavigationSounds, DiagnosticsReport]
        );
        let update = Shown {
            system_update: true,
            over_game: true,
            ..BARE
        };
        assert_eq!(
            section_entries(QuickAccessSection::System, update),
            [SystemUpdate, NavigationSounds],
            "the report needs the main window"
        );
        let nothing_else = Shown {
            over_game: true,
            ..BARE
        };
        assert_eq!(
            section_entries(QuickAccessSection::System, nothing_else),
            [NavigationSounds],
            "the navigation sounds always show"
        );
    }

    #[test]
    fn manage_lists_the_library_s_upkeep_then_artwork_and_metadata() {
        use QuickAccessEntry::*;
        let upkeep = [
            Sources,
            Duplicates,
            LibraryHealth,
            Proton,
            AddStore,
            AddGame,
            RefreshLibrary,
            Artwork,
            Metadata,
        ];
        assert_eq!(section_entries(QuickAccessSection::Manage, FULL), upkeep);
        assert_eq!(section_entries(QuickAccessSection::Manage, BARE), upkeep);
        let over_game = Shown {
            over_game: true,
            ..FULL
        };
        assert_eq!(
            section_entries(QuickAccessSection::Manage, over_game),
            [
                Sources,
                Duplicates,
                LibraryHealth,
                Proton,
                RefreshLibrary,
                Artwork,
                Metadata
            ],
            "Add a Store and Add a Game need the main window"
        );
    }

    #[test]
    fn every_entry_of_the_old_main_list_has_a_place_in_a_section() {
        use QuickAccessEntry::*;
        let listed: Vec<QuickAccessEntry> = QuickAccessSection::ALL
            .into_iter()
            .flat_map(|section| section_entries(section, FULL))
            .collect();
        for entry in [
            Notifications,
            Running,
            RefreshLibrary,
            SurpriseMe,
            Filters,
            Performance,
            Designs,
            Controllers,
            Controls,
            NavigationSounds,
            Artwork,
            Metadata,
            Sources,
            Duplicates,
            Network,
            Bluetooth,
            SoundOutput,
            SystemUpdate,
            LibraryHealth,
            AddStore,
            DiagnosticsReport,
            Proton,
            AddGame,
        ] {
            assert!(listed.contains(&entry), "{entry:?}");
        }
    }

    #[test]
    fn a_section_with_nothing_to_show_is_left_out() {
        assert_eq!(sections(BARE), QuickAccessSection::ALL);
        assert_eq!(sections(FULL), QuickAccessSection::ALL);
        let over_game = Shown {
            over_game: true,
            ..BARE
        };
        assert_eq!(
            sections(over_game),
            QuickAccessSection::ALL,
            "each still has something"
        );
        for section in QuickAccessSection::ALL {
            for shown in [BARE, FULL, over_game] {
                assert_eq!(
                    sections(shown).contains(&section),
                    !section_entries(section, shown).is_empty(),
                    "{section:?}"
                );
            }
        }
    }

    #[test]
    fn each_quick_access_level_has_the_name_its_page_goes_by() {
        assert_eq!(QuickAccessLevel::Main.name(), "main");
        assert_eq!(QuickAccessLevel::Controls.name(), "controls");
        assert_eq!(QuickAccessLevel::SoundOutput.name(), "sound-output");
        assert_eq!(QuickAccessLevel::SystemUpdate.name(), "system-update");
        assert_eq!(QuickAccessLevel::AddStore.name(), "add-store");
        assert_eq!(QuickAccessLevel::LibraryHealth.name(), "library-health");
        assert_eq!(
            QuickAccessLevel::DiagnosticsReport.name(),
            "diagnostics-report"
        );
    }

    #[test]
    fn a_level_is_found_again_by_its_name_and_no_other_name_finds_one() {
        for level in QuickAccessLevel::ALL {
            assert_eq!(QuickAccessLevel::from_name(level.name()), Some(level));
        }
        assert_eq!(
            QuickAccessLevel::from_name("sound-output"),
            Some(QuickAccessLevel::SoundOutput)
        );
        assert_eq!(QuickAccessLevel::from_name("Main"), None);
        assert_eq!(QuickAccessLevel::from_name(""), None);
        assert_eq!(
            QuickAccessLevel::from_name("settings"),
            None,
            "each setting is in its section"
        );
    }

    #[test]
    fn quick_access_toggles_its_panel() {
        let mut panels = Panels::new();
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn guide_and_a_or_the_deck_s_dots_button_toggle_quick_access_as_r3_does() {
        let mut panels = Panels::new();
        assert!(panels.handle(Intent::SystemQuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::SystemQuickAccess, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn b_steps_back_from_add_a_store_and_then_closes_quick_access() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::AddStore));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn proton_is_a_level_of_its_own_that_b_steps_back_from() {
        assert_eq!(
            QuickAccessLevel::from_name("proton"),
            Some(QuickAccessLevel::Proton)
        );
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Proton));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn sources_is_a_level_of_its_own_that_b_steps_back_from() {
        assert_eq!(
            QuickAccessLevel::from_name("sources"),
            Some(QuickAccessLevel::Sources)
        );
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Sources));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn duplicates_is_a_level_of_its_own_that_b_steps_back_from() {
        assert_eq!(
            QuickAccessLevel::from_name("duplicates"),
            Some(QuickAccessLevel::Duplicates)
        );
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Duplicates));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn b_steps_back_from_a_list_to_its_section_and_then_closes_quick_access() {
        for level in [
            QuickAccessLevel::Controls,
            QuickAccessLevel::Filters,
            QuickAccessLevel::Performance,
            QuickAccessLevel::Network,
            QuickAccessLevel::Bluetooth,
            QuickAccessLevel::SoundOutput,
            QuickAccessLevel::SystemUpdate,
            QuickAccessLevel::LibraryHealth,
            QuickAccessLevel::DiagnosticsReport,
        ] {
            let mut panels = Panels::new();
            panels.handle(Intent::QuickAccess, PAGE);
            panels.show_section(level.section());
            panels.show(Panel::QuickAccess(level));
            assert!(panels.handle(Intent::Back, PAGE), "{level:?}");
            assert_eq!(panels.open(), quick_access(), "{level:?}");
            assert_eq!(panels.section(), level.section(), "{level:?}");
            assert!(panels.handle(Intent::Back, PAGE), "{level:?}");
            assert_eq!(panels.open(), None, "{level:?}");
        }
    }

    #[test]
    fn a_section_leads_to_the_levels_of_its_entries_and_quick_to_the_system_s_too() {
        use QuickAccessLevel::*;
        use QuickAccessSection as S;
        assert!(S::Quick.leads_to(Network));
        assert!(S::System.leads_to(Network));
        assert!(!S::Manage.leads_to(Network));
        assert!(!S::System.leads_to(Filters));
        assert!(!S::Quick.leads_to(SystemUpdate));
        assert!(!S::Quick.leads_to(Main), "no section leads to the sections");
    }

    #[test]
    fn each_level_belongs_to_the_section_that_holds_its_entry() {
        use QuickAccessLevel::*;
        use QuickAccessSection as S;
        for (level, section) in [
            (Filters, S::Quick),
            (Performance, S::Quick),
            (Controls, S::Controller),
            (Network, S::System),
            (Bluetooth, S::System),
            (SoundOutput, S::System),
            (SystemUpdate, S::System),
            (DiagnosticsReport, S::System),
            (ReportLog, S::System),
            (Sources, S::Manage),
            (Duplicates, S::Manage),
            (LibraryHealth, S::Manage),
            (Proton, S::Manage),
            (AddStore, S::Manage),
        ] {
            assert_eq!(level.section(), section, "{level:?}");
        }
    }

    #[test]
    fn b_from_a_level_reached_from_quick_leads_back_to_quick() {
        for level in [
            QuickAccessLevel::Network,
            QuickAccessLevel::Bluetooth,
            QuickAccessLevel::SoundOutput,
        ] {
            let mut panels = Panels::new();
            panels.handle(Intent::QuickAccess, PAGE);
            panels.show(Panel::QuickAccess(level));
            assert!(panels.handle(Intent::Back, PAGE));
            assert_eq!(panels.open(), quick_access(), "{level:?}");
            assert_eq!(panels.section(), QuickAccessSection::Quick, "{level:?}");
        }
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Network));
        panels.handle(Intent::Back, PAGE);
        assert_eq!(
            panels.section(),
            QuickAccessSection::System,
            "shown without a section, its own"
        );
    }

    #[test]
    fn quick_access_opens_on_quick_and_b_closes_it_from_any_section() {
        for section in QuickAccessSection::ALL {
            let mut panels = Panels::new();
            assert!(panels.handle(Intent::QuickAccess, PAGE));
            assert_eq!(panels.open(), quick_access());
            assert_eq!(panels.section(), QuickAccessSection::Quick);
            panels.show_section(section);
            assert_eq!(panels.open(), quick_access());
            assert_eq!(panels.section(), section);
            assert!(panels.handle(Intent::Back, PAGE), "{section:?}");
            assert_eq!(panels.open(), None, "{section:?}");
        }
    }

    #[test]
    fn lb_and_rb_step_through_the_sections_and_wrap() {
        let mut panels = Panels::new();
        panels.handle(Intent::QuickAccess, PAGE);
        let mut seen = Vec::new();
        for _ in QuickAccessSection::ALL {
            assert!(panels.handle(Intent::NextTab, PAGE));
            assert_eq!(panels.open(), quick_access());
            seen.push(panels.section());
        }
        assert_eq!(
            seen,
            [
                QuickAccessSection::Notifications,
                QuickAccessSection::Design,
                QuickAccessSection::Controller,
                QuickAccessSection::System,
                QuickAccessSection::Manage,
                QuickAccessSection::Quick,
            ],
            "RB wraps from the last to the first"
        );
        assert!(panels.handle(Intent::PreviousTab, PAGE));
        assert_eq!(
            panels.section(),
            QuickAccessSection::Manage,
            "LB wraps from the first to the last"
        );
        assert!(panels.handle(Intent::PreviousTab, PAGE));
        assert_eq!(panels.section(), QuickAccessSection::System);
    }

    #[test]
    fn lb_and_rb_do_nothing_at_a_level() {
        let mut panels = Panels::new();
        panels.handle(Intent::QuickAccess, PAGE);
        panels.show_section(QuickAccessSection::System);
        panels.show(Panel::QuickAccess(QuickAccessLevel::DiagnosticsReport));
        for intent in [Intent::NextTab, Intent::PreviousTab] {
            assert!(panels.handle(intent, PAGE), "swallowed");
            assert_eq!(
                panels.open(),
                Some(Panel::QuickAccess(QuickAccessLevel::DiagnosticsReport))
            );
            assert_eq!(panels.section(), QuickAccessSection::System);
        }
    }

    #[test]
    fn quick_access_does_not_remember_the_section_it_was_closed_on() {
        let mut panels = Panels::new();
        panels.handle(Intent::QuickAccess, PAGE);
        panels.handle(Intent::NextTab, PAGE);
        panels.handle(Intent::NextTab, PAGE);
        panels.handle(Intent::QuickAccess, PAGE);
        assert_eq!(panels.open(), None);
        panels.handle(Intent::QuickAccess, PAGE);
        assert_eq!(panels.section(), QuickAccessSection::Quick);
    }

    #[test]
    fn b_steps_back_from_the_report_log_picker_to_the_report() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::ReportLog));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(
            panels.open(),
            Some(Panel::QuickAccess(QuickAccessLevel::DiagnosticsReport))
        );
        assert_eq!(QuickAccessLevel::ReportLog.name(), "report-log");
    }

    #[test]
    fn quick_access_toggles_shut_from_one_of_its_lists() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Controls));
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), None);
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access(), "it opens at its entries");
    }

    #[test]
    fn quick_access_opens_on_its_notifications_while_one_is_unread() {
        let unread = Under {
            unread: true,
            ..PAGE
        };
        let mut panels = Panels::new();
        assert!(panels.handle(Intent::QuickAccess, unread));
        assert_eq!(panels.open(), quick_access());
        assert_eq!(panels.section(), QuickAccessSection::Notifications);
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None, "B closes it from a section");
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(
            panels.section(),
            QuickAccessSection::Quick,
            "read, it opens on Quick"
        );
    }

    #[test]
    fn quick_access_and_the_other_panels_take_each_other_s_place() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access(), "it replaces the drawer");
        assert!(panels.handle(Intent::Menu, PAGE));
        assert_eq!(panels.open(), drawer(), "the drawer replaces it");

        let mut panels = Panels::new();
        panels.handle(Intent::Options, GAME);
        assert!(panels.handle(Intent::QuickAccess, GAME));
        assert_eq!(panels.open(), quick_access(), "it replaces the options");
        assert!(panels.handle(Intent::Options, GAME), "taken, not acted on");
        assert_eq!(panels.open(), quick_access());
        for intent in [Intent::Activate, Intent::Navigate(Direction::Up)] {
            assert!(!panels.handle(intent, GAME), "{intent:?} goes to the panel");
        }
        assert!(!panels.handle(Intent::QuickAccess, DIALOG));
        assert_eq!(panels.open(), quick_access(), "a dialog takes it first");
    }

    #[test]
    fn start_and_guide_toggle_the_drawer() {
        for intent in [Intent::Menu, Intent::Home] {
            let mut panels = Panels::new();
            assert!(panels.handle(intent, PAGE), "{intent:?}");
            assert_eq!(panels.open(), drawer());
            assert!(panels.handle(intent, PAGE), "{intent:?}");
            assert_eq!(panels.open(), None);
        }
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        panels.handle(Intent::Home, PAGE);
        assert_eq!(panels.open(), None, "Guide closes what Start opened");
    }

    #[test]
    fn b_closes_the_drawer_and_then_goes_back_on_the_page() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
        assert!(!panels.handle(Intent::Back, PAGE), "the page goes back");
    }

    #[test]
    fn b_leaves_the_collections_for_the_drawer_s_entries_first() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        panels.show(Panel::Drawer(DrawerLevel::Collections));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), drawer());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn view_or_x_on_a_collection_leads_to_its_actions_and_b_back_to_the_collections() {
        let collection = Under {
            collection: true,
            ..PAGE
        };
        let mut panels = Panels::new();
        panels.show(Panel::Drawer(DrawerLevel::Collections));
        assert!(panels.handle(Intent::Options, PAGE), "taken, not acted on");
        assert_eq!(
            panels.open(),
            Some(Panel::Drawer(DrawerLevel::Collections)),
            "no collection has focus"
        );
        assert!(panels.handle(Intent::Options, collection));
        assert_eq!(panels.open(), Some(Panel::Drawer(DrawerLevel::Collection)));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), Some(Panel::Drawer(DrawerLevel::Collections)));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), drawer());
        panels.handle(Intent::Menu, collection);
        assert!(
            !panels.handle(Intent::Options, collection),
            "closed, a game's"
        );
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn view_or_x_opens_the_options_for_a_focused_game_and_b_closes_them() {
        let mut panels = Panels::new();
        assert!(!panels.handle(Intent::Options, PAGE), "no game, no options");
        assert_eq!(panels.open(), None);
        assert!(panels.handle(Intent::Options, GAME));
        assert_eq!(panels.open(), options());
        assert!(
            panels.handle(Intent::Options, GAME),
            "a second press keeps them"
        );
        assert_eq!(panels.open(), options());
        panels.show(Panel::Options(OptionsLevel::Collections));
        assert!(panels.handle(Intent::Back, GAME));
        assert_eq!(panels.open(), options());
        assert!(panels.handle(Intent::Back, GAME));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn start_swaps_the_options_for_the_drawer_and_view_waits_while_it_is_open() {
        let mut panels = Panels::new();
        panels.handle(Intent::Options, GAME);
        assert!(panels.handle(Intent::Menu, GAME));
        assert_eq!(panels.open(), drawer());
        assert!(panels.handle(Intent::Options, GAME), "taken, not acted on");
        assert_eq!(panels.open(), drawer());
    }

    #[test]
    fn an_open_panel_keeps_the_page_s_buttons_and_lets_directions_and_a_through() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        for intent in [
            Intent::Search,
            Intent::PreviousTab,
            Intent::NextTab,
            Intent::PreviousSort,
            Intent::NextSort,
            Intent::BackHeld,
        ] {
            assert!(panels.handle(intent, PAGE), "{intent:?}");
        }
        for intent in [Intent::Activate, Intent::Navigate(Direction::Down)] {
            assert!(!panels.handle(intent, PAGE), "{intent:?}");
        }
        assert_eq!(panels.open(), drawer());
        let mut closed = Panels::new();
        assert!(!closed.handle(Intent::Search, PAGE));
        assert!(!closed.handle(Intent::NextTab, PAGE));
    }

    #[test]
    fn a_dialog_takes_the_buttons_before_the_panels() {
        let mut panels = Panels::new();
        assert!(!panels.handle(Intent::Menu, DIALOG));
        assert!(!panels.handle(Intent::Options, DIALOG));
        assert_eq!(panels.open(), None);
        // A dialog opened from a panel ("New Collection…") closes on B first.
        panels.handle(Intent::Options, GAME);
        panels.show(Panel::Options(OptionsLevel::Collections));
        assert!(!panels.handle(Intent::Back, DIALOG));
        assert_eq!(
            panels.open(),
            Some(Panel::Options(OptionsLevel::Collections))
        );
    }

    #[test]
    fn b_leaves_the_controls_level_for_the_drawer_s_entries_first_and_start_closes_it() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        panels.show(Panel::Drawer(DrawerLevel::Controls));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), drawer());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);

        panels.show(Panel::Drawer(DrawerLevel::Controls));
        assert!(panels.handle(Intent::Search, PAGE), "taken, not acted on");
        assert!(!panels.handle(Intent::Activate, PAGE), "A chooses a row");
        assert!(panels.handle(Intent::Menu, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn closing_forgets_the_level() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Controls));
        panels.close();
        assert_eq!(panels.open(), None);
        panels.handle(Intent::QuickAccess, PAGE);
        assert_eq!(panels.open(), quick_access());
    }

    #[test]
    fn b_leaves_the_power_level_for_the_drawer_s_entries_first() {
        let mut panels = Panels::new();
        panels.handle(Intent::Menu, PAGE);
        panels.show(Panel::Drawer(DrawerLevel::Power));
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), drawer());
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn the_drawer_lists_playing_now_first_and_collections_when_there_are_any() {
        use DrawerEntry::*;
        assert_eq!(
            drawer_entries(true, true, false),
            [PlayingNow, Home, Library, Collections, ExitFullscreen]
        );
        assert_eq!(
            drawer_entries(false, false, false),
            [Home, Library, ExitFullscreen],
            "settings live in Quick Access"
        );
    }

    #[test]
    fn the_drawer_lists_power_last_where_its_level_lists_anything_and_quit_lives_there() {
        use DrawerEntry::*;
        assert_eq!(
            drawer_entries(false, false, true),
            [Home, Library, ExitFullscreen, Power]
        );
        assert_eq!(
            drawer_entries(true, false, true),
            [PlayingNow, Home, Library, ExitFullscreen, Power]
        );
    }

    #[test]
    fn a_collection_moves_only_where_it_can_and_is_always_renamed_and_deleted() {
        use CollectionAction::*;
        assert_eq!(collection_actions(1, 3), [Rename, MoveUp, MoveDown, Delete]);
        assert_eq!(
            collection_actions(0, 3),
            [Rename, MoveDown, Delete],
            "first"
        );
        assert_eq!(collection_actions(2, 3), [Rename, MoveUp, Delete], "last");
        assert_eq!(collection_actions(0, 1), [Rename, Delete], "the only one");
    }

    #[test]
    fn quick_access_offers_filters_on_the_library_page_only() {
        let library = Under {
            library: true,
            ..PAGE
        };
        let library_game = Under {
            library: true,
            ..GAME
        };

        assert_eq!(page_entries(library), [PageEntry::Filters]);
        assert_eq!(page_entries(library_game), [PageEntry::Filters]);
        assert!(page_entries(PAGE).is_empty());
        assert!(page_entries(GAME).is_empty(), "a game page has no filters");
    }

    #[test]
    fn quick_access_offers_performance_for_a_focused_game_we_run_only() {
        let ours = Under {
            own_game: true,
            ..GAME
        };
        let ours_in_the_library = Under {
            library: true,
            ..ours
        };
        let store_game_in_the_library = Under {
            library: true,
            ..GAME
        };

        assert_eq!(page_entries(ours), [PageEntry::Performance]);
        assert_eq!(
            page_entries(ours_in_the_library),
            [PageEntry::Filters, PageEntry::Performance]
        );
        assert_eq!(
            page_entries(store_game_in_the_library),
            [PageEntry::Filters]
        );
        assert!(page_entries(GAME).is_empty(), "a store game has none");
        assert!(page_entries(PAGE).is_empty());
    }

    fn game(source: Source, kind: LaunchKind) -> Game {
        Game::new(GameId::new(source, "1"), "Celeste", kind, 0)
    }

    #[test]
    fn a_steam_game_s_options_end_with_opening_its_launcher_and_web_pages() {
        use GameOption::*;
        let steam = game(Source::Steam, LaunchKind::StoreUri);
        assert_eq!(
            game_options(&steam, Playing::No),
            [
                Play,
                AddToFavourites,
                Collections,
                ChangeArtwork,
                Hide,
                OpenInLauncher,
                ProtonDb,
                PcGamingWiki
            ]
        );
    }

    #[test]
    fn options_follow_the_game_s_favourite_hidden_and_playing_state() {
        use GameOption::*;
        let mut manual = game(Source::Manual, LaunchKind::HostCommand);
        manual.favourite = true;
        manual.hidden = true;
        assert_eq!(
            game_options(&manual, Playing::Started),
            [
                Stop,
                RemoveFromFavourites,
                Collections,
                ChangeArtwork,
                Unhide
            ]
        );
    }

    #[test]
    fn a_game_its_store_launcher_runs_offers_return_and_stop_instead_of_play() {
        use GameOption::*;
        let steam = game(Source::Steam, LaunchKind::StoreUri);
        assert_eq!(
            game_options(&steam, Playing::InStore),
            [
                Return,
                Stop,
                AddToFavourites,
                Collections,
                ChangeArtwork,
                Hide,
                OpenInLauncher,
                ProtonDb,
                PcGamingWiki
            ]
        );
    }

    #[test]
    fn every_game_offers_change_artwork_after_its_collections() {
        let options = game_options(&game(Source::Lutris, LaunchKind::HostCommand), Playing::No);
        let collections = options.iter().position(|o| *o == GameOption::Collections);

        assert_eq!(
            options.iter().position(|o| *o == GameOption::ChangeArtwork),
            collections.map(|index| index + 1)
        );
    }

    #[test]
    fn only_a_steam_game_s_options_end_with_its_protondb_and_pcgamingwiki_pages() {
        let steam = game_options(&game(Source::Steam, LaunchKind::StoreUri), Playing::No);
        assert_eq!(
            steam[steam.len() - 2..],
            [GameOption::ProtonDb, GameOption::PcGamingWiki]
        );

        let heroic = game_options(&game(Source::HeroicGog, LaunchKind::StoreUri), Playing::No);
        assert!(
            !heroic
                .iter()
                .any(|o| matches!(o, GameOption::ProtonDb | GameOption::PcGamingWiki))
        );
    }

    #[test]
    fn a_game_we_run_offers_a_safe_start_after_play_while_it_does_not_run() {
        use GameOption::*;
        let lutris = game(Source::Lutris, LaunchKind::HostCommand);
        let steam = game(Source::Steam, LaunchKind::StoreUri);

        assert_eq!(game_options(&lutris, Playing::No)[..2], [Play, StartSafely]);
        assert!(!game_options(&lutris, Playing::Started).contains(&StartSafely));
        assert!(!game_options(&steam, Playing::No).contains(&StartSafely));
    }

    #[test]
    fn a_windows_game_we_run_offers_its_save_snapshots_after_change_artwork() {
        use GameOption::*;
        let windows = game(Source::Manual, LaunchKind::Proton);
        let options = game_options(&windows, Playing::No);
        let artwork = options.iter().position(|o| *o == ChangeArtwork).unwrap();
        assert_eq!(options[artwork + 1], SaveSnapshots);
        assert!(game_options(&windows, Playing::Started).contains(&SaveSnapshots));
        for (source, kind) in [
            (Source::Lutris, LaunchKind::HostCommand),
            (Source::Steam, LaunchKind::StoreUri),
            (Source::Manual, LaunchKind::RuntimeNative),
        ] {
            assert!(!game_options(&game(source, kind), Playing::No).contains(&SaveSnapshots));
        }
    }

    #[test]
    fn any_game_with_save_folders_offers_its_save_snapshots_where_a_windows_game_does() {
        use GameOption::*;
        let windows = sheet_options(&game(Source::Manual, LaunchKind::Proton), Playing::No, 0);
        assert_eq!(with_save_snapshots(windows.clone()), windows);
        let before_hide = |options: &[GameOption]| {
            let hide = options.iter().position(|o| *o == Hide).unwrap();
            options[hide - 1]
        };
        assert_eq!(before_hide(&windows), SaveSnapshots);
        let steam = sheet_options(&game(Source::Steam, LaunchKind::StoreUri), Playing::No, 2);
        let offered = with_save_snapshots(steam.clone());
        assert_eq!(offered.len(), steam.len() + 1);
        assert_eq!(before_hide(&offered), SaveSnapshots);
        assert_eq!(with_save_snapshots(offered.clone()), offered);
    }

    #[test]
    fn every_game_offers_merging_after_its_artwork_and_a_group_its_copies_and_a_split() {
        use GameOption::*;
        let steam = game(Source::Steam, LaunchKind::StoreUri);

        assert_eq!(
            sheet_options(&steam, Playing::No, 0),
            [
                Play,
                AddToFavourites,
                Collections,
                ChangeArtwork,
                MergeWith,
                Hide,
                OpenInLauncher,
                ProtonDb,
                PcGamingWiki
            ]
        );
        assert_eq!(
            sheet_options(&steam, Playing::No, 2)[3..7],
            [ChangeArtwork, MergeWith, PreferredCopy, Split]
        );
    }

    fn theme(id: &str, name: &str, contract: u64) -> Theme {
        Theme {
            id: id.into(),
            name: name.into(),
            description: String::new(),
            dir: format!("/themes/{id}").into(),
            contract,
            layout: Default::default(),
            variants: Vec::new(),
            preview: None,
        }
    }

    fn shown(choices: &[DesignChoice]) -> Vec<(&str, bool, bool)> {
        choices
            .iter()
            .map(|choice| (choice.id.as_str(), choice.active, choice.available))
            .collect()
    }

    #[test]
    fn the_designs_list_default_first_and_check_the_active_one() {
        let themes = [theme("mango", "Mango", 1), theme("future", "Future", 99)];
        let choices = design_choices(&themes, "mango");
        assert_eq!(
            shown(&choices),
            [
                ("", false, true),
                ("mango", true, true),
                ("future", false, false)
            ]
        );
        assert_eq!(choices[0].name, None, "the shell names Adwaita");
        assert_eq!(choices[1].name.as_deref(), Some("Mango"));
        for active in ["", "gone", "future"] {
            let choices = design_choices(&themes, active);
            assert!(choices[0].active, "{active:?} leaves Adwaita in force");
            assert_eq!(
                choices.iter().filter(|choice| choice.active).count(),
                1,
                "{active:?}"
            );
        }
    }

    #[test]
    fn each_design_choice_carries_its_designs_preview() {
        let mut pictured = theme("mango", "Mango", 1);
        pictured.preview = Some("/themes/mango/preview.svg".into());
        let themes = [pictured, theme("plain", "Plain", 1)];
        let previews: Vec<_> = design_choices(&themes, "")
            .into_iter()
            .map(|choice| choice.preview)
            .collect();
        assert_eq!(
            previews,
            [None, Some("/themes/mango/preview.svg".into()), None],
            "Adwaita's built-in preview is the shell's"
        );
    }

    fn with_variants(mut theme: Theme, ids: &[&str]) -> Theme {
        theme.variants = ids
            .iter()
            .map(|id| Variant {
                id: (*id).to_owned(),
                name: id.to_uppercase(),
                stylesheet: None,
            })
            .collect();
        theme
    }

    #[test]
    fn the_variants_of_the_design_in_force_list_with_the_active_one_checked() {
        let themes = [with_variants(theme("duo", "Duo", 1), &["classic", "mono"])];
        let shown = |variant: &str| -> Vec<(String, String, bool)> {
            variant_choices(&themes, "duo", variant)
                .into_iter()
                .map(|choice| (choice.id, choice.name, choice.active))
                .collect()
        };
        assert_eq!(
            shown("mono"),
            [
                ("classic".into(), "CLASSIC".into(), false),
                ("mono".into(), "MONO".into(), true)
            ]
        );
        for fallback in ["", "gone"] {
            assert_eq!(
                shown(fallback)
                    .iter()
                    .map(|(id, _, active)| (id.as_str(), *active))
                    .collect::<Vec<_>>(),
                [("classic", true), ("mono", false)],
                "{fallback:?} falls back to the first variant"
            );
        }
    }

    #[test]
    fn there_are_no_variants_without_a_design_that_offers_them() {
        let themes = [
            theme("plain", "Plain", 1),
            with_variants(theme("future", "Future", 99), &["a", "b"]),
            with_variants(theme("duo", "Duo", 1), &["a", "b"]),
        ];
        for design in ["plain", "future", "", "gone"] {
            assert!(
                variant_choices(&themes, design, "a").is_empty(),
                "{design:?}"
            );
        }
    }
}
