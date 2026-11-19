//! The game page: hero art with the logo or title, Play with default focus, a ⋮ menu, a
//! facts row, why the last launch failed with a button per next step (see
//! `failed_launch`), the About group, the description and the Launch group: for games we
//! run, their launch settings; for store games, a read-only summary. In fullscreen mode it
//! shows hero, logo, a large Play, Favourite, Add to Collection and Options (both opening
//! the options sheet, which lists the next steps), a Playing Now card while the game runs,
//! a launch summary, why the last launch failed, the About group and a description a
//! controller scrolls through, with the starting screen over the page while Play starts
//! the game.

use std::cell::{Cell, OnceCell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};
use pindu_core::failed_launch::Explanation;
use pindu_core::gamepad::Direction;
use pindu_core::launch::{self, LaunchSettings, WebPage};
use pindu_core::library::{GameCopy, PlaytimeOrigin};
use pindu_core::manual;
use pindu_core::model::{
    Collection, ControllerSupport, DeckCompatibility, DeckRating, Game, GameId, LaunchConfig,
    LaunchKind, Source, Timestamp,
};
use pindu_core::panels::Playing;
use pindu_core::reading;

use crate::application::PinduApplication;
use crate::collection_actions::collection_menu;
use crate::failed_launch;
use crate::group_actions::group_menu;
use crate::launch_service::launcher_name;
use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::outcome_service;
use crate::permission_service;
use crate::proton_choice::{FirstChoice, ProtonChoice};
use crate::proton_launch;
use crate::sources::source_name;
use crate::widgets::textures;
use crate::window::PinduWindow;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/game_page.ui")]
    pub struct GamePage {
        #[template_child]
        pub hero: TemplateChild<gtk::Picture>,
        #[template_child]
        pub logo: TemplateChild<gtk::Picture>,
        #[template_child]
        pub title: TemplateChild<gtk::Label>,
        #[template_child]
        pub play_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub more_button: TemplateChild<gtk::MenuButton>,
        #[template_child]
        pub collection_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub options_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub playing_card: TemplateChild<gtk::Box>,
        #[template_child]
        pub launch_summary: TemplateChild<gtk::Label>,
        #[template_child]
        pub launch_overlay: TemplateChild<gtk::Box>,
        #[template_child]
        pub launch_art: TemplateChild<gtk::Picture>,
        #[template_child]
        pub launch_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub launch_status: TemplateChild<gtk::Label>,
        #[template_child]
        pub launch_cancel: TemplateChild<gtk::Button>,
        #[template_child]
        pub favourite_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub facts: TemplateChild<adw::WrapBox>,
        #[template_child]
        pub failure_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub copies_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub about_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub developer_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub genres_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub release_year_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub controller_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub deck_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub description_box: TemplateChild<gtk::Box>,
        #[template_child]
        pub description: TemplateChild<gtk::Label>,
        #[template_child]
        pub scroller: TemplateChild<gtk::ScrolledWindow>,
        #[template_child]
        pub content: TemplateChild<gtk::Box>,
        #[template_child]
        pub launch_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub launch_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub open_in_launcher: TemplateChild<gtk::Button>,
        #[template_child]
        pub steam_dialog_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub permission_banner: TemplateChild<adw::Banner>,
        #[template_child]
        pub settings_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub proton_row: TemplateChild<adw::ComboRow>,
        #[template_child]
        pub prefix_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub arguments_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub environment_row: TemplateChild<adw::EntryRow>,
        #[template_child]
        pub mangohud_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub gamemode_row: TemplateChild<adw::SwitchRow>,
        #[template_child]
        pub wrappers_row: TemplateChild<adw::EntryRow>,
        /// The shown game and its `notify::playing` handler.
        pub playing_handler: RefCell<Option<(GameObject, Vec<glib::SignalHandlerId>)>>,
        /// Whether the page shows fullscreen mode's presentation.
        pub fullscreen: Cell<bool>,
        /// The game whose launch settings are shown, and where they are stored.
        pub launch_target: RefCell<Option<(LibraryService, GameId)>>,
        /// The Proton row's builds, for Windows games.
        pub proton: OnceCell<Rc<ProtonChoice>>,
        /// The shown prefix, where the folder chooser starts.
        pub prefix: RefCell<Option<PathBuf>>,
        /// Set while the rows show stored settings, so only the user's changes are saved.
        pub filling: Cell<bool>,
        /// Why the game's last launch failed, and its next steps, once read.
        pub failure: RefCell<Option<Explanation>>,
        /// The failure group's step buttons, in the desktop window.
        pub failure_rows: RefCell<Vec<adw::ButtonRow>>,
        /// The application, and its handler that reads the last failure again whenever
        /// a launch of the game is recorded.
        pub recorded_handler: RefCell<Option<(PinduApplication, glib::SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GamePage {
        const NAME: &'static str = "PinduGamePage";
        type Type = super::GamePage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
            klass.bind_template_callbacks();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    #[gtk::template_callbacks]
    impl GamePage {
        #[template_callback]
        fn choose_prefix(&self) {
            self.obj().choose_prefix();
        }

        #[template_callback]
        fn apply_arguments(&self) {
            let row = &self.arguments_row;
            if let Some(arguments) = parsed(row, manual::split_arguments) {
                self.obj()
                    .edit_launch(move |config| config.arguments = arguments);
            }
        }

        #[template_callback]
        fn apply_environment(&self) {
            let row = &self.environment_row;
            if let Some(environment) = parsed(row, launch::parse_environment) {
                self.obj()
                    .edit_launch(move |config| config.environment = environment);
            }
        }

        #[template_callback]
        fn apply_wrappers(&self) {
            let row = &self.wrappers_row;
            if let Some(wrappers) = parsed(row, manual::split_arguments) {
                self.obj()
                    .edit_launch(move |config| config.wrappers = wrappers);
            }
        }
    }

    impl ObjectImpl for GamePage {
        fn dispose(&self) {
            if let Some((object, handlers)) = self.playing_handler.take() {
                for handler in handlers {
                    object.disconnect(handler);
                }
            }
            if let Some((application, handler)) = self.recorded_handler.take() {
                application.disconnect(handler);
            }
        }
    }
    impl WidgetImpl for GamePage {}

    impl NavigationPageImpl for GamePage {
        // Play has the focus whenever the page is shown, so a second activate launches.
        fn shown(&self) {
            self.parent_shown();
            self.play_button.grab_focus();
        }
    }
}

glib::wrapper! {
    /// The page of one game.
    pub struct GamePage(ObjectSubclass<imp::GamePage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GamePage {
    /// A page showing the game in `object`.
    pub fn new(object: &GameObject, library: &LibraryService) -> Self {
        let page: Self = glib::Object::new();
        page.show(object, library);
        page
    }

    fn show(&self, object: &GameObject, library: &LibraryService) {
        let imp = self.imp();
        let entry = object.entry();
        let game = &entry.game;
        let target = game.id.as_str().to_variant();
        self.set_title(&game.title);
        imp.title.set_label(&game.title);
        // The logo stands in for the title once it loads.
        imp.logo
            .update_property(&[gtk::accessible::Property::Label(&game.title)]);
        imp.play_button.set_action_target_value(Some(&target));
        let mut handlers: Vec<_> = ["playing", "playing-in-store"]
            .into_iter()
            .map(|property| {
                object.connect_notify_local(
                    Some(property),
                    glib::clone!(
                        #[weak(rename_to = page)]
                        self,
                        move |object, _| page.show_playing(object.playing())
                    ),
                )
            })
            .collect();
        let favourite_handler = object.connect_notify_local(
            Some("favourite"),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |object, _| page.show_favourite(object.game().favourite)
            ),
        );
        handlers.push(favourite_handler);
        imp.playing_handler
            .replace(Some((object.clone(), handlers)));
        self.show_playing(object.playing());
        imp.favourite_button.set_action_target_value(Some(&target));
        self.show_favourite(game.favourite);
        imp.open_in_launcher.set_action_target_value(Some(&target));
        imp.more_button.set_create_popup_func(glib::clone!(
            #[weak]
            object,
            #[weak]
            library,
            move |button| {
                let collections = library.model().manual_collections();
                let grouping = button
                    .root()
                    .and_downcast::<PinduWindow>()
                    .map(|window| group_menu(&window, &object));
                button.set_menu_model(Some(&more_menu(&object, &collections, grouping)));
            }
        ));
        imp.launch_summary.set_label(&launch_summary(game));
        imp.launch_title.set_label(&game.title);

        for (caption, value) in facts(game, entry.playtime_secs, entry.playtime_origin) {
            imp.facts.append(&fact(&caption, &value));
        }
        let mut first_choice: Option<gtk::CheckButton> = None;
        for (index, copy) in entry.copies.iter().enumerate() {
            let (row, choice) = copy_row(
                copy,
                index == 0,
                copy.game.id == game.id,
                first_choice.as_ref(),
            );
            imp.copies_group.add(&row);
            first_choice.get_or_insert(choice);
        }
        imp.copies_group.set_visible(!entry.copies.is_empty());

        imp.developer_row.set_visible(game.developer.is_some());
        imp.developer_row
            .set_subtitle(game.developer.as_deref().unwrap_or_default());
        imp.genres_row.set_visible(!game.genres.is_empty());
        let genres: Vec<String> = game.genres.iter().map(|genre| genre_label(genre)).collect();
        imp.genres_row.set_subtitle(&genres.join(", "));
        imp.release_year_row
            .set_visible(game.release_year.is_some());
        imp.release_year_row.set_subtitle(
            &game
                .release_year
                .map(|year| year.to_string())
                .unwrap_or_default(),
        );
        imp.controller_row
            .set_visible(game.controller_support.is_some());
        imp.controller_row.set_subtitle(
            &game
                .controller_support
                .map(controller_support_label)
                .unwrap_or_default(),
        );
        imp.deck_row.set_visible(game.deck_compatibility.is_some());
        imp.deck_row.set_subtitle(
            &game
                .deck_compatibility
                .map(|deck| deck_rating(&deck))
                .unwrap_or_default(),
        );
        imp.about_group.set_visible(
            game.developer.is_some()
                || !game.genres.is_empty()
                || game.release_year.is_some()
                || game.controller_support.is_some()
                || game.deck_compatibility.is_some(),
        );
        imp.description_box.set_visible(game.description.is_some());
        imp.description
            .set_label(game.description.as_deref().unwrap_or_default());

        imp.permission_banner
            .set_revealed(permission_service::needs_permission(game));
        let store_launcher = launcher_name(game.source());
        imp.launch_group.set_visible(shows_launch_group(game));
        if let Some(launcher) = store_launcher {
            imp.launch_row.set_subtitle(
                &gettext("{launcher} starts the game, keeps its settings and tracks its playtime")
                    .replace("{launcher}", launcher),
            );
            imp.open_in_launcher
                .set_label(&gettext("Open in {launcher}").replace("{launcher}", launcher));
        }
        imp.open_in_launcher
            .set_visible(pindu_core::launch::launcher_uri(game).is_some());
        if game.source() == Source::Steam {
            let row = &imp.steam_dialog_row;
            row.set_visible(true);
            row.set_active(game.steam_launch_dialog);
            let (library, id) = (library.clone(), game.id.clone());
            row.connect_active_notify(move |row| {
                library.set_steam_launch_dialog(&id, row.is_active())
            });
        }
        if let Some(settings) = launch::launch_settings(game) {
            imp.launch_target
                .replace(Some((library.clone(), game.id.clone())));
            self.connect_launch_settings(settings);
            // The stored setting stays, so it works again once MangoHud is installed.
            if !permission_service::mangohud_works(game) {
                imp.mangohud_row.set_sensitive(false);
                imp.mangohud_row
                    .set_subtitle(&gettext("MangoHud is not installed"));
            }
            self.load_launch_settings();
        }

        let (hero, cover, logo) = (
            entry.art.hero.clone(),
            entry.art.cover.clone(),
            entry.art.logo.clone(),
        );
        let game_id = game.id.clone();
        drop(entry);
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            // Without hero art, the cover stands in, cropped by the picture.
            if let Some(path) = hero.or(cover)
                && let (Ok(texture), Some(page)) = (textures::load(&path).await, page.upgrade())
            {
                let imp = page.imp();
                imp.hero.set_paintable(Some(&texture));
                imp.launch_art.set_paintable(Some(&texture));
                imp.launch_art.set_visible(true);
            }
            if let Some(path) = logo
                && let (Ok(texture), Some(page)) = (textures::load(&path).await, page.upgrade())
            {
                let imp = page.imp();
                imp.logo.set_paintable(Some(&texture));
                imp.logo.set_visible(true);
                imp.title.set_visible(false);
            }
        });
        if let Some(application) = gio::Application::default().and_downcast::<PinduApplication>() {
            let id = game_id;
            let handler = application.connect_launch_recorded(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |recorded: &GameId| {
                    if *recorded == id {
                        page.load_failure();
                    }
                }
            ));
            imp.recorded_handler.replace(Some((application, handler)));
        }
        self.load_failure();
    }
}

impl GamePage {
    /// The game the page shows.
    pub fn game(&self) -> Option<GameObject> {
        let handler = self.imp().playing_handler.borrow();
        handler.as_ref().map(|(object, _)| object.clone())
    }

    /// Shows fullscreen mode's presentation: larger art, Add to Collection and Options
    /// beside Favourite instead of the ⋮ menu, the Playing Now card and the launch summary
    /// instead of the Launch group, and a description that takes focus so a controller can
    /// scroll through it; or the desktop page.
    pub fn set_fullscreen(&self, fullscreen: bool) {
        let imp = self.imp();
        let was_fullscreen = imp.fullscreen.replace(fullscreen);
        imp.hero
            .set_height_request(if fullscreen { 440 } else { 320 });
        imp.logo.set_size_request(
            if fullscreen { 480 } else { 320 },
            if fullscreen { 180 } else { 120 },
        );
        imp.collection_button.set_visible(fullscreen);
        imp.options_button.set_visible(fullscreen);
        imp.more_button.set_visible(!fullscreen);
        imp.launch_summary.set_visible(fullscreen);
        // A selectable text would select itself all as it takes focus.
        imp.developer_row.set_subtitle_selectable(!fullscreen);
        imp.description.set_selectable(!fullscreen);
        imp.description_box.set_focusable(fullscreen);
        imp.launch_group.set_visible(
            !fullscreen
                && self
                    .game()
                    .is_some_and(|object| shows_launch_group(&object.game())),
        );
        if fullscreen {
            imp.settings_group.set_visible(false);
        } else {
            self.show_launch(None, false);
            if was_fullscreen {
                // The launch settings group shows once its settings are read.
                self.load_launch_settings();
            }
        }
        self.show_playing(self.game().map_or(Playing::No, |object| object.playing()));
        self.show_failure();
    }

    /// Reads why the game's last launch failed, if it did, and shows it.
    fn load_failure(&self) {
        let Some(game) = self.game().map(|object| object.game().clone()) else {
            return;
        };
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            let failure = outcome_service::last_failure(&game).await;
            if let Some(page) = page.upgrade() {
                page.imp().failure.replace(failure);
                page.show_failure();
            }
        });
    }

    /// Shows why the game's last launch failed: in the desktop window with a button for
    /// each next step, in fullscreen mode with a pointer to the options sheet, which lists
    /// them; or hides the group when it did not fail.
    fn show_failure(&self) {
        let imp = self.imp();
        for row in imp.failure_rows.take() {
            imp.failure_group.remove(&row);
        }
        let failure = imp.failure.borrow();
        let (Some(failure), Some(object)) = (failure.as_ref(), self.game()) else {
            imp.failure_group.set_visible(false);
            return;
        };
        let game = object.game();
        let reason = failed_launch::reason_text(&failure.reason, &game);
        let fullscreen = imp.fullscreen.get();
        let description = if fullscreen && !failure.steps.is_empty() {
            format!("{reason}\n{}", gettext("Options lists what to try next"))
        } else {
            reason
        };
        imp.failure_group.set_description(Some(&description));
        if !fullscreen {
            let mut rows = Vec::with_capacity(failure.steps.len());
            for &step in &failure.steps {
                let row = adw::ButtonRow::builder()
                    .title(failed_launch::step_label(step))
                    .build();
                match failed_launch::step_action(step, &game.id, failure.log.as_deref()) {
                    Some((action, target)) => {
                        row.set_action_name(Some(action));
                        row.set_action_target_value(target.as_ref());
                    }
                    None => {
                        row.connect_activated(glib::clone!(
                            #[weak(rename_to = page)]
                            self,
                            move |_| {
                                page.imp().proton_row.grab_focus();
                            }
                        ));
                    }
                }
                imp.failure_group.add(&row);
                rows.push(row);
            }
            imp.failure_rows.replace(rows);
        }
        imp.failure_group.set_visible(true);
    }

    /// Shows the starting screen with `status` ("Starting…") and, when `cancel`, its
    /// Cancel, which takes focus as it appears; or hides it.
    pub fn show_launch(&self, status: Option<&str>, cancel: bool) {
        let imp = self.imp();
        imp.launch_overlay.set_visible(status.is_some());
        imp.launch_status.set_label(status.unwrap_or_default());
        let appears = cancel && !imp.launch_cancel.is_visible();
        if !cancel && imp.launch_cancel.has_focus() {
            // Focus does not go with the screen.
            imp.play_button.grab_focus();
        }
        imp.launch_cancel.set_visible(cancel);
        if appears {
            imp.launch_cancel.grab_focus();
        }
    }

    /// Up or Down while the description has focus, in fullscreen mode: scrolls through a
    /// description taller than the page shows, and says whether it did, or kept focus at
    /// the page's end, rather than let focus move on (`pindu_core::reading`).
    pub fn scroll_description(&self, direction: Direction) -> bool {
        let imp = self.imp();
        if !imp.description_box.has_focus() {
            return false;
        }
        let Some(bounds) = imp.description_box.compute_bounds(&*imp.content) else {
            return false;
        };
        let adjustment = imp.scroller.vadjustment();
        let view = reading::View {
            top: adjustment.value(),
            height: adjustment.page_size(),
        };
        let text = reading::Span {
            top: f64::from(bounds.y()),
            bottom: f64::from(bounds.y() + bounds.height()),
        };
        // Only the Launch groups, hidden in fullscreen mode, lie below it.
        let more_below = imp.launch_group.is_visible() || imp.settings_group.is_visible();
        match reading::step(view, text, direction, more_below) {
            reading::Step::Scroll(top) => {
                adjustment.set_value(top);
                true
            }
            reading::Step::Stay => true,
            reading::Step::MoveOn => false,
        }
    }

    /// Gives focus to Play, or to Stop while the game runs.
    pub fn focus_play(&self) {
        self.imp().play_button.grab_focus();
    }

    /// The star shows whether the game is a favourite.
    fn show_favourite(&self, favourite: bool) {
        let button = &self.imp().favourite_button;
        let (icon, tooltip) = if favourite {
            ("starred-symbolic", gettext("Remove from Favourites"))
        } else {
            ("non-starred-symbolic", gettext("Add to Favourites"))
        };
        button.set_icon_name(icon);
        button.set_tooltip_text(Some(&tooltip));
    }

    /// Play turns into Stop while a game we started runs, and into Return while its store
    /// launcher runs it.
    fn show_playing(&self, playing: Playing) {
        let button = &self.imp().play_button;
        let (label, action, style) = match playing {
            Playing::Started => (gettext("_Stop"), "app.stop", "destructive-action"),
            Playing::InStore => (gettext("_Return"), "win.playing-return", "suggested-action"),
            Playing::No => (gettext("_Play"), "app.play", "suggested-action"),
        };
        button.set_label(&label);
        button.set_action_name(Some(action));
        for old_style in ["destructive-action", "suggested-action"] {
            button.remove_css_class(old_style);
        }
        button.add_css_class(style);
        let imp = self.imp();
        imp.playing_card
            .set_visible(playing != Playing::No && imp.fullscreen.get());
    }

    /// Shows the rows a game's settings have and saves what the user changes.
    fn connect_launch_settings(&self, settings: LaunchSettings) {
        let imp = self.imp();
        imp.proton_row.set_visible(settings.proton);
        imp.prefix_row.set_visible(settings.proton);
        imp.arguments_row.set_visible(settings.arguments);
        if settings.proton {
            let choice = ProtonChoice::new(&imp.proton_row, FirstChoice::Default, None);
            choice.connect_changed(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |name| page.edit_launch(move |config| config.proton_version = name)
            ));
            imp.proton.set(choice).expect("connected once");
        }
        for (row, set) in [
            (
                &*imp.mangohud_row,
                (|config, on| config.mangohud = on) as fn(&mut LaunchConfig, bool),
            ),
            (&*imp.gamemode_row, |config, on| config.gamemode = on),
        ] {
            row.connect_active_notify(glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |row| {
                    if !page.imp().filling.get() {
                        let on = row.is_active();
                        page.edit_launch(move |config| set(config, on));
                    }
                }
            ));
        }
        for row in [&imp.arguments_row, &imp.environment_row, &imp.wrappers_row] {
            row.connect_changed(|row| row.remove_css_class("error"));
        }
    }

    /// Reads the game's launch settings again and shows them; the Edit dialog may have
    /// changed them.
    pub fn load_launch_settings(&self) {
        let Some((library, id)) = self.imp().launch_target.borrow().clone() else {
            return;
        };
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            let config = library.launch_config(&id).await;
            if let Some(page) = page.upgrade() {
                page.show_launch_settings(&id, config);
            }
        });
    }

    fn show_launch_settings(&self, id: &GameId, config: Option<LaunchConfig>) {
        let imp = self.imp();
        let Some(config) = config else {
            imp.settings_group.set_visible(false);
            return;
        };
        imp.filling.set(true);
        if let Some(proton) = imp.proton.get() {
            proton.select(config.proton_version.clone());
        }
        let prefix = config
            .prefix_path
            .clone()
            .unwrap_or_else(|| proton_launch::defaults().prefixes.join(id.native_id()));
        imp.prefix_row.set_subtitle(&prefix.to_string_lossy());
        imp.prefix.replace(Some(prefix));
        imp.arguments_row
            .set_text(&manual::join_arguments(&config.arguments));
        imp.environment_row
            .set_text(&launch::join_environment(&config.environment));
        imp.mangohud_row.set_active(config.mangohud);
        imp.gamemode_row.set_active(config.gamemode);
        imp.wrappers_row
            .set_text(&manual::join_arguments(&config.wrappers));
        imp.filling.set(false);
        imp.settings_group.set_visible(!imp.fullscreen.get());
    }

    /// Saves a change to the game's launch config.
    fn edit_launch(&self, edit: impl FnOnce(&mut LaunchConfig) + Send + 'static) {
        if let Some((library, id)) = &*self.imp().launch_target.borrow() {
            library.edit_launch_config(id, edit);
        }
    }

    /// Picks the game's prefix. umu runs inside the sandbox, so the path the sandbox sees
    /// is the one kept.
    fn choose_prefix(&self) {
        let mut chooser = gtk::FileDialog::builder()
            .title(gettext("Choose the Game's Prefix"))
            .modal(true);
        if let Some(prefix) = &*self.imp().prefix.borrow() {
            chooser = chooser.initial_folder(&gio::File::for_path(prefix));
        }
        let chooser = chooser.build();
        let root = self.root().and_downcast::<gtk::Window>();
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            let Ok(folder) = chooser.select_folder_future(root.as_ref()).await else {
                return;
            };
            if let (Some(page), Some(path)) = (page.upgrade(), folder.path()) {
                let imp = page.imp();
                imp.prefix_row.set_subtitle(&path.to_string_lossy());
                imp.prefix.replace(Some(path.clone()));
                page.edit_launch(move |config| config.prefix_path = Some(path));
            }
        });
    }
}

/// The row's text read by `parse`; marks the row when the text cannot be read.
fn parsed<T>(row: &adw::EntryRow, parse: impl Fn(&str) -> Result<T, String>) -> Option<T> {
    match parse(&row.text()) {
        Ok(value) => Some(value),
        Err(error) => {
            glib::g_debug!("pindu", "Not saved: {error}");
            row.add_css_class("error");
            None
        }
    }
}

/// The page's "more" menu, built when it opens so collection changes show at once, with
/// `grouping`'s Merge With, Preferred Copy and Split after the collections.
fn more_menu(
    object: &GameObject,
    collections: &[Collection],
    grouping: Option<gio::Menu>,
) -> gio::Menu {
    let game = object.game();
    let hidden = game.hidden;
    let target = game.id.as_str().to_variant();
    let item = |label: String, action: &str| {
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some(action), Some(&target));
        item
    };
    let menu = gio::Menu::new();
    // Games we start ourselves can keep their output, and Proton's log, in a log, and
    // start once without their wrappers, overlay, GameMode or environment.
    if launch::launch_settings(&game).is_some() {
        let logs = gio::Menu::new();
        if !object.is_playing() {
            logs.append_item(&item(gettext("Play with _Logs"), "app.play-with-logs"));
            logs.append_item(&item(gettext("Start _Safely"), "app.start-safely"));
        }
        logs.append_item(&item(gettext("Open Latest L_og"), "win.open-log"));
        // About's Debugging Information with the newest log after it, for a bug report.
        logs.append_item(&item(
            gettext("_Debugging Information with Log"),
            "app.about-with-log",
        ));
        menu.append_section(None, &logs);
    }
    menu.append_section(None, &collection_menu(object, collections));
    if let Some(grouping) = grouping {
        menu.append_section(None, &grouping);
    }
    let manage = gio::Menu::new();
    manage.append_item(&item(gettext("_Edit…"), "app.edit-game"));
    manage.append_item(&item(gettext("Change _Artwork…"), "win.change-artwork"));
    // Any game's saves: a Windows game's prefix and the save folders the user adds.
    manage.append_item(&item(gettext("Save S_napshots…"), "win.save-snapshots"));
    if hidden {
        manage.append_item(&item(gettext("_Unhide"), "win.unhide-game"));
    } else {
        manage.append_item(&item(gettext("_Hide"), "win.hide-game"));
    }
    manage.append_item(&item(gettext("_Remove"), "win.remove-game"));
    if pindu_core::launch::launcher_uri(&game).is_some() {
        manage.append_item(&item(gettext("_Open in Launcher"), "app.open-in-launcher"));
    }
    if game.install_path.is_some() {
        manage.append_item(&item(
            gettext("Open _Install Folder"),
            "win.open-install-folder",
        ));
    }
    menu.append_section(None, &manage);
    // Steam games: pages other players keep, which the browser opens.
    let web = gio::Menu::new();
    if launch::web_page_uri(&game, WebPage::ProtonDb).is_some() {
        web.append_item(&item(gettext("View on _ProtonDB"), "app.open-protondb"));
    }
    if launch::web_page_uri(&game, WebPage::PcGamingWiki).is_some() {
        web.append_item(&item(
            gettext("View on PC_GamingWiki"),
            "app.open-pcgamingwiki",
        ));
    }
    if web.n_items() > 0 {
        menu.append_section(None, &web);
    }
    menu
}

/// Whether the desktop page shows the Launch group: for store games, whose launcher it
/// names.
fn shows_launch_group(game: &Game) -> bool {
    game.launch_kind == LaunchKind::StoreUri && launcher_name(game.source()).is_some()
}

/// How Play starts the game, for fullscreen mode's launch summary.
fn launch_summary(game: &Game) -> String {
    match (game.launch_kind, launcher_name(game.source())) {
        (LaunchKind::StoreUri, Some(launcher)) => {
            gettext("{launcher} starts the game, keeps its settings and tracks its playtime")
                .replace("{launcher}", launcher)
        }
        (LaunchKind::StoreUri, None) => gettext("Its launcher starts the game"),
        (LaunchKind::HostCommand | LaunchKind::DesktopEntry, _)
            if pindu_core::launch::tracks_sessions(game) =>
        {
            gettext("Starts on this computer; its playtime is recorded here")
        }
        (LaunchKind::HostCommand | LaunchKind::DesktopEntry, _) => {
            gettext("Starts on this computer")
        }
        (LaunchKind::Proton, _) => gettext("Starts on this computer with Proton"),
        (LaunchKind::RuntimeNative, _) => {
            gettext("Starts on this computer in the Steam Linux Runtime")
        }
    }
}

/// The facts row: launcher, playtime, last played, date added and, when known, install size.
fn facts(game: &Game, playtime_secs: i64, origin: Option<PlaytimeOrigin>) -> Vec<(String, String)> {
    let mut facts = vec![
        (gettext("Launcher"), source_name(game.source())),
        (playtime_caption(origin), playtime(playtime_secs)),
        (
            gettext("Last Played"),
            game.last_played_at.map_or_else(|| gettext("Never"), date),
        ),
        (gettext("Added"), date(game.added_at)),
    ];
    if let Some(size) = game.size_bytes.filter(|size| *size > 0) {
        facts.push((gettext("Size"), glib::format_size(size as u64).to_string()));
    }
    facts
}

/// A genre as the page shows it: the genres Steam's importer names, translated; any other
/// as its source wrote it.
fn genre_label(genre: &str) -> String {
    match genre {
        "Action" => gettext("Action"),
        "Strategy" => gettext("Strategy"),
        "RPG" => gettext("RPG"),
        "Casual" => gettext("Casual"),
        "Racing" => gettext("Racing"),
        "Sports" => gettext("Sports"),
        "Indie" => gettext("Indie"),
        "Adventure" => gettext("Adventure"),
        "Simulation" => gettext("Simulation"),
        "Massively Multiplayer" => gettext("Massively Multiplayer"),
        other => other.to_owned(),
    }
}

/// How well a game supports a controller, as the page shows it.
fn controller_support_label(support: ControllerSupport) -> String {
    match support {
        ControllerSupport::Full => gettext("Full support"),
        ControllerSupport::Partial => gettext("Partial support"),
    }
}

fn fact(caption: &str, value: &str) -> gtk::Box {
    let fact = gtk::Box::new(gtk::Orientation::Vertical, 2);
    fact.append(
        &gtk::Label::builder()
            .label(caption)
            .xalign(0.0)
            .css_classes(["caption", "dimmed"])
            .build(),
    );
    fact.append(
        &gtk::Label::builder()
            .label(value)
            .xalign(0.0)
            .css_classes(["heading"])
            .build(),
    );
    fact
}

/// One copy of a grouped game: a radio marking the preferred copy, which chooses the copy
/// when activated, its source, whether it is installed, its own playtime and, on the copy
/// Play starts, a note saying so.
fn copy_row(
    copy: &GameCopy,
    preferred: bool,
    plays: bool,
    group: Option<&gtk::CheckButton>,
) -> (adw::ActionRow, gtk::CheckButton) {
    let installed = if copy.game.installed && copy.game.in_library() {
        gettext("Installed")
    } else {
        gettext("Not installed")
    };
    let subtitle = if copy.playtime_secs > 0 {
        format!(
            "{installed} · {}",
            played(copy.playtime_secs, copy.playtime_origin)
        )
    } else {
        installed
    };
    let row = adw::ActionRow::builder()
        .title(source_name(copy.game.source()))
        .subtitle(subtitle)
        .use_markup(false)
        .build();
    let choose = gtk::CheckButton::builder()
        .active(preferred)
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Preferred copy"))
        .build();
    choose.set_group(group);
    let target = copy.game.id.as_str().to_variant();
    choose.connect_toggled(move |check| {
        if check.is_active() {
            // The page shows again with the copies in their new order.
            let _ = check.activate_action("win.prefer-copy", Some(&target));
        }
    });
    row.add_prefix(&choose);
    row.set_activatable_widget(Some(&choose));
    if plays {
        row.add_suffix(
            &gtk::Label::builder()
                .label(gettext("Play starts this copy"))
                .css_classes(["dimmed"])
                .build(),
        );
    }
    (row, choose)
}

/// A playtime with where it comes from, as a list row's subtitle shows it.
pub(crate) fn played(secs: i64, origin: Option<PlaytimeOrigin>) -> String {
    let played = playtime(secs);
    match origin {
        Some(PlaytimeOrigin::Source(source)) => gettext("{time} in {source}")
            .replace("{time}", &played)
            .replace("{source}", &source_name(source)),
        Some(PlaytimeOrigin::Sessions) => {
            gettext("{time}, tracked here").replace("{time}", &played)
        }
        Some(PlaytimeOrigin::Copies(copies)) => ngettext(
            "{time} across {n} copy",
            "{time} across {n} copies",
            copies as u32,
        )
        .replace("{time}", &played)
        .replace("{n}", &copies.to_string()),
        None => played,
    }
}

/// "Playtime", with where it comes from: the launcher, our own sessions, or a group's
/// copies played apart.
pub(crate) fn playtime_caption(origin: Option<PlaytimeOrigin>) -> String {
    match origin {
        Some(PlaytimeOrigin::Source(source)) => {
            gettext("Playtime · {source}").replace("{source}", &source_name(source))
        }
        Some(PlaytimeOrigin::Sessions) => gettext("Playtime · Tracked Here"),
        Some(PlaytimeOrigin::Copies(copies)) => ngettext(
            "Playtime · Across {n} Copy",
            "Playtime · Across {n} Copies",
            copies as u32,
        )
        .replace("{n}", &copies.to_string()),
        None => gettext("Playtime"),
    }
}

pub(crate) fn playtime(secs: i64) -> String {
    let minutes = secs / 60;
    let (hours, minutes) = (minutes / 60, minutes % 60);
    match (hours, minutes) {
        (0, 0) => gettext("None yet"),
        (0, minutes) => ngettext("{n} minute", "{n} minutes", minutes as u32)
            .replace("{n}", &minutes.to_string()),
        (hours, _) => {
            ngettext("{n} hour", "{n} hours", hours as u32).replace("{n}", &hours.to_string())
        }
    }
}

pub(crate) fn date(timestamp: Timestamp) -> String {
    glib::DateTime::from_unix_local(timestamp)
        .and_then(|date| date.format("%x"))
        .map(|text| text.to_string())
        .unwrap_or_default()
}

/// Steam's Steam Deck rating in words, with the day Valve tested the game when it says.
fn deck_rating(deck: &DeckCompatibility) -> String {
    let rating = match deck.rating {
        DeckRating::Verified => gettext("Verified"),
        DeckRating::Playable => gettext("Playable"),
        DeckRating::Unsupported => gettext("Unsupported"),
        DeckRating::Unknown => gettext("Unknown"),
    };
    match deck.tested_at {
        // Translators: a Steam Deck rating, such as "Verified", and the day Valve tested the game.
        Some(tested_at) => gettext("{rating}, tested {date}")
            .replace("{rating}", &rating)
            .replace("{date}", &date(tested_at)),
        None => rating,
    }
}
