//! Proton downloads: GE-Proton and Proton-CachyOS releases from the GitHub releases API,
//! installed into and removed from our tools folder. Everything here runs only when the
//! user asks on the Preferences' Proton page; launching a game never downloads anything.
//!
//! An install fetches the release's `.sha512sum` first when it has one, hashes the
//! tarball as it downloads, unpacks it with `tar` in a hidden work folder beside the
//! builds and moves the build in with one rename. Installs outlive the Preferences
//! dialog; a dialog opened later sees them through [`step`], and Quick Access's
//! notifications tell how each ended unless the user stopped it.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use gettextrs::gettext;
use gtk::{gio, glib};
use pindu_core::notifications::Kind;
use pindu_core::paths::default_tools_dir;
use pindu_core::proton::{self, ProtonBuild};
use pindu_core::proton_downloads::{self, Fork, Release, Step};
use soup::prelude::*;

use crate::application::PinduApplication;

/// How much of a tarball is read, hashed and written at a time.
const CHUNK: usize = 256 * 1024;

/// An install that is running.
#[derive(Debug)]
struct Install {
    step: Cell<Step>,
    stopped: Cell<bool>,
}

thread_local! {
    static SESSION: soup::Session = {
        let session = soup::Session::new();
        session.set_user_agent(concat!("pindu-launcher/", env!("CARGO_PKG_VERSION")));
        session
    };
    /// The running installs by build folder.
    static RUNNING: RefCell<HashMap<String, Rc<Install>>> = RefCell::default();
}

/// The step of the install of the build in `folder`, if one runs.
pub fn step(folder: &str) -> Option<Step> {
    RUNNING.with_borrow(|running| running.get(folder).map(|install| install.step.get()))
}

/// The build folders being installed.
pub fn running() -> Vec<String> {
    RUNNING.with_borrow(|running| running.keys().cloned().collect())
}

/// Stops the download of the build in `folder`; an install that is unpacking finishes.
pub fn stop(folder: &str) {
    RUNNING.with_borrow(|running| {
        if let Some(install) = running.get(folder) {
            install.stopped.set(true);
        }
    });
}

/// The builds in our tools folder, newest first. Leftovers of interrupted installs and
/// removals are deleted on the way.
pub async fn installed() -> Vec<ProtonBuild> {
    let running = running();
    gio::spawn_blocking(move || {
        let tools = default_tools_dir();
        proton_downloads::clean_leftovers(&tools, &running);
        proton::discover(&[tools])
    })
    .await
    .unwrap_or_default()
}

/// The newest releases of `fork` that have a tarball for this machine.
pub async fn releases(fork: Fork) -> Result<Vec<Release>, String> {
    let answer = get(&fork.releases_url()).await?;
    proton_downloads::parse_releases(fork, &String::from_utf8_lossy(&answer))
}

/// Downloads, checks and unpacks `release` into our tools folder, and posts how it ended.
pub async fn install(release: Release) -> Result<(), String> {
    let folder = release.folder().to_owned();
    let install = Rc::new(Install {
        step: Cell::new(Step::Downloading {
            received: 0,
            total: release.tarball.size,
        }),
        stopped: Cell::new(false),
    });
    let started = RUNNING.with_borrow_mut(|running| {
        if running.contains_key(&folder) {
            return false;
        }
        running.insert(folder.clone(), install.clone());
        true
    });
    if !started {
        return Err(gettext("{build} is already being installed").replace("{build}", &folder));
    }
    let work = proton_downloads::work_dir(&default_tools_dir(), &folder);
    let result = run_install(&release, &install, &work).await;
    let _ = gio::spawn_blocking(move || std::fs::remove_dir_all(work)).await;
    RUNNING.with_borrow_mut(|running| running.remove(&folder));
    if !install.stopped.get() {
        notify_install(&release.tag, &result);
    }
    result
}

/// Posts that the build `name` was installed, or why it was not.
fn notify_install(name: &str, result: &Result<(), String>) {
    let Some(application) = gio::Application::default().and_downcast::<PinduApplication>() else {
        return;
    };
    let (kind, title, body) = match result {
        Ok(()) => (
            Kind::Download,
            gettext("{build} installed"),
            gettext("Windows games can now run with it"),
        ),
        Err(error) => (
            Kind::DownloadFailed,
            gettext("{build} was not installed"),
            error.clone(),
        ),
    };
    application
        .notification_service()
        .post(kind, &title.replace("{build}", name), &body, None);
}

/// Removes the build in `folder` from our tools folder.
pub async fn remove(folder: String) -> Result<(), String> {
    gio::spawn_blocking(move || proton_downloads::remove_build(&default_tools_dir(), &folder))
        .await
        .map_err(|_| gettext("The build could not be removed"))?
        .map_err(|error| error.to_string())
}

async fn run_install(release: &Release, install: &Install, work: &Path) -> Result<(), String> {
    let unpacked = work.join("unpacked");
    let fresh = unpacked.clone();
    let work_dir = work.to_owned();
    gio::spawn_blocking(move || {
        let _ = std::fs::remove_dir_all(&work_dir);
        std::fs::create_dir_all(&fresh)
    })
    .await
    .map_err(|_| gettext("The download folder could not be made"))?
    .map_err(|error| error.to_string())?;

    let expected =
        match &release.checksum {
            Some(file) => {
                let text = get(&file.url).await?;
                let expected = proton_downloads::expected_sha512(
                    &String::from_utf8_lossy(&text),
                    &release.tarball.name,
                );
                Some(expected.ok_or_else(|| {
                    gettext("The release's checksum file does not list its tarball")
                })?)
            }
            None => None,
        };
    let tarball = work.join(&release.tarball.name);
    let sha512 = download(&release.tarball.url, &tarball, install).await?;
    if expected.is_some_and(|expected| expected != sha512) {
        return Err(gettext(
            "The download does not match the checksum the release published; nothing was installed",
        ));
    }

    install.step.set(Step::Unpacking);
    let folder = release.folder().to_owned();
    gio::spawn_blocking(move || unpack(&tarball, &unpacked, &folder))
        .await
        .map_err(|_| gettext("The tarball could not be unpacked"))?
}

/// Unpacks `tarball` into `unpacked` and moves the build into the tools folder.
fn unpack(tarball: &Path, unpacked: &Path, folder: &str) -> Result<(), String> {
    let status = Command::new("tar")
        .arg("--extract")
        .arg("--no-same-owner")
        .arg("--file")
        .arg(tarball)
        .arg("--directory")
        .arg(unpacked)
        .status()
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Err(gettext("The tarball could not be unpacked"));
    }
    proton_downloads::finish_install(unpacked, &default_tools_dir(), folder)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Downloads `url` to `path`, keeping the install's step up to date, and returns the
/// SHA-512 of what arrived.
async fn download(url: &str, path: &Path, install: &Install) -> Result<String, String> {
    let message = soup::Message::new("GET", url).map_err(|error| error.to_string())?;
    let session = SESSION.with(Clone::clone);
    let input = session
        .send_future(&message, glib::Priority::DEFAULT)
        .await
        .map_err(|error| unreachable(&error))?;
    check_status(message.status_code())?;
    let total = message
        .response_headers()
        .and_then(|headers| u64::try_from(headers.content_length()).ok())
        .filter(|length| *length > 0)
        .unwrap_or(match install.step.get() {
            Step::Downloading { total, .. } => total,
            Step::Unpacking => 0,
        });
    let output = gio::File::for_path(path)
        .replace_future(
            None,
            false,
            gio::FileCreateFlags::NONE,
            glib::Priority::DEFAULT,
        )
        .await
        .map_err(|error| error.to_string())?;
    let mut checksum =
        glib::Checksum::new(glib::ChecksumType::Sha512).expect("GLib computes SHA-512");
    let mut received = 0;
    loop {
        if install.stopped.get() {
            return Err(gettext("The download was stopped"));
        }
        let bytes = input
            .read_bytes_future(CHUNK, glib::Priority::DEFAULT)
            .await
            .map_err(|error| unreachable(&error))?;
        if bytes.is_empty() {
            break;
        }
        checksum.update(&bytes);
        received += bytes.len() as u64;
        output
            .write_all_future(bytes, glib::Priority::DEFAULT)
            .await
            .map_err(|(_, error)| error.to_string())?;
        install.step.set(Step::Downloading { received, total });
    }
    output
        .close_future(glib::Priority::DEFAULT)
        .await
        .map_err(|error| error.to_string())?;
    Ok(checksum.string().unwrap_or_default())
}

/// GETs a small answer from GitHub.
async fn get(url: &str) -> Result<Vec<u8>, String> {
    let message = soup::Message::new("GET", url).map_err(|error| error.to_string())?;
    if let Some(headers) = message.request_headers() {
        headers.append("Accept", "application/vnd.github+json");
    }
    let session = SESSION.with(Clone::clone);
    let bytes = session
        .send_and_read_future(&message, glib::Priority::DEFAULT)
        .await
        .map_err(|error| unreachable(&error))?;
    check_status(message.status_code())?;
    Ok(bytes.to_vec())
}

/// What the user is told when GitHub cannot be reached.
pub fn offline_message() -> String {
    gettext("You are offline. Connect to the internet to see and download Proton releases.")
}

/// The message for a request that failed. The request is always tried: the network
/// monitor is asked only afterwards, since some report no network where there is one.
fn unreachable(error: &glib::Error) -> String {
    glib::g_debug!("pindu", "A GitHub request failed: {error}");
    if error.matches(gio::ResolverError::NotFound)
        || error.matches(gio::ResolverError::TemporaryFailure)
        || error.matches(gio::IOErrorEnum::NetworkUnreachable)
        || error.matches(gio::IOErrorEnum::HostUnreachable)
        || !gio::NetworkMonitor::default().is_network_available()
    {
        offline_message()
    } else {
        gettext("GitHub could not be reached: {error}").replace("{error}", error.message())
    }
}

fn check_status(status: u32) -> Result<(), String> {
    match status {
        200 => Ok(()),
        // GitHub limits how often a network may ask without an account.
        403 | 429 => Err(gettext(
            "GitHub was asked too often from this network; try again in an hour",
        )),
        status => Err(gettext("GitHub answered {status}").replace("{status}", &status.to_string())),
    }
}
