//! The game page: hero art with the logo or title, Play with default focus, a ⋮ menu, a
//! facts row, the About group and, for store games, a read-only launch summary.

use std::cell::RefCell;

use adw::prelude::*;
use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::{gio, glib};
use pindu_core::model::{Collection, Game, LaunchKind, Source, Timestamp};

use crate::collection_actions::collection_menu;
use crate::launch_service::launcher_name;
use crate::library_service::LibraryService;
use crate::models::GameObject;
use crate::permission_service;
use crate::sources::source_name;
use crate::widgets::textures;

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
        pub facts: TemplateChild<adw::WrapBox>,
        #[template_child]
        pub about_group: TemplateChild<adw::PreferencesGroup>,
        #[template_child]
        pub developer_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub genres_row: TemplateChild<adw::ActionRow>,
        #[template_child]
        pub description: TemplateChild<gtk::Label>,
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
        /// The shown game and its `notify::playing` handler.
        pub playing_handler: RefCell<Option<(GameObject, glib::SignalHandlerId)>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GamePage {
        const NAME: &'static str = "PinduGamePage";
        type Type = super::GamePage;
        type ParentType = adw::NavigationPage;

        fn class_init(klass: &mut Self::Class) {
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for GamePage {
        fn dispose(&self) {
            if let Some((object, handler)) = self.playing_handler.take() {
                object.disconnect(handler);
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
    pub struct GamePage(ObjectSubclass<imp::GamePage>)
        @extends adw::NavigationPage, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget;
}

impl GamePage {
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
        imp.play_button.set_action_target_value(Some(&target));
        let handler = object.connect_notify_local(
            Some("playing"),
            glib::clone!(
                #[weak(rename_to = page)]
                self,
                move |object, _| page.show_playing(object.is_playing())
            ),
        );
        imp.playing_handler.replace(Some((object.clone(), handler)));
        self.show_playing(object.is_playing());
        imp.open_in_launcher.set_action_target_value(Some(&target));
        imp.more_button.set_create_popup_func(glib::clone!(
            #[weak]
            object,
            #[weak]
            library,
            move |button| {
                let collections = library.model().collections();
                button.set_menu_model(Some(&more_menu(&object, &collections)));
            }
        ));

        for (caption, value) in facts(game, entry.playtime_secs) {
            imp.facts.append(&fact(&caption, &value));
        }

        imp.developer_row.set_visible(game.developer.is_some());
        imp.developer_row
            .set_subtitle(game.developer.as_deref().unwrap_or_default());
        imp.genres_row.set_visible(!game.genres.is_empty());
        imp.genres_row.set_subtitle(&game.genres.join(", "));
        imp.about_group
            .set_visible(game.developer.is_some() || !game.genres.is_empty());
        imp.description.set_visible(game.description.is_some());
        imp.description
            .set_label(game.description.as_deref().unwrap_or_default());

        imp.permission_banner
            .set_revealed(permission_service::needs_permission(game));
        let store_launcher = launcher_name(game.source());
        imp.launch_group
            .set_visible(game.launch_kind == LaunchKind::StoreUri && store_launcher.is_some());
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

        let (hero, cover, logo) = (
            entry.art.hero.clone(),
            entry.art.cover.clone(),
            entry.art.logo.clone(),
        );
        drop(entry);
        let page = self.downgrade();
        glib::spawn_future_local(async move {
            // Without hero art, the cover stands in, cropped by the picture.
            if let Some(path) = hero.or(cover)
                && let (Ok(texture), Some(page)) = (textures::load(&path).await, page.upgrade())
            {
                page.imp().hero.set_paintable(Some(&texture));
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
    }
}

impl GamePage {
    /// Play turns into Stop while a game we started runs.
    fn show_playing(&self, playing: bool) {
        let button = &self.imp().play_button;
        let (label, action, style, old_style) = if playing {
            (
                gettext("_Stop"),
                "app.stop",
                "destructive-action",
                "suggested-action",
            )
        } else {
            (
                gettext("_Play"),
                "app.play",
                "suggested-action",
                "destructive-action",
            )
        };
        button.set_label(&label);
        button.set_action_name(Some(action));
        button.remove_css_class(old_style);
        button.add_css_class(style);
    }
}

/// The page's "more" menu, built when it opens so collection changes show at once.
fn more_menu(object: &GameObject, collections: &[Collection]) -> gio::Menu {
    let game = object.game();
    let target = game.id.as_str().to_variant();
    let item = |label: String, action: &str| {
        let item = gio::MenuItem::new(Some(&label), None);
        item.set_action_and_target_value(Some(action), Some(&target));
        item
    };
    let menu = gio::Menu::new();
    menu.append_section(None, &collection_menu(object, collections));
    let manage = gio::Menu::new();
    manage.append_item(&item(gettext("_Edit…"), "app.edit-game"));
    manage.append_item(&item(gettext("_Hide"), "win.hide-game"));
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
    menu
}

/// The facts row: launcher, playtime, last played, date added and, when known, install size.
fn facts(game: &Game, playtime_secs: i64) -> Vec<(String, String)> {
    let mut facts = vec![
        (gettext("Launcher"), source_name(game.source())),
        (gettext("Playtime"), playtime(playtime_secs)),
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

fn playtime(secs: i64) -> String {
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

fn date(timestamp: Timestamp) -> String {
    glib::DateTime::from_unix_local(timestamp)
        .and_then(|date| date.format("%x"))
        .map(|text| text.to_string())
        .unwrap_or_default()
}
