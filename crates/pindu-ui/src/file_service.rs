//! Files the user picks through the portal, and art fetched from an address.

use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use gtk::prelude::*;
use gtk::{gio, glib};
use pindu_core::paths::document_portal_parts;
use soup::prelude::*;

/// The largest image accepted as art.
const MAX_ART_BYTES: usize = 32 * 1024 * 1024;

/// An image the user picked, with the file extension to store it under.
#[derive(Debug, Clone)]
pub struct PickedImage {
    /// The image file's contents.
    pub bytes: Vec<u8>,
    /// The file extension, without the dot.
    pub extension: String,
}

/// The path of a picked file as the host sees it, for commands run on the host. Inside
/// Flatpak, picked files live under the document portal (`/run/user/<uid>/doc/<id>/…`);
/// the Documents portal tells where they really are.
pub async fn host_path(file: &gio::File) -> Option<PathBuf> {
    let path = file.path()?;
    let Some((doc_id, rest)) = document_portal_parts(&path) else {
        return Some(path);
    };
    let bus = gio::bus_get_future(gio::BusType::Session).await.ok()?;
    let reply = bus
        .call_future(
            Some("org.freedesktop.portal.Documents"),
            "/org/freedesktop/portal/documents",
            "org.freedesktop.portal.Documents",
            "GetHostPaths",
            Some(&(vec![doc_id.clone()],).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
        )
        .await
        .inspect_err(|error| glib::g_warning!("pindu", "No host path for {path:?}: {error}"))
        .ok()?;
    let (paths,): (std::collections::HashMap<String, Vec<u8>>,) = reply.get()?;
    let host = paths.get(&doc_id)?;
    // The portal gives a nul-terminated byte string.
    let host = host.strip_suffix(&[0]).unwrap_or(host);
    let host = PathBuf::from(std::ffi::OsStr::from_bytes(host));
    Some(rest.into_iter().fold(host, |path, part| path.join(part)))
}

/// Reads an image file the user picked.
pub async fn read_image(file: &gio::File) -> Result<PickedImage, String> {
    let (bytes, _) = file
        .load_contents_future()
        .await
        .map_err(|error| error.to_string())?;
    if bytes.len() > MAX_ART_BYTES {
        return Err("the image is too large".into());
    }
    let extension = file
        .basename()
        .as_deref()
        .and_then(Path::extension)
        .map_or_else(
            || "img".to_owned(),
            |ext| ext.to_string_lossy().to_lowercase(),
        );
    Ok(PickedImage {
        bytes: bytes.to_vec(),
        extension,
    })
}

/// Downloads an image from a web address. Only `http` and `https` are fetched, and the
/// answer must say it is an image.
pub async fn fetch_image(address: &str) -> Result<PickedImage, String> {
    if address.trim().is_empty() {
        return Err("no address was entered".into());
    }
    let uri = glib::Uri::parse(address.trim(), glib::UriFlags::NONE)
        .map_err(|error| error.to_string())?;
    if !matches!(uri.scheme().as_str(), "http" | "https") {
        return Err("only web addresses (http or https) can be used".into());
    }
    let session = soup::Session::new();
    session.set_user_agent(concat!("pindu-launcher/", env!("CARGO_PKG_VERSION")));
    let message = soup::Message::from_uri("GET", &uri);
    let bytes = session
        .send_and_read_future(&message, glib::Priority::DEFAULT)
        .await
        .map_err(|error| error.to_string())?;
    if message.status() != soup::Status::Ok {
        return Err(format!("the server answered {}", message.status_code()));
    }
    let content_type = message
        .response_headers()
        .and_then(|headers| headers.content_type().map(|(kind, _)| kind.to_string()))
        .unwrap_or_default();
    let Some(extension) = image_extension(&content_type) else {
        return Err(format!("the address is not an image ({content_type})"));
    };
    if bytes.len() > MAX_ART_BYTES {
        return Err("the image is too large".into());
    }
    Ok(PickedImage {
        bytes: bytes.to_vec(),
        extension: extension.to_owned(),
    })
}

fn image_extension(content_type: &str) -> Option<&'static str> {
    Some(match content_type {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/avif" => "avif",
        "image/svg+xml" => "svg",
        "image/x-icon" | "image/vnd.microsoft.icon" => "ico",
        _ => return None,
    })
}
