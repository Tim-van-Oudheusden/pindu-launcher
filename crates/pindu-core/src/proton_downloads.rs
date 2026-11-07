//! Proton builds to download into our tools folder: GE-Proton and Proton-CachyOS releases
//! from the GitHub releases API, the tarball and checksum each one offers for x86_64, and
//! what is installed. The HTTP calls, the download and the unpacking live in the UI's
//! services, and run only when the user asks in Preferences, never when a game starts.

use std::io;
use std::path::{Path, PathBuf};

use serde_json::Value as Json;

/// A Proton fork whose releases can be downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fork {
    /// GloriousEggroll's GE-Proton.
    GeProton,
    /// CachyOS's Proton-CachyOS.
    CachyOs,
}

/// A file attached to a release.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    /// The file name.
    pub name: String,
    /// Where the file downloads from.
    pub url: String,
    /// The file's size, in bytes.
    pub size: u64,
}

/// A release with a tarball for this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Release {
    /// The fork the release belongs to.
    pub fork: Fork,
    /// The release's Git tag.
    pub tag: String,
    /// When it was published, as GitHub gives it (ISO 8601, UTC).
    pub published_at: String,
    /// The x86_64 tarball.
    pub tarball: Asset,
    /// The `.sha512sum` file published with the tarball, if the release has one.
    pub checksum: Option<Asset>,
}

/// How many releases of each fork are listed.
const LISTED_RELEASES: u32 = 10;

impl Fork {
    /// Every fork, in the order they are listed.
    pub const ALL: [Self; 2] = [Self::GeProton, Self::CachyOs];

    /// The fork's GitHub repository.
    pub fn repository(self) -> &'static str {
        match self {
            Self::GeProton => "GloriousEggroll/proton-ge-custom",
            Self::CachyOs => "CachyOS/proton-cachyos",
        }
    }

    /// The newest releases, from the GitHub releases API.
    pub fn releases_url(self) -> String {
        format!(
            "https://api.github.com/repos/{}/releases?per_page={LISTED_RELEASES}",
            self.repository()
        )
    }
}

impl Release {
    /// The folder the tarball unpacks to, which is the build's folder in our tools folder.
    pub fn folder(&self) -> &str {
        tarball_stem(&self.tarball.name).expect("releases only hold tarballs")
    }
}

/// The tarball endings releases use; the `.tar.zst` some GE-Proton releases add is skipped.
const TARBALL_ENDINGS: [&str; 2] = [".tar.gz", ".tar.xz"];

/// The releases in a GitHub releases API answer that have an x86_64 tarball, as listed.
/// Drafts and prereleases are left out.
pub fn parse_releases(fork: Fork, json: &str) -> Result<Vec<Release>, String> {
    let answer: Json = serde_json::from_str(json).map_err(|error| error.to_string())?;
    let releases = answer
        .as_array()
        .ok_or("GitHub's answer lists no releases")?;
    Ok(releases
        .iter()
        .filter(|release| {
            !release["draft"].as_bool().unwrap_or(true)
                && !release["prerelease"].as_bool().unwrap_or(true)
        })
        .filter_map(|release| parse_release(fork, release))
        .collect())
}

fn parse_release(fork: Fork, release: &Json) -> Option<Release> {
    let tag = release["tag_name"].as_str()?;
    let assets: Vec<Asset> = release["assets"]
        .as_array()?
        .iter()
        .filter_map(|asset| {
            Some(Asset {
                name: asset["name"].as_str()?.to_owned(),
                url: asset["browser_download_url"].as_str()?.to_owned(),
                size: asset["size"].as_u64()?,
            })
        })
        .collect();
    // Newer releases name the architecture; older GE-Proton ones only had x86_64.
    let tarball = assets
        .iter()
        .find(|asset| tarball_stem(&asset.name).is_some_and(|stem| stem.ends_with("-x86_64")))
        .or_else(|| {
            assets
                .iter()
                .find(|asset| tarball_stem(&asset.name) == Some(tag))
        })?
        .clone();
    let stem = tarball_stem(&tarball.name)?;
    let checksum = assets
        .iter()
        .find(|asset| asset.name == format!("{stem}.sha512sum"))
        .cloned();
    Some(Release {
        fork,
        tag: tag.to_owned(),
        published_at: release["published_at"].as_str()?.to_owned(),
        tarball,
        checksum,
    })
}

/// A tarball's name without its ending, which must make a plain folder name; `None` for
/// other files.
fn tarball_stem(name: &str) -> Option<&str> {
    TARBALL_ENDINGS
        .iter()
        .find_map(|ending| name.strip_suffix(ending))
        .filter(|stem| is_folder_name(stem))
}

/// Whether `name` is one plain, visible folder name, safe to join to the tools folder.
fn is_folder_name(name: &str) -> bool {
    !name.is_empty() && !name.starts_with('.') && !name.contains(['/', '\\', '\0'])
}

/// The SHA-512 a `sha512sum` file gives for `file`, in lowercase hex. Each line is the
/// hash, then a space and either a space or `*` (binary mode), then the file name.
pub fn expected_sha512(text: &str, file: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (hash, name) = line.trim_end().split_once(' ')?;
        let name = name.strip_prefix([' ', '*'])?;
        (name == file && hash.len() == 128 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

/// Where an install is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Downloading the tarball, which is hashed as it comes.
    Downloading {
        /// Bytes received so far.
        received: u64,
        /// The tarball's size in bytes, or 0 when unknown.
        total: u64,
    },
    /// Unpacking the checked tarball into the tools folder.
    Unpacking,
}

impl Step {
    /// How far the step is, from 0 to 1, when that is known.
    pub fn fraction(self) -> Option<f64> {
        match self {
            Self::Downloading { received, total } if total > 0 => {
                Some((received as f64 / total as f64).min(1.0))
            }
            _ => None,
        }
    }
}

/// Whether a release is in our tools folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallState {
    /// Not installed, and no install runs.
    Available,
    /// An install runs, at this step.
    Installing(Step),
    /// Its folder is in the tools folder.
    Installed,
}

/// The state of `release`, given the folders installed in our tools folder and the step
/// of its install, if one runs.
pub fn install_state(release: &Release, installed: &[String], step: Option<Step>) -> InstallState {
    match step {
        Some(step) => InstallState::Installing(step),
        None if installed.iter().any(|folder| folder == release.folder()) => {
            InstallState::Installed
        }
        None => InstallState::Available,
    }
}

/// The folder an install of `folder` works in: inside the tools folder, so the finished
/// build moves in with one rename, and hidden, so it is never taken for a build.
pub fn work_dir(tools: &Path, folder: &str) -> PathBuf {
    tools.join(format!(".download-{folder}"))
}

/// Deletes what the install of `folder` left in its work folder: the tarball, whole or
/// in part, and anything unpacked from it. An install ends with this whether it finished,
/// failed or was cancelled, so only a finished one leaves a build behind.
pub fn discard_install(tools: &Path, folder: &str) {
    let _ = std::fs::remove_dir_all(work_dir(tools, folder));
}

/// Moves the one folder a tarball unpacked into `unpacked` to `tools/<folder>`, in one
/// rename, so the build appears whole or not at all. Returns its new path.
pub fn finish_install(unpacked: &Path, tools: &Path, folder: &str) -> io::Result<PathBuf> {
    let entries = std::fs::read_dir(unpacked)?.collect::<io::Result<Vec<_>>>()?;
    let [entry] = entries.as_slice() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the tarball does not hold exactly one folder",
        ));
    };
    if !entry.file_type()?.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the tarball does not hold a folder",
        ));
    }
    let target = build_path(tools, folder)?;
    if target.symlink_metadata().is_ok() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{folder} is already installed"),
        ));
    }
    std::fs::rename(entry.path(), &target)?;
    Ok(target)
}

/// Removes the build in `tools/<folder>`. It is first renamed to a hidden name, so a half
/// removed build is never taken for one.
pub fn remove_build(tools: &Path, folder: &str) -> io::Result<()> {
    let path = build_path(tools, folder)?;
    let doomed = tools.join(format!(".remove-{folder}"));
    if doomed.exists() {
        std::fs::remove_dir_all(&doomed)?;
    }
    std::fs::rename(&path, &doomed)?;
    std::fs::remove_dir_all(&doomed)
}

/// Deletes what interrupted installs and removals left in the tools folder, except the
/// work folders of the installs in `running`.
pub fn clean_leftovers(tools: &Path, running: &[String]) {
    let Ok(entries) = std::fs::read_dir(tools) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let leftover = match name.strip_prefix(".download-") {
            Some(folder) => !running.iter().any(|running| running == folder),
            None => name.starts_with(".remove-"),
        };
        if leftover {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}

/// `tools/<folder>`, for a plain folder name only.
fn build_path(tools: &Path, folder: &str) -> io::Result<PathBuf> {
    if is_folder_name(folder) {
        Ok(tools.join(folder))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{folder:?} is not a build's folder"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/proton_releases")
            .join(name);
        std::fs::read_to_string(path).unwrap()
    }

    #[test]
    fn ge_proton_releases_offer_their_x86_64_tarball_and_checksum() {
        let releases = parse_releases(Fork::GeProton, &fixture("ge-proton.json")).unwrap();
        let download = "https://github.com/GloriousEggroll/proton-ge-custom/releases/download";
        assert_eq!(
            releases[0],
            Release {
                fork: Fork::GeProton,
                tag: "GE-Proton11-12".into(),
                published_at: "2026-11-03T07:19:38Z".into(),
                tarball: Asset {
                    name: "GE-Proton11-12-x86_64.tar.gz".into(),
                    url: format!("{download}/GE-Proton11-12/GE-Proton11-12-x86_64.tar.gz"),
                    size: 566_859_228,
                },
                checksum: Some(Asset {
                    name: "GE-Proton11-12-x86_64.sha512sum".into(),
                    url: format!("{download}/GE-Proton11-12/GE-Proton11-12-x86_64.sha512sum"),
                    size: 159,
                }),
            }
        );
    }

    #[test]
    fn older_ge_proton_releases_offer_their_unnamed_gzip_tarball() {
        let releases = parse_releases(Fork::GeProton, &fixture("ge-proton.json")).unwrap();
        assert_eq!(releases[1].tag, "GE-Proton10-28");
        assert_eq!(releases[1].tarball.name, "GE-Proton10-28.tar.gz");
        assert_eq!(
            releases[1]
                .checksum
                .as_ref()
                .map(|asset| asset.name.as_str()),
            Some("GE-Proton10-28.sha512sum")
        );
    }

    #[test]
    fn proton_cachyos_releases_offer_the_plain_x86_64_tarball() {
        let releases = parse_releases(Fork::CachyOs, &fixture("proton-cachyos.json")).unwrap();
        let names: Vec<&str> = releases
            .iter()
            .map(|release| release.tarball.name.as_str())
            .collect();
        assert_eq!(
            names,
            [
                "proton-cachyos-11.0-20261015-slr-x86_64.tar.xz",
                "proton-cachyos-10.0-20251222-slr-x86_64.tar.xz",
            ]
        );
        assert_eq!(releases[0].tarball.size, 347_335_620);
        assert_eq!(releases[0].published_at, "2026-10-20T21:21:40Z");
    }

    #[test]
    fn drafts_prereleases_and_releases_without_a_tarball_are_left_out() {
        let json = r#"[
            {"tag_name": "a", "draft": true, "prerelease": false, "published_at": "2026-01-01T00:00:00Z",
             "assets": [{"name": "a-x86_64.tar.gz", "size": 1, "browser_download_url": "https://x/a"}]},
            {"tag_name": "b", "draft": false, "prerelease": true, "published_at": "2026-01-01T00:00:00Z",
             "assets": [{"name": "b-x86_64.tar.gz", "size": 1, "browser_download_url": "https://x/b"}]},
            {"tag_name": "c", "draft": false, "prerelease": false, "published_at": "2026-01-01T00:00:00Z",
             "assets": [{"name": "c-aarch64.tar.gz", "size": 1, "browser_download_url": "https://x/c"}]}
        ]"#;
        assert_eq!(parse_releases(Fork::GeProton, json), Ok(Vec::new()));
    }

    #[test]
    fn an_answer_that_is_not_a_release_list_is_an_error() {
        assert!(
            parse_releases(Fork::GeProton, r#"{"message": "API rate limit exceeded"}"#).is_err()
        );
    }

    #[test]
    fn a_checksum_file_gives_the_sha512_of_its_tarball() {
        let hash = "2e37447902529c9fc39138dc0b3f24aed3513b59a190b825e6b4a668e49e2033\
                    bdf6b8a7998e8686e067bbb3f61f3a6633e2408f0cc96f6dad983bd18e2db31e";
        let text = format!(
            "{}  other.tar.gz\n{}  GE-Proton11-12-x86_64.tar.gz\n",
            "0".repeat(128),
            hash.to_uppercase()
        );
        assert_eq!(
            expected_sha512(&text, "GE-Proton11-12-x86_64.tar.gz"),
            Some(hash.to_owned())
        );
        assert_eq!(expected_sha512(&text, "missing.tar.gz"), None);
        let binary = format!("{hash} *GE-Proton11-12-x86_64.tar.gz");
        assert_eq!(
            expected_sha512(&binary, "GE-Proton11-12-x86_64.tar.gz"),
            Some(hash.to_owned())
        );
        assert_eq!(expected_sha512("abc  a.tar.gz", "a.tar.gz"), None);
    }

    #[test]
    fn each_fork_lists_its_newest_releases_from_github() {
        assert_eq!(
            Fork::GeProton.releases_url(),
            "https://api.github.com/repos/GloriousEggroll/proton-ge-custom/releases?per_page=10"
        );
        assert_eq!(
            Fork::CachyOs.releases_url(),
            "https://api.github.com/repos/CachyOS/proton-cachyos/releases?per_page=10"
        );
    }

    #[test]
    fn a_release_installs_into_the_folder_its_tarball_unpacks_to() {
        let ge = parse_releases(Fork::GeProton, &fixture("ge-proton.json")).unwrap();
        assert_eq!(ge[0].folder(), "GE-Proton11-12-x86_64");
        assert_eq!(ge[1].folder(), "GE-Proton10-28");
        let cachyos = parse_releases(Fork::CachyOs, &fixture("proton-cachyos.json")).unwrap();
        assert_eq!(
            cachyos[0].folder(),
            "proton-cachyos-11.0-20261015-slr-x86_64"
        );
    }

    #[test]
    fn a_release_is_installed_once_its_folder_is_in_the_tools_folder() {
        let releases = parse_releases(Fork::GeProton, &fixture("ge-proton.json")).unwrap();
        let installed = vec!["GE-Proton10-28".to_owned(), "Other".to_owned()];
        assert_eq!(
            install_state(&releases[0], &installed, None),
            InstallState::Available
        );
        assert_eq!(
            install_state(&releases[1], &installed, None),
            InstallState::Installed
        );
        let step = Step::Downloading {
            received: 50,
            total: 200,
        };
        assert_eq!(
            install_state(&releases[0], &installed, Some(step)),
            InstallState::Installing(step)
        );
    }

    #[test]
    fn only_a_download_of_known_size_says_how_far_it_is() {
        let downloading = |received, total| Step::Downloading { received, total };
        assert_eq!(downloading(50, 200).fraction(), Some(0.25));
        assert_eq!(downloading(50, 0).fraction(), None);
        assert_eq!(Step::Unpacking.fraction(), None);
    }

    #[test]
    fn an_install_works_in_a_hidden_folder_beside_the_builds() {
        assert_eq!(
            work_dir(Path::new("/tools"), "GE-Proton11-12-x86_64"),
            Path::new("/tools/.download-GE-Proton11-12-x86_64")
        );
    }

    #[test]
    fn a_finished_install_moves_the_unpacked_folder_into_the_tools_folder() {
        let tools = tempfile::tempdir().unwrap();
        let unpacked = work_dir(tools.path(), "GE-Proton11-12-x86_64").join("unpacked");
        std::fs::create_dir_all(unpacked.join("GE-Proton11-12-x86_64/files")).unwrap();
        std::fs::write(unpacked.join("GE-Proton11-12-x86_64/proton"), "").unwrap();

        let path = finish_install(&unpacked, tools.path(), "GE-Proton11-12-x86_64").unwrap();

        assert_eq!(path, tools.path().join("GE-Proton11-12-x86_64"));
        assert!(path.join("proton").is_file());
        assert!(!unpacked.join("GE-Proton11-12-x86_64").exists());
    }

    #[test]
    fn an_install_refuses_a_tarball_without_exactly_one_folder() {
        let tools = tempfile::tempdir().unwrap();
        let unpacked = work_dir(tools.path(), "Build").join("unpacked");
        std::fs::create_dir_all(unpacked.join("one")).unwrap();
        std::fs::create_dir_all(unpacked.join("two")).unwrap();

        assert!(finish_install(&unpacked, tools.path(), "Build").is_err());
        assert!(!tools.path().join("Build").exists());
    }

    #[test]
    fn an_install_never_replaces_a_build_already_there() {
        let tools = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tools.path().join("Build/old")).unwrap();
        let unpacked = work_dir(tools.path(), "Build").join("unpacked");
        std::fs::create_dir_all(unpacked.join("Build/new")).unwrap();

        assert!(finish_install(&unpacked, tools.path(), "Build").is_err());
        assert!(tools.path().join("Build/old").exists());
    }

    #[test]
    fn removing_a_build_deletes_its_folder_and_nothing_else() {
        let tools = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tools.path().join("Build/files")).unwrap();
        std::fs::create_dir_all(tools.path().join("Other")).unwrap();

        remove_build(tools.path(), "Build").unwrap();

        let left: Vec<_> = std::fs::read_dir(tools.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["Other"]);
        assert!(remove_build(tools.path(), "../Other").is_err());
        assert!(remove_build(tools.path(), "").is_err());
        assert!(tools.path().join("Other").exists());
    }

    #[test]
    fn leftovers_of_interrupted_work_are_cleaned_but_running_installs_kept() {
        let tools = tempfile::tempdir().unwrap();
        for folder in [
            ".download-Old",
            ".download-Running",
            ".remove-Gone",
            "Build",
        ] {
            std::fs::create_dir_all(tools.path().join(folder).join("files")).unwrap();
        }

        clean_leftovers(tools.path(), &["Running".to_owned()]);

        let mut left: Vec<_> = std::fs::read_dir(tools.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        left.sort();
        assert_eq!(left, [".download-Running", "Build"]);
    }

    #[test]
    fn a_cancelled_install_leaves_no_partial_build_behind() {
        let tools = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tools.path().join("Build/files")).unwrap();
        let work = work_dir(tools.path(), "GE-Proton11-12-x86_64");
        std::fs::create_dir_all(work.join("unpacked/GE-Proton11-12-x86_64/files")).unwrap();
        std::fs::write(
            work.join("GE-Proton11-12-x86_64.tar.gz"),
            b"part of a tarball",
        )
        .unwrap();

        discard_install(tools.path(), "GE-Proton11-12-x86_64");

        let left: Vec<_> = std::fs::read_dir(tools.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(left, ["Build"]);
        discard_install(tools.path(), "GE-Proton11-12-x86_64");
        assert!(tools.path().join("Build/files").exists());
    }
}
