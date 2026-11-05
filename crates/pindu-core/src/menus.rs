//! Popover menus under a controller: B steps out of a submenu back to the item that opened
//! it, and closes the menu from its top level.

/// The name a popover menu gives its top level; each submenu has a name of its own.
pub const TOP_LEVEL: &str = "main";

/// What B does in an open menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// Go back from the submenu to the menu it opened from.
    StepOut,
    /// Close the menu.
    Close,
}

/// What B does in a menu that shows `submenu`, by its name; a menu without submenus (a
/// plain popover) has none.
pub fn back(submenu: Option<&str>) -> Back {
    match submenu {
        Some(name) if name != TOP_LEVEL => Back::StepOut,
        _ => Back::Close,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn b_closes_the_menu_from_its_top_level() {
        assert_eq!(back(Some(TOP_LEVEL)), Back::Close);
        assert_eq!(back(None), Back::Close, "a popover without submenus");
    }

    #[test]
    fn b_steps_out_of_a_submenu_first() {
        assert_eq!(back(Some("Add to Collection")), Back::StepOut);
    }
}
