//! SteamGridDB: the API key in the keyring (oo7; the Secret portal inside Flatpak), and
//! requests made one at a time, backing off on HTTP 429. Answers and images are cached,
//! so a picker that was opened once works offline.

use std::cell::Cell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::art_cache::default_cache_dir;
use pindu_core::manual;
use pindu_core::model::{ArtLayout, Artwork, Game, GameId, Source};
use pindu_core::paths::{default_library_db_path, default_user_art_dir};
use pindu_core::steamgriddb::{
    self, ArtRun, GameRef, Image, OnFailure, RequestError, RunEnd, SearchHit, layouts_to_fetch,
    matching_hit, on_failure, retry_delay,
};
use pindu_core::store::Store;
use pindu_core::tasks::{Progress, TaskKind};
use soup::prelude::*;

use crate::file_service::PickedImage;
use crate::task_service::TaskService;

/// Where the API key is filed in the keyring.
const KEY_ATTRIBUTES: [(&str, &str); 1] = [("pindu-launcher", "steamgriddb-api-key")];

/// The stored API key, if any.
pub async fn api_key() -> Result<Option<String>, String> {
    let keyring = oo7::Keyring::new()
        .await
        .map_err(|error| error.to_string())?;
    let items = keyring
        .search_items(&KEY_ATTRIBUTES)
        .await
        .map_err(|error| error.to_string())?;
    let Some(item) = items.first() else {
        return Ok(None);
    };
    let secret = item.secret().await.map_err(|error| error.to_string())?;
    Ok(
        Some(String::from_utf8_lossy(secret.as_bytes()).trim().to_owned())
            .filter(|k| !k.is_empty()),
    )
}

/// Stores the API key, replacing an earlier one. It never goes to GSettings or a file.
pub async fn set_api_key(key: &str) -> Result<(), String> {
    let keyring = oo7::Keyring::new()
        .await
        .map_err(|error| error.to_string())?;
    keyring
        .create_item(
            "SteamGridDB API key for Pindu launcher",
            &KEY_ATTRIBUTES,
            oo7::Secret::text(key.trim()),
            true,
        )
        .await
        .map_err(|error| error.to_string())
}

/// Fetches SteamGridDB art for the games in the library that lack it (see
/// [`layouts_to_fetch`]), the top image per layout, and says what it did; `on_start` hears
/// how many games want art before the first request. Without a stored key nothing is
/// requested and there is no run. Steam games are looked up by app ID, others by a search
/// that must find their exact title. A game SteamGridDB has nothing for is skipped; a
/// rejected key or no connection ends the run ([`on_failure`]). Answers already fetched
/// come from the cache. A run with games to fetch is a task in `tasks`, which the user can
/// cancel.
pub async fn fetch_missing_art(
    prefer_steamgriddb: bool,
    tasks: &TaskService,
    on_start: impl FnOnce(usize),
) -> Option<ArtRun> {
    let key = match api_key().await {
        Ok(key) => key?,
        Err(error) => {
            glib::g_debug!(
                "pindu",
                "No SteamGridDB art, the keyring cannot be used: {error}"
            );
            return None;
        }
    };
    let wanted = gio::spawn_blocking(move || games_wanting_art(prefer_steamgriddb))
        .await
        .unwrap_or_else(|_| Err("the library read stopped unexpectedly".into()));
    let wanted = match wanted {
        Ok(wanted) => wanted,
        Err(error) => {
            glib::g_warning!(
                "pindu",
                "Could not read the library for SteamGridDB art: {error}"
            );
            return None;
        }
    };
    on_start(wanted.len());
    let client = SteamGridDb::new(key);
    let mut run = ArtRun {
        stored: 0,
        games_with_art: 0,
        tried: 0,
        end: RunEnd::Done,
    };
    if wanted.is_empty() {
        return Some(run);
    }
    let cancellable = gio::Cancellable::new();
    let task = tasks.start(
        TaskKind::ArtFetch,
        &gettext("Artwork From SteamGridDB"),
        Some(&cancellable),
    );
    let total = wanted.len();
    for (game, layouts) in wanted {
        run.tried += 1;
        let fetched = gio::CancellableFuture::new(
            fetch_game_art(&client, &game, layouts),
            cancellable.clone(),
        )
        .await;
        match fetched {
            Ok(Ok(stored)) => {
                run.stored += stored;
                run.games_with_art += usize::from(stored > 0);
            }
            Ok(Err(end)) => {
                run.end = end;
                break;
            }
            Err(gio::Cancelled) => {
                run.end = RunEnd::Cancelled;
                break;
            }
        }
        tasks.set_progress(task, Progress::of(run.tried, total));
    }
    tasks.finish(task);
    Some(run)
}

/// Fetches and stores the top image per layout for one game; says how many were stored,
/// or how the run ends.
async fn fetch_game_art(
    client: &SteamGridDb,
    game: &Game,
    layouts: Vec<ArtLayout>,
) -> Result<usize, RunEnd> {
    // A failure that skips the game ends it with what was stored; one that stops the
    // run ends the run.
    let failed = |error: RequestError, stored: usize| {
        glib::g_debug!("pindu", "SteamGridDB art for {}: {error}", game.id);
        match on_failure(&error) {
            OnFailure::SkipGame => Ok(stored),
            OnFailure::Stop(end) => Err(end),
        }
    };
    let game_ref = match steam_app_id(game) {
        Some(app_id) => GameRef::Steam(app_id),
        None => match client.search(&game.title).await {
            Ok(hits) => match matching_hit(&game.title, &hits) {
                Some(id) => GameRef::Grid(id),
                None => return Ok(0),
            },
            Err(error) => return failed(error, 0),
        },
    };
    let mut stored = 0;
    for layout in layouts {
        let image = match client.images(layout, &game_ref).await {
            Ok(images) => images.into_iter().next(),
            Err(error) => return failed(error, stored),
        };
        let Some(image) = image else { continue };
        let picked = match client.image(&image.url).await {
            Ok(picked) => picked,
            // An image that cannot be had leaves the game's other layouts to try.
            Err(error) => {
                failed(error, stored)?;
                continue;
            }
        };
        let id = game.id.clone();
        let saved = gio::spawn_blocking(move || {
            let store = Store::open(&default_library_db_path()).map_err(|e| e.to_string())?;
            manual::set_steamgriddb_art(
                &store,
                &default_user_art_dir(),
                &id,
                layout,
                &picked.extension,
                &picked.bytes,
            )
            .map_err(|error| error.to_string())
        })
        .await;
        match saved {
            Ok(Ok(_)) => stored += 1,
            Ok(Err(error)) => {
                glib::g_warning!("pindu", "Could not store SteamGridDB art: {error}")
            }
            Err(_) => {}
        }
    }
    Ok(stored)
}

/// Games in the library, not hidden, with the layouts they want art for.
fn games_wanting_art(prefer_steamgriddb: bool) -> Result<Vec<(Game, Vec<ArtLayout>)>, String> {
    let store = Store::open(&default_library_db_path()).map_err(|error| error.to_string())?;
    let mut artwork: HashMap<GameId, Vec<Artwork>> = HashMap::new();
    for art in store.all_artwork().map_err(|error| error.to_string())? {
        artwork.entry(art.game_id.clone()).or_default().push(art);
    }
    let games = store.games().map_err(|error| error.to_string())?;
    Ok(games
        .into_iter()
        .filter(|game| game.in_library() && !game.hidden)
        .filter_map(|game| {
            let art = artwork.get(&game.id).map_or(&[][..], Vec::as_slice);
            let layouts = layouts_to_fetch(art, prefer_steamgriddb);
            (!layouts.is_empty()).then_some((game, layouts))
        })
        .collect())
}

fn steam_app_id(game: &Game) -> Option<u32> {
    (game.id.source() == Source::Steam)
        .then(|| game.id.native_id().parse().ok())
        .flatten()
}

/// A SteamGridDB client sharing one request slot.
#[derive(Debug, Clone)]
pub struct SteamGridDb {
    key: String,
    session: soup::Session,
    busy: Rc<Cell<bool>>,
}

impl SteamGridDb {
    /// A client using the API key `key`.
    pub fn new(key: String) -> Self {
        let session = soup::Session::new();
        session.set_user_agent(concat!("pindu-launcher/", env!("CARGO_PKG_VERSION")));
        Self {
            key,
            session,
            busy: Rc::default(),
        }
    }

    /// Games whose title matches.
    pub async fn search(&self, title: &str) -> Result<Vec<SearchHit>, RequestError> {
        let answer = self
            .cached_get(&steamgriddb::search_url(title), true)
            .await?;
        steamgriddb::parse_search(&String::from_utf8_lossy(&answer)).map_err(RequestError::Answer)
    }

    /// The images of a game in one layout.
    pub async fn images(
        &self,
        layout: ArtLayout,
        game: &GameRef,
    ) -> Result<Vec<Image>, RequestError> {
        let answer = self
            .cached_get(&steamgriddb::images_url(layout, game), true)
            .await?;
        steamgriddb::parse_images(&String::from_utf8_lossy(&answer)).map_err(RequestError::Answer)
    }

    /// An image or thumbnail as a file in the cache, fetched the first time.
    pub async fn image_file(&self, url: &str) -> Result<PathBuf, RequestError> {
        self.cached_get(url, false).await?;
        Ok(cache_file(url))
    }

    /// An image to store as a game's art.
    pub async fn image(&self, url: &str) -> Result<PickedImage, RequestError> {
        let bytes = self.cached_get(url, false).await?;
        let extension = url
            .rsplit('.')
            .next()
            .filter(|ext| ext.len() <= 4 && ext.chars().all(char::is_alphanumeric))
            .unwrap_or("png")
            .to_lowercase();
        Ok(PickedImage { bytes, extension })
    }

    /// GETs `url` (with the API key when `api` is set), cache first.
    async fn cached_get(&self, url: &str, api: bool) -> Result<Vec<u8>, RequestError> {
        let path = cache_file(url);
        if let Ok(bytes) = std::fs::read(&path) {
            return Ok(bytes);
        }
        let bytes = self.get(url, api).await?;
        if let Some(dir) = path.parent()
            && std::fs::create_dir_all(dir).is_ok()
            && let Err(error) = std::fs::write(&path, &bytes)
        {
            glib::g_debug!("pindu", "Could not cache {url}: {error}");
        }
        Ok(bytes)
    }

    /// One request at a time: waits for the slot, then retries rate-limited answers after
    /// the delay the policy gives.
    async fn get(&self, url: &str, api: bool) -> Result<Vec<u8>, RequestError> {
        while self.busy.replace(true) {
            glib::timeout_future(std::time::Duration::from_millis(100)).await;
        }
        let result = self.get_with_backoff(url, api).await;
        self.busy.set(false);
        result
    }

    async fn get_with_backoff(&self, url: &str, api: bool) -> Result<Vec<u8>, RequestError> {
        let mut attempt = 0;
        loop {
            let message = soup::Message::new("GET", url)
                .map_err(|error| RequestError::Answer(error.to_string()))?;
            if api && let Some(headers) = message.request_headers() {
                headers.append("Authorization", &format!("Bearer {}", self.key));
            }
            let bytes = self
                .session
                .send_and_read_future(&message, glib::Priority::DEFAULT)
                .await
                .map_err(|error| RequestError::Unreachable(error.to_string()))?;
            // libsoup's status enum has no name for 429.
            match message.status_code() {
                200 => return Ok(bytes.to_vec()),
                429 => {
                    let retry_after = message
                        .response_headers()
                        .and_then(|headers| headers.one("Retry-After"))
                        .and_then(|value| value.trim().parse().ok());
                    let Some(delay) = retry_delay(attempt, retry_after) else {
                        return Err(RequestError::Busy);
                    };
                    glib::timeout_future(delay).await;
                    attempt += 1;
                }
                status => return Err(RequestError::Status(status as u16)),
            }
        }
    }
}

fn cache_file(url: &str) -> PathBuf {
    let cache_home = default_cache_dir()
        .parent()
        .map(PathBuf::from)
        .unwrap_or_default();
    steamgriddb::cache_path(&cache_home, url)
}
