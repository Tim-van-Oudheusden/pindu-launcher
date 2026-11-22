//! Fullscreen mode's panels over the pages: the menu drawer on the left (Start or Guide),
//! Quick Access on the right (Guide + A, R3 or the Steam Deck's "…" button) and the
//! options sheet at the bottom (View or X, or a game page's Options and Add to Collection
//! buttons), whose levels list a game's collections, the games to merge it with and a
//! group's copies. Which panel is open, at which level, and what the controller does to
//! them is `pindu_core::panels`; this fills the drawer and the sheet, shows the panels,
//! keeps focus inside the open one and gives it back after, and does what their entries
//! say. They are widgets in the window rather than popover menus, which the compositor
//! dismisses on a controller press. Quick Access is a widget of its own, `PinduQuickAccess`
//! (`quick_access`), which the window hosts as the sidebar of its `quick_access` split
//! view: the window tells it which level and section show and what lies under it, and does
//! what it asks for. The drawer's Power level is `fullscreen_power`'s.

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::failed_launch::Step;
use pindu_core::gamepad::Intent;
use pindu_core::keys;
use pindu_core::launch;
use pindu_core::library::View;
use pindu_core::panels::{
    DrawerLevel, GameOption, OptionsLevel, Panel, Panels, QuickAccessLevel, QuickAccessSection,
    Under, page_entries, sheet_options, with_save_snapshots,
};

use crate::application::PinduApplication;
use crate::failed_launch;
use crate::launch_service::launcher_name;
use crate::merge_dialog::candidate_rows;
use crate::models::GameObject;
use crate::outcome_service;
use crate::save_service;
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
        // Closes the menu and searches with the on-screen keyboard, as Y does.
        let search = gio::ActionEntry::builder("drawer-search")
            .activate(|window: &Self, _, _| window.search_from_menu())
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
        self.add_action_entries([
            close,
            home,
            library,
            search,
            collections,
            collection,
            options,
            game_collections,
            quick_access,
        ]);
        // Up and Down stop at a list's ends; in the options sheet they go on to the next
        // list (`fullscreen_playing` takes the drawer's own list up to its Playing Now
        // cards, and Quick Access does its own).
        for list in [
            imp.options_steps.get(),
            imp.options_list.get(),
            imp.options_performance.get(),
        ] {
            list.connect_keynav_failed(|list, direction| {
                step_out_of_list(list, direction);
                glib::Propagation::Stop
            });
        }
        // Escape closes a panel rather than leaving fullscreen mode.
        for panel in [
            imp.drawer_panel.upcast_ref::<gtk::Widget>(),
            imp.options_panel.upcast_ref(),
        ] {
            let escape = gtk::ShortcutController::new();
            escape.add_shortcut(gtk::Shortcut::new(
                gtk::ShortcutTrigger::parse_string(&keys::ESCAPE.accelerator()),
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
        imp.options_steps.connect_row_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let step = usize::try_from(row.index()).ok().and_then(|index| {
                    let failure = window.imp().options_failure_explanation.borrow();
                    failure.as_ref()?.steps.get(index).copied()
                });
                if let Some(step) = step {
                    window.choose_step(step);
                }
            }
        ));
        imp.options_merge.connect_row_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let imp = window.imp();
                let other = usize::try_from(row.index())
                    .ok()
                    .and_then(|index| imp.merge_candidates.borrow().get(index).cloned());
                let game = imp.options_game.borrow().as_ref().map(GameObject::id);
                if let (Some(game), Some(other)) = (game, other) {
                    window.close_panel();
                    window.merge_games(&game, &other);
                }
            }
        ));
        imp.options_copies.connect_row_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, row| {
                let copy = usize::try_from(row.index()).ok().and_then(|index| {
                    let game = window.imp().options_game.borrow().clone()?;
                    let entry = game.entry();
                    entry.copies.get(index).map(|copy| copy.game.id.clone())
                });
                if let Some(copy) = copy {
                    window.close_panel();
                    window.prefer_copy(&copy);
                }
            }
        ));
        self.library().connect_collections_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.fill_drawer();
                window.follow_collections();
                if let Some(game) = window.imp().options_game.borrow().as_ref() {
                    window.fill_options_collections(game);
                }
            }
        ));
        // Library's and the collections' numbers of games.
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.fill_drawer()
        ));
        if let Some(application) = self.application().and_downcast::<PinduApplication>() {
            let gamepad = application.gamepad_service();
            imp.drawer_panel.show_search_button(gamepad.active_layout());
            gamepad.connect_active_layout_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |layout| window.imp().drawer_panel.show_search_button(layout)
            ));
        }
        self.setup_add_store();
        self.setup_fullscreen_add_game();
        self.setup_power();
        self.fill_drawer();
    }

    /// Hosts Quick Access: it takes its services from `application` and adds its rows'
    /// actions to the window, and the window does what it asks for. The options sheet's
    /// performance switches show the same state as Quick Access's.
    pub(crate) fn setup_quick_access(&self, application: &PinduApplication) {
        let imp = self.imp();
        let panel = &imp.quick_access_panel;
        panel.setup(application, self);
        panel.connect_close_requested(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.close_panel()
        ));
        panel.connect_level_requested(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, level| window.show_panel(Panel::QuickAccess(level))
        ));
        panel.connect_section_requested(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, section| window.show_quick_access_section(section)
        ));
        panel.connect_game_requested(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, id| {
                window.close_panel();
                window.return_to_game(id);
            }
        ));
        panel.connect_filters_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.apply_filters()
        ));
        if let Some(switches) = panel.performance() {
            switches.attach(&imp.options_overlay_row, &imp.options_gamemode_row);
            // Outside fullscreen mode the game page edits the same settings, so they are
            // read afresh the next time.
            self.connect_fullscreened_notify(move |window| {
                if !window.is_fullscreen() {
                    switches.forget();
                }
            });
        }
    }

    /// Acts on a controller intent for the panels; says whether they took it. Start, Guide,
    /// R3 and Guide + A open their panels in fullscreen mode only: in the desktop window
    /// Guide enters fullscreen mode.
    pub(crate) fn handle_panel_intent(&self, intent: Intent) -> bool {
        if matches!(
            intent,
            Intent::Menu | Intent::Home | Intent::QuickAccess | Intent::SystemQuickAccess
        ) && !self.is_fullscreen()
        {
            return false;
        }
        let game = self.focused_game();
        let imp = self.imp();
        let collection = match self.open_panel() {
            Some(Panel::Drawer(DrawerLevel::Main | DrawerLevel::Collections)) => {
                imp.drawer_panel.focused_collection()
            }
            _ => None,
        };
        let under = Under {
            game: game.is_some(),
            own_game: game.as_ref().is_some_and(own_game),
            dialog: self.visible_dialog().is_some(),
            unread: imp.quick_access_panel.has_unread_notifications(),
            library: self.library_under_panels(),
            collection: collection.is_some(),
            shown: imp.quick_access_panel.shown(),
        };
        let mut panels = imp.panels.get();
        if !panels.handle(intent, under) {
            return false;
        }
        match (panels.open(), imp.panels.get().open()) {
            (Some(Panel::Options(_)), None) => {
                if let Some(game) = game {
                    self.fill_options(&game);
                }
            }
            (
                Some(Panel::Drawer(DrawerLevel::Collection)),
                Some(Panel::Drawer(DrawerLevel::Main | DrawerLevel::Collections)),
            ) => {
                if let Some(id) = collection {
                    imp.drawer_panel
                        .show_collection(&self.library().collections(), id);
                }
            }
            _ => {}
        }
        self.set_panels(panels);
        true
    }

    /// Keeps the drawer's collection levels in step with the collections: the shown
    /// collection's actions after it moved or was renamed, or, once it is deleted, where
    /// its actions were opened from (the entries, or the collections); and the entries
    /// once no collection is left. The entries' own rows are `fill_drawer`'s.
    fn follow_collections(&self) {
        let imp = self.imp();
        let drawer = &imp.drawer_panel;
        let collections = self.library().collections();
        let mut panels = imp.panels.get();
        match panels.open() {
            Some(Panel::Drawer(DrawerLevel::Collection)) => {
                let shown = drawer.shown_collection();
                if shown.is_some_and(|id| drawer.show_collection(&collections, id)) {
                    return;
                }
                panels.handle(Intent::Back, Under::default());
            }
            Some(Panel::Drawer(DrawerLevel::Collections)) => {}
            _ => return,
        }
        if collections.is_empty() {
            panels.show(Panel::Drawer(DrawerLevel::Main));
        }
        self.set_panels(panels);
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

    /// Whether fullscreen mode's library is the page under the panels.
    fn library_under_panels(&self) -> bool {
        let imp = self.imp();
        self.is_fullscreen()
            && imp.navigation_view.visible_page().as_ref() == Some(&*imp.library_page)
    }

    /// Tells Quick Access, as it opens, which entries the page under it adds, and the
    /// focused game, whose performance switches it shows.
    fn show_page_entries(&self) {
        let game = self.focused_game();
        let under = Under {
            game: game.is_some(),
            own_game: game.as_ref().is_some_and(own_game),
            library: self.library_under_panels(),
            ..Under::default()
        };
        self.imp()
            .quick_access_panel
            .set_page(&page_entries(under), game.as_ref());
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

    /// Opens the options sheet for `game` at the games to merge it with.
    pub(crate) fn open_merge_options(&self, game: &GameObject) {
        if self.visible_dialog().is_some() {
            return;
        }
        self.fill_options(game);
        self.fill_options_merge(game);
        self.show_panel(Panel::Options(OptionsLevel::Merge));
    }

    pub(crate) fn show_panel(&self, panel: Panel) {
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

    /// Shows Quick Access's `section`: the pointer pressed its toggle in the strip.
    fn show_quick_access_section(&self, section: QuickAccessSection) {
        let mut panels = self.imp().panels.get();
        panels.show_section(section);
        self.set_panels(panels);
    }

    /// Shows `panels` as they now are: the drawer, Quick Access or the sheet, at its level
    /// (and Quick Access's section), with focus inside the open one and the page under it
    /// out of focus's way, or back on the page.
    fn set_panels(&self, panels: Panels) {
        let imp = self.imp();
        let replaced = imp.panels.replace(panels);
        let before = replaced.open();
        let open = panels.open();
        let same_section = open != Some(Panel::QuickAccess(QuickAccessLevel::Main))
            || replaced.section() == panels.section();
        if before == open && same_section {
            return;
        }
        if before.is_none() {
            imp.focus_before_panel
                .set(gtk::prelude::GtkWindowExt::focus(self).as_ref());
        }
        if matches!(open, Some(Panel::Drawer(_))) && !matches!(before, Some(Panel::Drawer(_))) {
            self.fill_drawer();
        }
        let quick_access_level = match open {
            Some(Panel::QuickAccess(level)) => Some(level),
            _ => None,
        };
        if quick_access_level.is_some() && !matches!(before, Some(Panel::QuickAccess(_))) {
            self.show_page_entries();
        }
        if open == Some(Panel::Drawer(DrawerLevel::Power)) {
            self.show_power();
        }
        let drawer = matches!(open, Some(Panel::Drawer(_)));
        let quick_access = quick_access_level.is_some();
        let sheet = matches!(open, Some(Panel::Options(_)));
        // Directions stay inside the open panel.
        imp.split_view
            .set_can_focus(!drawer && !quick_access && !sheet);
        imp.drawer.set_can_focus(!sheet);
        imp.drawer.set_show_sidebar(drawer);
        if quick_access {
            self.fit_quick_access(quick_access_level);
        }
        imp.quick_access.set_show_sidebar(quick_access);
        imp.options_sheet.set_open(sheet);
        // After the sidebar shows, as the split view gives focus back into it.
        imp.quick_access_panel
            .show_level(quick_access_level, panels.section());
        match open {
            Some(Panel::Drawer(level)) => imp.drawer_panel.show_level(level),
            Some(Panel::Options(level)) => imp.options_stack.set_visible_child_name(match level {
                OptionsLevel::Main => "main",
                OptionsLevel::Collections => "collections",
                OptionsLevel::Merge => "merge",
                OptionsLevel::Copies => "copies",
            }),
            Some(Panel::QuickAccess(_)) => {}
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
    /// first item (a list box would go back to the row it last had focus on). Quick Access
    /// focuses its own.
    pub(crate) fn focus_panel(&self, left: Option<Panel>) {
        let imp = self.imp();
        if let Some(Panel::QuickAccess(_)) = self.open_panel() {
            let left = match left {
                Some(Panel::QuickAccess(level)) => Some(level),
                _ => None,
            };
            imp.quick_access_panel.focus_level(left);
            return;
        }
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        let target: Option<gtk::Widget> = match (self.open_panel(), left) {
            (Some(Panel::Drawer(level)), left) => {
                let left = match left {
                    Some(Panel::Drawer(level)) => Some(level),
                    _ => None,
                };
                imp.drawer_panel.focus_target(level, left)
            }
            (Some(Panel::Options(OptionsLevel::Main)), Some(Panel::Options(level))) => {
                let entry = match level {
                    OptionsLevel::Collections => Some(GameOption::Collections),
                    OptionsLevel::Merge => Some(GameOption::MergeWith),
                    OptionsLevel::Copies => Some(GameOption::PreferredCopy),
                    OptionsLevel::Main => None,
                };
                let index = imp
                    .game_options
                    .borrow()
                    .iter()
                    .position(|option| Some(*option) == entry);
                index
                    .and_then(|index| imp.options_list.row_at_index(index as i32))
                    .map(|row| row.upcast())
                    .or_else(|| first_row(&imp.options_list))
            }
            (Some(Panel::Options(OptionsLevel::Main)), _) => first_row(&imp.options_list),
            (Some(Panel::Options(OptionsLevel::Collections)), _) => {
                first_row(&imp.options_collections)
            }
            (Some(Panel::Options(OptionsLevel::Merge)), _) => first_row(&imp.options_merge),
            (Some(Panel::Options(OptionsLevel::Copies)), _) => first_row(&imp.options_copies),
            (Some(Panel::QuickAccess(_)) | None, _) => None,
        };
        if let Some(target) = target {
            target.grab_focus();
        }
    }

    /// The drawer's entries as the library now is, with its Power entry. Its Playing Now
    /// cards are `fullscreen_playing`'s.
    fn fill_drawer(&self) {
        let application = self.application().and_downcast::<PinduApplication>();
        let power = application.as_ref().is_some_and(|application| {
            let state = application.system_power_service().state();
            !self.power_level_entries(&state).is_empty()
        });
        self.imp().drawer_panel.fill(self.library(), power);
    }

    /// The sheet's title and actions for `game`, its performance switches for a game we
    /// run ourselves, its collections and a group's copies.
    fn fill_options(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_game.replace(Some(game.clone()));
        imp.options_title.set_label(&game.game().title);
        let copies = game.entry().copies.len();
        let mut options = sheet_options(&game.game(), game.playing(), copies);
        if !save_service::save_folders(&game.id()).is_empty() {
            options = with_save_snapshots(options);
        }
        imp.options_list.remove_all();
        for option in &options {
            let (icon, label) = option_label(*option, game);
            let row = drawer_row(icon, &label);
            if matches!(
                option,
                GameOption::Collections | GameOption::MergeWith | GameOption::PreferredCopy
            ) {
                append_next_arrow(&row);
            }
            imp.options_list.append(&row);
        }
        imp.game_options.replace(options);
        let performance = own_game(game);
        imp.options_performance.set_visible(performance);
        if performance && let Some(switches) = imp.quick_access_panel.performance() {
            switches.show_for(game);
        }
        self.fill_options_collections(game);
        self.fill_options_copies(game);
        self.fill_options_failure(game);
    }

    /// The games to merge `game` with, likely copies first, each with where it comes from
    /// or how many copies it has.
    fn fill_options_merge(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_merge.remove_all();
        let rows = candidate_rows(game, &self.library().all_games(), "");
        let mut ids = Vec::with_capacity(rows.len());
        for (id, row) in rows {
            imp.options_merge.append(&row);
            ids.push(id);
        }
        imp.merge_candidates.replace(ids);
    }

    /// A group's copies, the one it shows and Play starts marked.
    fn fill_options_copies(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_copies.remove_all();
        for (index, copy) in game.entry().copies.iter().enumerate() {
            imp.options_copies
                .append(&crate::group_actions::copy_choice_row(copy, index == 0));
        }
    }

    /// One switch per collection kept by hand, on while the game is in it, then "New
    /// Collection…". Saved filters pick their games themselves.
    fn fill_options_collections(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_collections.remove_all();
        let id = game.id();
        let member_of = game.entry().collections.clone();
        for collection in self.library().manual_collections() {
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

    /// Does what the sheet's entry says, for its game: Collections, Merge With and
    /// Preferred Copy lead to their lists; every other entry closes the sheet first.
    fn choose_option(&self, option: GameOption) {
        let Some(game) = self.imp().options_game.borrow().clone() else {
            return;
        };
        let level = match option {
            GameOption::Collections => Some(OptionsLevel::Collections),
            GameOption::MergeWith => {
                self.fill_options_merge(&game);
                Some(OptionsLevel::Merge)
            }
            GameOption::PreferredCopy => Some(OptionsLevel::Copies),
            _ => None,
        };
        if let Some(level) = level {
            self.show_panel(Panel::Options(level));
            return;
        }
        self.close_panel();
        let target = game.id().as_str().to_variant();
        let (group, action) = match option {
            GameOption::Play => ("app", "play"),
            GameOption::StartSafely => ("app", "start-safely"),
            GameOption::Stop => ("app", "stop"),
            GameOption::Return => ("win", "playing-return"),
            GameOption::AddToFavourites | GameOption::RemoveFromFavourites => {
                ("win", "toggle-favourite")
            }
            GameOption::Hide => ("win", "hide-game"),
            GameOption::Unhide => ("win", "unhide-game"),
            GameOption::OpenInLauncher => ("app", "open-in-launcher"),
            GameOption::ProtonDb => ("app", "open-protondb"),
            GameOption::PcGamingWiki => ("app", "open-pcgamingwiki"),
            GameOption::ChangeArtwork => ("win", "change-artwork"),
            GameOption::SaveSnapshots => ("win", "save-snapshots"),
            GameOption::Split => ("win", "split-group"),
            GameOption::Collections | GameOption::MergeWith | GameOption::PreferredCopy => return,
        };
        if group == "app" {
            if let Some(application) = self.application() {
                application.activate_action(action, Some(&target));
            }
        } else {
            ActionGroupExt::activate_action(self, action, Some(&target));
        }
    }

    /// Shows why `game` last failed to launch, and the next steps that exist for it, above
    /// the sheet's actions once read; or nothing when it did not fail.
    fn fill_options_failure(&self, game: &GameObject) {
        let imp = self.imp();
        imp.options_failure.set_visible(false);
        imp.options_steps.set_visible(false);
        imp.options_steps.remove_all();
        imp.options_failure_explanation.take();
        let (window, id, game) = (self.downgrade(), game.id(), game.game().clone());
        glib::spawn_future_local(async move {
            let failure = outcome_service::last_failure(&game).await;
            let Some(window) = window.upgrade() else {
                return;
            };
            let imp = window.imp();
            let still_open = imp.options_game.borrow().as_ref().map(GameObject::id) == Some(id);
            let Some(failure) = failure.filter(|_| still_open) else {
                return;
            };
            imp.options_failure
                .set_label(&failed_launch::reason_text(&failure.reason, &game));
            imp.options_failure.set_visible(true);
            for &step in &failure.steps {
                imp.options_steps.append(&drawer_row(
                    failed_launch::step_icon(step),
                    &failed_launch::step_label(step),
                ));
            }
            imp.options_steps.set_visible(!failure.steps.is_empty());
            imp.options_failure_explanation.replace(Some(failure));
        });
    }

    /// Takes the next step `step` after the sheet's game failed to launch, closing the
    /// sheet first: another Proton build is chosen in Quick Access's Performance level.
    fn choose_step(&self, step: Step) {
        let Some(game) = self
            .imp()
            .options_game
            .borrow()
            .as_ref()
            .map(GameObject::id)
        else {
            return;
        };
        let log = self
            .imp()
            .options_failure_explanation
            .borrow()
            .as_ref()
            .and_then(|failure| failure.log.clone());
        self.close_panel();
        match failed_launch::step_action(step, &game, log.as_deref()) {
            Some((action, target)) => {
                if let Err(error) = WidgetExt::activate_action(self, action, target.as_ref()) {
                    glib::g_warning!(
                        "pindu",
                        "Could not take a step after a failed launch: {error}"
                    );
                }
            }
            None => self.show_panel(Panel::QuickAccess(QuickAccessLevel::Performance)),
        }
    }
}

/// Whether `game` is one we run ourselves, which has performance switches.
fn own_game(game: &GameObject) -> bool {
    launch::offers_performance(&game.game())
}

/// The icon and label of an options sheet entry for `game`.
fn option_label(option: GameOption, game: &GameObject) -> (&'static str, String) {
    match option {
        GameOption::Play => ("media-playback-start-symbolic", gettext("Play")),
        GameOption::StartSafely => ("security-high-symbolic", gettext("Start Safely")),
        GameOption::Stop => ("media-playback-stop-symbolic", gettext("Stop")),
        GameOption::Return => ("media-playback-start-symbolic", gettext("Return")),
        GameOption::AddToFavourites => ("non-starred-symbolic", gettext("Add to Favourites")),
        GameOption::RemoveFromFavourites => ("starred-symbolic", gettext("Remove from Favourites")),
        GameOption::Collections => ("folder-symbolic", gettext("Collections")),
        GameOption::ChangeArtwork => ("image-x-generic-symbolic", gettext("Change Artwork")),
        GameOption::MergeWith => ("list-add-symbolic", gettext("Merge With")),
        GameOption::PreferredCopy => ("edit-copy-symbolic", gettext("Preferred Copy")),
        GameOption::Split => ("edit-cut-symbolic", gettext("Split Into Separate Games")),
        GameOption::SaveSnapshots => ("document-save-symbolic", gettext("Save Snapshots")),
        GameOption::Hide => ("view-conceal-symbolic", gettext("Hide")),
        GameOption::Unhide => ("view-reveal-symbolic", gettext("Unhide")),
        GameOption::OpenInLauncher => (
            "send-to-symbolic",
            launcher_name(game.game().source()).map_or_else(
                || gettext("Open in Launcher"),
                |launcher| gettext("Open in {launcher}").replace("{launcher}", launcher),
            ),
        ),
        GameOption::ProtonDb => ("adw-external-link-symbolic", gettext("View on ProtonDB")),
        GameOption::PcGamingWiki => (
            "adw-external-link-symbolic",
            gettext("View on PCGamingWiki"),
        ),
    }
}

/// A row of the drawer or the sheet: an icon and a label.
pub(crate) fn drawer_row(icon: &str, label: &str) -> gtk::ListBoxRow {
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
pub(crate) fn step_out_of_list(list: &gtk::ListBox, direction: gtk::DirectionType) {
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
