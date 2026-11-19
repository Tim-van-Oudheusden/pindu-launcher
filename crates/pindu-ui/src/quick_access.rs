//! Quick Access, fullscreen mode's panel on the right (Guide + A, R3 or the Steam Deck's "…"
//! button), as a widget a window hosts: [`PinduQuickAccess`], with its own Blueprint
//! (`data/ui/quick_access.blp`). It holds the status header (`fullscreen_status`), the
//! level stack, the main list and every level, and takes the services it shows from the
//! application ([`PinduQuickAccess::setup`]), never from its host.
//!
//! Which level is open is the host's (`pindu_core::panels::Panels`, which also holds the
//! host's other panels): the host shows a level with [`PinduQuickAccess::show_level`],
//! focuses it once it shows with [`PinduQuickAccess::focus_level`], and says what lies
//! under the panel with [`PinduQuickAccess::set_page`]. What the panel cannot do itself it
//! asks of the host through its signals: close (`close-requested`), lead to a level
//! (`level-requested`), leave Activity once nothing runs (`activity-ended`), show a game
//! (`game-requested`) and apply fullscreen mode's changed library filters
//! (`filters-changed`). Over a running game (`game_overlay`) it leaves out the
//! entries that need the main window ([`PinduQuickAccess::set_over_game`]). Network and
//! Bluetooth ask their questions on the host the player sees (`asks_prompts`).
//!
//! The levels are filled by: Notifications `fullscreen_notifications`, Activity and Refresh
//! Library `fullscreen_activity`, Surprise Me `fullscreen_surprise`, Controllers
//! `fullscreen_controllers`, Controls `fullscreen_controls`, Filters (offered on the
//! library page) `fullscreen_library`, Performance (offered for a game we run ourselves)
//! `fullscreen_performance`, Network `fullscreen_network`, Bluetooth `fullscreen_bluetooth`,
//! Sound Output `fullscreen_sound_output`, System Update `fullscreen_system_update`,
//! Diagnostics Report `fullscreen_report`, Add a Store `add_store`, Proton
//! `fullscreen_proton_downloads` and Sources `fullscreen_sources`. Design and Settings are
//! this module's: the design, its variant, Home's background blur and dim and the user's
//! own stylesheet, then cover size, navigation sounds and their volume, exiting after a
//! launch, Reduce Motion and Larger Text, the SteamGridDB key (typed hidden with the
//! on-screen keyboard of `secret_dialog` and kept in the keyring) and Prefer SteamGridDB
//! Art, the same settings Preferences changes.

use std::cell::{Cell, OnceCell, RefCell};
use std::sync::LazyLock;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib::subclass::Signal;
use gtk::{gio, glib};
use pindu_core::accessibility::TEXT_SIZE_KEY;
use pindu_core::gamepad::Intent;
use pindu_core::gamescope::prompts::{PromptHost, prompt_host};
use pindu_core::hold::HOLD_TO_CONFIRM_KEY;
use pindu_core::home::{BLUR_KEY, DIM_KEY};
use pindu_core::keys;
use pindu_core::model::GameId;
use pindu_core::osk::Keyboard;
use pindu_core::panels::{PageEntry, QuickAccessLevel, design_choices, variant_choices};
use pindu_core::paths;
use pindu_core::tasks::Task;

use crate::add_store::PinduAddStoreLevel;
use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::fullscreen::bind_volume_slider;
use crate::fullscreen_bluetooth::PinduBluetoothLevel;
use crate::fullscreen_clock::Clock;
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
use crate::secret_dialog::ask_secret;
use crate::steamgriddb_service;

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
        pub playing_cards: TemplateChild<gtk::Box>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub notifications_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub unread: TemplateChild<gtk::Label>,
        #[template_child]
        pub notifications: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub activity_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub refresh_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub surprise_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub activity: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub design_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub designs: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub variant_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub variants: TemplateChild<gtk::ListBox>,
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
        pub controllers_row: TemplateChild<gtk::ListBoxRow>,
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
        pub controls_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub controls: TemplateChild<gtk::Box>,
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
        pub filters_row: TemplateChild<gtk::ListBoxRow>,
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
        pub hidden_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub clear_filters: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub performance_row: TemplateChild<gtk::ListBoxRow>,
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
        /// The header's clock, which ticks while the panel is open.
        pub(crate) time: OnceCell<Clock>,
        /// The Playing Now cards at the top of the main level.
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
        /// The intents Controls lists, in its rows' order across its groups.
        pub shown_controls: RefCell<Vec<Intent>>,
        /// The tasks Activity shows, each with its progress bar, and the next pulse of the
        /// bars of those with no telling how far they are.
        pub activity_shown: RefCell<Vec<(Task, gtk::ProgressBar)>>,
        pub activity_pulse: RefCell<Option<glib::SourceId>>,
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
            PinduAddStoreLevel::ensure_type();
            PinduReportLevel::ensure_type();
            PinduProtonLevel::ensure_type();
            PinduSoundOutputLevel::ensure_type();
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
            let _ = self
                .playing
                .set(PlayingCards::new(&self.playing_cards, &self.list));
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
                    Signal::builder(ACTIVITY_ENDED).build(),
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
    /// Fullscreen mode's Quick Access panel: its status header, its entries and their
    /// levels. A window hosts it (see the module documentation for what the host does).
    pub struct PinduQuickAccess(ObjectSubclass<imp::PinduQuickAccess>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

/// The panel asks its host to close it.
const CLOSE_REQUESTED: &str = "close-requested";
/// The panel asks its host to show one of its levels, by name.
const LEVEL_REQUESTED: &str = "level-requested";
/// Nothing runs in the background any more, so Activity has nothing left to list.
const ACTIVITY_ENDED: &str = "activity-ended";
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
        self.setup_add_store(application);
        self.setup_diagnostics_report(actions);
        self.setup_proton(actions);
        self.setup_sources(application);
    }

    /// Hosts the panel over a running game, before [`Self::setup`]: it leaves out the
    /// entries that need the main window, Surprise Me (it leads to a library page), Add a
    /// Store (its flow is the main window's) and Diagnostics Report (it needs the main
    /// window's room, and its page turns are the main window's).
    pub fn set_over_game(&self) {
        let imp = self.imp();
        imp.over_game.set(true);
        imp.surprise_row.set_visible(false);
        imp.add_store_row.set_visible(false);
        imp.report_row.set_visible(false);
    }

    /// Shows what Quick Access adds for the page under it, as it opens: `entries` from
    /// `pindu_core::panels::page_entries`, and `game`, the focused game or the one in
    /// front, whose switches Performance shows.
    pub fn set_page(&self, entries: &[PageEntry], game: Option<&GameObject>) {
        let imp = self.imp();
        imp.filters_row
            .set_visible(entries.contains(&PageEntry::Filters));
        let performance = entries.contains(&PageEntry::Performance);
        imp.performance_row.set_visible(performance);
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
    /// closed. The panel reads its services again as it opens, and its clock and Playing
    /// Now times run while it is open.
    pub fn show_level(&self, level: Option<QuickAccessLevel>) {
        let imp = self.imp();
        let before = imp.level.replace(level);
        if before == level {
            return;
        }
        if before == Some(QuickAccessLevel::Bluetooth) {
            self.leave_bluetooth();
        }
        match level {
            Some(QuickAccessLevel::Notifications) => self.show_notifications(),
            Some(QuickAccessLevel::Activity) => self.show_activity(),
            Some(QuickAccessLevel::Designs) => self.fill_designs(),
            Some(QuickAccessLevel::Controllers) => self.fill_controllers(),
            Some(QuickAccessLevel::Controls) => self.fill_controls(),
            Some(QuickAccessLevel::Filters) => self.show_filters(),
            Some(QuickAccessLevel::Bluetooth) => self.show_bluetooth(),
            Some(QuickAccessLevel::SoundOutput) => self.show_sound_output(),
            Some(QuickAccessLevel::SystemUpdate) => self.show_system_update(),
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

    /// Focuses the level shown: on the main level the entry of `left`, the level just left,
    /// or else its first item (a list box would go back to the row it last had focus on).
    pub fn focus_level(&self, left: Option<QuickAccessLevel>) {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        let target: Option<gtk::Widget> = match (self.level(), left) {
            (Some(QuickAccessLevel::Main), Some(left)) if self.entry(left).is_some() => {
                self.entry(left)
            }
            // The first Playing Now card's Return comes first.
            (Some(QuickAccessLevel::Main), _) if imp.playing_cards.is_visible() => imp
                .playing_cards
                .first_child()
                .as_ref()
                .and_then(card_return),
            (Some(QuickAccessLevel::Main), _) => Some(imp.notifications_row.get().upcast()),
            (Some(QuickAccessLevel::Notifications), _) => first_row(&imp.notifications),
            (Some(QuickAccessLevel::Activity), _) => first_row(&imp.activity),
            // The design in force.
            (Some(QuickAccessLevel::Designs), _) => {
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
                .or_else(|| first_row(&imp.designs))
            }
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
            // The first controller that can rumble, or else the first button prompt.
            (Some(QuickAccessLevel::Controllers), _) => {
                std::iter::successors(imp.controllers.first_child(), |row| row.next_sibling())
                    .find(|row| row.is::<gtk::ListBoxRow>() && row.is_sensitive())
                    .or_else(|| first_row(&imp.prompts))
            }
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

    /// Calls `callback` with the level an entry leads to, or with the main level when the
    /// section shown is gone.
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

    /// Calls `callback` when nothing runs in the background any more, so Activity, if it
    /// shows, leaves for the entries.
    pub fn connect_activity_ended<F: Fn(&Self) + 'static>(
        &self,
        callback: F,
    ) -> glib::SignalHandlerId {
        self.connect_local(ACTIVITY_ENDED, false, move |values| {
            callback(&quick_access(values));
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

    pub(crate) fn end_activity(&self) {
        self.emit_by_name::<()>(ACTIVITY_ENDED, &[]);
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

    /// The entry on the main level that leads to `level`; for Activity the one focus goes
    /// to once it is gone.
    fn entry(&self, level: QuickAccessLevel) -> Option<gtk::Widget> {
        let imp = self.imp();
        let row = match level {
            QuickAccessLevel::Main => return None,
            QuickAccessLevel::Activity => return Some(self.activity_entry()),
            QuickAccessLevel::Notifications => &imp.notifications_row,
            QuickAccessLevel::Designs => &imp.design_row,
            QuickAccessLevel::Controllers => &imp.controllers_row,
            QuickAccessLevel::Controls => &imp.controls_row,
            QuickAccessLevel::Settings => &imp.settings_row,
            QuickAccessLevel::Sources => &imp.sources_row,
            QuickAccessLevel::Filters => &imp.filters_row,
            QuickAccessLevel::Performance => &imp.performance_row,
            QuickAccessLevel::Bluetooth => &imp.bluetooth_row,
            QuickAccessLevel::SoundOutput => &imp.sound_output_row,
            QuickAccessLevel::SystemUpdate => &imp.system_update_row,
            QuickAccessLevel::AddStore => &imp.add_store_row,
            QuickAccessLevel::DiagnosticsReport | QuickAccessLevel::ReportLog => &imp.report_row,
            QuickAccessLevel::Proton => &imp.proton_row,
            QuickAccessLevel::Network => &imp.network_row,
        };
        Some(row.get().upcast())
    }

    /// One `win.quick-access-<level>` action per level its entry leads to.
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
    }

    /// Up and Down stop at a list's ends; in the levels with more than one list they go on
    /// to the next one (the Playing Now cards take the main list's Up). Escape asks to
    /// close the panel rather than leave fullscreen mode.
    fn setup_navigation(&self) {
        let imp = self.imp();
        let [network_devices, wifi_radio, wifi_networks] = imp.network_level.lists();
        let [bluetooth_adapter, bluetooth_paired, bluetooth_found] = imp.bluetooth_level.lists();
        let [system_versions, system_actions] = imp.system_update_level.lists();
        let [add_store_stores, add_store_actions] = imp.add_store_level.lists();
        let [report_actions] = imp.report_level.lists();
        let [
            proton_default,
            proton_tools,
            proton_downloaded,
            proton_check,
            proton_releases_1,
            proton_releases_2,
        ] = imp.proton_level.lists();
        for list in [
            imp.designs.get(),
            imp.variants.get(),
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
            imp.text_sizes.get(),
            imp.artwork.get(),
            imp.filters.get(),
            imp.clear_filters.get(),
            imp.performance.get(),
            imp.proton_builds.get(),
            bluetooth_adapter,
            bluetooth_paired,
            bluetooth_found,
            system_versions,
            system_actions,
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
            .bind(
                "fullscreen-reduce-motion",
                &*imp.reduce_motion_row,
                "active",
            )
            .build();
        settings
            .bind(HOLD_TO_CONFIRM_KEY, &*imp.hold_to_confirm_row, "active")
            .build();
        settings
            .bind("prefer-steamgriddb", &*imp.prefer_steamgriddb_row, "active")
            .build();
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
        actions.add_action(&settings.create_action(BLUR_KEY));
        actions.add_action(&settings.create_action(DIM_KEY));
        let service = application.style_service();
        // Another design has other variants: the list follows while it shows.
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
                if quick_access.level() == Some(QuickAccessLevel::Designs) {
                    quick_access.fill_variants();
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

/// A row of Design's design and variant lists: `title`, with a check on the stateful
/// `action` for `id`, which the row's activation sets.
fn radio_row(action: &str, id: &str, title: &str) -> adw::ActionRow {
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
