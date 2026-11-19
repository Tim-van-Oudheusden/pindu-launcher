//! The menu drawer, fullscreen mode's panel on the left, as a widget a window hosts:
//! [`PinduMenuDrawer`], with its own Blueprint (`data/ui/menu_drawer.blp`). It holds the
//! level stack: the main level with the Playing Now cards (`fullscreen_playing`) and the
//! entries, the collections and one collection's actions, Controls (`fullscreen_bindings`)
//! and Power (`fullscreen_power`).
//!
//! Which level is open is the host's (`pindu_core::panels::Panels`): the host shows a
//! level with `show_level` and fills the entries with `fill`; the rows' `win.*` actions
//! are the host's. The main window hosts it on the left, where Start or Guide opens it over
//! the page. Over a running game in a `--steam` gamescope the overlay window
//! (`game_overlay`) hosts it too, where Guide opens it over the game and it leaves out
//! what would end the session or change the controls (`set_over_game`).

use std::cell::{Cell, OnceCell, RefCell};

use adw::subclass::prelude::*;
use gettextrs::gettext;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamescope::Xid;
use pindu_core::model::Collection;
use pindu_core::panels::{
    CollectionAction, DrawerEntry, DrawerLevel, collection_actions, drawer_entries,
};

use crate::fullscreen_panels::{drawer_row, step_out_of_list};
use crate::fullscreen_playing::PlayingCards;
use crate::fullscreen_power::PinduPowerLevel;

mod imp {
    use super::*;

    #[derive(Debug, Default, gtk::CompositeTemplate)]
    #[template(resource = "/io/github/tim_van_oudheusden/pindu-launcher/ui/menu_drawer.ui")]
    pub struct PinduMenuDrawer {
        #[template_child]
        pub stack: TemplateChild<gtk::Stack>,
        #[template_child]
        pub playing_cards: TemplateChild<gtk::Box>,
        #[template_child]
        pub windows_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub windows: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub list: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub collections_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub controls_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub exit_fullscreen_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub power_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub collections: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub collection_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub collection_actions: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub controls: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub controls_reset: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub power_level: TemplateChild<PinduPowerLevel>,
        /// The Playing Now cards over the main level's entries.
        pub(crate) cards: OnceCell<PlayingCards>,
        /// The collection the collection level shows the actions of.
        pub collection: Cell<Option<i64>>,
        /// The collection level's actions, a row each.
        pub actions: RefCell<Vec<CollectionAction>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduMenuDrawer {
        const NAME: &'static str = "PinduMenuDrawer";
        type Type = super::PinduMenuDrawer;
        type ParentType = gtk::Box;

        fn class_init(klass: &mut Self::Class) {
            PinduPowerLevel::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduMenuDrawer {
        fn constructed(&self) {
            self.parent_constructed();
            // The windows sit between the cards and the entries. Up from the entries goes
            // to the last window first; this runs before the cards' own handler, which
            // takes Up to the nearest card's Return.
            self.list.connect_keynav_failed(glib::clone!(
                #[weak(rename_to = windows)]
                self.windows,
                #[upgrade_or]
                glib::Propagation::Proceed,
                move |_, direction| {
                    let last = windows.is_visible().then(|| windows.last_child()).flatten();
                    match last {
                        Some(row) if direction == gtk::DirectionType::Up => {
                            row.grab_focus();
                            glib::Propagation::Stop
                        }
                        _ => glib::Propagation::Proceed,
                    }
                }
            ));
            let _ = self
                .cards
                .set(PlayingCards::new(&self.playing_cards, &self.list));
            // Up from the first window goes to the nearest card's Return, Down from the last
            // to the entries.
            self.windows.connect_keynav_failed(glib::clone!(
                #[weak(rename_to = cards)]
                self.playing_cards,
                #[weak(rename_to = list)]
                self.list,
                #[upgrade_or]
                glib::Propagation::Stop,
                move |_, direction| {
                    let target = match direction {
                        gtk::DirectionType::Up => cards
                            .last_child()
                            .as_ref()
                            .and_then(crate::fullscreen_playing::card_return),
                        gtk::DirectionType::Down => list.row_at_index(0).map(|row| row.upcast()),
                        _ => None,
                    };
                    if let Some(target) = target {
                        target.grab_focus();
                    }
                    glib::Propagation::Stop
                }
            ));
            // Down from the last function goes on to Reset to Defaults, and Up back.
            for list in [self.controls.get(), self.controls_reset.get()] {
                list.connect_keynav_failed(|list, direction| {
                    step_out_of_list(list, direction);
                    glib::Propagation::Stop
                });
            }
        }
    }

    impl WidgetImpl for PinduMenuDrawer {}

    impl BoxImpl for PinduMenuDrawer {}
}

glib::wrapper! {
    /// Fullscreen mode's menu drawer: the Playing Now cards, Home, Library, Collections,
    /// Controls, leaving fullscreen mode and Power, with Collections, Controls and Power as
    /// levels of their own; quitting is on the Power level.
    pub struct PinduMenuDrawer(ObjectSubclass<imp::PinduMenuDrawer>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl PinduMenuDrawer {
    /// The drawer's Playing Now cards.
    pub(crate) fn cards(&self) -> &PlayingCards {
        self.imp().cards.get().expect("made as the drawer is built")
    }

    /// Leaves out the entries that would leave fullscreen mode, end the session or change
    /// the controls, for the drawer over a running game: Controls, Exit Fullscreen Mode and
    /// Power, which holds Exit.
    pub(crate) fn set_over_game(&self) {
        let imp = self.imp();
        imp.controls_row.set_visible(false);
        imp.exit_fullscreen_row.set_visible(false);
        imp.power_row.set_visible(false);
    }

    /// Shows the entries that apply: Collections while there are `collections`, whose
    /// level lists them, and Power while `power` says its level lists anything.
    pub(crate) fn fill(&self, collections: &[Collection], power: bool) {
        let imp = self.imp();
        let entries = drawer_entries(false, !collections.is_empty(), power);
        imp.collections_row
            .set_visible(entries.contains(&DrawerEntry::Collections));
        imp.power_row
            .set_visible(entries.contains(&DrawerEntry::Power));
        imp.collections.remove_all();
        for collection in collections {
            let row = drawer_row("folder-symbolic", &collection.name);
            row.set_action_name(Some("win.drawer-collection"));
            row.set_action_target_value(Some(&collection.id.to_variant()));
            imp.collections.append(&row);
        }
    }

    /// Lists `windows`, the windows of the game in front with their titles, when it has
    /// more than one, each leading to `win.game-window` with its window; hides the list
    /// otherwise.
    pub(crate) fn show_windows(&self, windows: &[(Xid, Option<String>)]) {
        let imp = self.imp();
        imp.windows.remove_all();
        let shown = windows.len() > 1;
        imp.windows_title.set_visible(shown);
        imp.windows.set_visible(shown);
        if !shown {
            return;
        }
        for (xid, title) in windows {
            let untitled = gettext("Untitled Window");
            let row = drawer_row(
                "focus-windows-symbolic",
                title.as_deref().unwrap_or(&untitled),
            );
            row.set_action_name(Some("win.game-window"));
            row.set_action_target_value(Some(&xid.to_variant()));
            imp.windows.append(&row);
        }
    }

    /// The collection whose row in the collections level has focus, if one does: what View
    /// or X shows the actions of.
    pub(crate) fn focused_collection(&self) -> Option<i64> {
        let list = &self.imp().collections;
        (0..)
            .map_while(|index| list.row_at_index(index))
            .find(|row| row.is_focus())
            .and_then(|row| row_collection(&row))
    }

    /// The collection the collection level shows the actions of.
    pub(crate) fn shown_collection(&self) -> Option<i64> {
        self.imp().collection.get()
    }

    /// Fills the collection level with the actions for the collection `id` of
    /// `collections`, each row leading to its `win.drawer-*-collection` action with the
    /// collection. Shown again for the same collection, after it moved or was renamed,
    /// focus stays on the action it was on, or the move that is left. Says whether
    /// `collections` still has it.
    pub(crate) fn show_collection(&self, collections: &[Collection], id: i64) -> bool {
        let imp = self.imp();
        let Some(index) = collections
            .iter()
            .position(|collection| collection.id == id)
        else {
            imp.collection.set(None);
            return false;
        };
        let focused = (imp.collection.get() == Some(id))
            .then(|| focused_row(&imp.collection_actions))
            .flatten()
            .and_then(|row| imp.actions.borrow().get(row).copied());
        imp.collection.set(Some(id));
        let name = &collections[index].name;
        imp.collection_title.set_label(name);
        imp.collection_actions
            .update_property(&[gtk::accessible::Property::Label(name)]);
        let actions = collection_actions(index, collections.len());
        imp.collection_actions.remove_all();
        for action in &actions {
            let (icon, label, name) = match action {
                CollectionAction::Rename => (
                    "document-edit-symbolic",
                    gettext("Rename…"),
                    "win.drawer-rename-collection",
                ),
                CollectionAction::MoveUp => (
                    "go-up-symbolic",
                    gettext("Move Up"),
                    "win.drawer-move-collection-up",
                ),
                CollectionAction::MoveDown => (
                    "go-down-symbolic",
                    gettext("Move Down"),
                    "win.drawer-move-collection-down",
                ),
                CollectionAction::Delete => (
                    "user-trash-symbolic",
                    gettext("Delete…"),
                    "win.drawer-delete-collection",
                ),
            };
            let row = drawer_row(icon, &label);
            row.set_action_name(Some(name));
            row.set_action_target_value(Some(&id.to_variant()));
            imp.collection_actions.append(&row);
        }
        if let Some(focused) = focused {
            let moves = |action: &CollectionAction| {
                matches!(
                    action,
                    CollectionAction::MoveUp | CollectionAction::MoveDown
                )
            };
            let position = actions
                .iter()
                .position(|action| *action == focused)
                .or_else(|| {
                    moves(&focused)
                        .then(|| actions.iter().position(moves))
                        .flatten()
                })
                .unwrap_or(0);
            if let Some(row) = imp.collection_actions.row_at_index(position as i32) {
                row.grab_focus();
            }
        }
        imp.actions.replace(actions);
        true
    }

    /// Shows `level`.
    pub(crate) fn show_level(&self, level: DrawerLevel) {
        self.imp().stack.set_visible_child_name(match level {
            DrawerLevel::Main => "main",
            DrawerLevel::Collections => "collections",
            DrawerLevel::Collection => "collection",
            DrawerLevel::Power => "power",
            DrawerLevel::Controls => "controls",
        });
    }

    /// Where focus goes as `level` shows, after `left` (the level focus comes from): the
    /// entry that led to a level just left (the collection whose actions were shown, too),
    /// the first Playing Now card's Return, or the level's first entry. Power's own focus
    /// is its host's.
    pub(crate) fn focus_target(
        &self,
        level: DrawerLevel,
        left: Option<DrawerLevel>,
    ) -> Option<gtk::Widget> {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        match (level, left) {
            (DrawerLevel::Main, Some(DrawerLevel::Collections)) => {
                Some(imp.collections_row.get().upcast())
            }
            (DrawerLevel::Main, Some(DrawerLevel::Power)) => Some(imp.power_row.get().upcast()),
            (DrawerLevel::Main, Some(DrawerLevel::Controls)) => {
                Some(imp.controls_row.get().upcast())
            }
            (DrawerLevel::Main, _) if imp.playing_cards.is_visible() => imp
                .playing_cards
                .first_child()
                .as_ref()
                .and_then(crate::fullscreen_playing::card_return),
            (DrawerLevel::Main, _) => first_row(&imp.list),
            (DrawerLevel::Collections, Some(DrawerLevel::Collection)) => {
                let shown = imp.collection.get();
                (0..)
                    .map_while(|index| imp.collections.row_at_index(index))
                    .find(|row| shown.is_some() && row_collection(row) == shown)
                    .map(|row| row.upcast())
                    .or_else(|| first_row(&imp.collections))
            }
            (DrawerLevel::Collections, _) => first_row(&imp.collections),
            (DrawerLevel::Collection, _) => first_row(&imp.collection_actions),
            (DrawerLevel::Power, _) => imp.power_level.focus_target(),
            (DrawerLevel::Controls, _) => first_row(&imp.controls),
        }
    }

    /// The drawer's Power level.
    pub(crate) fn power_level(&self) -> &PinduPowerLevel {
        &self.imp().power_level
    }

    /// The drawer's Power entry.
    pub(crate) fn power_row(&self) -> &gtk::ListBoxRow {
        &self.imp().power_row
    }

    /// The Controls level's list of functions, which its host fills.
    pub(crate) fn controls(&self) -> &gtk::ListBox {
        &self.imp().controls
    }
}

/// The index of the row of `list` that has focus, if one does.
fn focused_row(list: &gtk::ListBox) -> Option<usize> {
    (0..)
        .map_while(|index| list.row_at_index(index))
        .position(|row| row.is_focus())
}

/// The collection a row of the collections level opens.
fn row_collection(row: &gtk::ListBoxRow) -> Option<i64> {
    row.action_target_value()?.get::<i64>()
}
