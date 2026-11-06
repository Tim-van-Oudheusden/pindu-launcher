//! User-facing names of game sources.

use gettextrs::gettext;
use pindu_core::model::Source;

pub fn source_name(source: Source) -> String {
    match source {
        Source::Steam => "Steam".to_owned(),
        Source::HeroicEpic => gettext("Heroic: Epic"),
        Source::HeroicGog => gettext("Heroic: GOG"),
        Source::HeroicAmazon => gettext("Heroic: Amazon"),
        Source::HeroicSideload => gettext("Heroic: Sideloaded"),
        Source::Lutris => "Lutris".to_owned(),
        Source::Bottles => "Bottles".to_owned(),
        Source::Faugus => "Faugus".to_owned(),
        Source::Desktop => gettext("Desktop"),
        Source::Manual => gettext("Added by Hand"),
    }
}

/// The names of `sources` as a list in a sentence: "Steam", "Steam and Lutris", "Steam,
/// Lutris and Bottles".
pub fn source_list(sources: &[Source]) -> String {
    let names: Vec<String> = sources.iter().copied().map(source_name).collect();
    match names.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => {
            // Translators: the separator between the first items of a list.
            let items = rest.join(&gettext(", "));
            // Translators: a list's first items and its last, as in "Steam, Lutris and Bottles".
            gettext("{items} and {last}")
                .replace("{items}", &items)
                .replace("{last}", last)
        }
    }
}

/// A symbolic icon for a source's sidebar row.
pub fn source_icon(source: Source) -> &'static str {
    match source {
        Source::Desktop => "computer-symbolic",
        Source::Manual => "document-edit-symbolic",
        _ => "applications-games-symbolic",
    }
}
