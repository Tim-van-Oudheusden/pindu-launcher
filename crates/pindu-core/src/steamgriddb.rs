//! SteamGridDB: request addresses, answers and the retry policy. The HTTP calls, the
//! request queue and the API key (kept with oo7) live in the UI's services.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value as Json;

use crate::launch::percent_encode;
use crate::model::ArtLayout;
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

/// Where an answer or image fetched from `url` is kept, under the art cache's parent.
pub fn cache_path(cache_dir: &Path, url: &str) -> PathBuf {
    cache_dir
        .join("steamgriddb")
        .join(&sha256_hex(url.as_bytes())[..32])
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
