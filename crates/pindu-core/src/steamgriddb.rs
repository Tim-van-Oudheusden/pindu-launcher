//! SteamGridDB: request addresses, answers, the retry policy, and what a run fetching
//! missing art does after a failure and tells the user. The HTTP calls, the request queue
//! and the API key (kept with oo7) live in the UI's services.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value as Json;

use crate::launch::percent_encode;
use crate::model::{ArtLayout, ArtOrigin, Artwork};
use crate::search::normalize;
use crate::sources::sha256::sha256_hex;

const API: &str = "https://www.steamgriddb.com/api/v2";

/// Tries per request, the first included, before a rate limit is given up on.
pub const MAX_ATTEMPTS: u32 = 5;
/// The longest wait between tries.
const MAX_DELAY_SECS: u64 = 60;

/// How a game is known to SteamGridDB.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GameRef {
    /// By Steam app ID, for Steam games.
    Steam(u32),
    /// By SteamGridDB's own game ID, from a search.
    Grid(u64),
}

/// A game a title search found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub id: u64,
    pub name: String,
}

/// One image of a game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Image {
    pub id: u64,
    pub url: String,
    pub thumb: String,
    pub width: u32,
    pub height: u32,
}

/// The images of a game in one layout; covers are asked for at 600×900.
pub fn images_url(layout: ArtLayout, game: &GameRef) -> String {
    let kind = match layout {
        ArtLayout::Cover => "grids",
        ArtLayout::Hero => "heroes",
        ArtLayout::Logo => "logos",
        ArtLayout::Icon => "icons",
    };
    let by = match game {
        GameRef::Steam(app_id) => format!("steam/{app_id}"),
        GameRef::Grid(id) => format!("game/{id}"),
    };
    let query = if layout == ArtLayout::Cover {
        "?dimensions=600x900"
    } else {
        ""
    };
    format!("{API}/{kind}/{by}{query}")
}

/// The layouts to fetch for a game: those with no art yet, and, when SteamGridDB art is
/// preferred, those with only the source's. Art the user picked or fetched before stays.
pub fn layouts_to_fetch(artwork: &[Artwork], prefer_steamgriddb: bool) -> Vec<ArtLayout> {
    ArtLayout::ALL
        .iter()
        .copied()
        .filter(|layout| {
            !artwork.iter().any(|art| {
                art.layout == *layout
                    && match art.origin {
                        ArtOrigin::User | ArtOrigin::SteamGridDb => true,
                        ArtOrigin::Source => !prefer_steamgriddb,
                        ArtOrigin::Generated => false,
                    }
            })
        })
        .collect()
}

/// The SteamGridDB game a title search means: the first hit with the same name, letter
/// case, accents and punctuation aside. A near miss is no match, so a game never gets
/// another game's art.
pub fn matching_hit(title: &str, hits: &[SearchHit]) -> Option<u64> {
    let title = normalize(title);
    hits.iter()
        .find(|hit| normalize(&hit.name) == title)
        .map(|hit| hit.id)
}

/// A title search.
pub fn search_url(title: &str) -> String {
    format!("{API}/search/autocomplete/{}", percent_encode(title))
}

/// The games of a search answer.
pub fn parse_search(answer: &str) -> Result<Vec<SearchHit>, String> {
    Ok(data(answer)?
        .iter()
        .filter_map(|hit| {
            Some(SearchHit {
                id: hit.get("id")?.as_u64()?,
                name: hit.get("name")?.as_str()?.to_owned(),
            })
        })
        .collect())
}

/// The images of an images answer, leaving out ones marked not safe for work.
pub fn parse_images(answer: &str) -> Result<Vec<Image>, String> {
    Ok(data(answer)?
        .iter()
        .filter(|image| image.get("nsfw").and_then(Json::as_bool) != Some(true))
        .filter_map(|image| {
            let size = |key: &str| image.get(key)?.as_u64().map(|n| n as u32);
            Some(Image {
                id: image.get("id")?.as_u64()?,
                url: image.get("url")?.as_str()?.to_owned(),
                thumb: image.get("thumb")?.as_str()?.to_owned(),
                width: size("width").unwrap_or_default(),
                height: size("height").unwrap_or_default(),
            })
        })
        .collect())
}

/// The `data` array of a successful answer, or the answer's error message.
fn data(answer: &str) -> Result<Vec<Json>, String> {
    let json: Json = serde_json::from_str(answer).map_err(|error| error.to_string())?;
    if json.get("success").and_then(Json::as_bool) != Some(true) {
        let message = json
            .pointer("/errors/0")
            .and_then(Json::as_str)
            .unwrap_or("SteamGridDB refused the request");
        return Err(message.to_owned());
    }
    Ok(json
        .get("data")
        .and_then(Json::as_array)
        .cloned()
        .unwrap_or_default())
}

/// How long to wait after a rate-limited try (`attempt` counts from 0): the server's
/// `Retry-After` when it gives one, else 1, 2, 4… seconds, at most a minute; `None` once
/// [`MAX_ATTEMPTS`] tries were made.
pub fn retry_delay(attempt: u32, retry_after_secs: Option<u64>) -> Option<Duration> {
    if attempt >= MAX_ATTEMPTS {
        return None;
    }
    let secs = retry_after_secs.unwrap_or(1 << attempt.min(6));
    Some(Duration::from_secs(secs.min(MAX_DELAY_SECS)))
}

/// Why a SteamGridDB request failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestError {
    /// No answer came: offline, the name not resolved, the connection refused.
    Unreachable(String),
    /// SteamGridDB answered with this HTTP status.
    Status(u16),
    /// Still rate limited after [`MAX_ATTEMPTS`] tries.
    Busy,
    /// An answer that could not be read, or one with an error message.
    Answer(String),
}

impl RequestError {
    /// Whether SteamGridDB refused the API key.
    pub fn key_rejected(&self) -> bool {
        matches!(self, Self::Status(401 | 403))
    }
}

impl std::fmt::Display for RequestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            _ if self.key_rejected() => f.write_str("SteamGridDB did not accept the API key"),
            Self::Unreachable(error) | Self::Answer(error) => f.write_str(error),
            Self::Status(status) => write!(f, "SteamGridDB answered {status}"),
            Self::Busy => f.write_str("SteamGridDB is busy; try again later"),
        }
    }
}

/// How a run that fetches art for many games ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunEnd {
    /// Every game was tried.
    Done,
    /// SteamGridDB refused the API key.
    KeyRejected,
    /// SteamGridDB could not be reached, or kept refusing for a while.
    Unreachable,
}

/// What a run does after a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnFailure {
    /// Goes on with the next game: SteamGridDB has nothing for this one.
    SkipGame,
    /// Stops: the next requests would fail the same way.
    Stop(RunEnd),
}

/// What a failed request means for a run over many games.
pub fn on_failure(error: &RequestError) -> OnFailure {
    match error {
        _ if error.key_rejected() => OnFailure::Stop(RunEnd::KeyRejected),
        RequestError::Status(404) | RequestError::Answer(_) => OnFailure::SkipGame,
        RequestError::Unreachable(_) | RequestError::Busy | RequestError::Status(_) => {
            OnFailure::Stop(RunEnd::Unreachable)
        }
    }
}

/// What a run fetching missing art did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArtRun {
    /// Images stored.
    pub stored: usize,
    /// Games that got at least one image.
    pub games_with_art: usize,
    /// Games asked for before the run ended.
    pub tried: usize,
    pub end: RunEnd,
}

/// What the user is told when a run ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunNotice {
    /// This many games got art.
    Added(usize),
    NoneFound,
    KeyRejected,
    Unreachable,
}

/// Whether a run for `games` games says it started: only one the user asked for (by
/// saving a key), with games to fetch.
pub fn announces_start(games: usize, asked: bool) -> bool {
    asked && games > 0
}

/// What a run tells the user. One the user asked for always says how it went; one after
/// an import only speaks of new art or a refused key, so imports do not pile up notices.
pub fn run_notice(run: &ArtRun, asked: bool) -> Option<RunNotice> {
    match run.end {
        RunEnd::KeyRejected => Some(RunNotice::KeyRejected),
        RunEnd::Unreachable if asked => Some(RunNotice::Unreachable),
        RunEnd::Done if asked && run.games_with_art == 0 => Some(RunNotice::NoneFound),
        _ => (run.games_with_art > 0).then_some(RunNotice::Added(run.games_with_art)),
    }
}

/// Where an answer or image fetched from `url` is kept, under the art cache's parent.
pub fn cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join("steamgriddb")
        .join(&sha256_hex(url.as_bytes())[..32])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{GameId, Source};

    fn art(layout: ArtLayout, origin: ArtOrigin) -> Artwork {
        Artwork {
            game_id: GameId::new(Source::Desktop, "game"),
            layout,
            origin,
            path: "/art.png".into(),
        }
    }

    #[test]
    fn a_search_hit_is_used_only_when_its_name_is_the_title() {
        let hit = |id, name: &str| SearchHit {
            id,
            name: name.into(),
        };
        let hits = [hit(1, "Hades II"), hit(2, "Hades"), hit(3, "HADES")];

        assert_eq!(matching_hit("Hades", &hits), Some(2));
        assert_eq!(
            matching_hit("the witcher 3", &[hit(7, "The Witcher 3")]),
            Some(7)
        );
        assert_eq!(
            matching_hit("Pokemon Puzzle", &[hit(9, "Pokémon Puzzle")]),
            Some(9)
        );
        assert_eq!(matching_hit("Hades III", &hits), None);
    }

    #[test]
    fn art_is_fetched_for_layouts_nobody_filled_or_also_over_source_art_when_preferred() {
        let artwork = [
            art(ArtLayout::Cover, ArtOrigin::Source),
            art(ArtLayout::Hero, ArtOrigin::User),
            art(ArtLayout::Logo, ArtOrigin::Generated),
        ];

        assert_eq!(
            layouts_to_fetch(&artwork, false),
            [ArtLayout::Logo, ArtLayout::Icon]
        );
        assert_eq!(
            layouts_to_fetch(&artwork, true),
            [ArtLayout::Cover, ArtLayout::Logo, ArtLayout::Icon]
        );
        assert!(
            layouts_to_fetch(&[art(ArtLayout::Cover, ArtOrigin::SteamGridDb)], true)
                .iter()
                .all(|layout| *layout != ArtLayout::Cover)
        );
    }

    #[test]
    fn requests_go_by_steam_app_id_when_there_is_one() {
        assert_eq!(
            images_url(ArtLayout::Cover, &GameRef::Steam(570)),
            "https://www.steamgriddb.com/api/v2/grids/steam/570?dimensions=600x900"
        );
        assert_eq!(
            images_url(ArtLayout::Hero, &GameRef::Grid(42)),
            "https://www.steamgriddb.com/api/v2/heroes/game/42"
        );
        assert_eq!(
            images_url(ArtLayout::Logo, &GameRef::Grid(42)),
            "https://www.steamgriddb.com/api/v2/logos/game/42"
        );
        assert_eq!(
            images_url(ArtLayout::Icon, &GameRef::Steam(570)),
            "https://www.steamgriddb.com/api/v2/icons/steam/570"
        );
    }

    #[test]
    fn titles_are_searched_with_their_text_encoded() {
        assert_eq!(
            search_url("Half-Life 2: Episode One"),
            "https://www.steamgriddb.com/api/v2/search/autocomplete/Half-Life%202%3A%20Episode%20One"
        );
    }

    #[test]
    fn search_answers_list_games() {
        let answer = r#"{"success":true,"data":[
            {"id":5258,"name":"Hades","types":["steam"],"verified":true},
            {"id":38015,"name":"Hades II","types":[],"verified":false}]}"#;

        assert_eq!(
            parse_search(answer),
            Ok(vec![
                SearchHit {
                    id: 5258,
                    name: "Hades".into()
                },
                SearchHit {
                    id: 38015,
                    name: "Hades II".into()
                },
            ])
        );
    }

    #[test]
    fn image_answers_list_images_with_their_thumbnails() {
        let answer = r#"{"success":true,"page":0,"total":2,"limit":50,"data":[
            {"id":1,"score":3,"style":"alternate","width":600,"height":900,"nsfw":false,
             "humor":false,"mime":"image/png","language":"en",
             "url":"https://cdn2.steamgriddb.com/grid/a.png",
             "thumb":"https://cdn2.steamgriddb.com/thumb/a.jpg"},
            {"id":2,"width":600,"height":900,"nsfw":true,"humor":false,"mime":"image/jpeg",
             "url":"https://cdn2.steamgriddb.com/grid/b.jpg",
             "thumb":"https://cdn2.steamgriddb.com/thumb/b.jpg"}]}"#;

        assert_eq!(
            parse_images(answer),
            Ok(vec![Image {
                id: 1,
                url: "https://cdn2.steamgriddb.com/grid/a.png".into(),
                thumb: "https://cdn2.steamgriddb.com/thumb/a.jpg".into(),
                width: 600,
                height: 900,
            }])
        );
    }

    #[test]
    fn an_unsuccessful_answer_is_an_error_with_its_message() {
        let answer = r#"{"success":false,"errors":["Game not found"]}"#;

        assert_eq!(parse_images(answer), Err("Game not found".into()));
        assert!(parse_search("not json").is_err());
    }

    #[test]
    fn rate_limits_back_off_or_follow_retry_after() {
        assert_eq!(retry_delay(0, None), Some(Duration::from_secs(1)));
        assert_eq!(retry_delay(1, None), Some(Duration::from_secs(2)));
        assert_eq!(retry_delay(3, None), Some(Duration::from_secs(8)));
        assert_eq!(retry_delay(1, Some(12)), Some(Duration::from_secs(12)));
        assert_eq!(retry_delay(2, Some(600)), Some(Duration::from_secs(60)));
        assert_eq!(retry_delay(MAX_ATTEMPTS, None), None);
    }

    #[test]
    fn cached_answers_and_images_have_their_own_files() {
        let dir = std::path::Path::new("/cache");
        let a = cache_path(dir, "https://www.steamgriddb.com/api/v2/grids/steam/570");
        let b = cache_path(dir, "https://www.steamgriddb.com/api/v2/grids/steam/730");

        assert!(a.starts_with("/cache/steamgriddb"));
        assert_ne!(a, b);
        assert_eq!(
            a,
            cache_path(dir, "https://www.steamgriddb.com/api/v2/grids/steam/570")
        );
    }

    #[test]
    fn a_refused_key_or_no_connection_ends_a_run_and_a_missing_game_is_skipped() {
        use RequestError::*;

        assert_eq!(
            on_failure(&Status(401)),
            OnFailure::Stop(RunEnd::KeyRejected)
        );
        assert_eq!(
            on_failure(&Status(403)),
            OnFailure::Stop(RunEnd::KeyRejected)
        );
        assert_eq!(
            on_failure(&Unreachable("Error resolving host".into())),
            OnFailure::Stop(RunEnd::Unreachable)
        );
        assert_eq!(on_failure(&Busy), OnFailure::Stop(RunEnd::Unreachable));
        assert_eq!(
            on_failure(&Status(503)),
            OnFailure::Stop(RunEnd::Unreachable)
        );
        assert_eq!(on_failure(&Status(404)), OnFailure::SkipGame);
        assert_eq!(
            on_failure(&Answer("Game not found".into())),
            OnFailure::SkipGame
        );
    }

    fn run(games_with_art: usize, end: RunEnd) -> ArtRun {
        ArtRun {
            stored: games_with_art * 2,
            games_with_art,
            tried: 3,
            end,
        }
    }

    #[test]
    fn a_run_the_user_asked_for_says_how_it_went() {
        assert_eq!(
            run_notice(&run(2, RunEnd::Done), true),
            Some(RunNotice::Added(2))
        );
        assert_eq!(
            run_notice(&run(0, RunEnd::Done), true),
            Some(RunNotice::NoneFound)
        );
        assert_eq!(
            run_notice(&run(0, RunEnd::KeyRejected), true),
            Some(RunNotice::KeyRejected)
        );
        assert_eq!(
            run_notice(&run(1, RunEnd::Unreachable), true),
            Some(RunNotice::Unreachable)
        );
        assert!(announces_start(3, true));
        assert!(!announces_start(0, true));
    }

    #[test]
    fn a_run_after_an_import_speaks_only_of_new_art_or_a_refused_key() {
        assert_eq!(
            run_notice(&run(2, RunEnd::Done), false),
            Some(RunNotice::Added(2))
        );
        assert_eq!(
            run_notice(&run(1, RunEnd::Unreachable), false),
            Some(RunNotice::Added(1))
        );
        assert_eq!(
            run_notice(&run(0, RunEnd::KeyRejected), false),
            Some(RunNotice::KeyRejected)
        );
        assert_eq!(run_notice(&run(0, RunEnd::Done), false), None);
        assert_eq!(run_notice(&run(0, RunEnd::Unreachable), false), None);
        assert!(!announces_start(3, false));
    }
}
