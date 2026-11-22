//! Quick Access's Quick section as a widget: [`PinduQuickSection`], the box that holds the
//! Playing Now cards, the page cards (`.page-card`), the tiles (`.quick-tiles`) and the
//! activity strip (`.activity-strip`), top to bottom. It lays the tiles that show out
//! three to a row from the grid's start, and moves focus through the section itself, the
//! same for the D-pad, the left stick and the arrow keys: Up and Down from one card, row of
//! tiles or the strip to the next, Left and Right along a row of tiles, stopping at its
//! ends, and between the tiles as `pindu_core::panels::step_tile` says.

use std::cell::RefCell;

use adw::subclass::prelude::*;
use gtk::glib;
use gtk::prelude::*;
use pindu_core::gamepad::Direction;
use pindu_core::panels::{QUICK_TILE_COLUMNS, TileStep, step_tile};

use crate::fullscreen_playing::card_return;

mod imp {
    use super::*;

    #[derive(Debug, Default)]
    pub struct PinduQuickSection {
        /// The section's parts, once the panel gave them.
        pub(crate) parts: RefCell<Option<Parts>>,
        /// The tiles that show, in the grid's order.
        pub(crate) tiles: RefCell<Vec<gtk::Widget>>,
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
    /// The tiles, which keep their own rows.
    Tiles,
}

impl PinduQuickSection {
    /// Gives the section its parts, once.
    pub(crate) fn set_parts(&self, parts: Parts) {
        self.imp().parts.replace(Some(parts));
    }

    /// Lays `tiles`, the tiles that show in their order, out three to a row from the
    /// grid's start; the others are hidden.
    pub(crate) fn set_tiles(&self, tiles: Vec<gtk::Widget>) {
        let imp = self.imp();
        if let Some(parts) = imp.parts.borrow().as_ref()
            && let Some(layout) = parts.tiles.layout_manager()
        {
            for (index, tile) in (0..).zip(&tiles) {
                if let Ok(child) = layout.layout_child(tile).downcast::<gtk::GridLayoutChild>() {
                    child.set_column(index % to_i32(QUICK_TILE_COLUMNS));
                    child.set_row(index / to_i32(QUICK_TILE_COLUMNS));
                }
            }
        }
        imp.tiles.replace(tiles);
    }

    /// The first tile that can take focus.
    pub(crate) fn first_tile(&self) -> Option<gtk::Widget> {
        self.imp()
            .tiles
            .borrow()
            .iter()
            .find(|tile| takes_focus(tile))
            .cloned()
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
        if let Some(index) = tiles.iter().position(|tile| within(&focus, tile)) {
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
                leave(&stops, &tiles, here, direction);
                Some(Step::Moved)
            }
        }
    }

    /// Moves focus `direction` from the tile at `index` of `tiles`: to the tile
    /// `step_tile` says, passing those that cannot take focus, or out of the grid.
    fn step_from_tile(&self, tiles: &[gtk::Widget], index: usize, direction: Direction) {
        let count = u32::try_from(tiles.len()).unwrap_or(u32::MAX);
        let mut index = u32::try_from(index).unwrap_or(u32::MAX);
        loop {
            match step_tile(index, direction, count) {
                TileStep::Tile(next) if next == index => return,
                TileStep::Tile(next) => {
                    index = next;
                    let tile = &tiles[next as usize];
                    if takes_focus(tile) {
                        tile.grab_focus();
                        return;
                    }
                }
                TileStep::Above | TileStep::Below => {
                    let stops = self.stops(tiles);
                    if let Some(here) = stops.iter().position(|stop| matches!(stop, Stop::Tiles)) {
                        leave(&stops, tiles, here, direction);
                    }
                    return;
                }
            }
        }
    }

    /// The places Up and Down stop at, top to bottom, of those that show and can take
    /// focus.
    fn stops(&self, tiles: &[gtk::Widget]) -> Vec<Stop> {
        let parts = self.imp().parts.borrow();
        let Some(parts) = parts.as_ref() else {
            return Vec::new();
        };
        let mut stops = Vec::new();
        if parts.playing_cards.is_visible() {
            let cards = std::iter::successors(parts.playing_cards.first_child(), |card| {
                card.next_sibling()
            });
            for card in cards.filter(|card| card.is_visible()) {
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
        if tiles.iter().any(takes_focus) {
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
}

/// Moves focus from `stops[here]` to the stop above or below it (`direction`), if there
/// is one: into the tiles from above their first row's first tile, from below their last
/// row's.
fn leave(stops: &[Stop], tiles: &[gtk::Widget], here: usize, direction: Direction) {
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
            let columns = QUICK_TILE_COLUMNS as usize;
            let start = match direction {
                Direction::Up => tiles.len().saturating_sub(1) / columns * columns,
                _ => 0,
            };
            let tile = tiles[start..]
                .iter()
                .chain(tiles[..start].iter().rev())
                .find(|tile| takes_focus(tile));
            if let Some(tile) = tile {
                tile.grab_focus();
            }
        }
        None => {}
    }
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
