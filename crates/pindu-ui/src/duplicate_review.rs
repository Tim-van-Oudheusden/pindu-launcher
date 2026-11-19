//! How both shells word the duplicate review (`pindu_core::duplicates::review`): a
//! suggestion's titles, its sources, why the copies look alike and how sure that is, and a
//! group the sync made. The desktop's Duplicates page and fullscreen mode's Duplicates
//! level show the same words.

use gettextrs::gettext;
use pindu_core::duplicates::{Confidence, Reason, Review, Suggestion};
use pindu_core::model::{GameGroup, GameId, Source};

use crate::sources::source_list;

/// A suggestion's title: the copies' title, or both when they differ.
pub fn suggestion_title(review: &Review, suggestion: &Suggestion) -> String {
    let (first, second) = (
        review.title(&suggestion.first),
        review.title(&suggestion.second),
    );
    if first == second {
        first
    } else {
        // Translators: the titles of two copies that may be one game.
        gettext("{first} and {second}")
            .replace("{first}", &first)
            .replace("{second}", &second)
    }
}

/// A suggestion's subtitle: the copies' sources, why they look alike and how sure that is,
/// as in "Steam and Desktop · Same title and developer · Likely the same game".
pub fn suggestion_subtitle(suggestion: &Suggestion) -> String {
    let reason = match &suggestion.reason {
        Reason::CrossReference { .. } => gettext("Lutris links to this store game"),
        Reason::Title {
            same_developer: true,
            close_year: true,
        } => gettext("Same title, developer and year"),
        Reason::Title {
            same_developer: true,
            close_year: false,
        } => gettext("Same title and developer"),
        Reason::Title {
            same_developer: false,
            close_year: true,
        } => gettext("Same title and year"),
        Reason::Title { .. } => gettext("Same title"),
    };
    let confidence = match suggestion.confidence {
        Confidence::Exact => gettext("Certainly the same game"),
        Confidence::High => gettext("Likely the same game"),
        Confidence::Low => gettext("Possibly the same game"),
    };
    // Translators: a duplicate suggestion's sources, its reason and its confidence.
    gettext("{sources} · {reason} · {confidence}")
        .replace(
            "{sources}",
            &sources(&[&suggestion.first, &suggestion.second]),
        )
        .replace("{reason}", &reason)
        .replace("{confidence}", &confidence)
}

/// The title of a group the sync made: its first copy's.
pub fn group_title(review: &Review, group: &GameGroup) -> String {
    group
        .members
        .first()
        .map(|game| review.title(game))
        .unwrap_or_default()
}

/// The subtitle of a group the sync made: that it was, and its copies' sources.
pub fn group_subtitle(group: &GameGroup) -> String {
    let members: Vec<&GameId> = group.members.iter().collect();
    // Translators: a group of copies of one game the sync made, and their sources.
    gettext("Grouped automatically · {sources}").replace("{sources}", &sources(&members))
}

/// The sources of `games`, as a list in a sentence.
fn sources(games: &[&GameId]) -> String {
    let sources: Vec<Source> = games.iter().map(|game| game.source()).collect();
    source_list(&sources)
}
