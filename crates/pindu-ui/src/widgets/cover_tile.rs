//! The library's cover tile: 2:3 art with a launcher badge top left, a state badge top
//! right, a Play button on hover and keyboard focus, and the title below.

use std::cell::{OnceCell, RefCell};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib};

use super::CoverArt;
use crate::models::GameObject;
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
        pub title: TemplateChild<gtk::Label>,
        /// The shown game and the handler that follows its art.
        pub bound: RefCell<Option<(GameObject, glib::SignalHandlerId)>>,
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
            let menu = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
            menu.set_has_arrow(true);
            menu.set_parent(&*tile);
            self.menu.set(menu).expect("constructed once");

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
    pub fn new(size: TileSize, show_title: bool) -> Self {
        let tile: Self = glib::Object::new();
        let (width, height) = size.cover_size();
        let imp = tile.imp();
        imp.cover.set_size_request(width, height);
        imp.title.set_visible(show_title);
        tile
    }

    /// Shows `game` until [`Self::unbind`].
    pub fn bind(&self, game: &GameObject) {
        self.unbind();
        let imp = self.imp();
        let handler = game.connect_art_changed(glib::clone!(
            #[weak(rename_to = tile)]
            self,
            move |game| tile.show_art(game)
        ));
        imp.bound.replace(Some((game.clone(), handler)));
        let entry = game.entry();
        let source = entry.game.source();
        imp.title.set_label(&entry.game.title);
        imp.source_badge.set_icon_name(Some(source_icon(source)));
        imp.source_badge
            .set_tooltip_text(Some(&source_name(source)));
        let installed = entry.game.installed;
        imp.state_badge.set_visible(!installed);
        imp.state_badge
            .set_icon_name(Some("folder-download-symbolic"));
        imp.state_badge
            .set_tooltip_text(Some(&gettext("Not installed")));
        if installed {
            imp.cover.remove_css_class("not-installed");
        } else {
            imp.cover.add_css_class("not-installed");
        }
        imp.play_button
            .set_action_target_value(Some(&entry.game.id.as_str().to_variant()));
        drop(entry);
        self.show_art(game);
    }

    pub fn unbind(&self) {
        let imp = self.imp();
        if let Some((game, handler)) = imp.bound.take() {
            game.disconnect(handler);
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
        actions.append_item(&item(gettext("_Open in Launcher"), "app.open-in-launcher"));
        menu.append_section(None, &actions);
        let manage = gio::Menu::new();
        manage.append_item(&item(gettext("_Hide"), "win.hide-game"));
        menu.append_section(None, &manage);

        let popover = self.imp().menu.get().expect("constructed");
        popover.set_menu_model(Some(&menu));
        let rect = match position {
            Some((x, y)) => gdk::Rectangle::new(x as i32, y as i32, 1, 1),
            None => gdk::Rectangle::new(0, 0, self.width(), self.height()),
        };
        popover.set_pointing_to(Some(&rect));
        popover.popup();
    }

    /// What a screen reader says for the tile: title, launcher and state.
    pub fn accessible_label(game: &GameObject) -> String {
        let game = game.game();
        let state = if game.installed {
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
        imp.cover.show(
            cover,
            icon,
            glib::clone!(
                #[weak(rename_to = tile)]
                self,
                move |cover| tile.imp().placeholder.set_visible(!cover.has_art())
            ),
        );
    }
}
