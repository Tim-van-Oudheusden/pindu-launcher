//! Focus memory for the fullscreen shell: each page and tab keeps the item that last had
//! focus, so coming back puts focus where it was.

use std::collections::HashMap;
use std::hash::Hash;

/// The item that last had focus in each place (a page, or a tab of one).
#[derive(Debug)]
pub struct FocusMemory<Place, Item> {
    items: HashMap<Place, Item>,
}

impl<Place: Eq + Hash, Item: PartialEq> FocusMemory<Place, Item> {
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
        }
    }

    /// Notes that `item` has focus in `place`.
    pub fn remember(&mut self, place: Place, item: Item) {
        self.items.insert(place, item);
    }

    /// Where focus goes on coming back to `place`, which now shows `items`: the
    /// remembered item wherever it moved to.
    pub fn position(&self, place: &Place, items: &[Item]) -> usize {
        let Some(item) = self.items.get(place) else {
            return 0;
        };
        items
            .iter()
            .position(|candidate| candidate == item)
            .unwrap_or(0)
    }
}

impl<Place: Eq + Hash, Item: PartialEq> Default for FocusMemory<Place, Item> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn coming_back_to_a_tab_focuses_its_item_where_it_now_is() {
        let mut memory = FocusMemory::new();
        memory.remember("all", "skyrim");
        memory.remember("installed", "portal");

        assert_eq!(memory.position(&"all", &["doom", "skyrim", "portal"]), 1);
        assert_eq!(
            memory.position(&"installed", &["doom", "skyrim", "portal"]),
            2
        );
        // The list re-sorted: the item is found where it moved to.
        assert_eq!(memory.position(&"all", &["skyrim", "doom"]), 0);
    }

    #[test]
    fn a_new_place_or_a_vanished_item_starts_on_the_first_item() {
        let mut memory = FocusMemory::new();
        memory.remember("all", "skyrim");

        assert_eq!(memory.position(&"favourites", &["doom", "portal"]), 0);
        assert_eq!(memory.position(&"all", &["doom", "portal"]), 0);
    }
}
