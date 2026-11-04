use std::path::{Path, PathBuf};

use pindu_core::model::{ArtLayout, LaunchKind, Platform};
use pindu_core::sources::location::HostDirs;
use pindu_core::sources::{ImportedGame, Snapshot, desktop};

use crate::support::fixture;

/// The desktop fixture: a home, a host `/usr` and a system Flatpak installation.
fn desktop_dirs() -> (tempfile::TempDir, HostDirs) {
    let (dir, root) = fixture("desktop");
    let home = root.join("home");
    let dirs = HostDirs {
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: root.join("usr"),
        system_flatpak: root.join("var/lib/flatpak"),
        home,
    };
    (dir, dirs)
}

fn game<'a>(snapshot: &'a Snapshot, native_id: &str) -> &'a ImportedGame {
    snapshot
        .games
        .iter()
        .find(|game| game.native_id == native_id)
        .unwrap_or_else(|| panic!("no game {native_id} in {:?}", snapshot.games))
}

fn ids(snapshot: &Snapshot) -> Vec<&str> {
    let mut ids: Vec<_> = snapshot
        .games
        .iter()
        .map(|g| g.native_id.as_str())
        .collect();
    ids.sort_unstable();
    ids
}

#[test]
fn games_come_from_every_application_folder() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    assert_eq!(
        ids(&snapshot),
        [
            "celeste",
            "net.supertuxkart.SuperTuxKart",
            "net.veloren.airshipper",
            "neverball",
            "supertux2",
        ]
    );
}

#[test]
fn the_folder_reading_order_matches_the_xdg_search_path() {
    let (_dir, dirs) = desktop_dirs();

    assert_eq!(
        desktop::application_dirs(&dirs),
        vec![
            dirs.data_home.join("applications"),
            dirs.data_home.join("flatpak/exports/share/applications"),
            dirs.system_flatpak.join("exports/share/applications"),
            dirs.host_usr.join("share/applications"),
        ]
    );
}

#[test]
fn an_entry_in_the_users_folder_overrides_the_system_one() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    assert_eq!(game(&snapshot, "celeste").title, "Celeste");
    // A Hidden entry in the user's folder deletes the system entry of the same name.
    assert!(snapshot.games.iter().all(|game| game.native_id != "masked"));
}

#[test]
fn a_game_keeps_its_exec_line_for_host_spawn() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    let celeste = game(&snapshot, "celeste");
    assert_eq!(celeste.launch_kind, LaunchKind::DesktopEntry);
    assert!(celeste.installed);
    let config = celeste.launch_config.as_ref().unwrap();
    assert_eq!(config.executable, Path::new("/opt/Celeste Game/Celeste"));
    assert!(config.arguments.is_empty());
    assert_eq!(
        config.working_dir.as_deref(),
        Some(Path::new("/opt/Celeste Game"))
    );
}

#[test]
fn flatpak_games_run_through_flatpak() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    let airshipper = game(&snapshot, "net.veloren.airshipper");
    assert_eq!(airshipper.platform, Some(Platform::Linux));
    let config = airshipper.launch_config.as_ref().unwrap();
    assert_eq!(config.executable, Path::new("/usr/bin/flatpak"));
    assert_eq!(
        config.arguments,
        [
            "run",
            "--branch=stable",
            "--arch=x86_64",
            "--command=airshipper",
            "net.veloren.airshipper"
        ]
    );
}

#[test]
fn icons_are_found_by_name_largest_first() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    let icon = |id: &str| -> Vec<(ArtLayout, PathBuf)> { game(&snapshot, id).artwork.clone() };
    assert_eq!(
        icon("celeste"),
        [(
            ArtLayout::Icon,
            dirs.data_home
                .join("icons/hicolor/256x256/apps/celeste.png")
        )]
    );
    assert_eq!(
        icon("supertux2"),
        [(
            ArtLayout::Icon,
            dirs.host_usr
                .join("share/icons/hicolor/scalable/apps/supertux2.svg")
        )]
    );
    assert_eq!(
        icon("net.veloren.airshipper"),
        [(
            ArtLayout::Icon,
            dirs.system_flatpak
                .join("exports/share/icons/hicolor/128x128/apps/net.veloren.airshipper.png")
        )]
    );
    assert_eq!(
        icon("neverball"),
        [(
            ArtLayout::Icon,
            dirs.host_usr.join("share/pixmaps/neverball.png")
        )]
    );
    // No icon file anywhere.
    assert!(icon("net.supertuxkart.SuperTuxKart").is_empty());
}

#[test]
fn launchers_and_their_game_shortcuts_are_skipped() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    for skipped in [
        "steam",
        "Dota 2",
        "heroic-ut2004",
        "net.lutris.celeste-1",
        "net.lutris.Lutris",
        "bottles-epic",
        "faugus-hollow-knight",
        "com.heroicgameslauncher.hgl",
        "io.github.tim_van_oudheusden.pindu-launcher",
    ] {
        assert!(
            snapshot.games.iter().all(|game| game.native_id != skipped),
            "{skipped}"
        );
    }
}

#[test]
fn non_games_hidden_entries_and_links_are_skipped() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    for skipped in ["editor", "hidden-game", "link", "no-exec"] {
        assert!(
            snapshot.games.iter().all(|game| game.native_id != skipped),
            "{skipped}"
        );
    }
}

#[test]
fn a_game_with_a_broken_exec_line_is_reported() {
    let (_dir, dirs) = desktop_dirs();

    let snapshot = desktop::import(&dirs);

    assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
    assert!(
        snapshot.errors[0].contains("broken.desktop"),
        "{:?}",
        snapshot.errors
    );
}

#[test]
fn missing_folders_give_an_empty_source() {
    let dir = tempfile::tempdir().unwrap();
    let dirs = HostDirs {
        home: dir.path().into(),
        data_home: dir.path().join("data"),
        config_home: dir.path().join("config"),
        host_usr: dir.path().join("usr"),
        system_flatpak: dir.path().join("flatpak"),
    };

    let snapshot = desktop::import(&dirs);

    assert!(snapshot.games.is_empty());
    assert!(snapshot.errors.is_empty());
}
