//! The library's cover tile: 2:3 art with a launcher badge top left, a state badge top
//! right, a Play button on hover and keyboard focus, and the title below.

use std::cell::{Cell, OnceCell, RefCell};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib};

use super::CoverArt;
use crate::models::GameObject;
use crate::permission_service;
use crate::sources::{source_icon, source_name};

/// The three tile sizes, as cover width and height in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TileSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl TileSize {
    /// From the `tile-size` setting.
    pub fn from_setting(value: &str) -> Self {
        match value {
            "small" => Self::Small,
            "large" => Self::Large,
            _ => Self::Medium,
        }
    }

    pub fn cover_size(self) -> (i32, i32) {
        match self {
            Self::Small => (150, 225),
            Self::Medium => (200, 300),
            Self::Large => (250, 375),
        }
    }
}

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/cover_tile.ui")]
    pub struct CoverTile {
        #[template_child]
        pub cover: TemplateChild<CoverArt>,
        #[template_child]
        pub placeholder: TemplateChild<gtk::Image>,
        #[template_child]
        pub source_badge: TemplateChild<gtk::Image>,
        #[template_child]
        pub state_badge: TemplateChild<gtk::Image>,
        #[template_child]
        pub play_button: TemplateChild<gtk::Button>,
        #[template_child]
        pub title: TemplateChild<gtk::Inscription>,
        /// The shown game and the handler that follows its art.
        pub bound: RefCell<Option<(GameObject, Vec<glib::SignalHandlerId>)>>,
        /// From the `show-badges` setting.
        pub show_badges: Cell<bool>,
        /// The context menu, made on first use: a grid makes and drops tiles by the
        /// hundred while searching, and most never open their menu.
        pub menu: OnceCell<gtk::PopoverMenu>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for CoverTile {
        const NAME: &'static str = "PinduCoverTile";
        type Type = super::CoverTile;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            CoverArt::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for CoverTile {
        fn constructed(&self) {
            self.parent_constructed();
            let tile = self.obj();
            let right_click = gtk::GestureClick::builder()
                .button(gdk::BUTTON_SECONDARY)
                .build();
            right_click.connect_pressed(glib::clone!(
                #[weak]
                tile,
                move |gesture, _, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    tile.popup_menu_at(Some((x, y)));
                }
            ));
            tile.add_controller(right_click);

            let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
            long_press.connect_pressed(glib::clone!(
                #[weak]
                tile,
                move |gesture, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    tile.popup_menu_at(Some((x, y)));
                }
            ));
            tile.add_controller(long_press);
        }

        fn dispose(&self) {
            if let Some(menu) = self.menu.get() {
                menu.unparent();
            }
        }
    }
    impl WidgetImpl for CoverTile {}
    impl BoxImpl for CoverTile {}
}

glib::wrapper! {
    pub struct CoverTile(ObjectSubclass<imp::CoverTile>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl CoverTile {
    pub fn new(size: TileSize, show_title: bool, show_badges: bool) -> Self {
        let tile: Self = glib::Object::new();
        let (width, height) = size.cover_size();
        let imp = tile.imp();
        imp.cover.set_size_request(width, height);
        imp.title.set_visible(show_title);
        imp.show_badges.set(show_badges);
        imp.source_badge.set_visible(show_badges);
        tile
    }

    /// Shows `game` until [`Self::unbind`].
    pub fn bind(&self, game: &GameObject) {
        self.unbind();
        let imp = self.imp();
        let handlers = vec![
            game.connect_art_changed(glib::clone!(
                #[weak(rename_to = tile)]
                self,
                move |game| tile.show_art(game)
            )),
            game.connect_notify_local(
                Some("playing"),
                glib::clone!(
                    #[weak(rename_to = tile)]
                    self,
                    move |game, _| tile.show_state(game)
                ),
            ),
        ];
        imp.bound.replace(Some((game.clone(), handlers)));
        let entry = game.entry();
        let source = entry.game.source();
        imp.title.set_text(Some(&entry.game.title));
        imp.source_badge.set_icon_name(Some(source_icon(source)));
        imp.source_badge
            .set_tooltip_text(Some(&source_name(source)));
        if entry.game.installed {
            imp.cover.remove_css_class("not-installed");
        } else {
            imp.cover.add_css_class("not-installed");
        }
        imp.play_button
            .set_action_target_value(Some(&entry.game.id.as_str().to_variant()));
        drop(entry);
        self.show_state(game);
        self.show_art(game);
    }

    /// The state badge and the Play or Stop button: playing first, then needing
    /// permission, then not installed.
    fn show_state(&self, game: &GameObject) {
        let imp = self.imp();
        let playing = game.is_playing();
        let (installed, needs_permission) = {
            let game = game.game();
            (game.installed, permission_service::needs_permission(&game))
        };
        let state = if playing {
            Some(("media-playback-start-symbolic", gettext("Playing")))
        } else if needs_permission {
            Some(("changes-prevent-symbolic", gettext("Needs permission")))
        } else if !installed {
            Some(("folder-download-symbolic", gettext("Not installed")))
        } else {
            None
        };
        imp.state_badge
            .set_visible(state.is_some() && imp.show_badges.get());
        if let Some((icon, tooltip)) = state {
            imp.state_badge.set_icon_name(Some(icon));
            imp.state_badge.set_tooltip_text(Some(&tooltip));
        }
        let (label, action) = if playing {
            (gettext("Stop"), "app.stop")
        } else {
            (gettext("Play"), "app.play")
        };
        imp.play_button.set_label(&label);
        imp.play_button.set_action_name(Some(action));
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        if let Some((game, handlers)) = imp.bound.take() {
            for handler in handlers {
                game.disconnect(handler);
            }
        }
        imp.cover.clear();
    }

    /// The game this tile shows, if bound.
    pub fn game(&self) -> Option<GameObject> {
        self.imp()
            .bound
            .borrow()
            .as_ref()
            .map(|(game, _)| game.clone())
    }

    /// Opens the tile's context menu, pointing at `position` or, from the keyboard, at the
    /// whole tile.
    pub fn popup_menu_at(&self, position: Option<(f64, f64)>) {
        let Some(game) = self.game() else { return };
        let id = game.id();
        let target = id.as_str().to_variant();
        let item = |label: String, action: &str| {
            let item = gio::MenuItem::new(Some(&label), None);
            item.set_action_and_target_value(Some(action), Some(&target));
            item
        };
        let menu = gio::Menu::new();
        let actions = gio::Menu::new();
        actions.append_item(&item(gettext("_Play"), "app.play"));
        actions.append_item(&item(gettext("_Details"), "win.show-details"));
        if pindu_core::launch::launcher_uri(&game.game()).is_some() {
            actions.append_item(&item(gettext("_Open in Launcher"), "app.open-in-launcher"));
        }
        menu.append_section(None, &actions);
        if let Some(window) = self.root().and_downcast::<crate::window::PinduWindow>() {
            let collections = window.library().collections();
            menu.append_section(
                None,
                &crate::collection_actions::collection_menu(&game, &collections),
            );
        }
        let (favourite, pinned, hidden) = {
            let game = game.game();
            (game.favourite, game.pin_order.is_some(), game.hidden)
        };
        let keep = gio::Menu::new();
        keep.append_item(&item(
            if favourite {
                gettext("Remove from _Favourites")
            } else {
                gettext("Add to _Favourites")
            },
            "win.toggle-favourite",
        ));
        keep.append_item(&item(
            if pinned {
                gettext("Un_pin from Jump Back In")
            } else {
                gettext("_Pin to Jump Back In")
            },
            "win.toggle-pin",
        ));
        menu.append_section(None, &keep);
        let manage = gio::Menu::new();
        manage.append_item(&item(gettext("_Edit…"), "app.edit-game"));
        if hidden {
            manage.append_item(&item(gettext("_Unhide"), "win.unhide-game"));
        } else {
            manage.append_item(&item(gettext("_Hide"), "win.hide-game"));
        }
        manage.append_item(&item(gettext("_Remove"), "win.remove-game"));
        menu.append_section(None, &manage);

        let popover = self.imp().menu.get_or_init(|| {
            let popover = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
            popover.set_has_arrow(true);
            popover.set_parent(self);
            popover
        });
        popover.set_menu_model(Some(&menu));
        let rect = match position {
            Some((x, y)) => gdk::Rectangle::new(x as i32, y as i32, 1, 1),
            None => gdk::Rectangle::new(0, 0, self.width(), self.height()),
        };
        popover.set_pointing_to(Some(&rect));
        popover.popup();
    }

    /// What a screen reader says for the tile: title, launcher and state.
    pub fn accessible_label(game_object: &GameObject) -> String {
        let game = game_object.game();
        let state = if game_object.is_playing() {
            gettext("Playing")
        } else if permission_service::needs_permission(&game) {
            gettext("Needs permission")
        } else if game.installed {
            gettext("Installed")
        } else {
            gettext("Not installed")
        };
        format!("{}, {}, {state}", game.title, source_name(game.source()))
    }

    fn show_art(&self, game: &GameObject) {
        let imp = self.imp();
        let (cover, icon) = {
            let art = game.art();
            (art.cover.clone(), art.icon.clone())
        };
        imp.placeholder.set_visible(false);
        // Covers are stored at the tile's size in device pixels, so they stay sharp on
        // scaled displays.
        let (width, height) = imp.cover.size_request();
        let scale = self.scale_factor().max(1);
        let size = ((width * scale) as u32, (height * scale) as u32);
        imp.cover.show(
            cover,
            icon,
            size,
            glib::clone!(
                #[weak(rename_to = tile)]
                self,
                move |cover| tile.imp().placeholder.set_visible(!cover.has_art())
            ),
        );
    }
}
