//! The library's cover tile: 2:3 art with a launcher badge top left, a state badge top
//! right, a Play button on hover and keyboard focus, and the title below (not in
//! fullscreen mode).

use std::cell::{Cell, OnceCell, RefCell};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, gio, glib};
use pindu_core::library::{self, GameState};

use super::CoverArt;
use crate::models::{GameObject, LibrarySelection};
use crate::permission_service;
use crate::sources::{source_icon, source_name};

/// The three tile sizes, as cover width and height in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TileSize {
    /// 150 × 225.
    Small,
    /// 200 × 300.
    #[default]
    Medium,
    /// 250 × 375.
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

    /// The cover's width and height, in logical pixels.
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
        pub selected_check: TemplateChild<gtk::CheckButton>,
        #[template_child]
        pub title: TemplateChild<gtk::Inscription>,
        /// The shown game and the handlers that follow its changes.
        pub bound: RefCell<Option<(GameObject, Vec<glib::SignalHandlerId>)>>,
        /// From the `show-badges` setting.
        pub show_badges: Cell<bool>,
        /// The context menu, made on first use: a grid makes and drops tiles by the
        /// hundred while searching, and most never open their menu.
        pub menu: OnceCell<gtk::PopoverMenu>,
        /// Selection mode's selection and the handler that follows it.
        pub selection: RefCell<Option<(LibrarySelection, glib::SignalHandlerId)>>,
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
            // GTK names a button after its label through a relation, which would win over
            // the name `show_state` gives it.
            self.play_button
                .reset_relation(gtk::AccessibleRelation::LabelledBy);
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
            if let Some((selection, handler)) = self.selection.take() {
                selection.disconnect(handler);
            }
        }
    }
    impl WidgetImpl for CoverTile {}
    impl BoxImpl for CoverTile {}
}

glib::wrapper! {
    /// One game's tile in the library grid.
    pub struct CoverTile(ObjectSubclass<imp::CoverTile>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl CoverTile {
    /// A tile in `window`'s grid; it shows its title as `show_titles` asks, except while
    /// the window is in fullscreen mode, where the title takes no space, and a check while
    /// `selection` is in selection mode.
    pub fn new(
        window: &impl IsA<gtk::Window>,
        selection: &LibrarySelection,
        size: TileSize,
        show_titles: bool,
        show_badges: bool,
    ) -> Self {
        let tile: Self = glib::Object::new();
        let (width, height) = size.cover_size();
        let imp = tile.imp();
        imp.cover.set_size_request(width, height);
        // The binding goes with the tile, so tiles dropped while scrolling leave nothing
        // connected to the window.
        window
            .bind_property("fullscreened", &*imp.title, "visible")
            .transform_to(move |_, fullscreen: bool| {
                Some(library::tile_shows_title(show_titles, fullscreen))
            })
            .sync_create()
            .build();
        imp.show_badges.set(show_badges);
        imp.source_badge.set_visible(show_badges);
        let handler = selection.connect_changed(glib::clone!(
            #[weak]
            tile,
            move |_| tile.show_selection()
        ));
        imp.selection.replace(Some((selection.clone(), handler)));
        tile
    }

    /// Shows `game` until [`Self::unbind`].
    pub fn bind(&self, game: &GameObject, item: &gtk::ListItem) {
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
            game.connect_notify_local(
                Some("title"),
                glib::clone!(
                    #[weak(rename_to = tile)]
                    self,
                    #[weak]
                    item,
                    move |game, _| {
                        tile.show_title(game, &item);
                        tile.show_play_button_name(game, game.is_playing());
                    }
                ),
            ),
        ];
        imp.bound.replace(Some((game.clone(), handlers)));
        let entry = game.entry();
        let source = entry.game.source();
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
        self.show_title(game, item);
        self.show_state(game);
        self.show_art(game);
        self.show_selection();
    }

    fn show_title(&self, game: &GameObject, item: &gtk::ListItem) {
        let imp = self.imp();
        let data = game.game();
        let title = &data.title;
        imp.title.set_text(Some(title));
        imp.cover.set_alternative_text(title);
        imp.selected_check
            .update_property(&[gtk::accessible::Property::Label(&select_name(title))]);
        item.set_accessible_label(&Self::accessible_label(game));
    }

    /// The check of selection mode, ticked when the game is selected; the Play button
    /// gives way to it.
    fn show_selection(&self) {
        let imp = self.imp();
        let (active, selected) = match (&*imp.selection.borrow(), self.game()) {
            (Some((selection, _)), Some(game)) => {
                (selection.is_active(), selection.contains(&game.id()))
            }
            _ => (false, false),
        };
        imp.selected_check.set_visible(active);
        imp.selected_check.set_active(selected);
        imp.play_button.set_visible(!active);
    }

    /// The state badge and the Play or Stop button: playing first, then needing
    /// permission, then not installed.
    fn show_state(&self, game: &GameObject) {
        let imp = self.imp();
        let playing = game.is_playing();
        let state = game_state(game);
        let icon = match state {
            GameState::Playing => Some("media-playback-start-symbolic"),
            GameState::NeedsPermission => Some("changes-prevent-symbolic"),
            GameState::NotInstalled => Some("folder-download-symbolic"),
            GameState::Installed => None,
        };
        imp.state_badge
            .set_visible(icon.is_some() && imp.show_badges.get());
        if let Some(icon) = icon {
            imp.state_badge.set_icon_name(Some(icon));
            imp.state_badge.set_tooltip_text(Some(&state_name(state)));
        }
        let (label, action) = if playing {
            (gettext("Stop"), "app.stop")
        } else {
            (gettext("Play"), "app.play")
        };
        imp.play_button.set_label(&label);
        imp.play_button.set_action_name(Some(action));
        self.show_play_button_name(game, playing);
    }

    fn show_play_button_name(&self, game: &GameObject, playing: bool) {
        let name = Self::play_button_name(&game.game().title, playing);
        let button = &self.imp().play_button;
        button.update_property(&[gtk::accessible::Property::Label(&name)]);
    }

    /// What a screen reader says for a game's Play or Stop button: the visible word alone
    /// does not say which game it starts or stops.
    pub fn play_button_name(title: &str, playing: bool) -> String {
        if playing {
            gettext("Stop {title}")
        } else {
            gettext("Play {title}")
        }
        .replace("{title}", title)
    }

    /// Stops showing the bound game and clears the art.
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
        if let Some(game) = self.game() {
            popup_game_menu(self.upcast_ref(), &self.imp().menu, &game, position);
        }
    }

    /// What a screen reader says for the tile: title, launcher and state.
    pub fn accessible_label(game_object: &GameObject) -> String {
        let state = state_name(game_state(game_object));
        let game = game_object.game();
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

/// The game's state, as its tile's badge and its list row show it.
pub(crate) fn game_state(game: &GameObject) -> GameState {
    let playing = game.is_playing();
    let game = game.game();
    GameState::of(
        playing,
        permission_service::needs_permission(&game),
        game.installed,
    )
}

/// What a screen reader says for selection mode's check of a game.
pub(crate) fn select_name(title: &str) -> String {
    gettext("Select {title}").replace("{title}", title)
}

/// The words for a game's state.
pub(crate) fn state_name(state: GameState) -> String {
    match state {
        GameState::Playing => gettext("Playing"),
        GameState::NeedsPermission => gettext("Needs permission"),
        GameState::NotInstalled => gettext("Not installed"),
        GameState::Installed => gettext("Installed"),
    }
}

/// Opens a game's context menu over `anchor`, pointing at `position` or, from the
/// keyboard, at the whole of `anchor`. The popover is made on first use into `popover`:
/// a grid or list makes and drops widgets by the hundred while searching, and most never
/// open their menu.
pub(crate) fn popup_game_menu(
    anchor: &gtk::Widget,
    popover: &OnceCell<gtk::PopoverMenu>,
    game: &GameObject,
    position: Option<(f64, f64)>,
) {
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
    if let Some(window) = anchor.root().and_downcast::<crate::window::PinduWindow>() {
        let collections = window.library().manual_collections();
        menu.append_section(
            None,
            &crate::collection_actions::collection_menu(game, &collections),
        );
        menu.append_section(None, &crate::group_actions::group_menu(&window, game));
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

    let popover = popover.get_or_init(|| {
        let popover = gtk::PopoverMenu::from_model(None::<&gio::MenuModel>);
        popover.set_has_arrow(true);
        popover.set_parent(anchor);
        // Closing the menu drops the focus inside it: give focus back to the grid's cover
        // or the list's row, as a menu button takes it back, so the keyboard and the
        // controller go on from there.
        popover.connect_closed(glib::clone!(
            #[weak]
            anchor,
            move |popover| {
                let lost = popover.root().is_some_and(|root| root.focus().is_none());
                if lost {
                    let mut ancestor = anchor.parent();
                    while let Some(widget) = ancestor {
                        if widget.grab_focus() {
                            break;
                        }
                        ancestor = widget.parent();
                    }
                }
            }
        ));
        popover
    });
    popover.set_menu_model(Some(&menu));
    let rect = match position {
        Some((x, y)) => gdk::Rectangle::new(x as i32, y as i32, 1, 1),
        None => gdk::Rectangle::new(0, 0, anchor.width(), anchor.height()),
    };
    popover.set_pointing_to(Some(&rect));
    popover.popup();
}
