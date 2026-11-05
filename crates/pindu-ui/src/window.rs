use std::cell::{Cell, RefCell};

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gdk, gio, glib};
use pindu_core::focus::FocusMemory;
use pindu_core::gamepad::DeviceKind;
use pindu_core::grid;
use pindu_core::hints::Hint;
use pindu_core::library::{
    Counts, Filters, HistoryGroup, PlaytimeOrigin, SortMode, View, history_group,
};
use pindu_core::model::{GameId, Source};

use crate::application::PinduApplication;
use crate::config::APP_ID;
use crate::fullscreen::NavigationSounds;
use crate::game_page::GamePage;
use crate::home_page::HomePage;
use crate::library_service::LibraryService;
use crate::models::{GameObject, LibraryModel};
use crate::sources::{source_icon, source_name};
use crate::sync_service::SyncService;
use crate::widgets::{CoverArt, CoverTile, TileSize};

mod imp {
    use std::cell::OnceCell;

    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/window.ui")]
    pub struct PinduWindow {
        #[template_child]
        pub hint_toolbar: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub hint_bar: TemplateChild<gtk::Box>,
        #[template_child]
        pub toast_overlay: TemplateChild<adw::ToastOverlay>,
        #[template_child]
        pub split_view: TemplateChild<adw::OverlaySplitView>,
        #[template_child]
        pub sidebar_toolbar: TemplateChild<adw::ToolbarView>,
        #[template_child]
        pub navigation_view: TemplateChild<adw::NavigationView>,
        #[template_child]
        pub sidebar: TemplateChild<adw::Sidebar>,
        #[template_child]
        pub all_item: TemplateChild<adw::SidebarItem>,
        #[template_child]
        pub installed_item: TemplateChild<adw::SidebarItem>,
        #[template_child]
        pub favourites_item: TemplateChild<adw::SidebarItem>,
        #[template_child]
        pub history_item: TemplateChild<adw::SidebarItem>,
        #[template_child]
        pub all_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub installed_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub favourites_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub history_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub sources_section: TemplateChild<adw::SidebarSection>,
        #[template_child]
        pub collections_section: TemplateChild<adw::SidebarSection>,
        #[template_child]
        pub jump_back_in: TemplateChild<gtk::Box>,
        #[template_child]
        pub history_groups: TemplateChild<gtk::Box>,
        #[template_child]
        pub jump_cards: TemplateChild<gtk::Box>,
        #[template_child]
        pub new_collection_item: TemplateChild<adw::SidebarItem>,
        #[template_child]
        pub library_page: TemplateChild<adw::NavigationPage>,
        #[template_child]
        pub home_page: TemplateChild<HomePage>,
        #[template_child]
        pub sync_spinner: TemplateChild<adw::Spinner>,
        #[template_child]
        pub empty_page: TemplateChild<adw::StatusPage>,
        #[template_child]
        pub search_entry: TemplateChild<gtk::SearchEntry>,
        #[template_child]
        pub grid: TemplateChild<gtk::GridView>,
        #[template_child]
        pub content_stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub library_tabs: TemplateChild<gtk::Box>,
        #[template_child]
        pub tab_scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub tab_row: TemplateChild<gtk::Box>,
        #[template_child]
        pub sort_label: TemplateChild<gtk::Label>,
        #[template_child]
        pub filter_row: TemplateChild<adw::WrapBox>,
        #[template_child]
        pub installed_filter: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub not_installed_filter: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub favourites_filter: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub windows_filter: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub native_filter: TemplateChild<gtk::ToggleButton>,
        #[template_child]
        pub clear_filters: TemplateChild<gtk::Button>,
        pub settings: OnceCell<gio::Settings>,
        pub library: OnceCell<LibraryModel>,
        /// Every sidebar row with the view it selects and, for sources, its count label.
        pub views: RefCell<Vec<(adw::SidebarItem, View, gtk::Label)>>,
        /// The collection whose sidebar menu is open.
        pub menu_collection: Cell<Option<i64>>,
        /// Whether the grid was scrolled to the top when the current re-sort began.
        pub grid_at_top: Cell<bool>,
        /// The cover the controller was last on, where it comes back into the grid.
        pub grid_cursor: Cell<u32>,
        /// The cover the controller was last on in each view, for fullscreen mode's tabs.
        pub focus_memory: RefCell<FocusMemory<View, GameId>>,
        /// Fullscreen mode's navigation sounds, made on entering the mode with them on.
        pub sounds: OnceCell<NavigationSounds>,
        /// The hints the hint bar shows, and for which kind of controller.
        pub hints: RefCell<Option<(DeviceKind, Vec<Hint>)>>,
        /// Whether the sidebar showed before fullscreen mode hid it, to show it again after.
        pub sidebar_before_hiding: Cell<Option<bool>>,
        /// Fullscreen mode's library tabs, each with the view it shows.
        pub tabs: RefCell<Vec<(View, gtk::ToggleButton)>>,
        /// Whether the first cover takes focus once the current re-sort is done, after
        /// the triggers changed the order.
        pub focus_first_after_sort: Cell<bool>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduWindow {
        const NAME: &'static str = "PinduWindow";
        type Type = super::PinduWindow;
        type ParentType = adw::ApplicationWindow;

        fn class_init(klass: &mut Self::Class) {
            HomePage::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduWindow {
        fn constructed(&self) {
            self.parent_constructed();
            self.obj().restore_window_state();
            // The app icon's name is the app ID, which differs in development builds.
            self.empty_page.set_icon_name(Some(APP_ID));
        }
    }

    impl WidgetImpl for PinduWindow {}

    impl WindowImpl for PinduWindow {
        fn close_request(&self) -> glib::Propagation {
            self.obj().save_window_state();
            self.parent_close_request()
        }
    }

    impl ApplicationWindowImpl for PinduWindow {}
    impl AdwApplicationWindowImpl for PinduWindow {}
}

glib::wrapper! {
    pub struct PinduWindow(ObjectSubclass<imp::PinduWindow>)
        @extends gtk::Widget, gtk::Window, gtk::ApplicationWindow, adw::ApplicationWindow,
        @implements gio::ActionGroup, gio::ActionMap, gtk::Accessible, gtk::Buildable,
                    gtk::ConstraintTarget, gtk::Native, gtk::Root, gtk::ShortcutManager;
}

impl PinduWindow {
    pub fn new(
        application: &adw::Application,
        sync_service: &SyncService,
        library: &LibraryModel,
    ) -> Self {
        let window: Self = glib::Object::builder()
            .property("application", application)
            .build();
        window
            .imp()
            .library
            .set(library.clone())
            .expect("a new window has no library");
        window.setup_actions();
        window.setup_sidebar();
        window.setup_grid();
        window.setup_search_and_filters();
        window.observe_sync(sync_service);
        window.setup_home();
        window.setup_fullscreen_library();
        window.setup_fullscreen();
        if let Some(application) = application.downcast_ref::<PinduApplication>() {
            window.setup_controller_navigation(application);
            window.setup_hint_bar(application);
        }
        window
    }

    pub fn search_entry(&self) -> gtk::SearchEntry {
        self.imp().search_entry.get()
    }

    pub fn library(&self) -> &LibraryModel {
        self.imp().library.get().expect("set on construction")
    }

    fn setup_actions(&self) {
        let imp = self.imp();
        let show_sidebar =
            gio::PropertyAction::new("show-sidebar", &*imp.split_view, "show-sidebar");
        self.add_action(&show_sidebar);

        // Alt+Left (and the Back key) leave a game page for the library.
        let back = gio::ActionEntry::builder("back")
            .activate(|window: &Self, _, _| {
                window.imp().navigation_view.pop();
            })
            .build();
        self.add_action_entries([back]);

        let sort_mode = gio::SimpleAction::new_stateful(
            "sort-mode",
            Some(glib::VariantTy::STRING),
            &sort_mode_name(SortMode::default()).to_variant(),
        );
        sort_mode.connect_change_state(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |action, state| {
                let Some(mode) = state
                    .and_then(|state| state.str())
                    .and_then(sort_mode_from_name)
                else {
                    return;
                };
                window.scroll_grid_to_top();
                window.library().set_sort_mode(mode);
                action.set_state(&sort_mode_name(mode).to_variant());
            }
        ));
        self.add_action(&sort_mode);

        self.add_game_action("hide-game", Self::hide_game);
        self.add_game_action("unhide-game", Self::unhide_game);
        self.add_game_action("remove-game", Self::remove_game);
        let show_hidden = gio::SimpleAction::new_stateful("show-hidden", None, &false.to_variant());
        show_hidden.connect_change_state(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |action, state| {
                let Some(show) = state.and_then(bool::from_variant) else {
                    return;
                };
                action.set_state(&show.to_variant());
                window.show_hidden_games(show);
            }
        ));
        self.add_action(&show_hidden);
        self.add_game_action("toggle-favourite", Self::toggle_favourite);
        self.add_game_action("toggle-pin", Self::toggle_pin);
        self.add_game_action("show-details", Self::show_details);
        self.add_game_action("open-install-folder", Self::open_install_folder);

        self.setup_collection_actions();

        let search = gio::ActionEntry::builder("search")
            .activate(|window: &Self, _, _| {
                if window.is_fullscreen() {
                    window.start_search();
                } else {
                    window.imp().search_entry.grab_focus();
                }
            })
            .build();
        self.add_action_entries([search]);
    }

    /// Adds a `win.<name>` action that takes a game ID.
    fn add_game_action(&self, name: &str, activate: fn(&Self, &GameId)) {
        let action = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
        action.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, id| {
                if let Some(id) = id.and_then(|id| id.str()).and_then(|id| id.parse().ok()) {
                    activate(&window, &id);
                }
            }
        ));
        self.add_action(&action);
    }

    /// Pushes the game's page.
    fn show_details(&self, id: &GameId) {
        if let Some(game) = self.library().game_object(id) {
            let page = GamePage::new(&game, &self.library_service());
            self.imp().navigation_view.push(&page);
        }
    }

    /// Shows the open game page's launch settings again, after the Edit dialog saved.
    pub fn refresh_game_page(&self) {
        if let Some(page) = self
            .imp()
            .navigation_view
            .visible_page()
            .and_downcast::<GamePage>()
        {
            page.load_launch_settings();
        }
    }

    /// Opens the game's install folder in the file manager.
    fn open_install_folder(&self, id: &GameId) {
        let Some(path) = self
            .library()
            .game_object(id)
            .and_then(|game| game.game().install_path.clone())
        else {
            return;
        };
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(path)));
        launcher.launch(Some(self), gio::Cancellable::NONE, |result| {
            if let Err(error) = result {
                glib::g_warning!("pindu", "Could not open the install folder: {error}");
            }
        });
    }

    /// Search (Ctrl+F, or typing anywhere in the library), the filter toggles, and the
    /// status pages for an empty library and for no results.
    fn setup_search_and_filters(&self) {
        let imp = self.imp();
        imp.search_entry
            .set_key_capture_widget(Some(&*imp.content_stack));
        imp.search_entry.connect_search_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |entry| {
                window.scroll_grid_to_top();
                window.library().set_search(&entry.text());
                window.update_jump_back_in();
            }
        ));

        let toggles = [
            &*imp.installed_filter,
            &*imp.not_installed_filter,
            &*imp.favourites_filter,
            &*imp.windows_filter,
            &*imp.native_filter,
        ];
        for toggle in toggles {
            toggle.connect_toggled(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| window.apply_filters()
            ));
        }
        imp.clear_filters.connect_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                let imp = window.imp();
                for toggle in [
                    &*imp.installed_filter,
                    &*imp.not_installed_filter,
                    &*imp.favourites_filter,
                    &*imp.windows_filter,
                    &*imp.native_filter,
                ] {
                    toggle.set_active(false);
                }
            }
        ));

        let library = self.library();
        library.model().connect_items_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, _, _, _| window.update_content_page()
        ));
        library.connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| {
                window.update_content_page();
                window.update_jump_back_in();
            }
        ));
        self.update_content_page();
    }

    /// Applies the filter toggles; fullscreen mode's library has tabs instead, so they wait
    /// for the desktop window.
    pub(crate) fn apply_filters(&self) {
        let imp = self.imp();
        let toggles = Filters {
            installed: imp.installed_filter.is_active(),
            not_installed: imp.not_installed_filter.is_active(),
            favourites: imp.favourites_filter.is_active(),
            windows: imp.windows_filter.is_active(),
            native: imp.native_filter.is_active(),
        };
        imp.clear_filters.set_visible(toggles.any());
        let filters = if self.is_fullscreen() {
            Filters::default()
        } else {
            toggles
        };
        self.scroll_grid_to_top();
        self.library().set_filters(filters);
        // The controller comes back on the cover it was last on, wherever it now is.
        imp.grid_cursor.set(self.recall_cover());
        self.update_jump_back_in();
    }

    /// The grid when it has games; otherwise the first-run page for an empty library or
    /// the no-results page.
    fn update_content_page(&self) {
        let library = self.library();
        let page = if library.model().n_items() > 0 && library.view() == View::PlayHistory {
            self.show_history();
            "history"
        } else if library.model().n_items() > 0 {
            "grid"
        } else if library.counts().get(View::All) == 0 {
            "empty"
        } else if matches!(library.view(), View::Collection(id) if library.counts().get(View::Collection(id)) == 0)
        {
            "empty-collection"
        } else if library.view() == View::Hidden && library.counts().get(View::Hidden) == 0 {
            "no-hidden"
        } else {
            "no-results"
        };
        self.imp().content_stack.set_visible_child_name(page);
    }

    pub(crate) fn library_service(&self) -> LibraryService {
        self.application()
            .and_downcast::<PinduApplication>()
            .expect("windows belong to the application")
            .library_service()
            .clone()
    }

    /// Play history: the shown games, most recent first, grouped by Today, This week and
    /// Earlier, each with its playtime and where that comes from.
    fn show_history(&self) {
        let imp = self.imp();
        while let Some(group) = imp.history_groups.first_child() {
            imp.history_groups.remove(&group);
        }
        let model = self.library().model();
        let mut games: Vec<GameObject> = (0..model.n_items())
            .filter_map(|position| model.item(position).and_downcast::<GameObject>())
            .collect();
        games.sort_by_key(|game| std::cmp::Reverse(game.game().last_played_at));
        let now = glib::DateTime::now_local().ok();
        let (now_secs, offset) =
            now.map_or((0, 0), |now| (now.to_unix(), now.utc_offset().as_seconds()));
        let mut groups: Vec<(HistoryGroup, adw::PreferencesGroup)> = Vec::new();
        for game in &games {
            let (title, last_played, playtime, origin) = {
                let entry = game.entry();
                (
                    entry.game.title.clone(),
                    entry.game.last_played_at,
                    entry.playtime_secs,
                    entry.playtime_origin,
                )
            };
            let Some(last_played) = last_played else {
                continue;
            };
            let group = history_group(last_played, now_secs, offset);
            if groups.last().is_none_or(|(last, _)| *last != group) {
                let title = match group {
                    HistoryGroup::Today => gettext("Today"),
                    HistoryGroup::ThisWeek => gettext("This Week"),
                    HistoryGroup::Earlier => gettext("Earlier"),
                };
                let widget = adw::PreferencesGroup::builder().title(title).build();
                imp.history_groups.append(&widget);
                groups.push((group, widget));
            }
            let played = crate::game_page::playtime(playtime);
            let played = match origin {
                Some(PlaytimeOrigin::Source(source)) => gettext("{time} in {source}")
                    .replace("{time}", &played)
                    .replace("{source}", &source_name(source)),
                Some(PlaytimeOrigin::Sessions) => {
                    gettext("{time}, tracked here").replace("{time}", &played)
                }
                None => played,
            };
            let subtitle = format!("{} · {played}", crate::game_page::date(last_played));
            let row = adw::ActionRow::builder()
                .title(&title)
                .subtitle(&subtitle)
                .activatable(true)
                .use_markup(false)
                .build();
            row.set_action_name(Some("win.show-details"));
            row.set_action_target_value(Some(&game.id().as_str().to_variant()));
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            if let Some((_, widget)) = groups.last() {
                widget.add(&row);
            }
        }
    }

    /// The "Jump back in" row: shown in All Games while no search or filter is active, and
    /// not in fullscreen mode, where Home has it.
    fn update_jump_back_in(&self) {
        let imp = self.imp();
        let games = self.library().jump_back_in();
        let quiet = self.library().view() == View::All
            && imp.search_entry.text().is_empty()
            && !imp.clear_filters.is_visible()
            && !self.is_fullscreen();
        imp.jump_back_in.set_visible(quiet && !games.is_empty());
        while let Some(card) = imp.jump_cards.first_child() {
            imp.jump_cards.remove(&card);
        }
        if quiet {
            for game in &games {
                imp.jump_cards.append(&jump_card(game, (240, 112)));
            }
        }
    }

    fn toggle_favourite(&self, id: &GameId) {
        let Some(game) = self.library().game_object(id) else {
            return;
        };
        let favourite = !game.game().favourite;
        self.library_service().set_favourite(id, favourite);
    }

    /// Pins a game to "Jump back in", or unpins it; says so when every place is taken.
    fn toggle_pin(&self, id: &GameId) {
        let Some(game) = self.library().game_object(id) else {
            return;
        };
        let service = self.library_service();
        if game.game().pin_order.is_some() {
            service.unpin(id);
            return;
        }
        let (window, id) = (self.downgrade(), id.clone());
        glib::spawn_future_local(async move {
            let pinned = service.pin(&id).await;
            if let Some(window) = window.upgrade()
                && !pinned
            {
                window.add_toast(adw::Toast::new(&gettext(
                    "Three games are pinned already; unpin one first",
                )));
            }
        });
    }

    /// Hides a game, with a toast to undo it.
    /// Shows the hidden games (Ctrl+H), or goes back to All Games.
    fn show_hidden_games(&self, show: bool) {
        let imp = self.imp();
        if show {
            self.scroll_grid_to_top();
            self.library().set_view(View::Hidden);
            imp.grid_cursor.set(self.recall_cover());
            imp.library_page.set_title(&gettext("Hidden Games"));
            imp.sidebar.set_selected(gtk::INVALID_LIST_POSITION);
            self.select_tab();
            self.show_library();
            self.update_jump_back_in();
        } else if self.library().view() == View::Hidden {
            self.show_view_of(&imp.all_item);
        }
    }

    fn unhide_game(&self, id: &GameId) {
        self.library_service().set_hidden(id, false);
    }

    /// Removes a game from the library, with a toast to undo it. It stays out while its
    /// launcher lists it.
    fn remove_game(&self, id: &GameId) {
        let service = self.library_service();
        let title = self.library().game_title(id).unwrap_or_default();
        service.set_removed(id, true);
        let toast = adw::Toast::builder()
            .title(gettext("“{title}” removed").replace("{title}", &title))
            .button_label(gettext("_Undo"))
            .use_markup(false)
            .build();
        let id = id.clone();
        toast.connect_button_clicked(move |_| service.set_removed(&id, false));
        self.imp().toast_overlay.add_toast(toast);
    }

    fn hide_game(&self, id: &GameId) {
        let service = self.library_service();
        let title = self.library().game_title(id).unwrap_or_default();
        service.set_hidden(id, true);
        let toast = adw::Toast::builder()
            .title(gettext("“{title}” hidden").replace("{title}", &title))
            .button_label(gettext("_Undo"))
            .use_markup(false)
            .build();
        let id = id.clone();
        toast.connect_button_clicked(move |_| service.set_hidden(&id, false));
        self.imp().toast_overlay.add_toast(toast);
    }

    /// The tile that has keyboard focus, if any.
    pub(crate) fn focused_tile(&self) -> Option<CoverTile> {
        let focus = gtk::prelude::GtkWindowExt::focus(self)?;
        if !focus.is_ancestor(&*self.imp().grid) {
            return None;
        }
        focus.first_child().and_downcast::<CoverTile>()
    }

    /// Library rows and one row per source, with counts; sources without games are hidden.
    fn setup_sidebar(&self) {
        let imp = self.imp();
        let mut views = vec![
            (imp.all_item.get(), View::All, imp.all_count.get()),
            (
                imp.installed_item.get(),
                View::Installed,
                imp.installed_count.get(),
            ),
            (
                imp.favourites_item.get(),
                View::Favourites,
                imp.favourites_count.get(),
            ),
            (
                imp.history_item.get(),
                View::PlayHistory,
                imp.history_count.get(),
            ),
        ];
        for &source in Source::ALL {
            let count = gtk::Label::builder()
                .css_classes(["dimmed", "numeric"])
                .build();
            let item = adw::SidebarItem::builder()
                .title(source_name(source))
                .icon_name(source_icon(source))
                .suffix(&count)
                .visible(false)
                .build();
            imp.sources_section.append(item.clone());
            views.push((item, View::Source(source), count));
        }
        imp.views.replace(views);

        imp.sidebar.connect_activated(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |sidebar, _| {
                let Some(item) = sidebar.selected_item() else {
                    return;
                };
                if item == *window.imp().new_collection_item {
                    window.select_view(window.library().view());
                    window.new_collection(None);
                } else {
                    window.show_view_of(&item);
                }
            }
        ));
        imp.sidebar.connect_setup_menu(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_, item| {
                let collection = item.and_then(|item| {
                    window
                        .imp()
                        .views
                        .borrow()
                        .iter()
                        .find_map(|(candidate, view, _)| match view {
                            View::Collection(id) if candidate == item => Some(*id),
                            _ => None,
                        })
                });
                window.imp().menu_collection.set(collection);
            }
        ));
        self.library().connect_collections_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_collections()
        ));
        self.show_collections();
        self.library().connect_counts_changed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |library| window.show_counts(&library.counts())
        ));
        self.show_counts(&self.library().counts());
    }

    fn show_view_of(&self, item: &adw::SidebarItem) {
        let imp = self.imp();
        let Some(view) = imp
            .views
            .borrow()
            .iter()
            .find(|(candidate, _, _)| candidate == item)
            .map(|(_, view, _)| *view)
        else {
            return;
        };
        self.set_shown_view(view);
        // Choosing a view from a game page (or Home) goes to the library.
        self.show_library();
        if imp.split_view.is_collapsed() {
            imp.split_view.set_show_sidebar(false);
        }
    }

    /// Shows `view` in the library page, with its sidebar row and tab selected, without
    /// going to the page.
    pub(crate) fn set_shown_view(&self, view: View) {
        let imp = self.imp();
        self.scroll_grid_to_top();
        self.library().set_view(view);
        // The controller comes back into the grid on the cover it was last on in this view.
        imp.grid_cursor.set(self.recall_cover());
        if let Some(action) = self.lookup_action("show-hidden") {
            action.change_state(&false.to_variant());
        }
        self.update_jump_back_in();
        let title = imp
            .views
            .borrow()
            .iter()
            .find(|(_, candidate, _)| *candidate == view)
            .and_then(|(item, _, _)| item.title())
            .unwrap_or_default();
        imp.library_page.set_title(&title);
        self.select_view(view);
        self.select_tab();
    }

    /// One sidebar row per collection, in the user's order, keeping the shown view.
    fn show_collections(&self) {
        let imp = self.imp();
        let collections = self.library().collections();
        imp.collections_section.remove_all();
        imp.views
            .borrow_mut()
            .retain(|(_, view, _)| !matches!(view, View::Collection(_)));
        for collection in &collections {
            let count = gtk::Label::builder()
                .css_classes(["dimmed", "numeric"])
                .build();
            let item = adw::SidebarItem::builder()
                .title(&collection.name)
                .icon_name("folder-symbolic")
                .suffix(&count)
                .build();
            imp.collections_section.append(item.clone());
            imp.views
                .borrow_mut()
                .push((item, View::Collection(collection.id), count));
        }
        self.show_counts(&self.library().counts());
        let shown = self.library().view();
        if matches!(shown, View::Collection(id) if !collections.iter().any(|c| c.id == id)) {
            self.show_view_of(&imp.all_item);
        } else {
            self.select_view(shown);
        }
    }

    /// Selects the sidebar row of a view, without showing it again.
    fn select_view(&self, view: View) {
        let imp = self.imp();
        if let Some(item) = imp
            .views
            .borrow()
            .iter()
            .find(|(_, candidate, _)| *candidate == view)
            .map(|(item, _, _)| item.clone())
        {
            imp.sidebar.set_selected(item.index());
        }
    }

    fn show_counts(&self, counts: &Counts) {
        for (item, view, label) in self.imp().views.borrow().iter() {
            let count = counts.get(*view);
            label.set_label(&count.to_string());
            if let View::Source(source) = view {
                let shown = !self
                    .settings()
                    .strv("hidden-sources")
                    .iter()
                    .any(|hidden| hidden.as_str() == source.as_str());
                item.set_visible(count > 0 && shown);
            }
        }
    }

    /// The cover grid; tiles follow the `tile-size` and `show-titles` settings.
    fn setup_grid(&self) {
        let grid = &self.imp().grid;
        grid.set_model(Some(&gtk::NoSelection::new(Some(self.library().model()))));
        self.update_tile_factory();
        if let Some(width) = grid.hadjustment() {
            // The page size changes while the grid allocates its size, when a new column
            // cap would not take effect until the next resize: apply it right after.
            width.connect_page_size_notify(glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_| {
                    glib::idle_add_local_once(glib::clone!(
                        #[weak]
                        window,
                        move || window.update_grid_columns()
                    ));
                }
            ));
        }
        // While the sort runs, GTK's grid keeps the item at its top-left corner in view,
        // so a grid at the top would end up scrolled to wherever that item sorts to.
        // One that was at the top before the sort goes back to the top after it.
        let sorted = self.library().sort_model();
        sorted.connect_pending_notify(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |sorted| {
                let imp = window.imp();
                let at_top = || imp.grid.vadjustment().is_none_or(|v| v.value() < 1.0);
                if sorted.pending() == 0 {
                    if imp.grid_at_top.replace(false) {
                        window.scroll_grid_to_top();
                    }
                    if imp.focus_first_after_sort.take() {
                        window.focus_page();
                    }
                } else if !imp.grid_at_top.get() {
                    imp.grid_at_top.set(at_top());
                }
            }
        ));
        // Delete and the Menu key act on the focused tile only, so typing elsewhere is safe.
        let keys = gtk::EventControllerKey::new();
        keys.connect_key_pressed(glib::clone!(
            #[weak(rename_to = window)]
            self,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |_, key, _, modifiers| {
                let Some(tile) = window.focused_tile() else {
                    return glib::Propagation::Proceed;
                };
                let menu_key = key == gdk::Key::Menu
                    || (key == gdk::Key::F10 && modifiers.contains(gdk::ModifierType::SHIFT_MASK));
                if menu_key {
                    tile.popup_menu_at(None);
                } else if key == gdk::Key::Delete && modifiers.is_empty() {
                    let Some(game) = tile.game() else {
                        return glib::Propagation::Proceed;
                    };
                    window.remove_game(&game.id());
                } else {
                    return glib::Propagation::Proceed;
                }
                glib::Propagation::Stop
            }
        ));
        grid.add_controller(keys);
        // Activating a tile opens its game page.
        grid.connect_activate(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |grid, position| {
                let game = grid
                    .model()
                    .and_then(|model| model.item(position))
                    .and_downcast::<GameObject>();
                if let Some(game) = game {
                    window.show_details(&game.id());
                }
            }
        ));
        for key in ["tile-size", "show-titles", "show-badges"] {
            self.settings().connect_changed(
                Some(key),
                glib::clone!(
                    #[weak(rename_to = window)]
                    self,
                    move |_, _| window.update_tile_factory()
                ),
            );
        }
        self.settings().connect_changed(
            Some("prefer-steamgriddb"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| window.library_service().reload()
            ),
        );
        self.settings().connect_changed(
            Some("hidden-sources"),
            glib::clone!(
                #[weak(rename_to = window)]
                self,
                move |_, _| window.show_counts(&window.library().counts())
            ),
        );
    }

    fn update_tile_factory(&self) {
        let settings = self.settings();
        let size = TileSize::from_setting(&settings.string("tile-size"));
        let show_titles = settings.boolean("show-titles");
        let show_badges = settings.boolean("show-badges");
        let factory = gtk::SignalListItemFactory::new();
        factory.connect_setup(move |_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            item.set_child(Some(&CoverTile::new(size, show_titles, show_badges)));
        });
        factory.connect_bind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            let (Some(game), Some(tile)) = (
                item.item().and_downcast::<GameObject>(),
                item.child().and_downcast::<CoverTile>(),
            ) else {
                return;
            };
            tile.bind(&game);
            item.set_accessible_label(&CoverTile::accessible_label(&game));
        });
        factory.connect_unbind(|_, item| {
            let item = item.downcast_ref::<gtk::ListItem>().expect("a list item");
            if let Some(tile) = item.child().and_downcast::<CoverTile>() {
                tile.unbind();
            }
        });
        self.imp().grid.set_factory(Some(&factory));
        self.update_grid_columns();
    }

    /// A new search, filter, view or order starts at the top. Going there before the
    /// change, and stopping a fling, also keeps GTK's grid from drawing nothing when the
    /// list shrinks under a scroll still in motion.
    fn scroll_grid_to_top(&self) {
        let grid = &self.imp().grid;
        self.imp().grid_cursor.set(0);
        if let Some(scrolled) = grid.parent().and_downcast::<gtk::ScrolledWindow>() {
            // Turning kinetic scrolling off ends any deceleration in progress.
            scrolled.set_kinetic_scrolling(false);
            scrolled.set_kinetic_scrolling(true);
        }
        if grid.model().is_some_and(|model| model.n_items() > 0) {
            grid.scroll_to(0, gtk::ListScrollFlags::NONE, None);
        }
    }

    /// Caps the grid's columns at what fits its width (see [`grid::max_columns`]).
    fn update_grid_columns(&self) {
        let grid = &self.imp().grid;
        let Some(width) = grid.hadjustment() else {
            return;
        };
        let size = TileSize::from_setting(&self.settings().string("tile-size"));
        let column = size.cover_size().0 + GRID_CHILD_PADDING;
        grid.set_max_columns(grid::max_columns(width.page_size() - GRID_PADDING, column));
    }

    /// Shows a header spinner while importing, and one toast summarising any problems.
    fn observe_sync(&self, sync_service: &SyncService) {
        sync_service
            .bind_property("syncing", &*self.imp().sync_spinner, "visible")
            .sync_create()
            .build();
        sync_service.connect_finished(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |service| window.report_problems(&service.problems())
        ));
    }

    /// Searches the library for `text`, as typing it in the search entry would.
    pub fn search_for(&self, text: &str) {
        self.show_library();
        self.imp().search_entry.set_text(text);
        self.focus_search_entry();
    }

    /// Shows a toast over the window.
    pub fn add_toast(&self, toast: adw::Toast) {
        self.imp().toast_overlay.add_toast(toast);
    }

    fn report_problems(&self, problems: &[String]) {
        if problems.is_empty() {
            return;
        }
        let count = problems.len() as u32;
        let toast = adw::Toast::builder()
            .title(
                ngettext("One import problem", "{count} import problems", count)
                    .replace("{count}", &count.to_string()),
            )
            .button_label(gettext("Details"))
            .timeout(10)
            .build();
        let problems = problems.to_vec();
        toast.connect_button_clicked(glib::clone!(
            #[weak(rename_to = window)]
            self,
            move |_| window.show_problems(&problems)
        ));
        self.imp().toast_overlay.add_toast(toast);
    }

    /// One dialog listing every problem of the last import.
    fn show_problems(&self, problems: &[String]) {
        let dialog = adw::AlertDialog::builder()
            .heading(gettext("Some Games Could Not Be Imported"))
            .body(problems.join("\n"))
            .build();
        dialog.add_response("close", &gettext("_Close"));
        dialog.set_default_response(Some("close"));
        dialog.present(Some(self));
    }

    pub(crate) fn settings(&self) -> &gio::Settings {
        self.imp()
            .settings
            .get_or_init(|| gio::Settings::new(APP_ID))
    }

    fn restore_window_state(&self) {
        let settings = self.settings();
        self.set_default_size(settings.int("window-width"), settings.int("window-height"));
        if settings.boolean("window-maximized") {
            self.maximize();
        }
    }

    fn save_window_state(&self) {
        let settings = self.settings();
        let (width, height) = self.default_size();
        // Window state is best effort: a read-only settings backend must not block closing.
        let _ = settings.set_int("window-width", width);
        let _ = settings.set_int("window-height", height);
        let _ = settings.set_boolean("window-maximized", self.is_maximized());
    }
}

/// Horizontal padding around the cover grid and around each tile, as in `style.css`.
const GRID_PADDING: f64 = 24.0;
const GRID_CHILD_PADDING: i32 = 12;

const SORT_MODES: [(SortMode, &str); 5] = [
    (SortMode::LastPlayed, "last-played"),
    (SortMode::NameAscending, "name-ascending"),
    (SortMode::NameDescending, "name-descending"),
    (SortMode::RecentlyAdded, "recently-added"),
    (SortMode::Playtime, "playtime"),
];

/// A landscape card for "Jump back in" and fullscreen Home: the game's banner (or cover)
/// with its title, `width`×`height` pixels.
pub(crate) fn jump_card(game: &GameObject, (width, height): (i32, i32)) -> gtk::Button {
    let (title, pinned, art, icon) = {
        let entry = game.entry();
        let art = entry.art.hero.clone().or_else(|| entry.art.cover.clone());
        (
            entry.game.title.clone(),
            entry.game.pin_order.is_some(),
            art,
            entry.art.icon.clone(),
        )
    };
    // Cover art draws at its own fixed size, whatever the image's size.
    let picture = CoverArt::new();
    picture.set_size_request(width, height);
    let label = gtk::Label::builder()
        .label(&title)
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .valign(gtk::Align::End)
        .css_classes(["jump-title"])
        .build();
    let overlay = gtk::Overlay::builder().child(&picture).build();
    overlay.add_overlay(&label);
    if pinned {
        let pin = gtk::Image::builder()
            .icon_name("view-pin-symbolic")
            .tooltip_text(gettext("Pinned"))
            .halign(gtk::Align::End)
            .valign(gtk::Align::Start)
            .margin_top(6)
            .margin_end(6)
            .css_classes(["badge"])
            .build();
        overlay.add_overlay(&pin);
    }
    let card = gtk::Button::builder()
        .child(&overlay)
        .tooltip_text(&title)
        .action_name("win.show-details")
        .action_target(&game.id().as_str().to_variant())
        .css_classes(["jump-card", "card"])
        .build();
    let scale = card.scale_factor().max(1);
    picture.show(
        art,
        icon,
        ((width * scale) as u32, (height * scale) as u32),
        |_| {},
    );
    card
}

pub(crate) fn sort_mode_name(mode: SortMode) -> &'static str {
    SORT_MODES
        .iter()
        .find(|(candidate, _)| *candidate == mode)
        .map(|(_, name)| *name)
        .expect("every sort mode has a name")
}

pub(crate) fn sort_mode_from_name(name: &str) -> Option<SortMode> {
    SORT_MODES
        .iter()
        .find(|(_, candidate)| *candidate == name)
        .map(|(mode, _)| *mode)
}
