use std::path::{Path, PathBuf};

use pindu_core::model::{ArtLayout, LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::{ImportedGame, Snapshot, faugus};

use crate::support::fixture_into;

const NATIVE: &str = ".local/share/faugus-launcher";
const FLATPAK: &str = ".var/app/io.github.Faugus.faugus-launcher/data/faugus-launcher";
/// Central European Time, so naive local timestamps shift by an hour.
const CET: i64 = 3600;

fn faugus_home(at: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fixture_into("faugus", &home.join(at));
    let dirs = HostDirs {
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
        home,
    };
    let root = Location::of(Source::Faugus, &dirs)
        .unwrap()
        .resolve(None)
        .unwrap();
    (dir, root)
}

fn game<'a>(snapshot: &'a Snapshot, native_id: &str) -> &'a ImportedGame {
    snapshot
        .games
        .iter()
        .find(|game| game.native_id == native_id)
        .unwrap_or_else(|| panic!("no game {native_id} in {:?}", snapshot.games))
}

#[test]
fn native_and_flatpak_faugus_import_the_same_games() {
    for at in [NATIVE, FLATPAK] {
        let (_dir, root) = faugus_home(at);
        assert!(root.ends_with(at), "{at}");

        let snapshot = faugus::import(&root, CET).unwrap();

        let mut ids: Vec<_> = snapshot
            .games
            .iter()
            .map(|g| g.native_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, ["celeste", "hades", "stardew-valley"], "{at}");
        assert_eq!(snapshot.source, Source::Faugus);
    }
}

#[test]
fn native_faugus_games_start_through_faugus_on_the_host() {
    let (_dir, root) = faugus_home(NATIVE);

    let snapshot = faugus::import(&root, CET).unwrap();

    let hades = game(&snapshot, "hades");
    assert_eq!(hades.launch_kind, LaunchKind::HostCommand);
    let config = hades.launch_config.as_ref().unwrap();
    assert_eq!(config.executable, Path::new("faugus-launcher"));
    assert_eq!(config.arguments, ["--game", "hades"]);
}

#[test]
fn flatpak_faugus_games_start_through_its_flatpak_command() {
    let (_dir, root) = faugus_home(FLATPAK);

    let snapshot = faugus::import(&root, CET).unwrap();

    let config = game(&snapshot, "hades").launch_config.clone().unwrap();
    assert_eq!(config.executable, Path::new("flatpak"));
    assert_eq!(
        config.arguments,
        [
            "run",
            "--command=/app/bin/faugus-launcher",
            "io.github.Faugus.faugus-launcher",
            "--game",
            "hades"
        ]
    );
}

#[test]
fn a_game_takes_its_details_playtime_and_art() {
    let (_dir, root) = faugus_home(NATIVE);

    let snapshot = faugus::import(&root, CET).unwrap();

    let hades = game(&snapshot, "hades");
    assert_eq!(hades.title, "Hades");
    assert_eq!(hades.platform, Some(Platform::Windows));
    assert!(hades.installed);
    assert_eq!(
        hades.install_path.as_deref(),
        Some(Path::new("/home/me/Games/Hades/x64"))
    );
    assert_eq!(hades.playtime_secs, Some(7_260));
    // 2026-03-01T12:34:56 local time in CET is 11:34:56 UTC.
    assert_eq!(hades.last_played_at, Some(1_772_364_896));
    assert_eq!(
        hades.artwork,
        vec![
            (ArtLayout::Cover, root.join("covers/hades.png")),
            (ArtLayout::Hero, root.join("banners/hades.png")),
            (ArtLayout::Icon, root.join("icons/hades.png")),
        ]
    );
    assert_eq!(
        hades.source_flags.categories,
        ["Roguelikes", "Favorites Of 2020"]
    );
    assert!(!hades.source_flags.hidden);
}

#[test]
fn values_stored_as_strings_are_read_too() {
    let (_dir, root) = faugus_home(NATIVE);

    let snapshot = faugus::import(&root, CET).unwrap();

    let celeste = game(&snapshot, "celeste");
    assert_eq!(celeste.platform, Some(Platform::Linux));
    assert_eq!(celeste.playtime_secs, Some(120));
    assert!(celeste.source_flags.hidden);
    assert_eq!(celeste.source_flags.categories, ["Platformers"]);
    assert_eq!(celeste.last_played_at, None);
    // No cover or icon recorded; the banner is found by its game ID.
    assert_eq!(
        celeste.artwork,
        vec![(ArtLayout::Hero, root.join("banners/celeste.png"))]
    );
}

#[test]
fn unplayed_and_uncategorized_games_import_without_those() {
    let (_dir, root) = faugus_home(NATIVE);

    let snapshot = faugus::import(&root, CET).unwrap();

    let stardew = game(&snapshot, "stardew-valley");
    assert_eq!(stardew.playtime_secs, None);
    assert!(!stardew.source_flags.hidden);
    assert!(stardew.source_flags.categories.is_empty());
    // A timestamp with its own offset ignores ours.
    assert_eq!(stardew.last_played_at, Some(1_709_251_199));
}

#[test]
fn a_game_without_a_title_is_reported() {
    let (_dir, root) = faugus_home(NATIVE);

    let snapshot = faugus::import(&root, CET).unwrap();

    assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
    assert!(
        snapshot.errors[0].contains("no-title"),
        "{:?}",
        snapshot.errors
    );
}

#[test]
fn an_unreadable_games_file_fails_the_source() {
    let (_dir, root) = faugus_home(NATIVE);
    std::fs::write(root.join("games.json"), "{ not json").unwrap();

    let error = faugus::import(&root, CET).unwrap_err();

    assert_eq!(error.path, root.join("games.json"));
}
