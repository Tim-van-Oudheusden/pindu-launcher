//! Wikidata, the online metadata provider spike #528 chose: which games are looked up and
//! by what, the SPARQL query for a batch of them, its answer, and what a refresh tells the
//! user. The HTTP calls and the task in Activity live in the UI's service; a refresh runs
//! only when the user asks for one, never during an import.
//!
//! Games are looked up by Steam app ID only (Wikidata's `P1733`), never by title, so a
//! wrong match cannot happen: a Steam game by its own ID, another game by the Steam ID its
//! source links it to or by a grouped Steam copy's.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use serde_json::Value as Json;

use crate::launch::percent_encode;
use crate::metadata::Metadata;
use crate::model::{Game, GameGroup, GameId, MetadataOrigin, Source, SourceDetails, Timestamp};
use crate::store::{self, Store};

/// The Wikidata Query Service's SPARQL endpoint.
const ENDPOINT: &str = "https://query.wikidata.org/sparql";

/// The User-Agent requests carry: Wikimedia asks for one that says how to reach the
/// people behind a client.
pub const USER_AGENT: &str = concat!(
    "pindu-launcher/",
    env!("CARGO_PKG_VERSION"),
    " (https://github.com/Tim-van-Oudheusden/pindu-launcher)"
);

/// The media type of a SPARQL answer in JSON, which requests ask for.
pub const ACCEPT: &str = "application/sparql-results+json";

/// The most Steam app IDs one query asks for.
pub const BATCH_SIZE: usize = 100;

/// Steam app IDs a game in the library is looked up by, by game: hidden games and games
/// no longer in the library are left out. A game's own Steam ID comes first, then the one
/// its source links it to (Lutris's `steam` service), then a grouped copy's.
pub fn steam_app_ids(
    games: &[Game],
    details: &HashMap<GameId, SourceDetails>,
    groups: &[GameGroup],
) -> BTreeMap<GameId, u32> {
    let own = |game: &Game| match game.source() {
        Source::Steam => game.id.native_id().parse().ok(),
        _ => details
            .get(&game.id)
            .and_then(|details| details.service.as_ref())
            .filter(|service| service.service == "steam")
            .and_then(|service| service.service_id.parse().ok()),
    };
    let own_ids: HashMap<&GameId, u32> = games
        .iter()
        .filter_map(|game| Some((&game.id, own(game)?)))
        .collect();
    let grouped_ids: HashMap<&GameId, u32> = groups
        .iter()
        .filter_map(|group| {
            let app_id = group.members.iter().find_map(|id| own_ids.get(id))?;
            Some(group.members.iter().map(move |id| (id, *app_id)))
        })
        .flatten()
        .collect();
    games
        .iter()
        .filter(|game| game.in_library() && !game.hidden)
        .filter_map(|game| {
            let app_id = own_ids
                .get(&game.id)
                .or_else(|| grouped_ids.get(&game.id))?;
            Some((game.id.clone(), *app_id))
        })
        .collect()
}

/// The Steam app IDs to ask for, each once, in batches of at most [`BATCH_SIZE`], to
/// be asked for one after another.
pub fn batches(app_ids: &BTreeMap<GameId, u32>) -> Vec<Vec<u32>> {
    let unique: Vec<u32> = app_ids
        .values()
        .copied()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    unique.chunks(BATCH_SIZE).map(<[u32]>::to_vec).collect()
}

/// The languages labels are asked for in, best first, from the UI's locale names (as
/// `g_get_language_names` gives them): their languages as Wikidata writes them, then
/// English, then `mul`, the label many items keep for every language.
pub fn label_languages<S: AsRef<str>>(locale_names: &[S]) -> String {
    let mut languages: Vec<String> = Vec::new();
    let ui = locale_names.iter().filter_map(|name| {
        let name = name.as_ref();
        let language = name
            .split(['.', '@'])
            .next()?
            .to_lowercase()
            .replace('_', "-");
        let valid = !language.is_empty()
            && language
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b == b'-')
            && language != "c"
            && language != "posix";
        valid.then_some(language)
    });
    for language in ui.chain(["en".to_owned(), "mul".to_owned()]) {
        if !languages.contains(&language) {
            languages.push(language);
        }
    }
    languages.join(",")
}

/// The address of the query for one batch of Steam app IDs, with labels in `languages`
/// ([`label_languages`]).
pub fn query_url(app_ids: &[u32], languages: &str) -> String {
    format!(
        "{ENDPOINT}?query={}",
        percent_encode(&query(app_ids, languages))
    )
}

/// The SPARQL query for one batch: each item with one of the Steam app IDs (`P1733`),
/// with one row per developer (`P178`) and per genre (`P136`), labelled in `languages`.
fn query(app_ids: &[u32], languages: &str) -> String {
    let ids = app_ids
        .iter()
        .map(|id| format!("\"{id}\""))
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        "SELECT ?steam ?item ?field ?valueLabel WHERE {{\n\
         VALUES ?steam {{ {ids} }}\n\
         ?item wdt:P1733 ?steam .\n\
         {{ ?item wdt:P178 ?value . BIND(\"developer\" AS ?field) }}\n\
         UNION {{ ?item wdt:P136 ?value . BIND(\"genre\" AS ?field) }}\n\
         SERVICE wikibase:label {{ bd:serviceParam wikibase:language \"{languages}\" . }}\n\
         }}"
    )
}

/// What Wikidata holds about a game.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    /// The item's ID, such as `Q193581`.
    pub qid: String,
    /// Who made the game, sorted.
    pub developers: Vec<String>,
    /// The game's genres, sorted.
    pub genres: Vec<String>,
}

impl Item {
    /// The values the provider gives the game: its developers as one, and its genres.
    pub fn metadata(&self) -> Metadata {
        Metadata {
            developer: (!self.developers.is_empty()).then(|| self.developers.join(", ")),
            genres: self.genres.clone(),
            description: None,
        }
    }
}

/// The items of an answer, by the Steam app ID they were found by. A Steam ID that
/// Wikidata has no item for, or no developer or genre, is not there. A label Wikidata
/// has in none of the languages asked for comes back as the value's ID, and is left out.
pub fn parse(answer: &str) -> Result<HashMap<u32, Item>, String> {
    let json: Json = serde_json::from_str(answer).map_err(|error| error.to_string())?;
    let rows = json
        .pointer("/results/bindings")
        .and_then(Json::as_array)
        .ok_or("the answer has no results")?;
    let mut found: HashMap<u32, (String, BTreeSet<String>, BTreeSet<String>)> = HashMap::new();
    for row in rows {
        let text = |key: &str| row.pointer(&format!("/{key}/value")).and_then(Json::as_str);
        let (Some(steam), Some(item), Some(field), Some(label)) = (
            text("steam").and_then(|id| id.parse().ok()),
            text("item").and_then(|uri| uri.rsplit('/').next()),
            text("field"),
            text("valueLabel"),
        ) else {
            continue;
        };
        let (_, developers, genres) = found
            .entry(steam)
            .or_insert_with(|| (item.to_owned(), BTreeSet::new(), BTreeSet::new()));
        if is_item_id(label) {
            continue;
        }
        match field {
            "developer" => developers.insert(label.to_owned()),
            "genre" => genres.insert(label.to_owned()),
            _ => false,
        };
    }
    Ok(found
        .into_iter()
        .map(|(steam, (qid, developers, genres))| {
            let item = Item {
                qid,
                developers: developers.into_iter().collect(),
                genres: genres.into_iter().collect(),
            };
            (steam, item)
        })
        .collect())
}

/// Whether `text` is an item ID such as `Q42`, which the label service gives for a
/// missing label.
fn is_item_id(text: &str) -> bool {
    text.strip_prefix('Q')
        .is_some_and(|digits| !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit()))
}

/// How a refresh ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// Every batch was asked for.
    Done,
    /// Wikidata could not be reached, or kept refusing for a while.
    Unreachable,
    /// The user cancelled it.
    Cancelled,
}

/// What a refresh did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshRun {
    /// Games it could look up, by a Steam app ID.
    pub looked_up: usize,
    /// Games Wikidata had details for, which it stored.
    pub updated: usize,
    /// How it ended.
    pub end: RunEnd,
}

/// What the user is told when a refresh ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunNotice {
    /// This many games got details.
    Updated(usize),
    /// Wikidata had nothing for the games looked up.
    NoneFound,
    /// No game in the library has a Steam app ID to look it up by.
    NothingToLookUp,
    /// Wikidata could not be reached, or kept refusing for a while.
    Unreachable,
}

/// What a refresh tells the user: how it went, or, cancelled, only what it stored.
pub fn run_notice(run: &RefreshRun) -> Option<RunNotice> {
    match run.end {
        RunEnd::Unreachable => Some(RunNotice::Unreachable),
        RunEnd::Cancelled => (run.updated > 0).then_some(RunNotice::Updated(run.updated)),
        RunEnd::Done if run.looked_up == 0 => Some(RunNotice::NothingToLookUp),
        RunEnd::Done if run.updated == 0 => Some(RunNotice::NoneFound),
        RunEnd::Done => Some(RunNotice::Updated(run.updated)),
    }
}

/// The games a refresh looks up in the library `store` holds, with the Steam app ID each
/// is looked up by ([`steam_app_ids`]).
pub fn games_to_look_up(store: &Store) -> store::Result<BTreeMap<GameId, u32>> {
    Ok(steam_app_ids(
        &store.games()?,
        &store.all_source_details()?,
        &store.game_groups()?,
    ))
}

/// Stores the details Wikidata gave a batch as each game's provider values, set at
/// `now`; says how many games got some. A game whose Steam app ID has no item keeps what
/// it had.
pub fn store_items(
    store: &Store,
    app_ids: &BTreeMap<GameId, u32>,
    items: &HashMap<u32, Item>,
    now: Timestamp,
) -> store::Result<usize> {
    let mut updated = 0;
    for (id, app_id) in app_ids {
        if let Some(item) = items.get(app_id) {
            store.set_metadata(id, MetadataOrigin::Provider, &item.metadata(), now)?;
            updated += 1;
        }
    }
    Ok(updated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::LaunchKind;
    use crate::sources::ServiceRef;

    fn game(source: Source, native_id: &str) -> Game {
        Game::new(
            GameId::new(source, native_id),
            native_id,
            LaunchKind::StoreUri,
            0,
        )
    }

    fn linked_to_steam(app_id: &str) -> SourceDetails {
        SourceDetails {
            service: Some(ServiceRef {
                service: "steam".into(),
                service_id: app_id.into(),
            }),
            ..SourceDetails::default()
        }
    }

    #[test]
    fn games_are_looked_up_by_their_own_their_sources_or_a_grouped_copys_steam_id() {
        let steam = game(Source::Steam, "220");
        let wrapped = game(Source::Lutris, "7");
        let grouped = game(Source::HeroicGog, "1207658691");
        let unknown = game(Source::HeroicEpic, "Fortnite");
        let mut hidden = game(Source::Steam, "400");
        hidden.hidden = true;
        let mut removed = game(Source::Steam, "500");
        removed.user_removed = true;
        let shortcut = game(Source::Steam, "not-a-number");
        let details = HashMap::from([(wrapped.id.clone(), linked_to_steam("1245620"))]);
        let groups = [GameGroup {
            id: 1,
            members: vec![grouped.id.clone(), steam.id.clone()],
            preferred: None,
            automatic: true,
        }];
        let games = [
            steam.clone(),
            wrapped.clone(),
            grouped.clone(),
            unknown,
            hidden,
            removed,
            shortcut,
        ];

        assert_eq!(
            steam_app_ids(&games, &details, &groups),
            BTreeMap::from([(steam.id, 220), (wrapped.id, 1_245_620), (grouped.id, 220)])
        );
    }

    #[test]
    fn ids_are_asked_for_once_in_batches_of_a_hundred() {
        let mut app_ids: BTreeMap<GameId, u32> = (0..250)
            .map(|n| (GameId::new(Source::Steam, &n.to_string()), 1000 + n))
            .collect();
        app_ids.insert(GameId::new(Source::Lutris, "1"), 1000);

        let batches = batches(&app_ids);

        assert_eq!(
            batches.iter().map(Vec::len).collect::<Vec<_>>(),
            [100, 100, 50]
        );
        assert_eq!(batches.concat(), (1000..1250).collect::<Vec<_>>());
        assert!(super::batches(&BTreeMap::new()).is_empty());
    }

    #[test]
    fn labels_are_asked_for_in_the_ui_language_then_english_then_mul() {
        assert_eq!(
            label_languages(&["de_DE.UTF-8", "de_DE", "de.UTF-8", "de", "C"]),
            "de-de,de,en,mul"
        );
        assert_eq!(
            label_languages(&["en_GB.UTF-8@euro", "en_GB", "en", "C"]),
            "en-gb,en,mul"
        );
        assert_eq!(label_languages(&["C", "POSIX"]), "en,mul");
        assert_eq!(label_languages(&["x\"y"]), "en,mul");
    }

    #[test]
    fn a_batch_is_one_query_by_steam_app_id_for_developers_and_genres() {
        let query = query(&[220, 400], "de,en,mul");

        assert!(
            query.contains(r#"VALUES ?steam { "220" "400" }"#),
            "{query}"
        );
        assert!(query.contains("?item wdt:P1733 ?steam"), "{query}");
        assert!(query.contains("wdt:P178"), "{query}");
        assert!(query.contains("wdt:P136"), "{query}");
        assert!(
            query.contains(r#"wikibase:language "de,en,mul""#),
            "{query}"
        );
        assert_eq!(
            query_url(&[220, 400], "de,en,mul"),
            format!("{ENDPOINT}?query={}", percent_encode(&query))
        );
    }

    #[test]
    fn an_answer_gives_each_steam_ids_item_with_sorted_developers_and_genres() {
        let answer = include_str!("../tests/fixtures/wikidata/steam_ids.json");

        let items = parse(answer).unwrap();

        assert_eq!(items.len(), 3, "4000000000 has no item");
        assert_eq!(
            items[&220],
            Item {
                qid: "Q193581".into(),
                developers: vec!["Valve Corporation".into()],
                genres: vec![
                    "Ego-Shooter".into(),
                    "Science-Fiction-Computerspiel".into(),
                    "postapokalyptisches Videospiel".into(),
                    "science fiction horror".into(),
                ],
            }
        );
        assert_eq!(items[&1_245_620].qid, "Q64826862");
        assert_eq!(items[&1_245_620].developers, ["FromSoftware"]);
    }

    #[test]
    fn a_label_wikidata_lacks_in_every_language_asked_is_left_out() {
        let answer = r#"{"results": {"bindings": [
            {"steam": {"value": "10"}, "item": {"value": "http://www.wikidata.org/entity/Q1"},
             "field": {"value": "genre"}, "valueLabel": {"value": "Q999"}},
            {"steam": {"value": "10"}, "item": {"value": "http://www.wikidata.org/entity/Q1"},
             "field": {"value": "genre"}, "valueLabel": {"value": "platform game"}},
            {"steam": {"value": "10"}, "item": {"value": "http://www.wikidata.org/entity/Q1"},
             "field": {"value": "genre"}, "valueLabel": {"value": "platform game"}}
        ]}}"#;

        assert_eq!(parse(answer).unwrap()[&10].genres, ["platform game"]);
    }

    #[test]
    fn an_answer_that_is_not_sparql_json_is_an_error() {
        assert!(parse("<html>busy</html>").is_err());
        assert!(parse("{}").is_err());
    }

    #[test]
    fn an_items_developers_are_one_value_and_its_genres_another() {
        let item = Item {
            qid: "Q1".into(),
            developers: vec!["Arkane Studios".into(), "Bethesda Game Studios".into()],
            genres: vec!["immersive sim".into()],
        };

        assert_eq!(
            item.metadata(),
            Metadata {
                developer: Some("Arkane Studios, Bethesda Game Studios".into()),
                genres: vec!["immersive sim".into()],
                description: None,
            }
        );
        assert_eq!(Item::default().metadata(), Metadata::default());
    }

    fn run(looked_up: usize, updated: usize, end: RunEnd) -> RefreshRun {
        RefreshRun {
            looked_up,
            updated,
            end,
        }
    }

    #[test]
    fn a_refresh_says_how_it_went_or_cancelled_only_what_it_stored() {
        assert_eq!(
            run_notice(&run(40, 36, RunEnd::Done)),
            Some(RunNotice::Updated(36))
        );
        assert_eq!(
            run_notice(&run(4, 0, RunEnd::Done)),
            Some(RunNotice::NoneFound)
        );
        assert_eq!(
            run_notice(&run(0, 0, RunEnd::Done)),
            Some(RunNotice::NothingToLookUp)
        );
        assert_eq!(
            run_notice(&run(300, 90, RunEnd::Unreachable)),
            Some(RunNotice::Unreachable)
        );
        assert_eq!(
            run_notice(&run(300, 90, RunEnd::Cancelled)),
            Some(RunNotice::Updated(90))
        );
        assert_eq!(run_notice(&run(300, 0, RunEnd::Cancelled)), None);
    }

    #[test]
    fn a_refresh_stores_what_wikidata_has_as_provider_values_and_leaves_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("library.db")).unwrap();
        let mut steam = game(Source::Steam, "220");
        steam.developer = Some("Valve".into());
        let wrapped = game(Source::Lutris, "7");
        let mut unknown = game(Source::Steam, "4000000000");
        unknown.genres = vec!["Indie".into()];
        for game in [&steam, &wrapped, &unknown] {
            store.insert_game(game).unwrap();
        }
        store
            .set_source_details(&wrapped.id, &linked_to_steam("1245620"))
            .unwrap();

        let app_ids = games_to_look_up(&store).unwrap();
        assert_eq!(
            app_ids,
            BTreeMap::from([
                (steam.id.clone(), 220),
                (wrapped.id.clone(), 1_245_620),
                (unknown.id.clone(), 4_000_000_000),
            ])
        );
        let items = parse(include_str!("../tests/fixtures/wikidata/steam_ids.json")).unwrap();
        assert_eq!(store_items(&store, &app_ids, &items, 500).unwrap(), 2);

        let values = store.metadata_values(&steam.id).unwrap();
        let provider: Vec<_> = values
            .iter()
            .filter(|value| value.origin == MetadataOrigin::Provider)
            .collect();
        assert_eq!(provider.len(), 2, "{values:?}");
        assert!(provider.iter().all(|value| value.updated_at == 500));
        assert!(
            values
                .iter()
                .any(|value| value.origin == MetadataOrigin::Source),
            "the source's value stays"
        );
        let shown = store.game(&steam.id).unwrap().unwrap();
        assert_eq!(shown.developer.as_deref(), Some("Valve Corporation"));
        assert_eq!(
            store
                .game(&wrapped.id)
                .unwrap()
                .unwrap()
                .developer
                .as_deref(),
            Some("FromSoftware")
        );
        assert_eq!(
            store.game(&unknown.id).unwrap().unwrap().genres,
            ["Indie"],
            "no item, nothing changes"
        );
    }
}
