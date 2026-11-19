//! Game groups as the library shows them: which copy of a grouped game Play starts, and
//! the group's playtime and last played. The rules are spike #501's; the groups
//! themselves live in the store ([`Store::game_groups`]).
//!
//! Everything here is computed when read and never stored, so ungrouping changes nothing.

use std::cmp::{Ordering, Reverse};
use std::collections::HashMap;

use crate::model::{Game, GameGroup, GameId, Platform, Source, Timestamp};
use crate::store::{self, Store};

/// The source order the default copy follows until the user changes it: store clients
/// first, as they handle updates and cloud saves, and Steam before Lutris, so a Lutris
/// entry that only starts Steam never wins over the Steam copy it wraps.
pub const DEFAULT_SOURCE_ORDER: &[Source] = &[
    Source::Steam,
    Source::HeroicGog,
    Source::HeroicEpic,
    Source::HeroicAmazon,
    Source::Legendary,
    Source::Itch,
    Source::Lutris,
    Source::Faugus,
    Source::Bottles,
    Source::HeroicSideload,
    Source::Manual,
    Source::Desktop,
];

/// The source order the `source-order` setting stores, as [`copy_order`] takes it: its
/// sources in the order it lists them, then the ones it leaves out in
/// [`DEFAULT_SOURCE_ORDER`]. Values that name no source, and repeats, are dropped, so a
/// setting from an older or newer version still gives every source one place.
pub fn source_order<S: AsRef<str>>(setting: impl IntoIterator<Item = S>) -> Vec<Source> {
    let mut order: Vec<Source> = Vec::with_capacity(DEFAULT_SOURCE_ORDER.len());
    let named = setting
        .into_iter()
        .filter_map(|value| value.as_ref().parse::<Source>().ok());
    for source in named.chain(DEFAULT_SOURCE_ORDER.iter().copied()) {
        if !order.contains(&source) {
            order.push(source);
        }
    }
    order
}

/// `order` with `source` one place earlier (`earlier`) or later, swapped with its
/// neighbour: what Move Up and Move Down do. At either end, or for a source `order` lacks,
/// the order stays as it is.
pub fn moved_source(order: &[Source], source: Source, earlier: bool) -> Vec<Source> {
    let mut order = order.to_vec();
    if let Some(index) = order.iter().position(|named| *named == source) {
        let other = if earlier {
            index.checked_sub(1)
        } else {
            Some(index + 1).filter(|&other| other < order.len())
        };
        if let Some(other) = other {
            order.swap(index, other);
        }
    }
    order
}

/// The Lutris runner that only starts the Steam client, so Steam times its sessions too.
const LUTRIS_STEAM_RUNNER: &str = "steam";

/// Whether a copy can be started: installed and still in the library.
fn playable(game: &Game) -> bool {
    game.installed && game.in_library()
}

/// The default order of a group's copies, best first: installed before not installed;
/// Linux, then unknown, then Windows; `source_order` (sources it leaves out go last);
/// most recently played; game ID ascending.
pub fn copy_order(a: &Game, b: &Game, source_order: &[Source]) -> Ordering {
    let platform = |game: &Game| match game.platform {
        Some(Platform::Linux) => 0,
        None => 1,
        Some(Platform::Windows) => 2,
    };
    let source = |game: &Game| {
        source_order
            .iter()
            .position(|source| *source == game.source())
            .unwrap_or(source_order.len())
    };
    let key = |game: &Game| {
        (
            !playable(game),
            platform(game),
            source(game),
            Reverse(game.last_played_at),
        )
    };
    key(a).cmp(&key(b)).then_with(|| a.id.cmp(&b.id))
}

/// The copy a group shows and starts when the user chose none: the first in
/// [`copy_order`].
pub fn default_copy<'a>(
    members: impl IntoIterator<Item = &'a Game>,
    source_order: &[Source],
) -> Option<&'a Game> {
    members
        .into_iter()
        .min_by(|a, b| copy_order(a, b, source_order))
}

/// The group's preferred copy: the one the user chose, else [`default_copy`]. `members`
/// are the group's games.
pub fn preferred_copy<'a>(
    group: &GameGroup,
    members: impl IntoIterator<Item = &'a Game>,
    source_order: &[Source],
) -> Option<&'a Game> {
    let members: Vec<&Game> = members.into_iter().collect();
    group
        .preferred
        .as_ref()
        .and_then(|preferred| members.iter().copied().find(|game| game.id == *preferred))
        .or_else(|| default_copy(members, source_order))
}

/// The copy Play starts: the preferred copy, unless the user chose one that cannot be
/// started, then the first installed copy in [`copy_order`]. With no installed copy it is
/// the preferred copy, so Play can say what is missing.
pub fn copy_to_play<'a>(
    group: &GameGroup,
    members: impl IntoIterator<Item = &'a Game>,
    source_order: &[Source],
) -> Option<&'a Game> {
    let members: Vec<&Game> = members.into_iter().collect();
    let preferred = preferred_copy(group, members.iter().copied(), source_order)?;
    if playable(preferred) {
        return Some(preferred);
    }
    default_copy(
        members.into_iter().filter(|game| playable(game)),
        source_order,
    )
    .or(Some(preferred))
}

/// One copy's share in its group's playtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CopyPlaytime<'a> {
    /// The copy.
    pub game: &'a Game,
    /// What the copy shows on its own: its source's total if one is stored, otherwise the
    /// sum of our sessions, otherwise 0.
    pub secs: i64,
    /// The runner its source starts it with, when the source names one.
    pub runner: Option<&'a str>,
}

impl CopyPlaytime<'_> {
    /// Whether Steam timed this copy's sessions: the Steam copy, and any Lutris copy that
    /// only starts Steam.
    fn timed_by_steam(&self) -> bool {
        match self.game.source() {
            Source::Steam => true,
            Source::Lutris => self.runner.map(str::trim) == Some(LUTRIS_STEAM_RUNNER),
            _ => false,
        }
    }
}

/// A group's playtime across its copies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupPlaytime {
    /// Total seconds, without counting a session twice.
    pub secs: i64,
    /// How many separately timed copies have playtime; above one, the total is "across N
    /// copies".
    pub played_copies: usize,
    /// When exactly one separately timed copy has playtime, the copy whose figure the
    /// total is, for its origin.
    pub from: Option<GameId>,
}

/// A group's playtime: copies timed together (the Steam copy and Lutris copies that only
/// start Steam) count once, by the largest figure among them; copies timed apart add up.
pub fn group_playtime(copies: &[CopyPlaytime<'_>]) -> GroupPlaytime {
    let mut steam: Option<&CopyPlaytime<'_>> = None;
    let mut apart = Vec::new();
    for copy in copies {
        if !copy.timed_by_steam() {
            apart.push(copy);
        } else if steam.is_none_or(|best| {
            (copy.secs, copy.game.source() == Source::Steam)
                > (best.secs, best.game.source() == Source::Steam)
        }) {
            steam = Some(copy);
        }
    }
    let played: Vec<&CopyPlaytime<'_>> = steam
        .into_iter()
        .chain(apart)
        .filter(|copy| copy.secs > 0)
        .collect();
    GroupPlaytime {
        secs: played.iter().map(|copy| copy.secs).sum(),
        played_copies: played.len(),
        from: match played.as_slice() {
            [only] => Some(only.game.id.clone()),
            _ => None,
        },
    }
}

/// When any of a group's copies was last played.
pub fn group_last_played<'a>(members: impl IntoIterator<Item = &'a Game>) -> Option<Timestamp> {
    members
        .into_iter()
        .filter_map(|game| game.last_played_at)
        .max()
}

/// The games a change to `game` is written to: every copy of its group, ordered by ID, or
/// the game alone when it is in no group. Favourite, hidden, collections and removal are a
/// view on a group, so the shells write them to each copy.
pub fn copies_of(store: &Store, game: &GameId) -> store::Result<Vec<GameId>> {
    Ok(store
        .game_group_of(game)?
        .map_or_else(|| vec![game.clone()], |group| group.members))
}

/// A group with what the library shows for it as one game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupSummary {
    /// The group.
    pub group: GameGroup,
    /// Its playtime across copies.
    pub playtime: GroupPlaytime,
    /// When any of its copies was last played.
    pub last_played: Option<Timestamp>,
}

/// Every group with its playtime and last played, worked out from its copies' stored
/// figures, ordered by group ID. A copy gone from its source keeps counting.
pub fn load(store: &Store) -> store::Result<Vec<GroupSummary>> {
    let groups = store.game_groups()?;
    if groups.is_empty() {
        return Ok(Vec::new());
    }
    let games: HashMap<GameId, Game> = store
        .games()?
        .into_iter()
        .map(|game| (game.id.clone(), game))
        .collect();
    let imported = store.imported_totals()?;
    let sessions = store.session_totals()?;
    let details = store.all_source_details()?;
    Ok(groups
        .into_iter()
        .map(|group| {
            let members: Vec<&Game> = group
                .members
                .iter()
                .filter_map(|id| games.get(id))
                .collect();
            let copies: Vec<CopyPlaytime<'_>> = members
                .iter()
                .map(|game| CopyPlaytime {
                    game,
                    secs: imported
                        .get(&game.id)
                        .or_else(|| sessions.get(&game.id))
                        .copied()
                        .unwrap_or(0),
                    runner: details
                        .get(&game.id)
                        .and_then(|details| details.runner.as_deref()),
                })
                .collect();
            GroupSummary {
                playtime: group_playtime(&copies),
                last_played: group_last_played(members),
                group,
            }
        })
        .collect())
}

#[cfg(test)]
mod tests;
