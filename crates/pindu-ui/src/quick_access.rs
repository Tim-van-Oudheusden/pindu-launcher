//! Quick Access, fullscreen mode's panel on the right (Guide + A, R3 or the Steam Deck's "…"
//! button), as a widget a window hosts: [`PinduQuickAccess`], with its own Blueprint
//! (`data/ui/quick_access.blp`). It holds the status header (`fullscreen_status`), the
//! level stack, the six sections under their strip and every level, and takes the
//! services it shows from the application ([`PinduQuickAccess::setup`]), never from its
//! host.
//!
//! The main level is a strip of six toggles (`.panel-sections`, tabs to a screen reader)
//! over the shown section's page: Quick, Notifications, Design, Controller, System and
//! Manage. What each lists, and in which order, is `pindu_core::panels::section_entries`
//! for the facts the panel follows ([`PinduQuickAccess::shown`]). Quick holds the Playing
//! Now cards, a page card per entry the page under the panel offers, a grid of tiles and
//! the activity strip, and moves focus through them itself (`quick_section`); Sound
//! Output, Bluetooth, Network, Refresh Library and Add a Game have a tile in Quick and a
//! row where they belong, with one action and one second line. The pointer and touch
//! press the strip's toggles (`win.quick-access-section`); the controller's focus never
//! lands on them, LB and RB switch section. A section remembers the item it had focused
//! while the panel stays open.
//!
//! Which level and section show is the host's (`pindu_core::panels::Panels`, which also
//! holds the host's other panels): the host shows them with
//! [`PinduQuickAccess::show_level`], focuses them once they show with
//! [`PinduQuickAccess::focus_level`], and says what lies under the panel with
//! [`PinduQuickAccess::set_page`]. What the panel cannot do itself it asks of the host
//! through its signals: close (`close-requested`), lead to a level (`level-requested`),
//! show a section (`section-requested`), show a game (`game-requested`) and apply
//! fullscreen mode's changed library filters (`filters-changed`). Over a running game
//! (`game_overlay`) it leaves out the entries that need the main window
//! ([`PinduQuickAccess::set_over_game`]). Network and Bluetooth ask their questions on the
//! host the player sees (`asks_prompts`).
//!
//! The sections and levels are filled by: Notifications `fullscreen_notifications`, its
//! Running tasks and Refresh Library `fullscreen_activity`, Surprise Me
//! `fullscreen_surprise`, Controller `fullscreen_controllers`, Controls
//! `fullscreen_controls`, Filters (offered on the library page) `fullscreen_library`,
//! Performance (offered for a game we run ourselves) `fullscreen_performance`, Network
//! `fullscreen_network`, Bluetooth `fullscreen_bluetooth`, Sound Output
//! `fullscreen_sound_output`, System Update `fullscreen_system_update`, Library Health
//! `fullscreen_health`, Diagnostics Report `fullscreen_report`, Add a Store `add_store`,
//! Proton `fullscreen_proton_downloads`, Sources `fullscreen_sources` and Duplicates
//! `fullscreen_duplicates`; Add a Game opens the window's `fullscreen_add_game`. Design
//! and Settings are this module's: the design, its variant, Home's background blur and dim
//! and the user's own stylesheet, then cover size, navigation sounds and their volume,
//! exiting after a launch, Reduce Motion and Larger Text, the SteamGridDB key (typed
//! hidden with the on-screen keyboard of `secret_dialog` and kept in the keyring) and
//! Prefer SteamGridDB Art, the same settings Preferences changes.

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::LazyLock;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib::subclass::Signal;
use gtk::{gio, glib};
use pindu_core::accessibility::{Accessibility, TEXT_SIZE_KEY, TextSize};
use pindu_core::gamepad::Intent;
use pindu_core::gamescope::prompts::{PromptHost, prompt_host};
use pindu_core::hold::HOLD_TO_CONFIRM_KEY;
use pindu_core::home::{BACKGROUND_KEY, BLUR_KEY, DIM_KEY};
use pindu_core::keys;
use pindu_core::model::GameId;
use pindu_core::osk::Keyboard;
use pindu_core::panels::{
    PageEntry, QuickAccessEntry, QuickAccessLevel, QuickAccessSection, QuickFocus, Shown,
    design_choices, quick_first_focus, quick_tiles, section_entries, sections, variant_choices,
};
use pindu_core::paths;
use pindu_core::tasks::Task;

use crate::add_store::PinduAddStoreLevel;
use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::fullscreen::bind_volume_slider;
use crate::fullscreen_bluetooth::PinduBluetoothLevel;
use crate::fullscreen_clock::Clock;
use crate::fullscreen_health::PinduHealthLevel;
use crate::fullscreen_network::PinduNetworkLevel;
use crate::fullscreen_panels::step_out_of_list;
use crate::fullscreen_performance::{GameProton, PerformanceSwitches};
use crate::fullscreen_playing::{PlayingCards, card_return};
use crate::fullscreen_proton_downloads::PinduProtonLevel;
use crate::fullscreen_report::PinduReportLevel;
use crate::fullscreen_sound_output::PinduSoundOutputLevel;
use crate::fullscreen_system_update::PinduSystemUpdateLevel;
use crate::models::GameObject;
use crate::preferences::TILE_SIZES;
use crate::quick_section::{Parts, PinduQuickSection};
use crate::secret_dialog::ask_secret;
use crate::steamgriddb_service;
use crate::wikidata_service;
use crate::window::REDUCE_MOTION_KEY;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/quick_access.ui")]
    pub struct PinduQuickAccess {
        #[template_child]
        pub clock: TemplateChild<gtk::Label>,
        #[template_child]
        pub offline: TemplateChild<gtk::Box>,
        #[template_child]
        pub battery: TemplateChild<gtk::Box>,
        #[template_child]
        pub battery_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub battery_level: TemplateChild<gtk::Label>,
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub quick_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub notifications_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub design_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub controller_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub system_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub manage_tab: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub sections: TemplateChild<gtk::Stack>,
        #[template_child]
        pub quick_section: TemplateChild<PinduQuickSection>,
        #[template_child]
        pub playing_cards: TemplateChild<gtk::Box>,
        #[template_child]
        pub filters_card: TemplateChild<gtk::Button>,
        #[template_child]
        pub filters_card_state: TemplateChild<gtk::Label>,
        #[template_child]
        pub performance_card: TemplateChild<gtk::Button>,
        #[template_child]
        pub quick_tiles: TemplateChild<gtk::Grid>,
        #[template_child]
        pub quick_sound_output_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub quick_bluetooth_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub quick_network_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub surprise_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub quick_refresh_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub quick_add_game_tile: TemplateChild<gtk::Button>,
        #[template_child]
        pub activity_strip: TemplateChild<gtk::Button>,
        #[template_child]
        pub activity_strip_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub activity_strip_percent: TemplateChild<gtk::Label>,
        #[template_child]
        pub activity_strip_bar: TemplateChild<gtk::ProgressBar>,
        #[template_child]
        pub unread: TemplateChild<gtk::Label>,
        #[template_child]
        pub running_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub notifications: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub refresh_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub refresh_metadata_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub activity: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub designs: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub variant_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub variants: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub background_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub backgrounds: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub blur: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub dim: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub custom_style: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub custom_style_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub settings_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub sources_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub sources: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub source_order: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub duplicates_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub duplicates_empty: TemplateChild<gtk::Label>,
        #[template_child]
        pub duplicates: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub controllers: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub prompts: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub swap: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub repeat_delay: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub repeat_rate: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub dead_zone: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub controller_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub controls_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub controls: TemplateChild<gtk::Box>,
        #[template_child]
        pub system_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub manage_list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub tile_small: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub tile_medium: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub tile_large: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub sizes: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub switches: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub sounds_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub volume_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub volume: TemplateChild<gtk::Scale>,
        #[template_child]
        pub exit_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub accessibility: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub reduce_motion_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub text_sizes: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub hold_to_confirm_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub artwork: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub steamgriddb_key_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub prefer_steamgriddb_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub metadata: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub wikidata_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub filters: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub installed_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub not_installed_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub windows_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub native_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub never_played_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub hidden_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub genres: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub clear_filters: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub save_filters: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub performance_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub performance: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub overlay_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub gamemode_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub performance_note: TemplateChild<gtk::Label>,
        #[template_child]
        pub proton_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub proton_builds: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub sound_output_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub sound_output_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub sound_output_level: TemplateChild<PinduSoundOutputLevel>,
        #[template_child]
        pub system_update_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub system_update_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub system_update_level: TemplateChild<PinduSystemUpdateLevel>,
        #[template_child]
        pub health_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub health_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub health_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub health_level: TemplateChild<PinduHealthLevel>,
        #[template_child]
        pub add_store_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub add_store_level: TemplateChild<PinduAddStoreLevel>,
        #[template_child]
        pub report_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub report_level: TemplateChild<PinduReportLevel>,
        #[template_child]
        pub report_logs: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub report_logs_empty: TemplateChild<gtk::Label>,
        #[template_child]
        pub proton_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub proton_level: TemplateChild<PinduProtonLevel>,
        #[template_child]
        pub add_game_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub network_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub network_icon: TemplateChild<gtk::Image>,
        #[template_child]
        pub network_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub network_level: TemplateChild<PinduNetworkLevel>,
        #[template_child]
        pub bluetooth_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub bluetooth_subtitle: TemplateChild<gtk::Label>,
        #[template_child]
        pub bluetooth_level: TemplateChild<PinduBluetoothLevel>,
        /// The application whose services the panel shows, once set up.
        pub application: glib::WeakRef<PinduApplication>,
        pub settings: OnceCell<gio::Settings>,
        /// The level the host shows, while the panel is open.
        pub level: Cell<Option<QuickAccessLevel>>,
        /// The section the main level shows, or the one B leads back to from a level.
        pub section: Cell<QuickAccessSection>,
        /// What decides which entries the sections list (`pindu_core::panels::Shown`).
        pub shown: Cell<Shown>,
        /// The item each section had focused when it was left, while the panel stays
        /// open; a section without one shows for the first time since the panel opened.
        pub remembered: RefCell<Vec<(QuickAccessSection, glib::WeakRef<gtk::Widget>)>>,
        /// `win.quick-access-section`, whose state is the section shown, which checks its
        /// toggle in the strip.
        pub section_action: OnceCell<gio::SimpleAction>,
        /// The header's clock, which ticks while the panel is open.
        pub(crate) time: OnceCell<Clock>,
        /// The Playing Now cards at the top of the Quick section.
        pub(crate) playing: OnceCell<PlayingCards>,
        /// The Performance level's switches, which the host may show elsewhere too.
        pub performance_switches: OnceCell<PerformanceSwitches>,
        /// The Performance level's Proton build, for a game we run through Proton.
        pub game_proton: OnceCell<GameProton>,
        /// `win.design`, `win.design-variant` and `win.sound-output`, whose states are
        /// what their levels check.
        pub design_action: OnceCell<gio::SimpleAction>,
        pub variant_action: OnceCell<gio::SimpleAction>,
        pub sound_output_action: OnceCell<gio::SimpleAction>,
        /// `win.fullscreen-genre`, whose state is the genre Filters chose, empty for any.
        pub genre_action: OnceCell<gio::SimpleAction>,
        /// The intents Controls lists, in its rows' order across its groups.
        pub shown_controls: RefCell<Vec<Intent>>,
        /// The tasks Activity shows, each with its progress bar, and the next pulse of the
        /// bars of those with no telling how far they are.
        pub activity_shown: RefCell<Vec<(Task, gtk::ProgressBar)>>,
        pub activity_pulse: RefCell<Option<glib::SourceId>>,
        /// Whether the activity strip's task has no telling how far it is, so its bar
        /// moves.
        pub activity_strip_pulses: Cell<bool>,
        /// The game Surprise Me picked last, which it does not pick again next time.
        pub surprise_previous: RefCell<Option<GameId>>,
        /// Whether the panel is hosted over a running game.
        pub over_game: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduQuickAccess {
        const NAME: &'static str = "PinduQuickAccess";
        type Type = super::PinduQuickAccess;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            PinduNetworkLevel::ensure_type();
            PinduBluetoothLevel::ensure_type();
            PinduSystemUpdateLevel::ensure_type();
            PinduHealthLevel::ensure_type();
            PinduAddStoreLevel::ensure_type();
            PinduReportLevel::ensure_type();
            PinduProtonLevel::ensure_type();
            PinduSoundOutputLevel::ensure_type();
            PinduQuickSection::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduQuickAccess {
        fn constructed(&self) {
            self.parent_constructed();
            let obj = self.obj();
            let _ = self.time.set(Clock::new(&self.clock));
            let _ = self.playing.set(PlayingCards::new(&self.playing_cards));
            obj.setup_quick_section();
            obj.setup_settings();
            obj.setup_navigation();
        }

        fn signals() -> &'static [Signal] {
            static SIGNALS: LazyLock<Vec<Signal>> = LazyLock::new(|| {
                vec![
                    Signal::builder(CLOSE_REQUESTED).build(),
                    Signal::builder(LEVEL_REQUESTED)
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder(SECTION_REQUESTED)
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder(GAME_REQUESTED)
                        .param_types([String::static_type()])
                        .build(),
                    Signal::builder(FILTERS_CHANGED).build(),
                ]
            });
            SIGNALS.as_ref()
        }
    }

    impl WidgetImpl for PinduQuickAccess {}

    impl BoxImpl for PinduQuickAccess {}
}

glib::wrapper! {
    /// Fullscreen mode's Quick Access panel: its status header, its sections and their
    /// levels. A window hosts it (see the module documentation for what the host does).
    pub struct PinduQuickAccess(ObjectSubclass<imp::PinduQuickAccess>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

/// The panel asks its host to close it.
const CLOSE_REQUESTED: &str = "close-requested";
/// The panel asks its host to show one of its levels, by name.
const LEVEL_REQUESTED: &str = "level-requested";
/// The panel asks its host to show one of its sections, by name.
const SECTION_REQUESTED: &str = "section-requested";
/// The panel asks its host to close it and show a game, by ID.
const GAME_REQUESTED: &str = "game-requested";
/// Fullscreen mode's library filters changed.
const FILTERS_CHANGED: &str = "filters-changed";

/// The setting My Stylesheet turns on: `fullscreen.css` over the design.
const CUSTOM_STYLE_KEY: &str = "fullscreen-custom-style";

impl PinduQuickAccess {
    /// Takes the services the panel shows from `application`, follows them, and adds the
    /// `win.*` actions its rows activate to `actions`, its host window's. The host keeps
    /// `win.playing-return`, `win.playing-stop`, `win.preview-navigation-sound` and the
    /// Add a Store flow's `win.add-store-*` itself, as it has them for its other panels.
    pub fn setup(&self, application: &PinduApplication, actions: &impl IsA<gio::ActionMap>) {
        let imp = self.imp();
        imp.application.set(Some(application));
        let _ = imp.performance_switches.set(PerformanceSwitches::new(
            application.library_service(),
            application.gamescope_service(),
            &imp.performance_note,
        ));
        self.performance_switches()
            .attach(&imp.overlay_row, &imp.gamemode_row);
        let _ = imp.game_proton.set(GameProton::new(
            application.library_service(),
            &imp.proton_title,
            &imp.proton_builds,
        ));
        self.add_level_actions(actions);
        actions.add_action(&self.settings().create_action(TEXT_SIZE_KEY));
        self.setup_designs(application, actions);
        self.setup_status(application);
        self.setup_playing_cards(application);
        self.setup_notifications(application, actions);
        self.setup_activity(application, actions);
        self.setup_controllers(application, actions);
        self.setup_controls(application);
        self.setup_filters(actions);
        self.setup_surprise_me(application, actions);
        self.setup_network(application, actions);
        self.setup_bluetooth(application, actions);
        self.setup_sound_output(application, actions);
        self.setup_system_update(application, actions);
        self.setup_health(application, actions);
        self.setup_add_store(application);
        self.setup_diagnostics_report(actions);
        self.setup_proton(actions);
        self.setup_sources(application);
        self.setup_duplicates(application);
    }

    /// Hosts the panel over a running game, before [`Self::setup`]: it leaves out the
    /// entries that need the main window, Surprise Me (it leads to a library page), Add a
    /// Store and Add a Game (their flows are the main window's) and Diagnostics Report (it
    /// needs the main window's room, and its page turns are the main window's).
    pub fn set_over_game(&self) {
        self.imp().over_game.set(true);
        self.update_shown(|shown| shown.over_game = true);
    }

    /// Shows what Quick Access adds for the page under it, as it opens: `entries` from
    /// `pindu_core::panels::page_entries`, each a page card in Quick, and `game`, the
    /// focused game or the one in front, whose switches Performance shows and whose title
    /// names it.
    pub fn set_page(&self, entries: &[PageEntry], game: Option<&GameObject>) {
        let imp = self.imp();
        let performance = entries.contains(&PageEntry::Performance);
        self.update_shown(|shown| {
            shown.filters = entries.contains(&PageEntry::Filters);
            shown.performance = performance;
        });
        self.show_filters_card();
        if performance && let Some(game) = game {
            self.performance_switches().show_for(game);
            if let Some(proton) = imp.game_proton.get() {
                proton.show_for(game);
            }
            imp.performance_title.set_label(
                &gettext("Performance for {game}").replace("{game}", &game.game().title),
            );
        }
    }

    /// Shows `level`, filled as things now are, or nothing while the host's panel is
    /// closed; at the main level, `section` (`Panels::section`). The panel reads its
    /// services again as it opens, and its clock and Playing Now times run while it is
    /// open. Each section remembers the item it had focused as it is left, until the
    /// panel closes.
    pub fn show_level(&self, level: Option<QuickAccessLevel>, section: QuickAccessSection) {
        let imp = self.imp();
        let before = imp.level.replace(level);
        let before_section = imp.section.replace(section);
        let main = Some(QuickAccessLevel::Main);
        if before == level && (level != main || before_section == section) {
            return;
        }
        if before.is_none() {
            imp.remembered.borrow_mut().clear();
        } else if before == main {
            self.remember_focus(before_section);
        }
        if before == Some(QuickAccessLevel::Bluetooth) {
            self.leave_bluetooth();
        }
        match level {
            Some(QuickAccessLevel::Main) => self.fill_section(section, before == main),
            Some(QuickAccessLevel::Controls) => self.fill_controls(),
            Some(QuickAccessLevel::Filters) => {
                self.fill_genres();
                self.show_filters();
            }
            Some(QuickAccessLevel::Bluetooth) => self.show_bluetooth(),
            Some(QuickAccessLevel::SoundOutput) => self.show_sound_output(),
            Some(QuickAccessLevel::SystemUpdate) => self.show_system_update(),
            Some(QuickAccessLevel::LibraryHealth) => self.show_health(),
            Some(QuickAccessLevel::AddStore) => self.show_add_store(),
            // Back from the games with a log, the report stays as it was.
            Some(QuickAccessLevel::DiagnosticsReport)
                if before != Some(QuickAccessLevel::ReportLog) =>
            {
                self.show_diagnostics_report(None);
            }
            Some(QuickAccessLevel::ReportLog) => self.show_report_logs(),
            Some(QuickAccessLevel::Proton) => imp.proton_level.show(),
            Some(QuickAccessLevel::Network) => self.show_network(),
            Some(QuickAccessLevel::Sources) => self.show_sources(),
            Some(QuickAccessLevel::Duplicates) => self.show_duplicates(),
            Some(QuickAccessLevel::Settings) => self.show_steamgriddb_key(),
            _ => {}
        }
        if let Some(level) = level {
            imp.stack.set_visible_child_name(level.name());
        }
        if before.is_none() != level.is_none() {
            self.opened(level.is_some());
        }
    }

    /// The level shown, while the panel is open.
    pub fn level(&self) -> Option<QuickAccessLevel> {
        self.imp().level.get()
    }

    /// What decides which entries the sections list, as the panel last read it: for the
    /// host's `Under::shown`, which LB and RB step through the sections with.
    pub fn shown(&self) -> Shown {
        self.imp().shown.get()
    }

    /// Focuses the level shown once it shows. On the main level that is, after `left`, the
    /// level just left, the entry that leads to it in the section shown; after another
    /// section (`left` the main level), the item the section had focused when it was last
    /// left while the panel stayed open; and otherwise the section's first item. A level
    /// focuses its first item (a list box would go back to the row it last had focus on).
    pub fn focus_level(&self, left: Option<QuickAccessLevel>) {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        let section = imp.section.get();
        let target: Option<gtk::Widget> = match (self.level(), left) {
            (Some(QuickAccessLevel::Main), Some(QuickAccessLevel::Main)) => {
                if let Some(remembered) = self.remembered(section)
                    && remembered.grab_focus()
                {
                    return;
                }
                self.first_item(section)
            }
            (Some(QuickAccessLevel::Main), Some(left)) => self
                .entry(left, section)
                .or_else(|| self.first_item(section)),
            (Some(QuickAccessLevel::Main), None) => self.first_item(section),
            (Some(QuickAccessLevel::Settings), _) => {
                imp.tile_small.ancestor(gtk::ListBoxRow::static_type())
            }
            (Some(QuickAccessLevel::Filters), _) => first_row(&imp.filters),
            // The first switch that works: MangoHud's cannot when it is not installed.
            (Some(QuickAccessLevel::Performance), _) => {
                std::iter::successors(imp.performance.first_child(), |row| row.next_sibling())
                    .find(|row| row.is::<gtk::ListBoxRow>() && row.is_sensitive())
            }
            (Some(QuickAccessLevel::Bluetooth), _) => imp.bluetooth_level.focus_target(),
            // The output in use.
            (Some(QuickAccessLevel::SoundOutput), _) => self.sound_output_focus(),
            (Some(QuickAccessLevel::SystemUpdate), _) => imp.system_update_level.focus_target(),
            (Some(QuickAccessLevel::LibraryHealth), _) => imp.health_level.focus_target(),
            (Some(QuickAccessLevel::AddStore), _) => imp.add_store_level.focus_target(),
            (Some(QuickAccessLevel::DiagnosticsReport), Some(QuickAccessLevel::ReportLog)) => {
                Some(imp.report_level.add_log_row())
            }
            (Some(QuickAccessLevel::DiagnosticsReport), _) => imp.report_level.focus_target(),
            (Some(QuickAccessLevel::ReportLog), _) => first_row(&imp.report_logs),
            (Some(QuickAccessLevel::Proton), _) => imp.proton_level.focus_target(),
            (Some(QuickAccessLevel::Network), _) => imp.network_level.focus_target(),
            // Filled once the sources are read, which then focuses the first.
            (Some(QuickAccessLevel::Sources), _) => first_row(&imp.sources),
            // Filled once the review is read, which then focuses the first.
            (Some(QuickAccessLevel::Duplicates), _) => first_row(&imp.duplicates),
            (Some(QuickAccessLevel::Controls), _) => {
                std::iter::successors(imp.controls.first_child(), |child| child.next_sibling())
                    .find_map(|child| child.downcast::<gtk::ListBox>().ok())
                    .and_then(|list| first_row(&list))
            }
            (None, _) => None,
        };
        if let Some(target) = target {
            target.grab_focus();
        }
    }

    /// Whether a notification is unread, which has Quick Access open at its notifications.
    pub fn has_unread_notifications(&self) -> bool {
        self.application()
            .is_some_and(|application| application.notification_service().unread() > 0)
    }

    /// The Performance level's switches, which the host can attach its own switches to so
    /// that both show one state (see [`PerformanceSwitches::attach`]); `None` until
    /// [`Self::setup`].
    pub fn performance(&self) -> Option<PerformanceSwitches> {
        self.imp().performance_switches.get().cloned()
    }

    /// Calls `callback` when the panel asks to be closed: Escape, or an entry that did its
    /// work.
    pub fn connect_close_requested<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(CLOSE_REQUESTED, false, move |values| {
            callback(&quick_access(values));
            None
        })
    }

    /// Calls `callback` with the level an entry leads to, or with the main level to go back
    /// to the section when what the level shows is gone.
    pub fn connect_level_requested<F: Fn(&Self, QuickAccessLevel) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(LEVEL_REQUESTED, false, move |values| {
            let level = values[1]
                .get::<String>()
                .ok()
                .and_then(|name| QuickAccessLevel::from_name(&name));
            if let Some(level) = level {
                callback(&quick_access(values), level);
            }
            None
        })
    }

    /// Calls `callback` with the section the strip's toggle that was pressed shows
    /// (`win.quick-access-section`).
    pub fn connect_section_requested<F: Fn(&Self, QuickAccessSection) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(SECTION_REQUESTED, false, move |values| {
            let section = values[1]
                .get::<String>()
                .ok()
                .and_then(|name| QuickAccessSection::from_name(&name));
            if let Some(section) = section {
                callback(&quick_access(values), section);
            }
            None
        })
    }

    /// Calls `callback` with a game to close the panel for and show: a notification's, or
    /// Surprise Me's pick.
    pub fn connect_game_requested<F: Fn(&Self, &GameId) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(GAME_REQUESTED, false, move |values| {
            let id = values[1]
                .get::<String>()
                .ok()
                .and_then(|id| id.parse::<GameId>().ok());
            if let Some(id) = id {
                callback(&quick_access(values), &id);
            }
            None
        })
    }

    /// Calls `callback` when Filters changed fullscreen mode's library filters
    /// (`PinduApplication::fullscreen_filters`), for the host to apply them.
    pub fn connect_filters_changed<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(FILTERS_CHANGED, false, move |values| {
            callback(&quick_access(values));
            None
        })
    }

    pub(crate) fn application(&self) -> Option<PinduApplication> {
        self.imp().application.upgrade()
    }

    /// Whether the panel is hosted over a running game (see [`Self::set_over_game`]).
    pub(crate) fn over_game(&self) -> bool {
        self.imp().over_game.get()
    }

    /// Whether this panel's host asks what Network and Bluetooth want now: the overlay
    /// while Quick Access is open over the game, and the main window otherwise
    /// (`pindu_core::gamescope::prompts`).
    pub(crate) fn asks_prompts(&self) -> bool {
        let overlay_open = self
            .application()
            .is_some_and(|application| application.gamescope_service().quick_access_open());
        let host = if self.over_game() {
            PromptHost::Overlay
        } else {
            PromptHost::MainWindow
        };
        prompt_host(overlay_open) == host
    }

    pub(crate) fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get_or_init(|| gio::Settings::new(APP_ID))
    }

    /// The widget that has focus in the panel's window.
    pub(crate) fn focus_widget(&self) -> Option<gtk::Widget> {
        self.root().and_then(|root| root.focus())
    }

    pub(crate) fn performance_switches(&self) -> &PerformanceSwitches {
        self.imp()
            .performance_switches
            .get()
            .expect("set up before the level shows")
    }

    pub(crate) fn request_close(&self) {
        self.emit_by_name::<()>(CLOSE_REQUESTED, &[]);
    }

    pub(crate) fn request_level(&self, level: QuickAccessLevel) {
        self.emit_by_name::<()>(LEVEL_REQUESTED, &[&level.name()]);
    }

    pub(crate) fn request_game(&self, id: &GameId) {
        self.emit_by_name::<()>(GAME_REQUESTED, &[&id.as_str()]);
    }

    pub(crate) fn request_section(&self, section: QuickAccessSection) {
        self.emit_by_name::<()>(SECTION_REQUESTED, &[&section.name()]);
    }

    pub(crate) fn filters_changed(&self) {
        self.emit_by_name::<()>(FILTERS_CHANGED, &[]);
    }

    /// Adds `win.<name>` to `actions`, activating `activate` on the panel while it lives.
    pub(crate) fn add_host_action(
        &self,
        actions: &impl IsA<gio::ActionMap>,
        name: &str,
        parameter: Option<&glib::VariantTy>,
        activate: impl Fn(&Self, Option<&glib::Variant>) + 'static,
    ) -> gio::SimpleAction {
        let action = gio::SimpleAction::new(name, parameter);
        let quick_access = self.downgrade();
        action.connect_activate(move |_, target| {
            if let Some(quick_access) = quick_access.upgrade() {
                activate(&quick_access, target);
            }
        });
        actions.add_action(&action);
        action
    }

    /// The panel opened or closed: it reads the batteries and the outputs again as it
    /// opens, and runs its clock and the Playing Now times while open.
    fn opened(&self, open: bool) {
        let imp = self.imp();
        if open && let Some(application) = self.application() {
            application.power_service().refresh();
            application.audio_output_service().refresh();
        }
        if let Some(time) = imp.time.get() {
            time.run(open);
        }
        if let Some(playing) = imp.playing.get() {
            playing.set_shown(open);
        }
    }

    /// The entry of `section` that leads to `level`, while it shows and can be activated.
    fn entry(&self, level: QuickAccessLevel, section: QuickAccessSection) -> Option<gtk::Widget> {
        let entry = match level {
            QuickAccessLevel::Main => return None,
            QuickAccessLevel::Filters => QuickAccessEntry::Filters,
            QuickAccessLevel::Performance => QuickAccessEntry::Performance,
            QuickAccessLevel::Controls => QuickAccessEntry::Controls,
            QuickAccessLevel::Settings => QuickAccessEntry::Settings,
            QuickAccessLevel::Sources => QuickAccessEntry::Sources,
            QuickAccessLevel::Duplicates => QuickAccessEntry::Duplicates,
            QuickAccessLevel::Bluetooth => QuickAccessEntry::Bluetooth,
            QuickAccessLevel::SoundOutput => QuickAccessEntry::SoundOutput,
            QuickAccessLevel::SystemUpdate => QuickAccessEntry::SystemUpdate,
            QuickAccessLevel::LibraryHealth => QuickAccessEntry::LibraryHealth,
            QuickAccessLevel::AddStore => QuickAccessEntry::AddStore,
            QuickAccessLevel::DiagnosticsReport | QuickAccessLevel::ReportLog => {
                QuickAccessEntry::DiagnosticsReport
            }
            QuickAccessLevel::Proton => QuickAccessEntry::Proton,
            QuickAccessLevel::Network => QuickAccessEntry::Network,
        };
        self.entry_widgets(section, entry)
            .into_iter()
            .find(|widget| widget.is_visible() && widget.is_sensitive())
    }

    /// The widgets that make `entry` in `section`, which show and hide together. Sound
    /// Output, Bluetooth, Network, Refresh Library and Add a Game have a tile in Quick and
    /// a row where they belong, each with the same action and second line. Design's and
    /// Controller's own lists always show, so they have none to show and hide.
    fn entry_widgets(
        &self,
        section: QuickAccessSection,
        entry: QuickAccessEntry,
    ) -> Vec<gtk::Widget> {
        use QuickAccessEntry as Entry;
        use QuickAccessSection as Section;
        let imp = self.imp();
        let row = |row: &gtk::ListBoxRow| vec![row.clone().upcast::<gtk::Widget>()];
        let button = |button: &gtk::Button| vec![button.clone().upcast::<gtk::Widget>()];
        match (section, entry) {
            (Section::Quick, Entry::PlayingNow) => vec![imp.playing_cards.get().upcast()],
            (Section::Quick, Entry::Filters) => button(&imp.filters_card),
            (Section::Quick, Entry::Performance) => button(&imp.performance_card),
            (Section::Quick, Entry::SoundOutput) => button(&imp.quick_sound_output_tile),
            (Section::Quick, Entry::Bluetooth) => button(&imp.quick_bluetooth_tile),
            (Section::Quick, Entry::Network) => button(&imp.quick_network_tile),
            (Section::Quick, Entry::SurpriseMe) => button(&imp.surprise_tile),
            (Section::Quick, Entry::RefreshLibrary) => button(&imp.quick_refresh_tile),
            (Section::Quick, Entry::AddGame) => button(&imp.quick_add_game_tile),
            (Section::Notifications, Entry::Running) => vec![
                imp.running_title.get().upcast(),
                imp.activity.get().upcast(),
            ],
            (Section::Notifications, Entry::Notifications) => {
                vec![imp.notifications.get().upcast()]
            }
            (Section::Controller, Entry::Controls) => row(&imp.controls_row),
            (Section::System, Entry::Network) => row(&imp.network_row),
            (Section::System, Entry::Bluetooth) => row(&imp.bluetooth_row),
            (Section::System, Entry::SoundOutput) => row(&imp.sound_output_row),
            (Section::System, Entry::SystemUpdate) => row(&imp.system_update_row),
            (Section::System, Entry::Settings) => row(&imp.settings_row),
            (Section::System, Entry::DiagnosticsReport) => row(&imp.report_row),
            (Section::Manage, Entry::Sources) => row(&imp.sources_row),
            (Section::Manage, Entry::Duplicates) => row(&imp.duplicates_row),
            (Section::Manage, Entry::LibraryHealth) => row(&imp.health_row),
            (Section::Manage, Entry::Proton) => row(&imp.proton_row),
            (Section::Manage, Entry::AddStore) => row(&imp.add_store_row),
            (Section::Manage, Entry::AddGame) => row(&imp.add_game_row),
            (Section::Manage, Entry::RefreshLibrary) => row(&imp.refresh_row),
            (Section::Manage, Entry::RefreshMetadata) => row(&imp.refresh_metadata_row),
            _ => Vec::new(),
        }
    }

    /// Changes what decides which entries the sections list (a service changed, the page
    /// under the panel, or a game it is over) and shows them as it now says.
    pub(crate) fn update_shown(&self, update: impl FnOnce(&mut Shown)) {
        let imp = self.imp();
        let mut shown = imp.shown.get();
        update(&mut shown);
        imp.shown.set(shown);
        self.apply_shown(shown);
    }

    /// Shows each section's entries that `pindu_core::panels::section_entries` lists for
    /// `shown`, and hides the others, with Quick's tiles laid out from the grid's start
    /// (`pindu_core::panels::quick_tiles`) and its activity strip while something runs, and
    /// the strip's toggles of the sections that have anything to show
    /// (`pindu_core::panels::sections`).
    fn apply_shown(&self, shown: Shown) {
        let every = Shown {
            playing: true,
            filters: true,
            performance: true,
            running: true,
            metadata: true,
            network: true,
            bluetooth: true,
            sound_output: true,
            system_update: true,
            over_game: false,
        };
        for section in QuickAccessSection::ALL {
            let listed = section_entries(section, shown);
            for entry in section_entries(section, every) {
                for widget in self.entry_widgets(section, entry) {
                    widget.set_visible(listed.contains(&entry));
                }
            }
        }
        let tiles = quick_tiles(shown)
            .into_iter()
            .flat_map(|tile| self.entry_widgets(QuickAccessSection::Quick, tile.entry()))
            .collect();
        let imp = self.imp();
        imp.quick_section.set_tiles(tiles);
        imp.activity_strip.set_visible(shown.running);
        let listed = sections(shown);
        for section in QuickAccessSection::ALL {
            self.tab(section).set_visible(listed.contains(&section));
        }
    }

    /// The strip's toggle for `section`.
    fn tab(&self, section: QuickAccessSection) -> gtk::ToggleButton {
        let imp = self.imp();
        match section {
            QuickAccessSection::Quick => imp.quick_tab.get(),
            QuickAccessSection::Notifications => imp.notifications_tab.get(),
            QuickAccessSection::Design => imp.design_tab.get(),
            QuickAccessSection::Controller => imp.controller_tab.get(),
            QuickAccessSection::System => imp.system_tab.get(),
            QuickAccessSection::Manage => imp.manage_tab.get(),
        }
    }

    /// Whether the panel shows `section` now, at its main level.
    pub(crate) fn shows_section(&self, section: QuickAccessSection) -> bool {
        self.level() == Some(QuickAccessLevel::Main) && self.imp().section.get() == section
    }

    /// Shows `section` under the strip, filled as things now are: Quick its activity
    /// strip, Notifications lists what runs and marks the notifications read, and Design
    /// and Controller read their choices again. From another section (`switched`) it
    /// crossfades, unless Pindu moves less.
    fn fill_section(&self, section: QuickAccessSection, switched: bool) {
        let imp = self.imp();
        if let Some(action) = imp.section_action.get() {
            action.set_state(&section.name().to_variant());
        }
        for each in QuickAccessSection::ALL {
            self.tab(each)
                .update_state(&[gtk::accessible::State::Selected(Some(each == section))]);
        }
        imp.sections
            .set_transition_type(if switched && !self.reduces_motion() {
                gtk::StackTransitionType::Crossfade
            } else {
                gtk::StackTransitionType::None
            });
        imp.sections.set_visible_child_name(section.name());
        match section {
            QuickAccessSection::Quick => self.show_activity_strip(),
            QuickAccessSection::Notifications => {
                self.show_activity();
                self.show_notifications();
            }
            QuickAccessSection::Design => self.fill_designs(),
            QuickAccessSection::Controller => self.fill_controllers(),
            QuickAccessSection::System | QuickAccessSection::Manage => {}
        }
    }

    /// Whether Pindu moves less (`pindu_core::accessibility`), as the window it is in does
    /// in fullscreen mode, the only place the panel shows.
    fn reduces_motion(&self) -> bool {
        let (animations_enabled, system_reduced_motion) = gtk::Settings::default()
            .map(|settings| {
                (
                    settings.is_gtk_enable_animations(),
                    settings.gtk_interface_reduced_motion() == gtk::ReducedMotion::Reduce,
                )
            })
            .unwrap_or((true, false));
        Accessibility {
            animations_enabled,
            system_reduced_motion,
            reduce_motion: self.settings().boolean(REDUCE_MOTION_KEY),
            text_size: TextSize::Default,
            fullscreen: true,
        }
        .reduces_motion()
    }

    /// Remembers the item focused in `section`, which is being left, for when it shows
    /// again while the panel stays open.
    fn remember_focus(&self, section: QuickAccessSection) {
        let imp = self.imp();
        let Some(page) = imp.sections.child_by_name(section.name()) else {
            return;
        };
        let Some(focus) = self.focus_widget().filter(|focus| focus.is_ancestor(&page)) else {
            return;
        };
        let mut remembered = imp.remembered.borrow_mut();
        remembered.retain(|(each, _)| *each != section);
        remembered.push((section, focus.downgrade()));
    }

    /// The item `section` had focused when it was last left, while it still shows and can
    /// take focus.
    fn remembered(&self, section: QuickAccessSection) -> Option<gtk::Widget> {
        let imp = self.imp();
        let page = imp.sections.child_by_name(section.name())?;
        let widget = imp
            .remembered
            .borrow()
            .iter()
            .find(|(each, _)| *each == section)?
            .1
            .upgrade()?;
        let shows = std::iter::successors(Some(widget.clone()), |widget| widget.parent())
            .take_while(|widget| *widget != page)
            .all(|widget| widget.is_visible());
        (shows && widget.is_ancestor(&page) && widget.is_sensitive()).then_some(widget)
    }

    /// The item focus goes to in `section` when it shows for the first time since the
    /// panel opened: in Quick the first page card, or else the first Playing Now card's
    /// Return, or else the first tile (`pindu_core::panels::quick_first_focus`); in
    /// Notifications the first task while something runs, or else the first
    /// notification; in Design the design in force; in Controller the first controller
    /// that can rumble, or else the first button prompt; and in System and Manage their
    /// first entry.
    fn first_item(&self, section: QuickAccessSection) -> Option<gtk::Widget> {
        let imp = self.imp();
        match section {
            QuickAccessSection::Quick => match quick_first_focus(imp.shown.get()) {
                QuickFocus::PageCard => [imp.filters_card.get(), imp.performance_card.get()]
                    .into_iter()
                    .find(|card| card.is_visible() && card.is_sensitive())
                    .map(Cast::upcast),
                QuickFocus::PlayingNowReturn => imp
                    .playing_cards
                    .first_child()
                    .as_ref()
                    .and_then(card_return),
                QuickFocus::Tile => imp.quick_section.first_tile(),
            },
            QuickAccessSection::Notifications => {
                first_shown_row(&imp.activity).or_else(|| first_shown_row(&imp.notifications))
            }
            QuickAccessSection::Design => {
                let checked = imp.design_action.get().and_then(|action| action.state());
                let mut rows =
                    std::iter::successors(imp.designs.first_child(), |row| row.next_sibling());
                rows.find(|row| {
                    row.downcast_ref::<adw::ActionRow>()
                        .and_then(adw::ActionRow::activatable_widget)
                        .and_downcast::<gtk::CheckButton>()
                        .and_then(|check| check.action_target_value())
                        == checked
                })
                .or_else(|| first_shown_row(&imp.designs))
            }
            QuickAccessSection::Controller => {
                std::iter::successors(imp.controllers.first_child(), |row| row.next_sibling())
                    .find(|row| row.is::<gtk::ListBoxRow>() && row.is_sensitive())
                    .or_else(|| first_shown_row(&imp.prompts))
            }
            QuickAccessSection::System => first_shown_row(&imp.system_list),
            QuickAccessSection::Manage => first_shown_row(&imp.manage_list),
        }
    }

    /// Moves focus off `entry`, a row or tile which is going or can no longer be
    /// activated, to the nearest one after it that shows and can take focus, or else the
    /// nearest before it.
    pub(crate) fn focus_beside(&self, entry: &impl IsA<gtk::Widget>) {
        let entry = entry.upcast_ref::<gtk::Widget>();
        let takes =
            |widget: &gtk::Widget| widget != entry && widget.is_visible() && widget.is_sensitive();
        let target = std::iter::successors(entry.next_sibling(), |widget| widget.next_sibling())
            .find(|widget| takes(widget))
            .or_else(|| {
                std::iter::successors(entry.prev_sibling(), |widget| widget.prev_sibling())
                    .find(|widget| takes(widget))
            });
        if let Some(target) = target {
            target.grab_focus();
        }
    }

    /// One `win.quick-access-<level>` action per level an entry leads to, and
    /// `win.quick-access-section`, which the strip's toggles activate with their section's
    /// name and whose state, the section shown, checks one of them.
    fn add_level_actions(&self, actions: &impl IsA<gio::ActionMap>) {
        for level in QuickAccessLevel::ALL {
            if level == QuickAccessLevel::Main {
                continue;
            }
            let name = format!("quick-access-{}", level.name());
            self.add_host_action(actions, &name, None, move |quick_access, _| {
                quick_access.request_level(level);
            });
        }
        let section = gio::SimpleAction::new_stateful(
            "quick-access-section",
            Some(glib::VariantTy::STRING),
            &self.imp().section.get().name().to_variant(),
        );
        section.connect_activate(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_, target| {
                let section = target
                    .and_then(|target| target.str())
                    .and_then(QuickAccessSection::from_name);
                if let Some(section) = section {
                    quick_access.request_section(section);
                }
            }
        ));
        actions.add_action(&section);
        let _ = self.imp().section_action.set(section);
    }

    /// Quick's parts, which its section moves focus between itself, and the activity
    /// strip, which shows the Notifications section.
    fn setup_quick_section(&self) {
        let imp = self.imp();
        imp.quick_section.set_parts(Parts {
            playing_cards: imp.playing_cards.get(),
            page_cards: vec![
                imp.filters_card.get().upcast(),
                imp.performance_card.get().upcast(),
            ],
            tiles: imp.quick_tiles.get(),
            strip: imp.activity_strip.get().upcast(),
        });
        imp.activity_strip.connect_clicked(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_| quick_access.request_section(QuickAccessSection::Notifications)
        ));
    }

    /// Up and Down stop at a list's ends; in the levels with more than one list they go on
    /// to the next one. Quick moves through itself (`PinduQuickSection`). Escape asks to
    /// close the panel rather than leave fullscreen mode.
    fn setup_navigation(&self) {
        let imp = self.imp();
        let [network_devices, wifi_radio, wifi_networks] = imp.network_level.lists();
        let [bluetooth_adapter, bluetooth_paired, bluetooth_found] = imp.bluetooth_level.lists();
        let [system_versions, system_actions] = imp.system_update_level.lists();
        let [add_store_stores, add_store_actions] = imp.add_store_level.lists();
        let [report_actions] = imp.report_level.lists();
        let [
            health_sources,
            health_launching,
            health_artwork,
            health_library,
        ] = imp.health_level.lists();
        let [
            proton_default,
            proton_tools,
            proton_downloaded,
            proton_check,
            proton_releases_1,
            proton_releases_2,
        ] = imp.proton_level.lists();
        for list in [
            imp.activity.get(),
            imp.notifications.get(),
            imp.designs.get(),
            imp.variants.get(),
            imp.backgrounds.get(),
            imp.blur.get(),
            imp.dim.get(),
            imp.custom_style.get(),
            imp.controllers.get(),
            imp.prompts.get(),
            imp.swap.get(),
            imp.repeat_delay.get(),
            imp.repeat_rate.get(),
            imp.dead_zone.get(),
            imp.sizes.get(),
            imp.switches.get(),
            imp.accessibility.get(),
            imp.sources.get(),
            imp.source_order.get(),
            imp.duplicates.get(),
            imp.text_sizes.get(),
            imp.artwork.get(),
            imp.metadata.get(),
            imp.filters.get(),
            imp.genres.get(),
            imp.clear_filters.get(),
            imp.save_filters.get(),
            imp.performance.get(),
            imp.proton_builds.get(),
            bluetooth_adapter,
            bluetooth_paired,
            bluetooth_found,
            system_versions,
            system_actions,
            health_sources,
            health_launching,
            health_artwork,
            health_library,
            add_store_stores,
            add_store_actions,
            report_actions,
            imp.report_logs.get(),
            proton_default,
            proton_tools,
            proton_downloaded,
            proton_check,
            proton_releases_1,
            proton_releases_2,
            network_devices,
            wifi_radio,
            wifi_networks,
            imp.controller_list.get(),
            imp.system_list.get(),
            imp.manage_list.get(),
        ] {
            list.connect_keynav_failed(|list, direction| {
                step_out_of_list(list, direction);
                glib::Propagation::Stop
            });
        }
        let escape = gtk::ShortcutController::new();
        let quick_access = self.downgrade();
        escape.add_shortcut(gtk::Shortcut::new(
            gtk::ShortcutTrigger::parse_string(&keys::ESCAPE.accelerator()),
            Some(gtk::CallbackAction::new(move |_, _| {
                if let Some(quick_access) = quick_access.upgrade() {
                    quick_access.request_close();
                }
                glib::Propagation::Stop
            })),
        ));
        self.add_controller(escape);
    }

    /// Proton: the defaults bound to their settings, and `win.check-proton-releases`.
    fn setup_proton(&self, actions: &impl IsA<gio::ActionMap>) {
        let level = &self.imp().proton_level;
        level.setup(self.settings());
        self.add_host_action(actions, "check-proton-releases", None, |quick_access, _| {
            quick_access.imp().proton_level.check_releases();
        });
    }

    /// Settings: cover size, navigation sounds and their volume, exiting after a launch,
    /// Reduce Motion and Hold to Confirm, the same settings Preferences changes, applied as
    /// they change. Text Size chooses through its setting's action. Design's My Stylesheet
    /// is bound to its setting too.
    fn setup_settings(&self) {
        let imp = self.imp();
        let settings = self.settings();
        settings
            .bind("navigation-sounds", &*imp.sounds_row, "active")
            .build();
        bind_volume_slider(settings, &imp.volume_row, &imp.volume);
        settings
            .bind("exit-after-launch", &*imp.exit_row, "active")
            .build();
        settings
            .bind(REDUCE_MOTION_KEY, &*imp.reduce_motion_row, "active")
            .build();
        settings
            .bind(HOLD_TO_CONFIRM_KEY, &*imp.hold_to_confirm_row, "active")
            .build();
        settings
            .bind("prefer-steamgriddb", &*imp.prefer_steamgriddb_row, "active")
            .build();
        settings
            .bind(wikidata_service::SETTINGS_KEY, &*imp.wikidata_row, "active")
            .build();
        let show_metadata = glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |settings: &gio::Settings| {
                let on = settings.boolean(wikidata_service::SETTINGS_KEY);
                quick_access.update_shown(|shown| shown.metadata = on);
            }
        );
        show_metadata(settings);
        settings.connect_changed(Some(wikidata_service::SETTINGS_KEY), move |settings, _| {
            show_metadata(settings);
        });
        imp.steamgriddb_key_row.connect_activated(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_| quick_access.ask_steamgriddb_key()
        ));
        let toggles = [
            imp.tile_small.get(),
            imp.tile_medium.get(),
            imp.tile_large.get(),
        ];
        for (toggle, size) in toggles.iter().zip(TILE_SIZES) {
            toggle.connect_toggled(glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |toggle| {
                    let settings = quick_access.settings();
                    if toggle.is_active() && settings.string("tile-size") != size {
                        let _ = settings.set_string("tile-size", size);
                    }
                }
            ));
        }
        let show_size = move |settings: &gio::Settings| {
            let size = settings.string("tile-size");
            for (toggle, name) in toggles.iter().zip(TILE_SIZES) {
                toggle.set_active(size == name);
            }
        };
        show_size(settings);
        settings.connect_changed(Some("tile-size"), move |settings, _| show_size(settings));
        settings
            .bind(CUSTOM_STYLE_KEY, &*imp.custom_style_row, "active")
            .build();
        settings.connect_changed(
            Some(CUSTOM_STYLE_KEY),
            glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |_, _| quick_access.show_custom_style()
            ),
        );
    }

    /// Design chooses through `win.design`, whose state is the design in force: it checks
    /// the matching row, and follows the setting, which Preferences can change too. Its
    /// variants do the same through `win.design-variant`, and Blur and Dim through their
    /// settings' actions.
    fn setup_designs(&self, application: &PinduApplication, actions: &impl IsA<gio::ActionMap>) {
        let imp = self.imp();
        let design = gio::SimpleAction::new_stateful(
            "design",
            Some(glib::VariantTy::STRING),
            &self.design_in_force().unwrap_or_default().to_variant(),
        );
        design.connect_change_state(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_, state| {
                if let Some(id) = state.and_then(String::from_variant) {
                    quick_access.choose_design(&id);
                }
            }
        ));
        actions.add_action(&design);
        let variant = gio::SimpleAction::new_stateful(
            "design-variant",
            Some(glib::VariantTy::STRING),
            &self.variant_in_force().unwrap_or_default().to_variant(),
        );
        variant.connect_change_state(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_, state| {
                if let Some(id) = state.and_then(String::from_variant) {
                    quick_access.choose_variant(&id);
                }
            }
        ));
        actions.add_action(&variant);
        let _ = imp.design_action.set(design.clone());
        let _ = imp.variant_action.set(variant.clone());
        let settings = self.settings();
        actions.add_action(&settings.create_action(BACKGROUND_KEY));
        actions.add_action(&settings.create_action(BLUR_KEY));
        actions.add_action(&settings.create_action(DIM_KEY));
        let service = application.style_service();
        // Another design has other variants, and may or may not offer the choice of
        // background: the lists follow while they show.
        service.connect_design_changed(glib::clone!(
            #[weak]
            design,
            #[weak]
            variant,
            #[weak(rename_to = quick_access)]
            self,
            move || {
                design.set_state(
                    &quick_access
                        .design_in_force()
                        .unwrap_or_default()
                        .to_variant(),
                );
                variant.set_state(
                    &quick_access
                        .variant_in_force()
                        .unwrap_or_default()
                        .to_variant(),
                );
                if quick_access.shows_section(QuickAccessSection::Design) {
                    quick_access.fill_variants();
                    quick_access.show_background_choice();
                }
            }
        ));
        // The rows follow the action's state, so focus stays on the chosen one.
        service.connect_variant_changed(glib::clone!(
            #[weak]
            variant,
            #[weak(rename_to = quick_access)]
            self,
            move || {
                variant.set_state(
                    &quick_access
                        .variant_in_force()
                        .unwrap_or_default()
                        .to_variant(),
                );
            }
        ));
        // The layout in force also changes with high contrast, which switches designs off.
        service.connect_layout_changed(glib::clone!(
            #[weak(rename_to = quick_access)]
            self,
            move |_| quick_access.show_background_choice()
        ));
    }

    /// Design: Default, then each installed design, with a check on the one in force. A
    /// design for a newer Pindu launcher shows but cannot be chosen. Then the variants of
    /// the design in force and My Stylesheet as things now are.
    fn fill_designs(&self) {
        let imp = self.imp();
        let Some(application) = self.application() else {
            return;
        };
        let service = application.style_service();
        imp.designs.remove_all();
        for choice in design_choices(&service.designs(), &service.design()) {
            let subtitle = if choice.available {
                choice.description
            } else {
                needs_newer_launcher()
            };
            let row = radio_row(
                "win.design",
                &choice.id,
                &choice.name.unwrap_or_else(no_design_name),
            );
            row.set_subtitle(&subtitle);
            row.set_sensitive(choice.available);
            imp.designs.append(&row);
        }
        self.fill_variants();
        self.show_background_choice();
        self.show_custom_style();
    }

    /// The Variant list: the variants of the design in force, with a check on the one in
    /// force; hidden for a design without variants.
    fn fill_variants(&self) {
        let imp = self.imp();
        let Some(application) = self.application() else {
            return;
        };
        let service = application.style_service();
        let choices = variant_choices(&service.designs(), &service.design(), &service.variant());
        imp.variants.remove_all();
        for choice in &choices {
            let row = radio_row("win.design-variant", &choice.id, &choice.name);
            imp.variants.append(&row);
        }
        imp.variant_title.set_visible(!choices.is_empty());
        imp.variants.set_visible(!choices.is_empty());
    }

    /// The Background heading and its list: shown only while the design in force offers
    /// the choice (`ThemeLayout::home_background_choice`). Hidden, Up and Down pass them
    /// by, and Home shows the default background whatever the setting holds.
    fn show_background_choice(&self) {
        let imp = self.imp();
        let offered = self
            .application()
            .is_some_and(|application| application.style_service().layout().home_background_choice);
        imp.background_title.set_visible(offered);
        imp.backgrounds.set_visible(offered);
    }

    /// My Stylesheet: it can be turned on once there is a `fullscreen.css`, and its
    /// subtitle says where that goes until there is; on without one, it can still be
    /// turned off.
    fn show_custom_style(&self) {
        let Some(application) = self.application() else {
            return;
        };
        let service = application.style_service();
        let exists = service.has_custom_stylesheet();
        let row = &self.imp().custom_style_row;
        row.set_sensitive(exists || self.settings().boolean(CUSTOM_STYLE_KEY));
        row.set_subtitle(&if exists {
            gettext("Your fullscreen.css, over the design")
        } else {
            gettext("Create {path} to restyle fullscreen mode").replace(
                "{path}",
                &paths::home_relative(
                    service.custom_stylesheet_path(),
                    &std::env::home_dir().unwrap_or_default(),
                ),
            )
        });
    }

    /// Applies the design `id` (Adwaita, no design, for an empty one) unless it is in force
    /// already; Quick Access stays open.
    fn choose_design(&self, id: &str) {
        if self.design_in_force().as_deref() == Some(id) {
            return;
        }
        if let Some(application) = self.application() {
            application.style_service().set_design(id);
        }
    }

    /// Applies the variant `id` of the design in force unless it is in force already.
    fn choose_variant(&self, id: &str) {
        if self.variant_in_force().as_deref() == Some(id) {
            return;
        }
        if let Some(application) = self.application() {
            application.style_service().set_variant(id);
        }
    }

    /// The id of the variant Design checks: the chosen one, or the design's first; `None`
    /// for a design without variants.
    fn variant_in_force(&self) -> Option<String> {
        let application = self.application()?;
        let service = application.style_service();
        variant_choices(&service.designs(), &service.design(), &service.variant())
            .into_iter()
            .find(|choice| choice.active)
            .map(|choice| choice.id)
    }

    /// The id of the design Design checks: the chosen one if it can apply, or empty for
    /// Adwaita.
    fn design_in_force(&self) -> Option<String> {
        let application = self.application()?;
        let service = application.style_service();
        design_choices(&service.designs(), &service.design())
            .into_iter()
            .find(|choice| choice.active)
            .map(|choice| choice.id)
    }

    /// The SteamGridDB key's row: whether a key is stored, without showing it, or why the
    /// keyring cannot be used, in which case there is nothing to type the key for.
    fn show_steamgriddb_key(&self) {
        let quick_access = self.downgrade();
        glib::spawn_future_local(async move {
            let stored = steamgriddb_service::api_key().await;
            let Some(quick_access) = quick_access.upgrade() else {
                return;
            };
            let row = &quick_access.imp().steamgriddb_key_row;
            row.set_subtitle(&steamgriddb_service::key_status(&stored));
            row.set_sensitive(stored.is_ok());
        });
    }

    /// Asks for the SteamGridDB key, hidden, with the on-screen keyboard, and stores it in
    /// the keyring; missing art is fetched once it is. Nothing is kept on Cancel or with
    /// nothing typed.
    fn ask_steamgriddb_key(&self) {
        ask_secret(
            self,
            &gettext("SteamGridDB Key"),
            &gettext(
                "Type your API key from SteamGridDB's preferences. It is kept in your keyring.",
            ),
            Keyboard::passphrase(),
            glib::clone!(
                #[weak(rename_to = quick_access)]
                self,
                move |key: Option<String>| {
                    let Some(key) = key.filter(|key| !key.trim().is_empty()) else {
                        return;
                    };
                    glib::spawn_future_local(async move {
                        let saved = steamgriddb_service::set_api_key(&key).await;
                        match saved {
                            Ok(()) => {
                                quick_access.show_steamgriddb_key();
                                if let Some(application) = quick_access.application() {
                                    application
                                        .activate_action("fetch-art", Some(&true.to_variant()));
                                }
                            }
                            Err(error) => quick_access.imp().steamgriddb_key_row.set_subtitle(
                                &gettext("The key was not saved: {error}")
                                    .replace("{error}", &error),
                            ),
                        }
                    });
                }
            ),
        );
    }
}

/// The panel a signal of it was emitted on.
fn quick_access(values: &[glib::Value]) -> PinduQuickAccess {
    values[0]
        .get::<PinduQuickAccess>()
        .expect("the panel emits its own signals")
}

/// What Design calls the choice of no design: Adwaita, the stock libadwaita look it leaves
/// in force. A name, so it is not translated.
fn no_design_name() -> String {
    "Adwaita".to_owned()
}

/// A row of a list of choices (Design's designs and variants, Filters' genres): `title`,
/// with a check on the stateful `action` for `id`, which the row's activation sets.
pub(crate) fn radio_row(action: &str, id: &str, title: &str) -> adw::ActionRow {
    let check = gtk::CheckButton::builder()
        .action_name(action)
        .action_target(&id.to_variant())
        .can_focus(false)
        .build();
    let row = adw::ActionRow::builder()
        .title(title)
        .use_markup(false)
        .activatable_widget(&check)
        .build();
    row.add_prefix(&check);
    row
}

/// Why Design cannot apply a design written for a newer style contract.
fn needs_newer_launcher() -> String {
    gettext("Needs a newer Pindu launcher")
}

/// The first row of `list` that shows and can take focus, while the list shows.
fn first_shown_row(list: &gtk::ListBox) -> Option<gtk::Widget> {
    if !list.is_visible() {
        return None;
    }
    std::iter::successors(list.first_child(), |row| row.next_sibling())
        .find(|row| row.is::<gtk::ListBoxRow>() && row.is_visible() && row.is_sensitive())
}
