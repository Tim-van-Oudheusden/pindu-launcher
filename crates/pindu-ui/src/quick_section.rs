//! Quick Access's Quick section as a widget: [`PinduQuickSection`], the box that holds the
//! Playing Now cards, the page cards (`.page-card`), the tiles (`.quick-tiles`) and the
//! activity strip (`.activity-strip`), top to bottom. It lays the tiles that show out as
//! the design arranges them (`pindu_core::themes::QuickTiles`): three to a row from the
//! grid's start, each tile's icon over its name; two to a row as pills, the icon beside the
//! name; one to a row, the second line at the row's end; or on the shelves Devices and
//! Library under their headings (`pindu_core::panels::tile_shelves`). It moves focus
//! through the section itself, the same for the D-pad, the left stick and the arrow keys:
//! Up and Down from one card, the tiles or the strip to the next, and between the tiles as
//! `pindu_core::panels::step_arranged_tile` says, on shelves landing on the tile a shelf
//! last had focused.

use std::cell::{Cell, RefCell};

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::Direction;
use pindu_core::panels::{
    QUICK_PILL_COLUMNS, QUICK_TILE_COLUMNS, QuickTile, TileShelf, TileStep, step_arranged_tile,
    tile_shelves,
};
use pindu_core::themes::QuickTiles;

use crate::fullscreen_playing::card_return;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct PinduQuickSection {
        /// The section's parts, once the panel gave them.
        pub(crate) parts: RefCell<Option<Parts>>,
        /// The tiles that show, in their order, each with its widget.
        pub(crate) tiles: RefCell<Vec<(QuickTile, gtk::Widget)>>,
        /// How the design arranges the tiles.
        pub(crate) arrangement: Cell<QuickTiles>,
        /// The tile each shelf last had focused, for Up and Down to land on.
        pub(crate) last: RefCell<Vec<QuickTile>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for PinduQuickSection {
        const NAME: &'static str = "PinduQuickSection";
        type Type = super::PinduQuickSection;
        type ParentType = gtk::Box;
    }

    impl ObjectImpl for PinduQuickSection {}

    impl WidgetImpl for PinduQuickSection {
        fn focus(&self, direction: gtk::DirectionType) -> bool {
            match self.obj().step(direction) {
                Some(Step::Moved) => true,
                Some(Step::Along) => {
                    // Along a Playing Now card's buttons; at its ends focus stays.
                    self.parent_focus(direction);
                    true
                }
                None => self.parent_focus(direction),
            }
        }
    }

    impl BoxImpl for PinduQuickSection {}
}

/// What a direction does in the section.
enum Step {
    /// The section moved focus, or kept it where it was at an end.
    Moved,
    /// Left or Right on a Playing Now card, which GTK moves along its buttons.
    Along,
}

glib::wrapper! {
    /// Quick Access's Quick section, which lays out its tiles and moves focus through
    /// itself (see the module documentation).
    pub struct PinduQuickSection(ObjectSubclass<imp::PinduQuickSection>)
        @extends gtk::Box, gtk::Widget,
        @implements gtk::Accessible, gtk::Buildable, gtk::ConstraintTarget, gtk::Orientable;
}

/// The parts of the section focus moves between, top to bottom.
#[derive(Debug)]
pub(crate) struct Parts {
    /// The box of Playing Now cards.
    pub(crate) playing_cards: gtk::Box,
    /// The page cards, in their order.
    pub(crate) page_cards: Vec<gtk::Widget>,
    /// The grid of tiles.
    pub(crate) tiles: gtk::Grid,
    /// The headings of the shelves Devices and Library, in the grid of tiles.
    pub(crate) headings: [(TileShelf, gtk::Widget); 2],
    /// The activity strip.
    pub(crate) strip: gtk::Widget,
}

/// A place Up and Down stop at in the section.
enum Stop {
    /// A Playing Now card (`area`, focused on its Return), a page card or the strip.
    Widget {
        area: gtk::Widget,
        target: gtk::Widget,
    },
    /// The tiles, which keep their own rows or shelves.
    Tiles,
}

impl PinduQuickSection {
    /// Gives the section its parts, once.
    pub(crate) fn set_parts(&self, parts: Parts) {
        self.imp().parts.replace(Some(parts));
        self.arrange_tiles();
    }

    /// Lays `tiles`, the tiles that show in their order, each with its widget, out as the
    /// design arranges them; the others are hidden.
    pub(crate) fn set_tiles(&self, tiles: Vec<(QuickTile, gtk::Widget)>) {
        self.imp().tiles.replace(tiles);
        self.place_tiles();
    }

    /// Arranges the tiles as `arrangement` says, the design's.
    pub(crate) fn set_arrangement(&self, arrangement: QuickTiles) {
        if self.imp().arrangement.replace(arrangement) != arrangement {
            self.arrange_tiles();
        }
    }

    /// The first tile that can take focus.
    pub(crate) fn first_tile(&self) -> Option<gtk::Widget> {
        self.imp()
            .tiles
            .borrow()
            .iter()
            .map(|(_, tile)| tile)
            .find(|tile| takes_focus(tile))
            .cloned()
    }

    /// Lays out every tile's parts as the arrangement has them, then places the tiles.
    fn arrange_tiles(&self) {
        let arrangement = self.imp().arrangement.get();
        if let Some(parts) = self.imp().parts.borrow().as_ref() {
            for tile in
                children(parts.tiles.upcast_ref()).filter(|tile| tile.has_css_class("quick-tile"))
            {
                arrange_tile(&tile, arrangement);
            }
        }
        self.place_tiles();
    }

    /// Places the tiles that show in the grid: in rows of the arrangement's columns from
    /// the grid's start, or each shelf's heading over a row of its tiles, a shelf without
    /// one left out.
    fn place_tiles(&self) {
        let imp = self.imp();
        let parts = imp.parts.borrow();
        let Some(parts) = parts.as_ref() else {
            return;
        };
        let Some(layout) = parts.tiles.layout_manager() else {
            return;
        };
        let place = |widget: &gtk::Widget, column: u32, row: u32, span: u32| {
            if let Ok(child) = layout
                .layout_child(widget)
                .downcast::<gtk::GridLayoutChild>()
            {
                child.set_column(to_i32(column));
                child.set_row(to_i32(row));
                child.set_column_span(to_i32(span));
            }
        };
        let tiles = imp.tiles.borrow();
        let arrangement = imp.arrangement.get();
        for (_, heading) in &parts.headings {
            heading.set_visible(false);
        }
        if arrangement == QuickTiles::Shelf {
            let kinds: Vec<QuickTile> = tiles.iter().map(|(tile, _)| *tile).collect();
            let mut row = 0;
            for (shelf, on) in tile_shelves(&kinds) {
                if let Some((_, heading)) = parts.headings.iter().find(|(each, _)| *each == shelf) {
                    heading.set_visible(true);
                    place(heading, 0, row, QUICK_TILE_COLUMNS);
                }
                row += 1;
                for (column, tile) in (0..).zip(&on) {
                    if let Some((_, widget)) = tiles.iter().find(|(each, _)| each == tile) {
                        place(widget, column, row, 1);
                    }
                }
                row += 1;
            }
            return;
        }
        let columns = columns(arrangement);
        for (index, (_, widget)) in (0..).zip(tiles.iter()) {
            place(widget, index % columns, index / columns, 1);
        }
    }

    /// Moves focus `direction` from the item in the section that has it; `None` for a
    /// direction the section leaves to GTK (Tab, or focus outside the section).
    fn step(&self, direction: gtk::DirectionType) -> Option<Step> {
        let direction = match direction {
            gtk::DirectionType::Up => Direction::Up,
            gtk::DirectionType::Down => Direction::Down,
            gtk::DirectionType::Left => Direction::Left,
            gtk::DirectionType::Right => Direction::Right,
            _ => return None,
        };
        let focus = self.root()?.focus()?;
        if !focus.is_ancestor(self) {
            return None;
        }
        let tiles = self.imp().tiles.borrow().clone();
        if let Some(index) = tiles.iter().position(|(_, tile)| within(&focus, tile)) {
            self.step_from_tile(&tiles, index, direction);
            return Some(Step::Moved);
        }
        let stops = self.stops(&tiles);
        let Some(here) = stops.iter().position(|stop| match stop {
            Stop::Widget { area, .. } => within(&focus, area),
            Stop::Tiles => false,
        }) else {
            return Some(Step::Moved);
        };
        match direction {
            Direction::Left | Direction::Right => {
                let on_card = self
                    .imp()
                    .parts
                    .borrow()
                    .as_ref()
                    .is_some_and(|parts| focus.is_ancestor(&parts.playing_cards));
                Some(if on_card { Step::Along } else { Step::Moved })
            }
            Direction::Up | Direction::Down => {
                self.leave(&stops, &tiles, here, direction);
                Some(Step::Moved)
            }
        }
    }

    /// Moves focus `direction` from the tile at `index` of `tiles`: to the tile
    /// `step_arranged_tile` says, passing those that cannot take focus, or out of the
    /// tiles. The tile left is the one its shelf last had focused.
    fn step_from_tile(
        &self,
        tiles: &[(QuickTile, gtk::Widget)],
        index: usize,
        direction: Direction,
    ) {
        let imp = self.imp();
        let kinds: Vec<QuickTile> = tiles.iter().map(|(tile, _)| *tile).collect();
        {
            let left = kinds[index];
            let mut last = imp.last.borrow_mut();
            last.retain(|tile| tile.shelf() != left.shelf());
            last.push(left);
        }
        let arrangement = imp.arrangement.get();
        let mut index = u32::try_from(index).unwrap_or(u32::MAX);
        loop {
            let step =
                step_arranged_tile(arrangement, &kinds, index, direction, &imp.last.borrow());
            match step {
                TileStep::Tile(next) if next == index => return,
                TileStep::Tile(next) => {
                    index = next;
                    let tile = &tiles[next as usize].1;
                    if takes_focus(tile) {
                        tile.grab_focus();
                        return;
                    }
                }
                TileStep::Above | TileStep::Below => {
                    let stops = self.stops(tiles);
                    if let Some(here) = stops.iter().position(|stop| matches!(stop, Stop::Tiles)) {
                        self.leave(&stops, tiles, here, direction);
                    }
                    return;
                }
            }
        }
    }

    /// The places Up and Down stop at, top to bottom, of those that show and can take
    /// focus.
    fn stops(&self, tiles: &[(QuickTile, gtk::Widget)]) -> Vec<Stop> {
        let parts = self.imp().parts.borrow();
        let Some(parts) = parts.as_ref() else {
            return Vec::new();
        };
        let mut stops = Vec::new();
        if parts.playing_cards.is_visible() {
            for card in children(parts.playing_cards.upcast_ref()).filter(|card| card.is_visible())
            {
                if let Some(target) = card_return(&card).filter(takes_focus) {
                    stops.push(Stop::Widget { area: card, target });
                }
            }
        }
        for card in parts.page_cards.iter().filter(|card| takes_focus(card)) {
            stops.push(Stop::Widget {
                area: card.clone(),
                target: card.clone(),
            });
        }
        if tiles.iter().any(|(_, tile)| takes_focus(tile)) {
            stops.push(Stop::Tiles);
        }
        if takes_focus(&parts.strip) {
            stops.push(Stop::Widget {
                area: parts.strip.clone(),
                target: parts.strip.clone(),
            });
        }
        stops
    }

    /// Moves focus from `stops[here]` to the stop above or below it (`direction`), if
    /// there is one: into the tiles where [`Self::tile_entered`] says.
    fn leave(
        &self,
        stops: &[Stop],
        tiles: &[(QuickTile, gtk::Widget)],
        here: usize,
        direction: Direction,
    ) {
        let next = match direction {
            Direction::Up => here.checked_sub(1),
            Direction::Down => Some(here + 1),
            Direction::Left | Direction::Right => None,
        };
        match next.and_then(|next| stops.get(next)) {
            Some(Stop::Widget { target, .. }) => {
                target.grab_focus();
            }
            Some(Stop::Tiles) => {
                let start = self.tile_entered(tiles, direction);
                let tile = tiles[start..]
                    .iter()
                    .chain(tiles[..start].iter().rev())
                    .map(|(_, tile)| tile)
                    .find(|tile| takes_focus(tile));
                if let Some(tile) = tile {
                    tile.grab_focus();
                }
            }
            None => {}
        }
    }

    /// The index of the tile focus enters the tiles at going `direction`: from above the
    /// first row's first tile, from below the last row's; on shelves, the tile the first
    /// or the last shelf last had focused, else its first.
    fn tile_entered(&self, tiles: &[(QuickTile, gtk::Widget)], direction: Direction) -> usize {
        let imp = self.imp();
        let arrangement = imp.arrangement.get();
        if arrangement == QuickTiles::Shelf {
            let kinds: Vec<QuickTile> = tiles.iter().map(|(tile, _)| *tile).collect();
            let shelves = tile_shelves(&kinds);
            let shelf = match direction {
                Direction::Up => shelves.last(),
                _ => shelves.first(),
            };
            let last = imp.last.borrow();
            return shelf
                .and_then(|(_, on)| on.iter().find(|tile| last.contains(tile)).or(on.first()))
                .and_then(|landing| kinds.iter().position(|tile| tile == landing))
                .unwrap_or(0);
        }
        let columns = columns(arrangement) as usize;
        match direction {
            Direction::Up => tiles.len().saturating_sub(1) / columns * columns,
            _ => 0,
        }
    }
}

/// The tiles to a row in `arrangement`, but for shelves, whose rows are their own.
fn columns(arrangement: QuickTiles) -> u32 {
    match arrangement {
        QuickTiles::Grid | QuickTiles::Shelf => QUICK_TILE_COLUMNS,
        QuickTiles::Pills => QUICK_PILL_COLUMNS,
        QuickTiles::List => 1,
    }
}

/// Lays out `tile`'s parts (`.quick-tile-icon`, `.quick-tile-text` with the name and the
/// second line, and `.quick-tile-arrow` on a tile that leads to a level) as `arrangement`
/// has them: in a grid or on shelves the icon over the name and the second line, the arrow
/// in the top corner; as a pill the icon at the start, the name over the second line
/// beside it and the arrow at the end; in a list the same, but the second line at the
/// row's end.
fn arrange_tile(tile: &gtk::Widget, arrangement: QuickTiles) {
    let Some(content) = tile.first_child().and_downcast::<gtk::Grid>() else {
        return;
    };
    let Some(layout) = content.layout_manager() else {
        return;
    };
    let place = |widget: &gtk::Widget, column: i32, row: i32, span: i32| {
        if let Ok(child) = layout
            .layout_child(widget)
            .downcast::<gtk::GridLayoutChild>()
        {
            child.set_column(column);
            child.set_row(row);
            child.set_column_span(span);
        }
    };
    let stacked = matches!(arrangement, QuickTiles::Grid | QuickTiles::Shelf);
    content.set_row_spacing(if stacked { 4 } else { 0 });
    content.set_valign(if stacked {
        gtk::Align::Fill
    } else {
        gtk::Align::Center
    });
    // A pill's name has half a column: the icon and the arrow keep close to it.
    content.set_column_spacing(match arrangement {
        QuickTiles::Grid | QuickTiles::Shelf => 0,
        QuickTiles::Pills => 8,
        QuickTiles::List => 12,
    });
    for part in children(content.upcast_ref()) {
        if part.has_css_class("quick-tile-icon") {
            place(&part, 0, 0, 1);
            part.set_hexpand(stacked);
            part.set_vexpand(stacked);
            part.set_valign(if stacked {
                gtk::Align::Start
            } else {
                gtk::Align::Center
            });
        } else if part.has_css_class("quick-tile-arrow") {
            if stacked {
                place(&part, 1, 0, 1);
            } else {
                place(&part, 2, 0, 1);
            }
            part.set_valign(if stacked {
                gtk::Align::Start
            } else {
                gtk::Align::Center
            });
        } else if part.has_css_class("quick-tile-text") {
            if stacked {
                place(&part, 0, 1, 2);
            } else {
                place(&part, 1, 0, 1);
            }
            part.set_hexpand(!stacked);
            part.set_valign(if stacked {
                gtk::Align::Fill
            } else {
                gtk::Align::Center
            });
            arrange_tile_text(&part, arrangement);
        }
    }
}

/// Lays out a tile's name and second line: the second line at the row's end in a list,
/// under the name otherwise. A name wraps between words, and breaks inside a word only
/// where the word is wider than the tile can be: the panel's width is fixed, so a tile
/// cannot grow wider than its share of it.
fn arrange_tile_text(text: &gtk::Widget, arrangement: QuickTiles) {
    let Some(text) = text.downcast_ref::<gtk::Box>() else {
        return;
    };
    let in_a_row = arrangement == QuickTiles::List;
    text.set_orientation(if in_a_row {
        gtk::Orientation::Horizontal
    } else {
        gtk::Orientation::Vertical
    });
    text.set_spacing(if in_a_row { 12 } else { 4 });
    let mut lines =
        children(text.upcast_ref()).filter_map(|line| line.downcast::<gtk::Label>().ok());
    if let Some(name) = lines.next() {
        name.set_hexpand(in_a_row);
    }
    for state in lines {
        state.set_xalign(if in_a_row { 1.0 } else { 0.0 });
        state.set_valign(if in_a_row {
            gtk::Align::Center
        } else {
            gtk::Align::Fill
        });
        state.set_max_width_chars(if in_a_row { 20 } else { 10 });
    }
}

/// The children of `widget`, first to last.
fn children(widget: &gtk::Widget) -> impl Iterator<Item = gtk::Widget> {
    std::iter::successors(widget.first_child(), |child| child.next_sibling())
}

/// Whether `widget` is `area` or inside it.
fn within(widget: &gtk::Widget, area: &impl IsA<gtk::Widget>) -> bool {
    widget == area.upcast_ref() || widget.is_ancestor(area)
}

/// Whether `widget` shows and can take focus.
fn takes_focus(widget: &gtk::Widget) -> bool {
    widget.is_visible() && widget.is_sensitive()
}

/// A grid position as GTK takes it.
fn to_i32(value: u32) -> i32 {
    i32::try_from(value).unwrap_or(i32::MAX)
}
