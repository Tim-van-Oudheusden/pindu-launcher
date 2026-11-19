//! Duplicate suggestions: pairs of records in the library that look like copies of one
//! game, each with its reason and a confidence. The rules are spike #501's.
//!
//! Matching is staged and conservative. First, exact cross-references: a Lutris record's
//! store link (`service`, `service_id`) to the Steam, Heroic GOG or Heroic Epic record with
//! that ID. Then equal normalised titles ([`normalize`]) of records from different
//! sources, more sure when the developer or the year agrees. Only exact cross-references
//! are grouped without the user ([`group_exact_matches`]); a title match is only ever a
//! suggestion. A pair remembered as not the same game is never suggested again.
//!
//! Finding suggestions writes nothing.

use std::cmp::Reverse;
use std::collections::{HashMap, HashSet};

use crate::model::{Game, GameGroup, GameId, Source, SourceDetails};
use crate::search::normalize;
use crate::store::{self, Store};

/// The Lutris `service` values whose `service_id` is another source's own ID, and that
/// source. Amazon is left out: Lutris stores the entitlement ID, Heroic the product ID.
const CROSS_REFERENCES: &[(&str, Source)] = &[
    ("steam", Source::Steam),
    ("steamwindows", Source::Steam),
    ("gog", Source::HeroicGog),
    ("egs", Source::HeroicEpic),
];

/// Words a developer's name may carry or leave out without naming someone else.
const COMPANY_WORDS: &[&str] = &["inc", "ltd", "llc", "gmbh", "corp", "co"];

/// How sure a suggestion is that two records are copies of one game, least sure first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Confidence {
    /// Equal titles, and neither the developer nor the year agrees on both records.
    Low,
    /// Equal titles, and the developer or the year agrees on both records.
    High,
    /// A store cross-reference; grouped without asking, with an undo.
    Exact,
}

/// Why two records look like copies of one game.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Reason {
    /// A Lutris record links to the other record's store ID.
    CrossReference {
        /// The Lutris `service` the link is in, such as `steam`, `gog` or `egs`.
        service: String,
    },
    /// The normalised titles are equal.
    Title {
        /// Whether both records name a developer and the names agree.
        same_developer: bool,
        /// Whether both records know the year and the years are at most one apart.
        close_year: bool,
    },
}

/// Two records that look like copies of one game.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Suggestion {
    /// The lower of the two game IDs.
    pub first: GameId,
    /// The higher of the two game IDs.
    pub second: GameId,
    /// Why they look alike.
    pub reason: Reason,
    /// How sure the suggestion is.
    pub confidence: Confidence,
}

/// The pairs of records in the library that look like copies of one game: exact
/// cross-references first, then title matches by falling confidence, each by game ID.
///
/// `details` are the records' [`SourceDetails`], `groups` the library's groups and `splits`
/// the pairs remembered as not the same game, in either order. Records gone from their
/// source or removed by the user, pairs already in one group and remembered pairs are
/// left out. A record may appear in several suggestions.
pub fn suggestions(
    games: &[Game],
    details: &HashMap<GameId, SourceDetails>,
    groups: &[GameGroup],
    splits: &[(GameId, GameId)],
) -> Vec<Suggestion> {
    let library: HashMap<&GameId, &Game> = games
        .iter()
        .filter(|game| game.in_library())
        .map(|game| (&game.id, game))
        .collect();
    let group_of: HashMap<&GameId, i64> = groups
        .iter()
        .flat_map(|group| group.members.iter().map(move |game| (game, group.id)))
        .collect();
    let splits: HashSet<(&GameId, &GameId)> = splits
        .iter()
        .map(|(first, second)| ordered(first, second))
        .collect();

    // Candidates: each cross-reference, and each pair of records with one title.
    let mut candidates: HashSet<(&GameId, &GameId)> = HashSet::new();
    let mut by_title: HashMap<String, Vec<&GameId>> = HashMap::new();
    for game in library.values() {
        if let Some((target, _)) = cross_reference(game, details)
            && let Some((&target, _)) = library.get_key_value(&target)
        {
            candidates.insert(ordered(&game.id, target));
        }
        let title = normalize(&game.title);
        if !title.is_empty() {
            by_title.entry(title).or_default().push(&game.id);
        }
    }
    for same_title in by_title.values() {
        for (index, first) in same_title.iter().enumerate() {
            for second in &same_title[index + 1..] {
                candidates.insert(ordered(first, second));
            }
        }
    }

    let mut found: Vec<Suggestion> = candidates
        .into_iter()
        .filter(|pair| !splits.contains(pair))
        .filter(
            |(first, second)| match (group_of.get(first), group_of.get(second)) {
                (Some(first), Some(second)) => first != second,
                _ => true,
            },
        )
        .filter_map(|(first, second)| {
            let (reason, confidence) = pair_match(library[first], library[second], details)?;
            Some(Suggestion {
                first: first.clone(),
                second: second.clone(),
                reason,
                confidence,
            })
        })
        .collect();
    found.sort_unstable_by(|a, b| {
        (Reverse(a.confidence), &a.first, &a.second).cmp(&(
            Reverse(b.confidence),
            &b.first,
            &b.second,
        ))
    });
    found
}

/// Why and how surely two records look like copies of one game, in either order; `None`
/// when they do not.
fn pair_match(
    a: &Game,
    b: &Game,
    details: &HashMap<GameId, SourceDetails>,
) -> Option<(Reason, Confidence)> {
    if a.source() == b.source() {
        return None;
    }
    for (from, to) in [(a, b), (b, a)] {
        if let Some((target, service)) = cross_reference(from, details)
            && target == to.id
        {
            return Some((
                Reason::CrossReference {
                    service: service.to_owned(),
                },
                Confidence::Exact,
            ));
        }
    }
    let title = normalize(&a.title);
    if title.is_empty() || title != normalize(&b.title) {
        return None;
    }
    let same_developer = match (developer(a), developer(b)) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    };
    let year = |game: &Game| details.get(&game.id).and_then(|details| details.year);
    let close_year = match (year(a), year(b)) {
        (Some(a), Some(b)) => a.abs_diff(b) <= 1,
        _ => false,
    };
    let confidence = if same_developer || close_year {
        Confidence::High
    } else {
        Confidence::Low
    };
    Some((
        Reason::Title {
            same_developer,
            close_year,
        },
        confidence,
    ))
}

/// The record a Lutris record's store link names, and the link's `service`; `None` for
/// other sources and for links to stores we hold no records of.
fn cross_reference<'a>(
    game: &Game,
    details: &'a HashMap<GameId, SourceDetails>,
) -> Option<(GameId, &'a str)> {
    if game.source() != Source::Lutris {
        return None;
    }
    let link = details.get(&game.id)?.service.as_ref()?;
    let service = link.service.trim();
    let service_id = link.service_id.trim();
    let (_, source) = CROSS_REFERENCES.iter().find(|(name, _)| *name == service)?;
    (!service_id.is_empty()).then(|| (GameId::new(*source, service_id), service))
}

/// A record's developer, normalised and without company words such as "Inc"; `None`
/// when it names none.
fn developer(game: &Game) -> Option<String> {
    let name = normalize(game.developer.as_deref()?);
    let words: Vec<&str> = name
        .split(' ')
        .filter(|word| !COMPANY_WORDS.contains(word))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

fn ordered<'a>(first: &'a GameId, second: &'a GameId) -> (&'a GameId, &'a GameId) {
    if first <= second {
        (first, second)
    } else {
        (second, first)
    }
}

/// The library's [`suggestions`], read from the store. Writes nothing.
pub fn load(store: &Store) -> store::Result<Vec<Suggestion>> {
    Ok(suggestions(
        &store.games()?,
        &store.all_source_details()?,
        &store.game_groups()?,
        &store.splits()?,
    ))
}

/// Groups each pair of records an exact cross-reference links, unless either is in a
/// group already ([`Store::create_automatic_group`]); returns the new groups' IDs. Title
/// matches are never grouped here. [`Store::split_group`] undoes a group and remembers the
/// pair, so it is not grouped again.
pub fn group_exact_matches(store: &Store) -> store::Result<Vec<i64>> {
    let mut grouped = Vec::new();
    for suggestion in load(store)? {
        if suggestion.confidence == Confidence::Exact
            && let Some(id) = store.create_automatic_group(&suggestion.first, &suggestion.second)?
        {
            grouped.push(id);
        }
    }
    Ok(grouped)
}

/// What the user reviews: the [`suggestions`] to accept or dismiss, and the groups made
/// automatically from exact cross-references, which [`Store::split_group`] undoes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    /// The library's suggestions, as [`load`] finds them.
    pub suggestions: Vec<Suggestion>,
    /// The groups the sync made and the user has not changed since, ordered by ID.
    pub automatic: Vec<GameGroup>,
    /// The title of every record the review names.
    titles: HashMap<GameId, String>,
}

impl Review {
    /// A record's title; its ID when the store holds no such record.
    pub fn title(&self, game: &GameId) -> String {
        self.titles
            .get(game)
            .cloned()
            .unwrap_or_else(|| game.to_string())
    }
}

/// The library's [`Review`], read from the store. Writes nothing.
pub fn review(store: &Store) -> store::Result<Review> {
    let games = store.games()?;
    let groups = store.game_groups()?;
    let suggestions = suggestions(
        &games,
        &store.all_source_details()?,
        &groups,
        &store.splits()?,
    );
    let automatic: Vec<GameGroup> = groups.into_iter().filter(|group| group.automatic).collect();
    let named: HashSet<&GameId> = suggestions
        .iter()
        .flat_map(|suggestion| [&suggestion.first, &suggestion.second])
        .chain(automatic.iter().flat_map(|group| &group.members))
        .collect();
    let titles = games
        .iter()
        .filter(|game| named.contains(&game.id))
        .map(|game| (game.id.clone(), game.title.clone()))
        .collect();
    Ok(Review {
        suggestions,
        automatic,
        titles,
    })
}

#[cfg(test)]
mod tests;
