//! The library's cover tile: 2:3 art with a launcher badge top left, a state badge top
//! right, a Play button on hover and keyboard focus, and the title below.

use std::cell::RefCell;

use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use gtk::subclass::prelude::*;

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

    impl ObjectImpl for CoverTile {}
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
