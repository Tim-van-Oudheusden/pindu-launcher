//! One cell of the library list: a game's title, source, state, last played date or
//! playtime, with the game's context menu on right-click or a long press.

use std::cell::{Cell, OnceCell, RefCell};

use gettextrs::gettext;
use gtk::prelude::*;
use gtk::subclass::prelude::*;
use gtk::{gdk, glib};

use super::cover_tile::{game_state, popup_game_menu, state_name};
use crate::models::GameObject;
use crate::sources::{source_icon, source_name};

/// The library list's columns, in their order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListColumn {
    /// The game's title.
    #[default]
    Title,
    /// The launcher the game comes from, with its icon.
    Source,
    /// Playing, needing permission, installed or not.
    State,
    /// The day the game was last played.
    LastPlayed,
    /// The total playtime.
    Playtime,
}

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct GameListCell {
        /// The column the cell is in.
        pub column: Cell<ListColumn>,
        /// The source's icon, in the Source column.
        pub icon: gtk::Image,
        /// The cell's text.
        pub label: gtk::Label,
        /// The shown game and the handlers that follow its changes.
        pub bound: RefCell<Option<(GameObject, Vec<glib::SignalHandlerId>)>>,
        /// The context menu, made on first use.
        pub menu: OnceCell<gtk::PopoverMenu>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for GameListCell {
        const NAME: &'static str = "PinduGameListCell";
        type Type = super::GameListCell;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for GameListCell {
        fn constructed(&self) {
            self.parent_constructed();
            let cell = self.obj();
            cell.set_spacing(6);
            self.icon.set_visible(false);
            self.icon
                .set_accessible_role(gtk::AccessibleRole::Presentation);
            self.label.set_xalign(0.0);
            self.label.set_hexpand(true);
            cell.append(&self.icon);
            cell.append(&self.label);

            let right_click = gtk::GestureClick::builder()
                .button(gdk::BUTTON_SECONDARY)
                .build();
            right_click.connect_pressed(glib::clone!(
                #[weak]
                cell,
                move |gesture, _, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    cell.popup_menu_at(Some((x, y)));
                }
            ));
            cell.add_controller(right_click);

            let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
            long_press.connect_pressed(glib::clone!(
                #[weak]
                cell,
                move |gesture, x, y| {
                    gesture.set_state(gtk::EventSequenceState::Claimed);
                    cell.popup_menu_at(Some((x, y)));
                }
            ));
            cell.add_controller(long_press);
        }

        fn dispose(&self) {
            self.obj().unbind();
            if let Some(menu) = self.menu.get() {
                menu.unparent();
            }
        }
    }

    impl WidgetImpl for GameListCell {}
    impl BoxImpl for GameListCell {}
}

glib::wrapper! {
    /// One game's cell in a column of the library list.
    pub struct GameListCell(ObjectSubclass<imp::GameListCell>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl GameListCell {
    /// A cell of `column`.
    pub fn new(column: ListColumn) -> Self {
        let cell: Self = glib::Object::new();
        cell.imp().column.set(column);
        if column == ListColumn::Title {
            cell.imp()
                .label
                .set_ellipsize(gtk::pango::EllipsizeMode::End);
        }
        cell
    }

    /// Shows `game` until [`Self::unbind`], following its changes.
    pub fn bind(&self, game: &GameObject) {
        self.unbind();
        let handlers = vec![
            game.connect_changed(glib::clone!(
                #[weak(rename_to = cell)]
                self,
                move |game| cell.show(game)
            )),
            game.connect_notify_local(
                Some("playing"),
                glib::clone!(
                    #[weak(rename_to = cell)]
                    self,
                    move |game, _| cell.show(game)
                ),
            ),
        ];
        self.imp().bound.replace(Some((game.clone(), handlers)));
        self.show(game);
    }

    /// Stops showing the bound game.
    pub fn unbind(&self) {
        if let Some((game, handlers)) = self.imp().bound.take() {
            for handler in handlers {
                game.disconnect(handler);
            }
        }
    }

    /// The game this cell shows, if bound.
    pub fn game(&self) -> Option<GameObject> {
        self.imp()
            .bound
            .borrow()
            .as_ref()
            .map(|(game, _)| game.clone())
    }

    /// Opens the game's context menu, pointing at `position` or, from the keyboard, at
    /// the whole cell.
    pub fn popup_menu_at(&self, position: Option<(f64, f64)>) {
        if let Some(game) = self.game() {
            popup_game_menu(self.upcast_ref(), &self.imp().menu, &game, position);
        }
    }

    fn show(&self, game: &GameObject) {
        let imp = self.imp();
        let text = match imp.column.get() {
            ListColumn::Title => game.game().title.clone(),
            ListColumn::Source => {
                let source = game.game().source();
                imp.icon.set_icon_name(Some(source_icon(source)));
                imp.icon.set_visible(true);
                source_name(source)
            }
            ListColumn::State => state_name(game_state(game)),
            ListColumn::LastPlayed => game
                .game()
                .last_played_at
                .map_or_else(|| gettext("Never"), crate::game_page::date),
            ListColumn::Playtime => crate::game_page::playtime(game.entry().playtime_secs),
        };
        imp.label.set_label(&text);
    }
}
