use std::ffi::OsString;
use std::path::{Path, PathBuf};

use pindu_core::model::{LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location, sandbox_hides};
use pindu_core::sources::{ImportedGame, Snapshot, legendary};

use crate::support::fixture_into;

fn game<'a>(snapshot: &'a Snapshot, native_id: &str) -> &'a ImportedGame {
    snapshot
        .games
        .iter()
        .find(|game| game.native_id == native_id)
        .unwrap_or_else(|| panic!("no game {native_id} in {:?}", snapshot.games))
}

/// Legendary's folder in `~/.config`, found the way the app finds it outside Flatpak.
fn native_root() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fixture_into("legendary", &home.join(".config/legendary"));
    let dirs = HostDirs::from_vars(&home, false, |_| None::<OsString>);
    let root = Location::of(Source::Legendary, &dirs)
        .unwrap()
        .resolve(None)
        .unwrap();
    (dir, root)
}

#[test]
fn native_legendary_is_found_in_the_config_folder() {
    let (dir, root) = native_root();

    assert_eq!(root, dir.path().join("home/.config/legendary"));
}

#[test]
fn inside_flatpak_the_hosts_config_folder_is_the_candidate() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let config = dir.path().join("config");
    let dirs = HostDirs::from_vars(&home, true, |name| {
        (name == "HOST_XDG_CONFIG_HOME").then(|| config.clone().into_os_string())
    });

    let location = Location::of(Source::Legendary, &dirs).unwrap();

    assert_eq!(location.candidates(), [config.join("legendary")]);
}

#[test]
fn the_sandbox_hides_legendary_so_its_folder_is_chosen_by_hand() {
    assert!(sandbox_hides(Source::Legendary));
    for source in [Source::Steam, Source::HeroicEpic, Source::Faugus] {
        assert!(!sandbox_hides(source), "{source}");
    }
}

#[test]
fn a_folder_chosen_through_the_portal_imports_the_same_games() {
    let dir = tempfile::tempdir().unwrap();
    // The document portal's view of the folder the user picked.
    let chosen = dir.path().join("run/user/1000/doc/1a2b3c4d/legendary");
    fixture_into("legendary", &chosen);
    let dirs = HostDirs::from_vars(&dir.path().join("home"), true, |_| None::<OsString>);
    let root = Location::of(Source::Legendary, &dirs)
        .unwrap()
        .resolve(Some(&chosen))
        .unwrap();
    let (_native_dir, native) = native_root();

    let snapshot = legendary::import(&root).unwrap();

    assert_eq!(snapshot, legendary::import(&native).unwrap());
}

#[test]
fn installed_games_import_and_dlc_is_skipped() {
    let (_dir, root) = native_root();

    let snapshot = legendary::import(&root).unwrap();

    let mut ids: Vec<_> = snapshot
        .games
        .iter()
        .map(|g| g.native_id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(ids, ["Fortune", "Sugar"]);
    assert_eq!(snapshot.source, Source::Legendary);
    assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
}

#[test]
fn the_credentials_file_is_never_opened() {
    let (_dir, root) = native_root();
    let (_clean_dir, clean) = native_root();
    // A folder in `user.json`'s place: it exists, but reading it fails even for root, which
    // CI's test user is and which ignores a 0o000 mode.
    std::fs::create_dir(root.join("user.json")).unwrap();

    let snapshot = legendary::import(&root).unwrap();

    assert!(snapshot.errors.is_empty(), "{:?}", snapshot.errors);
    assert_eq!(snapshot, legendary::import(&clean).unwrap());
}

#[test]
fn a_game_takes_its_install_and_metadata() {
    let (_dir, root) = native_root();

    let snapshot = legendary::import(&root).unwrap();

    let celeste = game(&snapshot, "Sugar");
    assert_eq!(celeste.title, "Celeste");
    assert!(celeste.installed);
    assert_eq!(
        celeste.install_path.as_deref(),
        Some(Path::new("/home/me/Games/Celeste"))
    );
    assert_eq!(celeste.size_bytes, Some(1_288_490_188));
    assert_eq!(celeste.platform, Some(Platform::Windows));
    assert_eq!(celeste.developer.as_deref(), Some("Maddy Makes Games"));
    assert!(
        celeste
            .description
            .as_deref()
            .is_some_and(|text| text.starts_with("Help Madeline")),
        "{:?}",
        celeste.description
    );
}

#[test]
fn a_game_without_a_title_takes_the_one_in_its_metadata() {
    let (_dir, root) = native_root();

    let snapshot = legendary::import(&root).unwrap();

    assert_eq!(game(&snapshot, "Fortune").title, "Hades");
}

#[test]
fn games_start_through_legendary_on_the_host() {
    let (_dir, root) = native_root();

    let snapshot = legendary::import(&root).unwrap();

    let celeste = game(&snapshot, "Sugar");
    assert_eq!(celeste.launch_kind, LaunchKind::HostCommand);
    let config = celeste.launch_config.as_ref().unwrap();
    assert_eq!(config.executable, Path::new("legendary"));
    assert_eq!(config.arguments, ["launch", "Sugar"]);
}

#[test]
fn a_broken_metadata_file_is_reported_and_the_game_kept() {
    let (_dir, root) = native_root();
    std::fs::write(root.join("metadata/Sugar.json"), "{ not json").unwrap();

    let snapshot = legendary::import(&root).unwrap();

    assert_eq!(game(&snapshot, "Sugar").title, "Celeste");
    assert_eq!(game(&snapshot, "Sugar").developer, None);
    assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
    assert!(
        snapshot.errors[0].contains("Sugar.json"),
        "{:?}",
        snapshot.errors
    );
}

#[test]
fn an_unreadable_installed_file_fails_the_source() {
    let (_dir, root) = native_root();
    std::fs::write(root.join("installed.json"), "{ not json").unwrap();

    let error = legendary::import(&root).unwrap_err();

    assert_eq!(error.path, root.join("installed.json"));
}
