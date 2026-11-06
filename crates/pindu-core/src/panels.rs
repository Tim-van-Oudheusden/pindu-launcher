//! Fullscreen mode's panels over its pages: the menu drawer on the left (where to go,
//! leaving), the Quick Access panel on the right (notifications, what runs in the
//! background, the design, the controllers, a few settings, and entries for the page
//! under it such as the library's filters) and the options sheet at the bottom (what to
//! do with one game). This decides which one is open and at which level, what Start,
//! Guide, R3, View or X and B do to them, and what each lists.

use crate::gamepad::Intent;
use crate::launch;
use crate::model::Game;
use crate::themes::Theme;

/// A panel over the page, at one of its levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Drawer(DrawerLevel),
    Options(OptionsLevel),
    QuickAccess(QuickAccessLevel),
}

/// What the drawer shows: its entries, or the collections one of them leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerLevel {
    Main,
    /// The collections, each opening its tab in the library.
    Collections,
}

/// What the options sheet shows: the game's actions, or its collections to add it to or
/// take it out of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionsLevel {
    Main,
    Collections,
}

/// What the Quick Access panel shows: its entries, or the list one of them leads to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickAccessLevel {
    Main,
    /// The session's notifications, newest first.
    Notifications,
    /// What runs in the background: syncs, art fetches and Proton downloads.
    Activity,
    /// The designs to pick from.
    Designs,
    /// The connected controllers, the button prompts and the confirm and back swap.
    Controllers,
    /// Cover size, navigation sounds and their volume, and exiting after a launch.
    Settings,
    /// The library filters, while the library is under the panel (see [`page_entries`]).
    Filters,
}

/// What lies under the panels when an intent comes in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Under {
    /// A game has focus, or the page is a game's: View or X has a game to show options for.
    pub game: bool,
    /// A dialog is open over everything, and takes the buttons first.
    pub dialog: bool,
    /// A notification is unread: Quick Access opens at the notifications.
    pub unread: bool,
    /// The page is fullscreen mode's library: Quick Access offers its filters.
    pub library: bool,
}

/// The panel that is open, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Panels {
    open: Option<Panel>,
}

impl Panels {
    pub fn new() -> Self {
        Self::default()
    }

    /// The panel that is open, at its level.
    pub fn open(&self) -> Option<Panel> {
        self.open
    }

    /// Acts on a controller intent and says whether the panels took it, so the page does
    /// not act on it too. Start and Guide toggle the drawer, R3 toggles Quick Access (at
    /// its notifications while one is unread, as Steam's does), View or X opens the
    /// options for a game, and B steps back a level or closes. Opening one panel closes
    /// the other. While a panel is open the page's own buttons (search, tabs, sort) wait;
    /// directions and A move and act inside it.
    pub fn handle(&mut self, intent: Intent, under: Under) -> bool {
        if under.dialog {
            return false;
        }
        match (intent, self.open) {
            (Intent::Menu | Intent::Home, Some(Panel::Drawer(_))) => self.open = None,
            (Intent::Menu | Intent::Home, _) => self.open = Some(Panel::Drawer(DrawerLevel::Main)),
            (Intent::QuickAccess, Some(Panel::QuickAccess(_))) => self.open = None,
            (Intent::QuickAccess, _) => {
                let level = if under.unread {
                    QuickAccessLevel::Notifications
                } else {
                    QuickAccessLevel::Main
                };
                self.open = Some(Panel::QuickAccess(level));
            }
            (Intent::Options, None) if under.game => {
                self.open = Some(Panel::Options(OptionsLevel::Main));
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
            (Intent::Back, Some(Panel::Drawer(_))) => {
                self.open = Some(Panel::Drawer(DrawerLevel::Main));
            }
            (Intent::Back, Some(Panel::Options(_))) => {
                self.open = Some(Panel::Options(OptionsLevel::Main));
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
    /// its entries leading to a list.
    pub fn show(&mut self, panel: Panel) {
        self.open = Some(panel);
    }

    /// Closes the open panel: an entry did its work, the pointer clicked outside it, or
    /// the window left fullscreen mode.
    pub fn close(&mut self) {
        self.open = None;
    }

    /// Nothing runs in the background any more: Quick Access leaves its Activity, which
    /// has nothing left to list, for its entries.
    pub fn activity_ended(&mut self) {
        if self.open == Some(Panel::QuickAccess(QuickAccessLevel::Activity)) {
            self.open = Some(Panel::QuickAccess(QuickAccessLevel::Main));
        }
    }
}

/// An entry of the menu drawer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawerEntry {
    /// A card for a game we started that is running, with Return and Stop.
    PlayingNow,
    Home,
    Library,
    Collections,
    ExitFullscreen,
    Quit,
}

/// The drawer's entries, top to bottom: a Playing Now card while a game we started runs,
/// and Collections when there are any.
pub fn drawer_entries(playing: bool, collections: bool) -> Vec<DrawerEntry> {
    use DrawerEntry::*;
    let playing = playing.then_some(PlayingNow);
    let collections = collections.then_some(Collections);
    [
        playing,
        Some(Home),
        Some(Library),
        collections,
        Some(ExitFullscreen),
        Some(Quit),
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
}

/// The entries Quick Access adds for the page `under` it, top to bottom: the library's
/// Filters.
pub fn page_entries(under: Under) -> Vec<PageEntry> {
    under
        .library
        .then_some(PageEntry::Filters)
        .into_iter()
        .collect()
}

/// An action of the options sheet for one game.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GameOption {
    Play,
    /// Stop a game we started, which runs.
    Stop,
    AddToFavourites,
    RemoveFromFavourites,
    /// Lead to the collections, to add the game to or take it out of.
    Collections,
    Hide,
    Unhide,
    /// Open the game in its store launcher, when it has one.
    OpenInLauncher,
}

/// The options sheet's actions for `game`, top to bottom; `playing` says whether we
/// started it and it runs.
pub fn game_options(game: &Game, playing: bool) -> Vec<GameOption> {
    use GameOption::*;
    [
        Some(if playing { Stop } else { Play }),
        Some(if game.favourite {
            RemoveFromFavourites
        } else {
            AddToFavourites
        }),
        Some(Collections),
        Some(if game.hidden { Unhide } else { Hide }),
        launch::launcher_uri(game).map(|_| OpenInLauncher),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// A design the Quick Access panel offers, or Adwaita (no design, `id` empty).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DesignChoice {
    /// What the `fullscreen-design` setting holds for it; empty for Adwaita.
    pub id: String,
    /// The theme's name; `None` for Adwaita, which the shell names.
    pub name: Option<String>,
    pub description: String,
    /// The design in force, which the panel checks.
    pub active: bool,
    /// Whether this Pindu launcher can apply it.
    pub available: bool,
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
    };
    let designs = themes.iter().map(|theme| DesignChoice {
        id: theme.id.clone(),
        name: Some(theme.name.clone()),
        description: theme.description.clone(),
        active: applies && theme.id == active,
        available: theme.supported(),
    });
    std::iter::once(default).chain(designs).collect()
}

/// A variant of the design in force the Quick Access panel offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantChoice {
    /// What the `fullscreen-design-variant` setting holds for it.
    pub id: String,
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
        dialog: false,
        unread: false,
        library: false,
    };
    const GAME: Under = Under {
        game: true,
        dialog: false,
        unread: false,
        library: false,
    };
    const DIALOG: Under = Under {
        game: true,
        dialog: true,
        unread: false,
        library: false,
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
    fn quick_access_toggles_its_panel() {
        let mut panels = Panels::new();
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access());
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), None);
    }

    #[test]
    fn b_steps_back_from_a_list_and_then_closes_quick_access() {
        for level in [
            QuickAccessLevel::Notifications,
            QuickAccessLevel::Activity,
            QuickAccessLevel::Designs,
            QuickAccessLevel::Controllers,
            QuickAccessLevel::Settings,
            QuickAccessLevel::Filters,
        ] {
            let mut panels = Panels::new();
            panels.handle(Intent::QuickAccess, PAGE);
            panels.show(Panel::QuickAccess(level));
            assert!(panels.handle(Intent::Back, PAGE), "{level:?}");
            assert_eq!(panels.open(), quick_access(), "{level:?}");
            assert!(panels.handle(Intent::Back, PAGE), "{level:?}");
            assert_eq!(panels.open(), None, "{level:?}");
        }
    }

    #[test]
    fn quick_access_toggles_shut_from_its_settings() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Settings));
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), None);
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(panels.open(), quick_access(), "it opens at its entries");
    }

    #[test]
    fn quick_access_opens_at_its_notifications_while_one_is_unread() {
        let unread = Under {
            unread: true,
            ..PAGE
        };
        let mut panels = Panels::new();
        assert!(panels.handle(Intent::QuickAccess, unread));
        assert_eq!(
            panels.open(),
            Some(Panel::QuickAccess(QuickAccessLevel::Notifications))
        );
        assert!(panels.handle(Intent::Back, PAGE));
        assert_eq!(panels.open(), quick_access(), "B leads to its entries");
        panels.close();
        assert!(panels.handle(Intent::QuickAccess, PAGE));
        assert_eq!(
            panels.open(),
            quick_access(),
            "read, it opens at its entries"
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
    fn closing_forgets_the_level() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Settings));
        panels.close();
        assert_eq!(panels.open(), None);
        panels.handle(Intent::QuickAccess, PAGE);
        assert_eq!(panels.open(), quick_access());
    }

    #[test]
    fn quick_access_leaves_its_activity_once_nothing_runs() {
        let mut panels = Panels::new();
        panels.show(Panel::QuickAccess(QuickAccessLevel::Activity));
        panels.activity_ended();
        assert_eq!(panels.open(), quick_access());

        for open in [
            None,
            drawer(),
            options(),
            quick_access(),
            Some(Panel::QuickAccess(QuickAccessLevel::Settings)),
        ] {
            let mut panels = Panels::new();
            if let Some(panel) = open {
                panels.show(panel);
            }
            panels.activity_ended();
            assert_eq!(panels.open(), open, "{open:?} stays");
        }
    }

    #[test]
    fn the_drawer_lists_playing_now_first_and_collections_when_there_are_any() {
        use DrawerEntry::*;
        assert_eq!(
            drawer_entries(true, true),
            [PlayingNow, Home, Library, Collections, ExitFullscreen, Quit]
        );
        assert_eq!(
            drawer_entries(false, false),
            [Home, Library, ExitFullscreen, Quit],
            "settings live in Quick Access"
        );
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

    fn game(source: Source, kind: LaunchKind) -> Game {
        Game::new(GameId::new(source, "1"), "Celeste", kind, 0)
    }

    #[test]
    fn a_store_game_s_options_end_with_opening_its_launcher() {
        use GameOption::*;
        let steam = game(Source::Steam, LaunchKind::StoreUri);
        assert_eq!(
            game_options(&steam, false),
            [Play, AddToFavourites, Collections, Hide, OpenInLauncher]
        );
    }

    #[test]
    fn options_follow_the_game_s_favourite_hidden_and_playing_state() {
        use GameOption::*;
        let mut manual = game(Source::Manual, LaunchKind::HostCommand);
        manual.favourite = true;
        manual.hidden = true;
        assert_eq!(
            game_options(&manual, true),
            [Stop, RemoveFromFavourites, Collections, Unhide]
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
