//! Fullscreen mode's panels over the pages: the menu drawer on the left (Start or Guide),
//! Quick Access on the right (R3 or the Steam Deck's "…" button) and the options sheet at
//! the bottom (View or X, or a game page's Options and Add to Collection buttons). Which
//! panel is open, at which level, and what the controller does to them is
//! `pindu_core::panels`; this fills them, shows them, keeps focus inside the open one and
//! gives it back after, and does what their entries say. They are widgets in the window
//! rather than popover menus, which the compositor dismisses on a controller press.
//! Quick Access's notifications are `fullscreen_notifications`'s, its Activity and Refresh
//! Library `fullscreen_activity`'s, its Surprise Me `fullscreen_surprise`'s, its
//! Controllers `fullscreen_controllers`'s.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::gamepad::Intent;
use pindu_core::library::View;
use pindu_core::panels::{
    DrawerEntry, DrawerLevel, GameOption, OptionsLevel, Panel, Panels, QuickAccessLevel, Under,
    design_choices, drawer_entries, game_options,
};

use crate::application::PinduApplication;
use crate::fullscreen_playing::card_return;
use crate::launch_service::launcher_name;
use crate::models::GameObject;
use crate::preferences::TILE_SIZES;
use crate::window::PinduWindow;

impl PinduWindow {
    pub(crate) fn setup_panels(&self) {
        let imp = self.imp();
        let close = gio::ActionEntry::builder("close-panel")
            .activate(|window: &Self, _, _| window.close_panel())
            .build();
        let home = gio::ActionEntry::builder("drawer-home")
            .activate(|window: &Self, _, _| {
                window.close_panel();
                window.show_home();
            })
            .build();
        let library = gio::ActionEntry::builder("drawer-library")
            .activate(|window: &Self, _, _| {
                window.close_panel();
                window.show_library();
            })
            .build();
        let collections = gio::ActionEntry::builder("drawer-collections")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::Drawer(DrawerLevel::Collections));
            })
            .build();
        let collection = gio::ActionEntry::builder("drawer-collection")
            .parameter_type(Some(glib::VariantTy::INT64))
            .activate(|window: &Self, _, target| {
                if let Some(id) = target.and_then(i64::from_variant) {
                    window.close_panel();
                    window.show_library();
                    window.show_tab(View::Collection(id));
                }
            })
            .build();
        let options = gio::ActionEntry::builder("game-options")
            .activate(|window: &Self, _, _| window.open_options(OptionsLevel::Main))
            .build();
        let game_collections = gio::ActionEntry::builder("game-collections")
            .activate(|window: &Self, _, _| window.open_options(OptionsLevel::Collections))
            .build();
        // R3 and Ctrl+2 without a controller or keyboard: the pointer, or a test.
        let quick_access = gio::ActionEntry::builder("quick-access")
            .activate(|window: &Self, _, _| {
                window.handle_panel_intent(Intent::QuickAccess);
            })
            .build();
        let notifications = gio::ActionEntry::builder("quick-access-notifications")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::QuickAccess(QuickAccessLevel::Notifications));
            })
            .build();
        let activity = gio::ActionEntry::builder("quick-access-activity")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::QuickAccess(QuickAccessLevel::Activity));
            })
            .build();
        let designs = gio::ActionEntry::builder("quick-access-designs")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::QuickAccess(QuickAccessLevel::Designs));
            })
            .build();
        let controllers = gio::ActionEntry::builder("quick-access-controllers")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::QuickAccess(QuickAccessLevel::Controllers));
            })
            .build();
        let quick_settings = gio::ActionEntry::builder("quick-access-settings")
            .activate(|window: &Self, _, _| {
                window.show_panel(Panel::QuickAccess(QuickAccessLevel::Settings));
            })
            .build();
        self.add_action_entries([
            close,
            home,
            library,
            collections,
            collection,
            options,
            game_collections,
            quick_access,
            notifications,
            activity,
            designs,
            controllers,
            quick_settings,
        ]);
        // Up and Down stop at a list's ends; in Quick Access's Controllers and settings they
        // go on to the next list (`fullscreen_playing` takes the drawer's and Quick Access's
        // own lists up to their Playing Now cards).
        for list in [
            imp.quick_access_controllers.get(),
            imp.quick_access_prompts.get(),
            imp.quick_access_swap.get(),
            imp.quick_access_sizes.get(),
            imp.quick_access_switches.get(),
        ] {
            list.connect_keynav_failed(|list, direction| {
                step_out_of_list(list, direction);
                glib::Propagation::Stop
            });
        }
        // Escape closes a panel rather than leaving fullscreen mode.
        for panel in [
            imp.drawer_panel.get(),
            imp.quick_access_panel.get(),
            imp.options_panel.get(),
        ] {
            let escape = gtk::ShortcutController::new();
            escape.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string("Escape"),
                Some(gtk::NamedAction::new("win.close-panel")),
            ));
            panel.add_controller(escape);
        }
        // A click beside a panel closes it.
        imp.drawer.connect_show_sidebar_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |drawer| {
                let open = window.imp().panels.get().open();
                if !drawer.shows_sidebar() && matches!(open, Some(Panel::Drawer(_))) {
                    window.close_panel();
                }
            }
        ));
        imp.quick_access.connect_show_sidebar_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |quick_access| {
                let open = window.imp().panels.get().open();
                if !quick_access.shows_sidebar() && matches!(open, Some(Panel::QuickAccess(_))) {
                    window.close_panel();
                }
            }
        ));
        imp.options_sheet.connect_open_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |sheet| {
                let open = window.imp().panels.get().open();
                if !sheet.is_open() && matches!(open, Some(Panel::Options(_))) {
                    window.close_panel();
                }
            }
        ));
        imp.options_list.connect_row_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let option = usize::try_from(row.index())
                    .ok()
                    .and_then(|index| window.imp().game_options.borrow().get(index).copied());
                if let Some(option) = option {
                    window.choose_option(option);
                }
            }
        ));
        self.library().connect_collections_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.fill_drawer();
                if let Some(game) = window.imp().options_game.borrow().as_ref() {
                    window.fill_options_collections(game);
                }
            }
        ));
        self.setup_quick_access_settings();
        self.setup_quick_access();
        self.fill_drawer();
    }

    /// Quick Access's settings: cover size, navigation sounds and exiting after a launch,
    /// the same settings Preferences changes, applied as they change.
    fn setup_quick_access_settings(&self) {
        let imp = self.imp();
        let settings = self.settings();
        settings
            .bind("navigation-sounds", &*imp.quick_access_sounds_row, "active")
            .build();
        settings
            .bind("exit-after-launch", &*imp.quick_access_exit_row, "active")
            .build();
        let toggles = [
            imp.tile_small.get(),
            imp.tile_medium.get(),
            imp.tile_large.get(),
        ];
        for (toggle, size) in toggles.iter().zip(TILE_SIZES) {
            toggle.connect_toggled(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |toggle| {
                    if toggle.is_active() && window.settings().string("tile-size") != size {
                        let _ = window.settings().set_string("tile-size", size);
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
    }

    /// Quick Access's designs choose through `win.design`, whose state is the design in
    /// force: it checks the matching row, and follows the setting, which Preferences can
    /// change too.
    fn setup_quick_access(&self) {
        let design = gio::SimpleAction::new_stateful(
            "design",
            Some(glib::VariantTy::STRING),
            &self.design_in_force().unwrap_or_default().to_variant(),
        );
        design.connect_change_state(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, state| {
                if let Some(id) = state.and_then(String::from_variant) {
                    window.choose_design(&id);
                }
            }
        ));
        self.add_action(&design);
        if let Some(application) = self.application().and_downcast::<PinduApplication>() {
            application
                .style_service()
                .connect_design_changed(glib::clone!(
                    #[weak]
                    design,
                    #[weak(rename_to = window)]
                    self,
                    move || {
                        design
                            .set_state(&window.design_in_force().unwrap_or_default().to_variant());
                    }
                ));
        }
    }

    /// Acts on a controller intent for the panels; says whether they took it. Start, Guide
    /// and R3 open their panels in fullscreen mode only: in the desktop window Guide enters
    /// fullscreen mode.
    pub(crate) fn handle_panel_intent(&self, intent: Intent) -> bool {
        if matches!(intent, Intent::Menu | Intent::Home | Intent::QuickAccess)
            && !self.is_fullscreen()
        {
            return false;
        }
        let game = self.focused_game();
        let under = Under {
            game: game.is_some(),
            dialog: self.visible_dialog().is_some(),
            unread: self.has_unread_notifications(),
        };
        let mut panels = self.imp().panels.get();
        if !panels.handle(intent, under) {
            return false;
        }
        if let (Some(Panel::Options(_)), None) = (panels.open(), self.imp().panels.get().open())
            && let Some(game) = game
        {
            self.fill_options(&game);
        }
        self.set_panels(panels);
        true
    }

    /// The game a focused cover or card shows, or the shown game page's.
    pub(crate) fn focused_game(&self) -> Option<GameObject> {
        if let Some(page) = self.visible_game_page() {
            return page.game();
        }
        if let Some(game) = self.focused_tile().and_then(|tile| tile.game()) {
            return Some(game);
        }
        let imp = self.imp();
        let home = imp.navigation_view.visible_page().as_ref()
            == Some(imp.home_page.upcast_ref::<adw::NavigationPage>());
        home.then(|| imp.home_page.focused_game()).flatten()
    }

    /// The panel that is open, if any.
    pub(crate) fn open_panel(&self) -> Option<Panel> {
        self.imp().panels.get().open()
    }

    /// Opens the options sheet for the focused game, at `level`: the game page's Options
    /// and Add to Collection buttons.
    fn open_options(&self, level: OptionsLevel) {
        if self.visible_dialog().is_some() {
            return;
        }
        let Some(game) = self.focused_game() else {
            return;
        };
        self.fill_options(&game);
        self.show_panel(Panel::Options(level));
    }

    fn show_panel(&self, panel: Panel) {
        let mut panels = self.imp().panels.get();
        panels.show(panel);
        self.set_panels(panels);
    }

    /// Closes the open panel, giving focus back to where it was.
    pub(crate) fn close_panel(&self) {
        let mut panels = self.imp().panels.get();
        panels.close();
        self.set_panels(panels);
    }

    /// Nothing runs any more: Quick Access leaves its Activity for its entries.
    pub(crate) fn leave_activity(&self) {
        let mut panels = self.imp().panels.get();
        panels.activity_ended();
        self.set_panels(panels);
    }

    /// Shows `panels` as they now are: the drawer, Quick Access or the sheet, at its level,
    /// with focus inside the open one and the page under it out of focus's way, or back on
    /// the page.
    fn set_panels(&self, panels: Panels) {
        let imp = self.imp();
        let before = imp.panels.replace(panels).open();
        let open = panels.open();
        if before == open {
            return;
        }
        if before.is_none() {
            imp.focus_before_panel
                .set(gtk::prelude::GtkWindowExt::focus(self).as_ref());
        }
        if matches!(open, Some(Panel::Drawer(_))) && !matches!(before, Some(Panel::Drawer(_))) {
            self.fill_drawer();
        }
        match open {
            Some(Panel::QuickAccess(QuickAccessLevel::Notifications)) => self.show_notifications(),
            Some(Panel::QuickAccess(QuickAccessLevel::Activity)) => self.show_activity(),
            Some(Panel::QuickAccess(QuickAccessLevel::Designs)) => self.fill_designs(),
            Some(Panel::QuickAccess(QuickAccessLevel::Controllers)) => self.fill_controllers(),
            _ => {}
        }
        let drawer = matches!(open, Some(Panel::Drawer(_)));
        let quick_access = matches!(open, Some(Panel::QuickAccess(_)));
        let sheet = matches!(open, Some(Panel::Options(_)));
        // Directions stay inside the open panel.
        imp.split_view
            .set_can_focus(!drawer && !quick_access && !sheet);
        imp.drawer.set_can_focus(!sheet);
        imp.drawer.set_show_sidebar(drawer);
        imp.quick_access.set_show_sidebar(quick_access);
        imp.options_sheet.set_open(sheet);
        match open {
            Some(Panel::Drawer(level)) => imp.drawer_stack.set_visible_child_name(match level {
                DrawerLevel::Main => "main",
                DrawerLevel::Collections => "collections",
            }),
            Some(Panel::Options(level)) => imp.options_stack.set_visible_child_name(match level {
                OptionsLevel::Main => "main",
                OptionsLevel::Collections => "collections",
            }),
            Some(Panel::QuickAccess(level)) => {
                imp.quick_access_stack.set_visible_child_name(match level {
                    QuickAccessLevel::Main => "main",
                    QuickAccessLevel::Notifications => "notifications",
                    QuickAccessLevel::Activity => "activity",
                    QuickAccessLevel::Designs => "designs",
                    QuickAccessLevel::Controllers => "controllers",
                    QuickAccessLevel::Settings => "settings",
                });
            }
            None => {
                imp.options_game.take();
                let previous = imp.focus_before_panel.upgrade();
                imp.focus_before_panel.set(None);
                if !previous.is_some_and(|widget| widget.is_mapped() && widget.grab_focus()) {
                    self.focus_page();
                }
            }
        }
        if open.is_some() {
            // Once the panel is shown and can take focus.
            glib::idle_add_local_once(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move || window.focus_panel(before)
            ));
        }
        self.update_hints();
    }

    /// Focuses the open panel: the entry that led to the list being left, or else its
    /// first item (a list box would go back to the row it last had focus on).
    pub(crate) fn focus_panel(&self, left: Option<Panel>) {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        let target: Option<gtk::Widget> = match (self.open_panel(), left) {
            (
                Some(Panel::Drawer(DrawerLevel::Main)),
                Some(Panel::Drawer(DrawerLevel::Collections)),
            ) => Some(imp.drawer_collections_row.get().upcast()),
            (
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
                Some(Panel::QuickAccess(QuickAccessLevel::Notifications)),
            ) => Some(imp.quick_access_notifications_row.get().upcast()),
            // Activity, or the entry after it once nothing runs.
            (
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
                Some(Panel::QuickAccess(QuickAccessLevel::Activity)),
            ) => Some(self.activity_entry()),
            (
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
                Some(Panel::QuickAccess(QuickAccessLevel::Designs)),
            ) => Some(imp.quick_access_design_row.get().upcast()),
            (
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
                Some(Panel::QuickAccess(QuickAccessLevel::Controllers)),
            ) => Some(imp.quick_access_controllers_row.get().upcast()),
            (
                Some(Panel::QuickAccess(QuickAccessLevel::Main)),
                Some(Panel::QuickAccess(QuickAccessLevel::Settings)),
            ) => Some(imp.quick_access_settings_row.get().upcast()),
            // The first Playing Now card's Return comes first.
            (Some(Panel::Drawer(DrawerLevel::Main)), _) if imp.playing_cards.is_visible() => imp
                .playing_cards
                .first_child()
                .as_ref()
                .and_then(card_return),
            (Some(Panel::QuickAccess(QuickAccessLevel::Main)), _)
                if imp.quick_access_playing_cards.is_visible() =>
            {
                imp.quick_access_playing_cards
                    .first_child()
                    .as_ref()
                    .and_then(card_return)
            }
            (Some(Panel::QuickAccess(QuickAccessLevel::Main)), _) => {
                Some(imp.quick_access_notifications_row.get().upcast())
            }
            (Some(Panel::QuickAccess(QuickAccessLevel::Notifications)), _) => {
                first_row(&imp.quick_access_notifications)
            }
            (Some(Panel::QuickAccess(QuickAccessLevel::Activity)), _) => {
                first_row(&imp.quick_access_activity)
            }
            // The design in force.
            (Some(Panel::QuickAccess(QuickAccessLevel::Designs)), _) => {
                let checked = self.action_state("design");
                let mut rows =
                    std::iter::successors(imp.quick_access_designs.first_child(), |row| {
                        row.next_sibling()
                    });
                rows.find(|row| {
                    row.downcast_ref::<adw::ActionRow>()
                        .and_then(adw::ActionRow::activatable_widget)
                        .and_downcast::<gtk::CheckButton>()
                        .and_then(|check| check.action_target_value())
                        == checked
                })
                .or_else(|| first_row(&imp.quick_access_designs))
            }
            (
                Some(Panel::Options(OptionsLevel::Main)),
                Some(Panel::Options(OptionsLevel::Collections)),
            ) => {
                let index = imp
                    .game_options
                    .borrow()
                    .iter()
                    .position(|option| *option == GameOption::Collections);
                index
                    .and_then(|index| imp.options_list.row_at_index(index as i32))
                    .map(|row| row.upcast())
            }
            (Some(Panel::Drawer(DrawerLevel::Main)), _) => first_row(&imp.drawer_list),
            (Some(Panel::Drawer(DrawerLevel::Collections)), _) => {
                first_row(&imp.drawer_collections)
            }
            (Some(Panel::QuickAccess(QuickAccessLevel::Settings)), _) => {
                imp.tile_small.ancestor(gtk::ListBoxRow::static_type())
            }
            // The first controller that can rumble, or else the first button prompt.
            (Some(Panel::QuickAccess(QuickAccessLevel::Controllers)), _) => {
                std::iter::successors(imp.quick_access_controllers.first_child(), |row| {
                    row.next_sibling()
                })
                .find(|row| row.is::<gtk::ListBoxRow>() && row.is_sensitive())
                .or_else(|| first_row(&imp.quick_access_prompts))
            }
            (Some(Panel::Options(OptionsLevel::Main)), _) => first_row(&imp.options_list),
            (Some(Panel::Options(OptionsLevel::Collections)), _) => {
                first_row(&imp.options_collections)
            }
            (None, _) => None,
        };
        if let Some(target) = target {
            target.grab_focus();
        }
    }

    /// The drawer's collections. Its Playing Now cards are `fill_playing_cards`'s.
    fn fill_drawer(&self) {
        let imp = self.imp();
        let playing = self
            .application()
            .and_downcast::<PinduApplication>()
            .is_some_and(|application| application.session_service().any_playing());
        let collections = self.library().collections();
        let entries = drawer_entries(playing, !collections.is_empty());
        imp.drawer_collections_row
            .set_visible(entries.contains(&DrawerEntry::Collections));
        imp.drawer_collections.remove_all();
        for collection in &collections {
            let row = drawer_row("folder-symbolic", &collection.name);
            row.set_action_name(Some("win.drawer-collection"));
            row.set_action_target_value(Some(&collection.id.to_variant()));
            imp.drawer_collections.append(&row);
        }
    }

    /// Quick Access's designs: Default, then each installed design, with a check on the
    /// one in force. A design for a newer Pindu launcher shows but cannot be chosen.
    fn fill_designs(&self) {
        let imp = self.imp();
        let Some(application) = self.application().and_downcast::<PinduApplication>() else {
            return;
        };
        let service = application.style_service();
        imp.quick_access_designs.remove_all();
        for choice in design_choices(&service.designs(), &service.design()) {
            let check = gtk::CheckButton::builder()
                .action_name("win.design")
                .action_target(&choice.id.to_variant())
                .can_focus(false)
                .build();
            let subtitle = if choice.available {
                choice.description
            } else {
                needs_newer_launcher()
            };
            let row = adw::ActionRow::builder()
                .title(choice.name.unwrap_or_else(no_design_name))
                .subtitle(subtitle)
                .use_markup(false)
                .activatable_widget(&check)
                .sensitive(choice.available)
                .build();
            row.add_prefix(&check);
            imp.quick_access_designs.append(&row);
        }
    }

    /// Applies the design `id` (Adwaita, no design, for an empty one) unless it is in force
    /// already; Quick Access stays open.
    fn choose_design(&self, id: &str) {
        if self.design_in_force().as_deref() == Some(id) {
            return;
        }
        if let Some(application) = self.application().and_downcast::<PinduApplication>() {
            application.style_service().set_design(id);
        }
    }

    /// The id of the design Quick Access checks: the chosen one if it can apply, or empty
    /// for Adwaita.
    fn design_in_force(&self) -> Option<String> {
        let application = self.application().and_downcast::<PinduApplication>()?;
        let service = application.style_service();
        design_choices(&service.designs(), &service.design())
            .into_iter()
            .find(|choice| choice.active)
            .map(|choice| choice.id)
    }

    /// The sheet's title and actions for `game`, and its collections.
    fn fill_options(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_game.replace(Some(game.clone()));
        imp.options_title.set_label(&game.game().title);
        let options = game_options(&game.game(), game.is_playing());
        imp.options_list.remove_all();
        for option in &options {
            let (icon, label) = option_label(*option, game);
            let row = drawer_row(icon, &label);
            if *option == GameOption::Collections {
                append_next_arrow(&row);
            }
            imp.options_list.append(&row);
        }
        imp.game_options.replace(options);
        self.fill_options_collections(game);
    }

    /// One switch per collection, on while the game is in it, then "New Collection…".
    fn fill_options_collections(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_collections.remove_all();
        let id = game.id();
        let member_of = game.entry().collections.clone();
        for collection in self.library().collections() {
            let row = adw::SwitchRow::builder()
                .title(&collection.name)
                .use_markup(false)
                .active(member_of.contains(&collection.id))
                .build();
            let target = (id.as_str(), collection.id).to_variant();
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |row| {
                    let action = if row.is_active() {
                        "add-to-collection"
                    } else {
                        "remove-from-collection"
                    };
                    ActionGroupExt::activate_action(&window, action, Some(&target));
                }
            ));
            imp.options_collections.append(&row);
        }
        let new = adw::ButtonRow::builder()
            .title(gettext("_New Collection…"))
            .use_underline(true)
            .start_icon_name("list-add-symbolic")
            .action_name("win.new-collection")
            .action_target(&id.as_str().to_variant())
            .build();
        imp.options_collections.append(&new);
    }

    /// Does what the sheet's entry says, for its game: Collections leads to the game's
    /// collections; every other entry closes the sheet first.
    fn choose_option(&self, option: GameOption) {
        let Some(game) = self.imp().options_game.borrow().clone() else {
            return;
        };
        if option == GameOption::Collections {
            self.show_panel(Panel::Options(OptionsLevel::Collections));
            return;
        }
        self.close_panel();
        let target = game.id().as_str().to_variant();
        let (group, action) = match option {
            GameOption::Play => ("app", "play"),
            GameOption::Stop => ("app", "stop"),
            GameOption::AddToFavourites | GameOption::RemoveFromFavourites => {
                ("win", "toggle-favourite")
            }
            GameOption::Hide => ("win", "hide-game"),
            GameOption::Unhide => ("win", "unhide-game"),
            GameOption::OpenInLauncher => ("app", "open-in-launcher"),
            GameOption::Collections => return,
        };
        if group == "app" {
            if let Some(application) = self.application() {
                application.activate_action(action, Some(&target));
            }
        } else {
            ActionGroupExt::activate_action(self, action, Some(&target));
        }
    }
}

/// What Quick Access calls the choice of no design: Adwaita, the stock libadwaita look it
/// leaves in force. A name, so it is not translated.
fn no_design_name() -> String {
    "Adwaita".to_owned()
}

/// Why Quick Access cannot apply a design written for a newer style contract.
fn needs_newer_launcher() -> String {
    gettext("Needs a newer Pindu launcher")
}

/// The icon and label of an options sheet entry for `game`.
fn option_label(option: GameOption, game: &GameObject) -> (&'static str, String) {
    match option {
        GameOption::Play => ("media-playback-start-symbolic", gettext("Play")),
        GameOption::Stop => ("media-playback-stop-symbolic", gettext("Stop")),
        GameOption::AddToFavourites => ("non-starred-symbolic", gettext("Add to Favourites")),
        GameOption::RemoveFromFavourites => ("starred-symbolic", gettext("Remove from Favourites")),
        GameOption::Collections => ("folder-symbolic", gettext("Collections")),
        GameOption::Hide => ("view-conceal-symbolic", gettext("Hide")),
        GameOption::Unhide => ("view-reveal-symbolic", gettext("Unhide")),
        GameOption::OpenInLauncher => (
            "send-to-symbolic",
            launcher_name(game.game().source()).map_or_else(
                || gettext("Open in Launcher"),
                |launcher| gettext("Open in {launcher}").replace("{launcher}", launcher),
            ),
        ),
    }
}

/// A row of the drawer or the sheet: an icon and a label.
fn drawer_row(icon: &str, label: &str) -> gtk::ListBoxRow {
    let content = gtk::Box::builder().spacing(16).build();
    content.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build(),
    );
    content.append(
        &gtk::Label::builder()
            .label(label)
            .xalign(0.0)
            .hexpand(true)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .build(),
    );
    gtk::ListBoxRow::builder().child(&content).build()
}

/// The arrow of a row that leads to a list.
fn append_next_arrow(row: &gtk::ListBoxRow) {
    if let Some(content) = row.child().and_downcast::<gtk::Box>() {
        content.append(
            &gtk::Image::builder()
                .icon_name("go-next-symbolic")
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
    }
}

/// Moves focus from the end of `list` to the nearest widget above or below it that takes
/// focus, as `direction` says: a list box keeps Up and Down to itself, ringing the bell at
/// its ends.
fn step_out_of_list(list: &gtk::ListBox, direction: gtk::DirectionType) {
    let next = |widget: &gtk::Widget| match direction {
        gtk::DirectionType::Down => widget.next_sibling(),
        gtk::DirectionType::Up => widget.prev_sibling(),
        _ => None,
    };
    let mut sibling = next(list.upcast_ref());
    while let Some(widget) = sibling {
        if widget.is_visible() && widget.child_focus(direction) {
            return;
        }
        sibling = next(&widget);
    }
}
