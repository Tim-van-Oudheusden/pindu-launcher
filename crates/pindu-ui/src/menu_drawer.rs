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
//! level with `show_level`, marks the row for the page under the drawer with
//! `set_current`, and fills the entries with `fill` and the dock with `fill_dock` as the
//! library and the system change; the rows' and the dock's `win.*` actions are the
//! host's. The main window hosts it on the left, where Start or Guide opens it over the
//! page. Over a running game in a `--steam` gamescope the overlay window (`game_overlay`)
//! hosts it too, where Guide opens it over the game and it leaves out what would leave the
//! game or end the session (`set_over_game`), the dock among them.
//!
//! A design may arrange the main level's entries as shelves (`"menu-entries": "shelves"`,
//! `.menu-shelves`): Go To with Home, Library and Search, and Collections with every
//! collection, each a heading over a row of cards (`pindu_core::panels::menu_shelves`),
//! in place of the list, with the power dock under them as under the list. Left and Right
//! move along a shelf, Up and Down change shelf, landing on the card that shelf last had
//! focused; Up from the main level's first item wraps to where the dock lands, Down from
//! the last shelf enters the dock, and Up from the dock returns to the card the last shelf
//! last had focused (`pindu_core::panels::step_menu_shelves`). The card for the page under
//! the drawer carries `.current` as its row does, and while a shelf's card has focus the
//! other shelves' headings and rows carry `.unfocused-shelf`. The drawer follows the
//! design's layout (`follow_layout`), the Playing Now cards' look too.

use std::cell::{Cell, OnceCell, RefCell};

use adw::subclass::prelude::*;
use gettextrs::{gettext, ngettext};
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::{Direction, Intent, Layout};
use pindu_core::gamescope::Xid;
use pindu_core::gamescope::session::{DockEntry, SwitchToDesktop, power_dock};
use pindu_core::hints;
use pindu_core::library::View;
use pindu_core::model::Collection;
use pindu_core::panels::{
    CollectionAction, DrawerEntry, DrawerLevel, MenuShelf, MenuSpot, ShelfStep, collection_actions,
    drawer_entries, menu_shelves, step_menu_shelves,
};
use pindu_core::system::power::PowerState;
use pindu_core::themes::{MenuEntries, PlayingCard, ThemeLayout};

use crate::application::PinduApplication;
use crate::collection_actions::collection_icon;
use crate::fullscreen_panels::drawer_row;
use crate::fullscreen_playing::PlayingCards;
use crate::fullscreen_power::PinduPowerDock;
use crate::hint_bar::{GLYPH_SIZE, glyph_label};
use crate::models::LibraryModel;
use crate::window::set_css_class;

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
        pub home_row: TemplateChild<gtk::ListBoxRow>,
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
        pub shelves: TemplateChild<gtk::Box>,
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
        /// The library's number of games, which its row says to a screen reader.
        pub library_games: Cell<u32>,
        /// The row for the page under the drawer, Home or Library, if either.
        pub current: Cell<Option<DrawerEntry>>,
        /// The Playing Now cards over the main level's entries.
        pub(crate) cards: OnceCell<PlayingCards>,
        /// The collection the collection level shows the actions of.
        pub collection: Cell<Option<i64>>,
        /// The collection level's actions, a row each.
        pub actions: RefCell<Vec<CollectionAction>>,
        /// How the design arranges the main level's entries.
        pub entries: Cell<MenuEntries>,
        /// Every collection's card, in the user's order, for the Collections shelf.
        pub(crate) collection_cards: RefCell<Vec<CollectionCard>>,
        /// The dock's entries, for moving to, from and along it under the shelves.
        pub dock_entries: RefCell<Vec<DockEntry>>,
        /// The shelves' cards, top to bottom, each with its button.
        pub shelf_cards: RefCell<CardShelves>,
        /// The card each shelf last had focused, for Up and Down to land on.
        pub shelf_last: RefCell<Vec<DrawerEntry>>,
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
                        .css_classes(["heading", "panel-heading"])
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
        /// Moves to, from and along the power dock or the shelves where they lie that way,
        /// before the focused widget's own lists and boxes move focus.
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            let obj = self.obj();
            match obj
                .dock_step(direction)
                .or_else(|| obj.shelf_step(direction))
            {
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
        imp.library_games.set(library_count);
        self.describe_places();
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
        imp.collection_cards.replace(
            collections
                .iter()
                .map(|collection| CollectionCard {
                    id: collection.id,
                    name: collection.name.clone(),
                    saved_filter: library.saved_filter(collection.id).is_some(),
                    games: counts.get(View::Collection(collection.id)),
                })
                .collect(),
        );
        self.fill_shelves();
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

    /// Marks the row for `current`, the page under the drawer (Home or Library), and its
    /// card on the shelves, with `.current`, and says so to a screen reader; any other
    /// entry, or `None`, marks none.
    pub(crate) fn set_current(&self, current: Option<DrawerEntry>) {
        let imp = self.imp();
        imp.current.set(current);
        for (row, entry) in [
            (&*imp.home_row, DrawerEntry::Home),
            (&*imp.library_row, DrawerEntry::Library),
        ] {
            set_css_class(row, "current", current == Some(entry));
        }
        self.describe_places();
    }

    /// Tells a screen reader, beside each place's name, the library's number of games and
    /// which row or card is the page under the drawer, and marks that card `.current`.
    fn describe_places(&self) {
        let imp = self.imp();
        let current = imp.current.get();
        let games = game_count(imp.library_games.get());
        let library = if current == Some(DrawerEntry::Library) {
            // Translators: how a screen reader describes the menu's Library row while the
            // library is the page under the menu; {games} is how many games it holds, as
            // "12 games".
            gettext("Current page, {games}").replace("{games}", &games)
        } else {
            games
        };
        imp.library_row
            .update_property(&[gtk::accessible::Property::Description(&library)]);
        let home = (current == Some(DrawerEntry::Home)).then(|| gettext("Current page"));
        match &home {
            Some(home) => imp
                .home_row
                .update_property(&[gtk::accessible::Property::Description(home)]),
            None => imp
                .home_row
                .reset_property(gtk::AccessibleProperty::Description),
        }
        for (card, button) in imp.shelf_cards.borrow().iter().flat_map(|(_, cards)| cards) {
            let entry @ (DrawerEntry::Home | DrawerEntry::Library) = *card else {
                continue;
            };
            set_css_class(button, "current", current == Some(entry));
            match (entry, &home) {
                (DrawerEntry::Library, _) => {
                    button.update_property(&[gtk::accessible::Property::Description(&library)]);
                }
                (_, Some(home)) => {
                    button.update_property(&[gtk::accessible::Property::Description(home)]);
                }
                (_, None) => button.reset_property(gtk::AccessibleProperty::Description),
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

    /// The collection whose row or card has focus, in the main level's entries or shelves
    /// or in the collections level, if one does: what View or X shows the actions of.
    pub(crate) fn focused_collection(&self) -> Option<i64> {
        let imp = self.imp();
        let row = [imp.list.get(), imp.collections.get()]
            .iter()
            .flat_map(collection_rows)
            .find(|row| row.is_focus())
            .and_then(|row| row_collection(&row));
        row.or_else(|| {
            self.focused_card()
                .and_then(|(_, card)| card_collection(&card))
        })
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

    /// Follows the layout of the fullscreen design in force from `application`'s style
    /// service, as Home does: now, and at once whenever it changes.
    pub(crate) fn follow_layout(&self, application: &PinduApplication) {
        let style = application.style_service();
        style.connect_layout_changed(glib::clone!(
            #[weak(rename_to = drawer)]
            self,
            move |layout| drawer.set_layout(layout)
        ));
        self.set_layout(style.layout());
    }

    /// Arranges the drawer as `layout` says: the main level's entries as a list or as
    /// shelves (`.menu-shelves`), over the dock either way, and the Playing Now cards
    /// compact or as banners (`.playing-card-banner`). Focus on an entry that moves goes
    /// to the main level's first item.
    fn set_layout(&self, layout: ThemeLayout) {
        let imp = self.imp();
        let banner = layout.playing_card == PlayingCard::Banner;
        set_css_class(self, "playing-card-banner", banner);
        self.cards().set_arrangement(layout.playing_card);
        if imp.entries.replace(layout.menu_entries) == layout.menu_entries {
            return;
        }
        let shelves = layout.menu_entries == MenuEntries::Shelves;
        set_css_class(self, "menu-shelves", shelves);
        let focus = self
            .root()
            .and_then(|root| gtk::prelude::RootExt::focus(&root));
        let moved = [
            imp.list.upcast_ref::<gtk::Widget>(),
            imp.shelves.upcast_ref(),
        ]
        .into_iter()
        .any(|area| focus.as_ref().is_some_and(|focus| focus.is_ancestor(area)));
        imp.list.set_visible(!shelves);
        imp.shelves.set_visible(shelves);
        self.fill_shelves();
        if moved && let Some(target) = self.focus_target(DrawerLevel::Main, None) {
            target.grab_focus();
        }
    }

    /// Builds the shelves (`pindu_core::panels::menu_shelves`) as the library now is,
    /// while the design asks for them: each a heading over a row of cards that scrolls
    /// sideways. A card that had focus gives it to the one now in its place.
    fn fill_shelves(&self) {
        let imp = self.imp();
        let focused = self.focused_card().map(|(card, button)| {
            let shelf = self.shelf_of(card);
            let along = imp
                .shelf_cards
                .borrow()
                .iter()
                .find_map(|(_, cards)| cards.iter().position(|(_, each)| *each == button));
            (shelf, along.unwrap_or(0), card_collection(&button))
        });
        while let Some(child) = imp.shelves.first_child() {
            imp.shelves.remove(&child);
        }
        imp.shelf_cards.borrow_mut().clear();
        if imp.entries.get() != MenuEntries::Shelves {
            return;
        }
        let collections = imp.collection_cards.borrow().clone();
        let library_count = imp.library_count.label();
        let mut built = Vec::new();
        for (shelf, cards) in menu_shelves(collections.len(), imp.over_game.get()) {
            let row = gtk::Box::builder()
                .spacing(12)
                .homogeneous(true)
                .css_classes(["menu-shelf"])
                .build();
            let mut buttons = Vec::new();
            for card in cards {
                let button = match card {
                    DrawerEntry::Home => place_card(
                        "go-home-symbolic",
                        &gettext("Home"),
                        None,
                        "win.drawer-home",
                    ),
                    DrawerEntry::Library => place_card(
                        "view-grid-symbolic",
                        &gettext("Library"),
                        Some(&library_count),
                        "win.drawer-library",
                    ),
                    DrawerEntry::Search => place_card(
                        "system-search-symbolic",
                        &gettext("Search"),
                        None,
                        "win.drawer-search",
                    ),
                    DrawerEntry::Collection(index) => {
                        let Some(collection) = collections.get(index) else {
                            continue;
                        };
                        collection_card(collection)
                    }
                    // Neither is on a shelf: the cards sit over the shelves, and every
                    // collection is on its own.
                    DrawerEntry::PlayingNow | DrawerEntry::AllCollections => continue,
                };
                row.append(&button);
                buttons.push((card, button));
            }
            let heading = gtk::Label::builder()
                .label(shelf_name(shelf))
                .xalign(0.0)
                .css_classes(["heading", "menu-shelf-heading"])
                .build();
            row.update_property(&[gtk::accessible::Property::Label(&shelf_name(shelf))]);
            row.set_accessible_role(gtk::AccessibleRole::Group);
            // The row is laid out at its natural width, the height it was measured at, so
            // its cards are never squeezed taller than the shelf.
            let viewport = gtk::Viewport::builder()
                .hscroll_policy(gtk::ScrollablePolicy::Natural)
                .child(&row)
                .build();
            let scroller = gtk::ScrolledWindow::builder()
                .hscrollbar_policy(gtk::PolicyType::Automatic)
                .vscrollbar_policy(gtk::PolicyType::Never)
                .propagate_natural_height(true)
                .child(&viewport)
                .build();
            // While a card of this shelf has focus, the other shelves are marked.
            let focus = gtk::EventControllerFocus::new();
            focus.connect_enter(glib::clone!(
                #[weak(rename_to = drawer)]
                self,
                #[weak]
                scroller,
                move |_| drawer.mark_shelves(Some(scroller.upcast_ref()))
            ));
            focus.connect_leave(glib::clone!(
                #[weak(rename_to = drawer)]
                self,
                move |_| drawer.mark_shelves(None)
            ));
            scroller.add_controller(focus);
            imp.shelves.append(&heading);
            imp.shelves.append(&scroller);
            built.push((shelf, buttons));
        }
        imp.shelf_cards.replace(built);
        self.describe_places();
        let Some((shelf, along, collection)) = focused else {
            return;
        };
        let cards = imp.shelf_cards.borrow();
        let target = collection
            .and_then(|id| {
                cards
                    .iter()
                    .flat_map(|(_, cards)| cards)
                    .find(|(_, button)| card_collection(button) == Some(id))
            })
            .or_else(|| {
                let (_, on) = cards.iter().find(|(each, _)| Some(*each) == shelf)?;
                on.get(along).or(on.last())
            })
            .map(|(_, button)| button.clone());
        drop(cards);
        if let Some(button) = target {
            button.grab_focus();
        }
    }

    /// Marks the heading and the row of every shelf but `focused`, the row whose card has
    /// focus, with `.unfocused-shelf`; with no shelf focused, none.
    fn mark_shelves(&self, focused: Option<&gtk::Widget>) {
        let shelves = self.imp().shelves.get();
        // Each shelf is its heading followed by its row.
        let mut child = shelves.first_child();
        while let Some(heading) = child {
            let Some(row) = heading.next_sibling() else {
                break;
            };
            let away = focused.is_some_and(|focused| *focused != row);
            set_css_class(&heading, "unfocused-shelf", away);
            set_css_class(&row, "unfocused-shelf", away);
            child = row.next_sibling();
        }
    }

    /// The card that has focus on the shelves, with its button.
    fn focused_card(&self) -> Option<(DrawerEntry, gtk::Button)> {
        self.imp()
            .shelf_cards
            .borrow()
            .iter()
            .flat_map(|(_, cards)| cards)
            .find(|(_, button)| button.is_focus())
            .cloned()
    }

    /// The shelf `card` sits on.
    fn shelf_of(&self, card: DrawerEntry) -> Option<MenuShelf> {
        self.imp()
            .shelf_cards
            .borrow()
            .iter()
            .find(|(_, cards)| cards.iter().any(|(each, _)| *each == card))
            .map(|(shelf, _)| *shelf)
    }

    /// The button of `card` on the shelves.
    fn card_button(&self, card: DrawerEntry) -> Option<gtk::Widget> {
        self.imp()
            .shelf_cards
            .borrow()
            .iter()
            .flat_map(|(_, cards)| cards)
            .find(|(each, _)| *each == card)
            .map(|(_, button)| button.clone().upcast())
    }

    /// The card focus lands on in the first shelf, or the last (`last`): the one it last
    /// had focused, else its first.
    fn shelf_landing(&self, last: bool) -> Option<gtk::Widget> {
        let imp = self.imp();
        let shelves = imp.shelf_cards.borrow();
        let (_, cards) = if last {
            shelves.last()?
        } else {
            shelves.first()?
        };
        let remembered = imp.shelf_last.borrow();
        cards
            .iter()
            .find(|(card, _)| remembered.contains(card))
            .or(cards.first())
            .map(|(_, button)| button.clone().upcast())
    }

    /// Where `direction` takes focus to, from or along the shelves and the power dock
    /// under them, while the main level shows them: on a card or a dock button as
    /// `pindu_core::panels::step_menu_shelves` says, out of the first shelf Up to the
    /// windows or the Playing Now cards over them; into the first shelf Down from the
    /// last card or window over it; and from the main level's first item Up round to
    /// where the dock lands. `None` where the shelves have no part in the move.
    fn shelf_step(&self, direction: gtk::DirectionType) -> Option<gtk::Widget> {
        let imp = self.imp();
        if imp.entries.get() != MenuEntries::Shelves
            || imp.stack.visible_child_name().as_deref() != Some("main")
        {
            return None;
        }
        let rtl = self.direction() == gtk::TextDirection::Rtl;
        let direction = match direction {
            gtk::DirectionType::Up => Direction::Up,
            gtk::DirectionType::Down => Direction::Down,
            gtk::DirectionType::Left if rtl => Direction::Right,
            gtk::DirectionType::Left => Direction::Left,
            gtk::DirectionType::Right if rtl => Direction::Left,
            gtk::DirectionType::Right => Direction::Right,
            _ => return None,
        };
        let focus = gtk::prelude::RootExt::focus(&self.root()?)?;
        let docked = imp.dock_holder.is_visible() && imp.dock.is_visible();
        let at = match self.focused_card() {
            Some((card, _)) => {
                let shelf = self.shelf_of(card);
                let mut last = imp.shelf_last.borrow_mut();
                last.retain(|each| self.shelf_of(*each) != shelf);
                last.push(card);
                MenuSpot::Card(card)
            }
            None => match imp.dock.focused().filter(|_| docked) {
                Some(index) => MenuSpot::Dock(index),
                None => {
                    return match direction {
                        Direction::Up if self.is_first_item(&focus) && docked => imp.dock.landing(),
                        Direction::Down if self.is_over_shelves(&focus) => {
                            self.shelf_landing(false)
                        }
                        _ => None,
                    };
                }
            },
        };
        let shelves: Vec<(MenuShelf, Vec<DrawerEntry>)> = imp
            .shelf_cards
            .borrow()
            .iter()
            .map(|(shelf, cards)| (*shelf, cards.iter().map(|(card, _)| *card).collect()))
            .collect();
        let dock = if docked {
            imp.dock_entries.borrow().clone()
        } else {
            Vec::new()
        };
        let above = self.above_shelves();
        let step = step_menu_shelves(
            &shelves,
            &dock,
            at,
            direction,
            &imp.shelf_last.borrow(),
            above.is_some(),
        );
        match step {
            ShelfStep::To(MenuSpot::Card(card)) => self.card_button(card),
            ShelfStep::To(MenuSpot::Dock(index)) => imp.dock.button(index),
            ShelfStep::Above => above,
            ShelfStep::Below => Some(focus),
        }
    }

    /// What sits over the shelves, where Up from the first shelf goes: the last window,
    /// else the last Playing Now card's Return; `None` when nothing does.
    fn above_shelves(&self) -> Option<gtk::Widget> {
        let imp = self.imp();
        if imp.windows.is_visible()
            && let Some(row) = last_row(&imp.windows)
        {
            return Some(row);
        }
        if !imp.playing_cards.is_visible() {
            return None;
        }
        imp.playing_cards
            .last_child()
            .as_ref()
            .and_then(crate::fullscreen_playing::card_return)
    }

    /// Whether `focus` is on the last item over the shelves: the last window, or else the
    /// last Playing Now card.
    fn is_over_shelves(&self, focus: &gtk::Widget) -> bool {
        let imp = self.imp();
        if imp.windows.is_visible() {
            return last_row(&imp.windows).is_some_and(|row| row == *focus);
        }
        imp.playing_cards.is_visible()
            && imp
                .playing_cards
                .last_child()
                .is_some_and(|card| focus.is_ancestor(&card))
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
        if level == DrawerLevel::Main && imp.entries.get() == MenuEntries::Shelves {
            return match left {
                Some(DrawerLevel::Collection | DrawerLevel::Collections) => {
                    let shown = imp.collection.get();
                    let cards = imp.shelf_cards.borrow();
                    let collections = cards
                        .iter()
                        .find(|(shelf, _)| *shelf == MenuShelf::Collections)
                        .map(|(_, cards)| cards.as_slice())
                        .unwrap_or_default();
                    collections
                        .iter()
                        .find(|(_, button)| shown.is_some() && card_collection(button) == shown)
                        .or(collections.first())
                        .map(|(_, button)| button.clone().upcast())
                        .or_else(|| self.shelf_landing(false))
                }
                _ if imp.playing_cards.is_visible() => imp
                    .playing_cards
                    .first_child()
                    .as_ref()
                    .and_then(crate::fullscreen_playing::card_return),
                _ => {
                    let cards = imp.shelf_cards.borrow();
                    let first = cards.first().and_then(|(_, cards)| cards.first());
                    first.map(|(_, button)| button.clone().upcast())
                }
            };
        }
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
    /// last row, or to the last shelf's card it last had focused.
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
        let emptied = entries.is_empty();
        imp.dock_entries.replace(entries);
        if !had_focus || !emptied {
            return;
        }
        let target = if imp.entries.get() == MenuEntries::Shelves {
            self.shelf_landing(true)
        } else {
            last_row(&imp.list)
        };
        if let Some(target) = target {
            target.grab_focus();
        }
    }

    /// Where `direction` takes focus to, from or along the power dock under the list,
    /// while the main level shows it: from the main level's first item (a Playing Now
    /// card's buttons, or else Home) Up wraps to where the dock lands; Down from the
    /// list's last row enters the dock at its first button; on the dock Up goes to the
    /// list's last row, Down stays, and Left and Right move along it and stop at its
    /// ends. `None` where the dock has no part in the move, and under the shelves, where
    /// `shelf_step` moves to and from the dock.
    fn dock_step(&self, direction: gtk::DirectionType) -> Option<gtk::Widget> {
        let imp = self.imp();
        let dock = &imp.dock;
        if !dock.is_visible()
            || !imp.dock_holder.is_visible()
            || imp.entries.get() == MenuEntries::Shelves
        {
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

    /// Whether `focus` is on the main level's first item outside the shelves: a button
    /// of the first Playing Now card while a game runs, or else Home's row.
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

/// The menu's shelves, top to bottom, each card with its button.
type CardShelves = Vec<(MenuShelf, Vec<(DrawerEntry, gtk::Button)>)>;

/// What a collection's card on the Collections shelf shows.
#[derive(Debug, Clone)]
pub(crate) struct CollectionCard {
    /// The collection's ID, which the card opens.
    id: i64,
    /// Its name.
    name: String,
    /// Whether it is a saved filter, which its icon shows.
    saved_filter: bool,
    /// Its number of games.
    games: u32,
}

/// The heading over `shelf`.
fn shelf_name(shelf: MenuShelf) -> String {
    match shelf {
        MenuShelf::GoTo => gettext("Go To"),
        MenuShelf::Collections => gettext("Collections"),
    }
}

/// A card on one of the menu's shelves (`.menu-card`): its icon (`.menu-card-icon`) over
/// its name (`.menu-card-name`, wrapped between words), and under it `detail`, a number of
/// games, when there is one (`.menu-card-detail`).
fn menu_card(icon: &str, name: &str, detail: Option<&str>) -> gtk::Button {
    let content = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    content.append(
        &gtk::Image::builder()
            .icon_name(icon)
            .accessible_role(gtk::AccessibleRole::Presentation)
            .css_classes(["menu-card-icon"])
            .build(),
    );
    // Wrapped between words only, so a card is at least as wide as its longest word and
    // grows taller for more lines: no name is cut or broken inside a word.
    content.append(
        &gtk::Label::builder()
            .label(name)
            .justify(gtk::Justification::Center)
            .wrap(true)
            .wrap_mode(gtk::pango::WrapMode::Word)
            .max_width_chars(12)
            .css_classes(["menu-card-name"])
            .build(),
    );
    if let Some(detail) = detail {
        content.append(
            &gtk::Label::builder()
                .label(detail)
                .justify(gtk::Justification::Center)
                .wrap(true)
                .wrap_mode(gtk::pango::WrapMode::Word)
                .max_width_chars(10)
                .accessible_role(gtk::AccessibleRole::Presentation)
                .css_classes(["menu-card-detail", "caption", "dim-label", "numeric"])
                .build(),
        );
    }
    gtk::Button::builder()
        .child(&content)
        .css_classes(["menu-card"])
        .build()
}

/// A card of the Go To shelf, leading to `action`, as its row does.
fn place_card(icon: &str, name: &str, detail: Option<&str>, action: &str) -> gtk::Button {
    let card = menu_card(icon, name, detail);
    card.set_action_name(Some(action));
    card
}

/// A collection's card: its icon, its name and its number of games, opening the
/// collection in the library as its row does.
fn collection_card(collection: &CollectionCard) -> gtk::Button {
    let card = menu_card(
        collection_icon(collection.saved_filter),
        &collection.name,
        Some(&collection.games.to_string()),
    );
    card.update_property(&[gtk::accessible::Property::Description(&game_count(
        collection.games,
    ))]);
    card.set_action_name(Some("win.drawer-collection"));
    card.set_action_target_value(Some(&collection.id.to_variant()));
    card
}

/// The collection a card opens, if it opens one.
fn card_collection(card: &gtk::Button) -> Option<i64> {
    if card.action_name().as_deref() != Some("win.drawer-collection") {
        return None;
    }
    card.action_target_value()?.get::<i64>()
}
