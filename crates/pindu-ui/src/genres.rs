//! The names the shells show for the library's genres, which fullscreen Home calls
//! categories: a game's page lists them, and Home names its categories with them.

use gettextrs::gettext;

/// A genre as Pindu shows it: each of the categories providers map onto
/// (`pindu_core::category::CATEGORIES`), translated; any other, such as one the user
/// typed, as it was written.
pub fn genre_label(genre: &str) -> String {
    match genre {
        "Action" => gettext("Action"),
        "Strategy" => gettext("Strategy"),
        "RPG" => gettext("RPG"),
        "Casual" => gettext("Casual"),
        "Racing" => gettext("Racing"),
        "Sports" => gettext("Sports"),
        "Indie" => gettext("Indie"),
        "Adventure" => gettext("Adventure"),
        "Simulation" => gettext("Simulation"),
        "Massively Multiplayer" => gettext("Massively Multiplayer"),
        "Shooter" => gettext("Shooter"),
        "Platformer" => gettext("Platformer"),
        "Puzzle" => gettext("Puzzle"),
        "Fighting" => gettext("Fighting"),
        other => other.to_owned(),
    }
}
