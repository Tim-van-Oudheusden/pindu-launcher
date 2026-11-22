//! The names the shells show for the library's genres, which fullscreen Home calls
//! categories: a game's page lists them, and Home names its categories with them.

use gettextrs::gettext;

/// A genre as Pindu shows it: the genres Steam's importer names, translated; any other as
/// its source wrote it.
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
        other => other.to_owned(),
    }
}
