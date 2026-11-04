use std::path::PathBuf;

use pindu_core::model::{ArtLayout, LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::{ImportedGame, Snapshot, bottles};

use crate::support::fixture_into;

const NATIVE: &str = ".local/share/bottles";
const FLATPAK: &str = ".var/app/com.usebottles.bottles/data/bottles";

const CELESTE: &str = "7f1e2d3c-1111-4a4a-8b8b-000000000001";
const EPIC: &str = "7f1e2d3c-2222-4a4a-8b8b-000000000002";
const HALF_LIFE: &str = "7f1e2d3c-3333-4a4a-8b8b-000000000003";
const AC_DC: &str = "7f1e2d3c-7777-4a4a-8b8b-000000000007";

/// Copies the Bottles fixture to `<home>/<at>` and returns the temp dir and the resolved
/// Bottles folder.
fn bottles_home(at: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fixture_into("bottles", &home.join(at));
    let dirs = HostDirs {
        home: home.clone(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
    };
    let root = Location::of(Source::Bottles, &dirs)
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
fn native_and_flatpak_bottles_import_the_same_programs() {
    for at in [NATIVE, FLATPAK] {
        let (_dir, root) = bottles_home(at);
        assert!(root.ends_with(at), "{at}");

        let snapshot = bottles::import(&root).unwrap();

        let mut ids: Vec<_> = snapshot
            .games
            .iter()
            .map(|g| g.native_id.as_str())
            .collect();
        ids.sort_unstable();
        assert_eq!(ids, [CELESTE, EPIC, HALF_LIFE, AC_DC], "{at}");
        assert_eq!(snapshot.source, Source::Bottles);
    }
}

#[test]
fn a_program_starts_through_bottles_run() {
    let (_dir, root) = bottles_home(NATIVE);

    let snapshot = bottles::import(&root).unwrap();

    let celeste = game(&snapshot, CELESTE);
    assert_eq!(celeste.title, "Celeste");
    assert_eq!(celeste.platform, Some(Platform::Windows));
    assert!(celeste.installed);
    assert_eq!(celeste.launch_kind, LaunchKind::StoreUri);
    assert_eq!(
        celeste.launch_uri.as_deref(),
        Some("bottles:run/Gaming/Celeste")
    );
    // Bottles splits the URI on `/` and does not decode it, so names go in as they are.
    assert_eq!(
        game(&snapshot, EPIC).launch_uri.as_deref(),
        Some("bottles:run/My Apps/Epic Games Launcher")
    );
    assert_eq!(game(&snapshot, HALF_LIFE).title, "Half-Life 2: Episode One");
}

#[test]
fn grid_art_comes_from_the_custom_bottles_path() {
    let (_dir, root) = bottles_home(NATIVE);

    let snapshot = bottles::import(&root).unwrap();

    assert_eq!(
        game(&snapshot, CELESTE).artwork,
        vec![
            (
                ArtLayout::Cover,
                root.join("custom/Gaming/grids/Gaming_Celeste.png")
            ),
            (
                ArtLayout::Icon,
                root.join("bottles/Gaming/icons/Celeste.png")
            ),
        ]
    );
}

#[test]
fn without_a_custom_path_grid_art_comes_from_the_default_bottles_folder() {
    let (_dir, root) = bottles_home(FLATPAK);
    std::fs::remove_file(root.join("data.yml")).unwrap();

    let snapshot = bottles::import(&root).unwrap();

    let celeste = game(&snapshot, CELESTE);
    assert_eq!(
        celeste.artwork[0],
        (
            ArtLayout::Cover,
            root.join("bottles/Gaming/grids/Gaming_Celeste.png")
        )
    );
}

#[test]
fn programs_without_art_import_without_it() {
    let (_dir, root) = bottles_home(NATIVE);

    let snapshot = bottles::import(&root).unwrap();

    assert!(game(&snapshot, EPIC).artwork.is_empty());
    // The thumbnail names a grid file that is not there.
    assert!(game(&snapshot, HALF_LIFE).artwork.is_empty());
}

#[test]
fn steam_and_umu_entries_are_left_to_their_own_sources() {
    let (_dir, root) = bottles_home(NATIVE);

    let snapshot = bottles::import(&root).unwrap();

    assert!(
        snapshot
            .games
            .iter()
            .all(|game| game.title != "Steam Game In A Proton Prefix" && game.title != "A umu game")
    );
}

#[test]
fn entries_without_a_name_or_a_usable_uri_are_reported() {
    let (_dir, root) = bottles_home(NATIVE);

    let snapshot = bottles::import(&root).unwrap();

    assert_eq!(snapshot.errors.len(), 2, "{:?}", snapshot.errors);
    assert!(snapshot.errors.iter().any(|e| e.contains("60718293")));
    assert!(snapshot.errors.iter().any(|e| e.contains("AC/DC Live")));
    // A name Bottles' URI cannot carry still imports, without a launch URI.
    assert_eq!(game(&snapshot, AC_DC).launch_uri, None);
}

#[test]
fn an_unreadable_library_fails_the_source() {
    let (_dir, root) = bottles_home(NATIVE);
    std::fs::write(root.join("library.yml"), "key: [unclosed").unwrap();

    let error = bottles::import(&root).unwrap_err();

    assert_eq!(error.path, root.join("library.yml"));
}

#[test]
fn an_empty_library_imports_nothing() {
    let (_dir, root) = bottles_home(NATIVE);
    std::fs::write(root.join("library.yml"), "{}\n").unwrap();

    let snapshot = bottles::import(&root).unwrap();

    assert!(snapshot.games.is_empty());
    assert!(snapshot.errors.is_empty());
}
