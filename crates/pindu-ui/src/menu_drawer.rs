//! The menu drawer, fullscreen mode's panel on the left, as a widget a window hosts:
//! [`PinduMenuDrawer`], with its own Blueprint (`data/ui/menu_drawer.blp`). It holds the
//! level stack: the main level with the Playing Now cards (`fullscreen_playing`) and the
//! places (Home, Library with its number of games, Search with the glyph of the button
//! that searches, and the first collections under a Collections heading), every
//! collection, and one collection's actions. Under the main level's scrolling list sits the
//! power dock (`fullscreen_power`), with the ways out of the launcher and the machine's
//! power actions. Up from the main level's first item wraps to the dock, Down from the
//! list's last row enters it; Up from the dock goes back to the list's last row, and Left
//! and Right move along it, stopping at its ends.
//!
//! Which level is open is the host's (`pindu_core::panels::Panels`): the host shows a
//! level with `show_level` and fills the entries with `fill` and the dock with `fill_dock`
//! as the library and the system change; the rows' and the dock's `win.*` actions are the
//! host's. The main window hosts it on the left, where Start or Guide opens it over the
//! page. Over a running game in a `--steam` gamescope the overlay window (`game_overlay`)
//! hosts it too, where Guide opens it over the game and it leaves out what would leave the
//! game or end the session (`set_over_game`), the dock among them.

use std::cell::{Cell, OnceCell, RefCell};

use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::{Intent, Layout};
use pindu_core::gamescope::Xid;
use pindu_core::gamescope::session::{SwitchToDesktop, power_dock};
use pindu_core::hints;
use pindu_core::library::View;
use pindu_core::model::Collection;
use pindu_core::panels::{
    CollectionAction, DrawerEntry, DrawerLevel, collection_actions, drawer_entries,
};
use pindu_core::system::power::PowerState;

use crate::collection_actions::collection_icon;
use crate::fullscreen_panels::drawer_row;
use crate::fullscreen_playing::PlayingCards;
use crate::fullscreen_power::PinduPowerDock;
use crate::hint_bar::{GLYPH_SIZE, glyph_label};
use crate::models::LibraryModel;

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
        pub library_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub library_count: TemplateChild<gtk::Label>,
        #[template_child]
        pub search_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub search_glyph: TemplateChild<gtk::Image>,
        #[template_child]
        pub all_collections_row: TemplateChild<gtk::ListBoxRow>,
        #[template_child]
        pub dock_holder: TemplateChild<adw::Bin>,
        #[template_child]
        pub dock: TemplateChild<PinduPowerDock>,
        #[template_child]
        pub collections: TemplateChild<gtk::ListBox>,
        #[template_child]
        pub collection_title: TemplateChild<gtk::Label>,
        #[template_child]
        pub collection_actions: TemplateChild<gtk::ListBox>,
        /// The first collections' rows in the main level's entries, before All Collections.
        pub inline: RefCell<Vec<gtk::ListBoxRow>>,
        /// Whether the drawer is hosted over a running game.
        pub over_game: Cell<bool>,
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
            PinduPowerDock::ensure_type();
            klass.bind_template();
        }

        fn instance_init(obj: &glib::subclass::InitializingObject<Self>) {
            obj.init_template();
        }
    }

    impl ObjectImpl for PinduMenuDrawer {
        fn constructed(&self) {
            self.parent_constructed();
            // The Collections heading over the first collection among the entries.
            self.list.set_header_func(|row, before| {
                let first = row_collection(row).is_some()
                    && before.is_none_or(|before| row_collection(before).is_none());
                if !first {
                    row.set_header(None::<&gtk::Widget>);
                } else if row.header().is_none() {
                    let heading = gtk::Label::builder()
                        .label(gettext("Collections"))
                        .xalign(0.0)
                        .margin_top(12)
                        .margin_bottom(6)
                        .margin_start(12)
                        .css_classes(["heading"])
                        .build();
                    row.set_header(Some(&heading));
                }
            });
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
            let cards = PlayingCards::new(&self.playing_cards);
            cards.lead_up_from(&self.list);
            let _ = self.cards.set(cards);
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
        }
    }

    impl WidgetImpl for PinduMenuDrawer {
        /// Moves to, from and along the power dock where it lies that way, before the
        /// focused widget's own lists and boxes move focus.
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            match self.obj().dock_step(direction) {
                Some(target) => {
                    target.grab_focus();
                    true
                }
                None => self.parent_focus(direction),
            }
        }
    }

    impl BoxImpl for PinduMenuDrawer {}
}

glib::wrapper! {
    /// Fullscreen mode's menu drawer: the Playing Now cards, Home, Library, Search, the
    /// first collections and All Collections, over the power dock with leaving fullscreen
    /// mode, quitting and the machine's power actions; every collection and one
    /// collection's actions are levels of their own.
    pub struct PinduMenuDrawer(ObjectSubclass<imp::PinduMenuDrawer>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

impl PinduMenuDrawer {
    /// The drawer's Playing Now cards.
    pub(crate) fn cards(&self) -> &PlayingCards {
        self.imp().cards.get().expect("made as the drawer is built")
    }

    /// Leaves out the entries that would leave the game, leave fullscreen mode or end the
    /// session, for the drawer over a running game: Search, and the power dock, which
    /// `fill_dock` then leaves empty.
    pub(crate) fn set_over_game(&self) {
        let imp = self.imp();
        imp.over_game.set(true);
        imp.search_row.set_visible(false);
    }

    /// Shows the entries that apply (`pindu_core::panels::drawer_entries`) as `library`
    /// now is: Library's number of games, the first collections under a Collections
    /// heading, each with its number of games, and All Collections when there are more;
    /// and every collection in the Collections level. A collection that had focus keeps
    /// it.
    pub(crate) fn fill(&self, library: &LibraryModel) {
        let imp = self.imp();
        let collections = library.collections();
        let counts = library.counts();
        let entries = drawer_entries(false, collections.len(), imp.over_game.get());
        let library_count = counts.get(View::All);
        imp.library_count.set_label(&library_count.to_string());
        imp.library_row
            .update_property(&[gtk::accessible::Property::Description(&game_count(
                library_count,
            ))]);
        imp.search_row
            .set_visible(entries.contains(&DrawerEntry::Search));
        imp.all_collections_row
            .set_visible(entries.contains(&DrawerEntry::AllCollections));
        let focused = self.focused_collection();
        let row_for = |collection: &Collection| {
            collection_row(
                collection,
                library.saved_filter(collection.id).is_some(),
                counts.get(View::Collection(collection.id)),
            )
        };
        for row in imp.inline.take() {
            imp.list.remove(&row);
        }
        let position = imp.all_collections_row.index();
        let inline: Vec<gtk::ListBoxRow> = entries
            .iter()
            .filter_map(|entry| match entry {
                DrawerEntry::Collection(index) => collections.get(*index),
                _ => None,
            })
            .map(row_for)
            .collect();
        for (offset, row) in (0..).zip(&inline) {
            imp.list.insert(row, position + offset);
        }
        imp.inline.replace(inline);
        imp.collections.remove_all();
        for collection in &collections {
            imp.collections.append(&row_for(collection));
        }
        if let Some(id) = focused {
            let list = match imp.stack.visible_child_name().as_deref() {
                Some("collections") => imp.collections.get(),
                _ => imp.list.get(),
            };
            let row = collection_rows(&list).find(|row| row_collection(row) == Some(id));
            if let Some(row) = row {
                row.grab_focus();
            }
        }
    }

    /// Ends the Search entry with the glyph of the button that searches on `layout`, the
    /// controller in use; without one, the entry ends with its name.
    pub(crate) fn show_search_button(&self, layout: Option<Layout>) {
        let imp = self.imp();
        let glyph = &imp.search_glyph;
        let Some(layout) = layout else {
            glyph.set_visible(false);
            imp.search_row
                .reset_property(gtk::AccessibleProperty::Description);
            return;
        };
        let button = layout.button(Intent::Search);
        glyph.set_icon_name(Some(hints::glyph(layout.glyphs, button)));
        glyph.set_pixel_size(GLYPH_SIZE);
        glyph.set_visible(true);
        imp.search_row
            .update_property(&[gtk::accessible::Property::Description(&glyph_label(
                hints::glyph_name(layout.glyphs, button),
            ))]);
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

    /// The collection whose row has focus, in the main level's entries or in the
    /// collections level, if one does: what View or X shows the actions of.
    pub(crate) fn focused_collection(&self) -> Option<i64> {
        let imp = self.imp();
        [imp.list.get(), imp.collections.get()]
            .iter()
            .flat_map(collection_rows)
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

    /// Shows `level`, with the power dock under the main level.
    pub(crate) fn show_level(&self, level: DrawerLevel) {
        let imp = self.imp();
        imp.stack.set_visible_child_name(match level {
            DrawerLevel::Main => "main",
            DrawerLevel::Collections => "collections",
            DrawerLevel::Collection => "collection",
        });
        imp.dock_holder.set_visible(level == DrawerLevel::Main);
    }

    /// Where focus goes as `level` shows, after `left` (the level focus comes from): the
    /// entry that led to a level just left (the collection whose actions were shown, or
    /// All Collections when it is no longer among the entries), the first Playing Now
    /// card's Return, or the level's first entry.
    pub(crate) fn focus_target(
        &self,
        level: DrawerLevel,
        left: Option<DrawerLevel>,
    ) -> Option<gtk::Widget> {
        let imp = self.imp();
        let first_row = |list: &gtk::ListBox| list.row_at_index(0).map(|row| row.upcast());
        match (level, left) {
            (DrawerLevel::Main, Some(DrawerLevel::Collections)) => {
                Some(imp.all_collections_row.get().upcast())
            }
            (DrawerLevel::Main, Some(DrawerLevel::Collection)) => {
                let shown = imp.collection.get();
                let inline = imp.inline.borrow();
                inline
                    .iter()
                    .find(|row| shown.is_some() && row_collection(row) == shown)
                    .map(|row| row.clone().upcast())
                    .or_else(|| {
                        let all = imp.all_collections_row.get();
                        all.is_visible().then(|| all.upcast())
                    })
                    .or_else(|| inline.first().map(|row| row.clone().upcast()))
                    .or_else(|| first_row(&imp.list))
            }
            (DrawerLevel::Main, _) if imp.playing_cards.is_visible() => imp
                .playing_cards
                .first_child()
                .as_ref()
                .and_then(crate::fullscreen_playing::card_return),
            (DrawerLevel::Main, _) => first_row(&imp.list),
            (DrawerLevel::Collections, Some(DrawerLevel::Collection)) => {
                let shown = imp.collection.get();
                collection_rows(&imp.collections)
                    .find(|row| shown.is_some() && row_collection(row) == shown)
                    .map(|row| row.upcast())
                    .or_else(|| first_row(&imp.collections))
            }
            (DrawerLevel::Collections, _) => first_row(&imp.collections),
            (DrawerLevel::Collection, _) => first_row(&imp.collection_actions),
        }
    }

    /// Fills the power dock (`pindu_core::gamescope::session::power_dock`) for `state`,
    /// with `switch` for gamescope-session-plus's client and Exit when `exit`; over a
    /// running game it stays empty. Focus on a button that goes away moves to the list's
    /// last row.
    pub(crate) fn fill_dock(
        &self,
        state: &PowerState,
        switch: Option<SwitchToDesktop>,
        exit: bool,
    ) {
        let imp = self.imp();
        let had_focus = imp.dock.focused().is_some();
        let entries = power_dock(state, switch, exit, imp.over_game.get());
        imp.dock.fill(&entries, state.sandboxed);
        if had_focus
            && entries.is_empty()
            && let Some(row) = last_row(&imp.list)
        {
            row.grab_focus();
        }
    }

    /// Where `direction` takes focus to, from or along the power dock, while the main
    /// level shows it: from the main level's first item (a Playing Now card's buttons, or
    /// else Home) Up wraps to where the dock lands; Down from the list's last row enters
    /// the dock at its first button; on the dock Up goes to the list's last row, Down
    /// stays, and Left and Right move along it and stop at its ends. `None` where the
    /// dock has no part in the move.
    fn dock_step(&self, direction: gtk::DirectionType) -> Option<gtk::Widget> {
        let imp = self.imp();
        let dock = &imp.dock;
        if !dock.is_visible() || !imp.dock_holder.is_visible() {
            return None;
        }
        let focus = gtk::prelude::RootExt::focus(&self.root()?)?;
        if let Some(index) = dock.focused() {
            let rtl = self.direction() == gtk::TextDirection::Rtl;
            let along = |forward: bool| {
                let next = if forward {
                    index.checked_add(1)
                } else {
                    index.checked_sub(1)
                };
                next.and_then(|next| dock.button(next))
                    .or(Some(focus.clone()))
            };
            return match direction {
                gtk::DirectionType::Up => last_row(&imp.list).or(Some(focus)),
                gtk::DirectionType::Down => Some(focus),
                gtk::DirectionType::Left => along(rtl),
                gtk::DirectionType::Right => along(!rtl),
                _ => None,
            };
        }
        match direction {
            gtk::DirectionType::Up if self.is_first_item(&focus) => dock.landing(),
            gtk::DirectionType::Down if last_row(&imp.list).is_some_and(|row| row == focus) => {
                dock.button(0)
            }
            _ => None,
        }
    }

    /// Whether `focus` is on the main level's first item: a button of the first Playing
    /// Now card while a game runs, or else Home.
    fn is_first_item(&self, focus: &gtk::Widget) -> bool {
        let imp = self.imp();
        if imp.playing_cards.is_visible() {
            return imp
                .playing_cards
                .first_child()
                .is_some_and(|card| focus.is_ancestor(&card));
        }
        imp.list
            .row_at_index(0)
            .is_some_and(|home| home.upcast_ref::<gtk::Widget>() == focus)
    }
}

/// The last row of `list` that shows.
fn last_row(list: &gtk::ListBox) -> Option<gtk::Widget> {
    std::iter::successors(list.last_child(), |row| row.prev_sibling())
        .find(|row| row.is_visible() && row.is::<gtk::ListBoxRow>())
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

/// The rows of `list` that open a collection, each with the collection as its target.
fn collection_rows(list: &gtk::ListBox) -> impl Iterator<Item = gtk::ListBoxRow> {
    let list = list.clone();
    (0..)
        .map_while(move |index| list.row_at_index(index))
        .filter(|row| row_collection(row).is_some())
}

/// A collection's row: its icon (a folder, or a saved search for a saved filter), its
/// name, which ends in an ellipsis when it is too long, and its number of `games`, which
/// stays in view. It opens the collection in the library.
fn collection_row(collection: &Collection, saved_filter: bool, games: u32) -> gtk::ListBoxRow {
    let row = drawer_row(collection_icon(saved_filter), &collection.name);
    if let Some(content) = row.child().and_downcast::<gtk::Box>() {
        content.append(
            &gtk::Label::builder()
                .label(games.to_string())
                .css_classes(["dim-label", "numeric"])
                .accessible_role(gtk::AccessibleRole::Presentation)
                .build(),
        );
    }
    row.update_property(&[gtk::accessible::Property::Description(&game_count(games))]);
    row.set_action_name(Some("win.drawer-collection"));
    row.set_action_target_value(Some(&collection.id.to_variant()));
    row
}

/// How many games a place holds, as a screen reader reads it.
fn game_count(count: u32) -> String {
    ngettext("{count} game", "{count} games", count).replace("{count}", &count.to_string())
}
