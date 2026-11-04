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

/// A symbolic icon for a source's sidebar row.
pub fn source_icon(source: Source) -> &'static str {
    match source {
        Source::Desktop => "computer-symbolic",
        Source::Manual => "document-edit-symbolic",
        _ => "applications-games-symbolic",
    }
}
