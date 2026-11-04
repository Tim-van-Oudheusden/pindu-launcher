//! SteamGridDB: the API key in the keyring (oo7; the Secret portal inside Flatpak), and
//! requests made one at a time, backing off on HTTP 429. Answers and images are cached,
//! so a picker that was opened once works offline.

use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;

use gtk::glib;
use pindu_core::art_cache::default_cache_dir;
use pindu_core::model::ArtLayout;
use pindu_core::steamgriddb::{self, GameRef, Image, SearchHit, retry_delay};
use soup::prelude::*;

use crate::file_service::PickedImage;

/// The error of a request SteamGridDB refused for its API key.
pub const KEY_REJECTED: &str = "SteamGridDB did not accept the API key";

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

/// A SteamGridDB client sharing one request slot.
#[derive(Debug, Clone)]
pub struct SteamGridDb {
    key: String,
    session: soup::Session,
    busy: Rc<Cell<bool>>,
}

impl SteamGridDb {
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
    pub async fn search(&self, title: &str) -> Result<Vec<SearchHit>, String> {
        let answer = self
            .cached_get(&steamgriddb::search_url(title), true)
            .await?;
        steamgriddb::parse_search(&String::from_utf8_lossy(&answer))
    }

    /// The images of a game in one layout.
    pub async fn images(&self, layout: ArtLayout, game: &GameRef) -> Result<Vec<Image>, String> {
        let answer = self
            .cached_get(&steamgriddb::images_url(layout, game), true)
            .await?;
        steamgriddb::parse_images(&String::from_utf8_lossy(&answer))
    }

    /// An image or thumbnail as a file in the cache, fetched the first time.
    pub async fn image_file(&self, url: &str) -> Result<PathBuf, String> {
        self.cached_get(url, false).await?;
        Ok(cache_file(url))
    }

    /// An image to store as a game's art.
    pub async fn image(&self, url: &str) -> Result<PickedImage, String> {
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
    async fn cached_get(&self, url: &str, api: bool) -> Result<Vec<u8>, String> {
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
    async fn get(&self, url: &str, api: bool) -> Result<Vec<u8>, String> {
        while self.busy.replace(true) {
            glib::timeout_future(std::time::Duration::from_millis(100)).await;
        }
        let result = self.get_with_backoff(url, api).await;
        self.busy.set(false);
        result
    }

    async fn get_with_backoff(&self, url: &str, api: bool) -> Result<Vec<u8>, String> {
        let mut attempt = 0;
        loop {
            let message = soup::Message::new("GET", url).map_err(|error| error.to_string())?;
            if api && let Some(headers) = message.request_headers() {
                headers.append("Authorization", &format!("Bearer {}", self.key));
            }
            let bytes = self
                .session
                .send_and_read_future(&message, glib::Priority::DEFAULT)
                .await
                .map_err(|error| error.to_string())?;
            // libsoup's status enum has no name for 429.
            match message.status_code() {
                200 => return Ok(bytes.to_vec()),
                429 => {
                    let retry_after = message
                        .response_headers()
                        .and_then(|headers| headers.one("Retry-After"))
                        .and_then(|value| value.trim().parse().ok());
                    let Some(delay) = retry_delay(attempt, retry_after) else {
                        return Err("SteamGridDB is busy; try again later".into());
                    };
                    glib::timeout_future(delay).await;
                    attempt += 1;
                }
                401 => return Err(KEY_REJECTED.into()),
                status => return Err(format!("SteamGridDB answered {status}")),
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
