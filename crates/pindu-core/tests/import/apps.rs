use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use pindu_core::apps::{self, App, Session};
use pindu_core::sources::location::HostDirs;

use crate::support::fixture;

/// The apps fixture: a home, a host `/usr` and a system Flatpak installation, with
/// `usr/bin/tool` executable and `usr/bin/readme` not.
fn apps_dirs() -> (tempfile::TempDir, HostDirs) {
    let (dir, dirs) = fixture_dirs("apps");
    let tool = dirs.host_usr.join("bin/tool");
    std::fs::set_permissions(&tool, std::fs::Permissions::from_mode(0o755)).unwrap();
    (dir, dirs)
}

/// The folders of the fixture `name`, laid out as the apps fixture is.
fn fixture_dirs(name: &str) -> (tempfile::TempDir, HostDirs) {
    let (dir, root) = fixture(name);
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

/// A GNOME session in English.
fn gnome() -> Session {
    Session {
        desktops: vec!["GNOME".to_owned()],
        locale: Some("en_GB.UTF-8".to_owned()),
    }
}

fn names(apps: &[App]) -> Vec<&str> {
    apps.iter().map(|app| app.name.as_str()).collect()
}

fn app<'a>(apps: &'a [App], id: &str) -> &'a App {
    apps.iter()
        .find(|app| app.id == id)
        .unwrap_or_else(|| panic!("no app {id} in {apps:?}"))
}

#[test]
fn apps_come_from_every_application_folder_by_name() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    // The user's folder, the user's Flatpak exports and the host's `/usr`.
    for name in ["Player", "Heroic Games Launcher", "Text Editor"] {
        assert!(names(&apps).contains(&name), "{name} is missing: {apps:?}");
    }
    let mut sorted = names(&apps);
    sorted.sort_by_key(|name| name.to_lowercase());
    assert_eq!(names(&apps), sorted);
}

#[test]
fn hidden_no_display_terminal_and_link_entries_are_not_apps() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    for name in ["Background Helper", "Htop", "Website", "Broken"] {
        assert!(!names(&apps).contains(&name), "{name} is listed: {apps:?}");
    }
}

#[test]
fn an_entry_whose_try_exec_program_is_missing_or_not_executable_is_not_an_app() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    for name in ["Not Installed", "Not Executable"] {
        assert!(!names(&apps).contains(&name), "{name} is listed: {apps:?}");
    }
    // Found on the host's `PATH`, by name or by its path under the host's `/usr`.
    for name in ["Tool", "Absolute Tool"] {
        assert!(names(&apps).contains(&name), "{name} is missing: {apps:?}");
    }
}

#[test]
fn only_show_in_and_not_show_in_follow_the_session_s_desktops() {
    let (_dir, dirs) = apps_dirs();
    let session = |desktops: &[&str]| Session {
        desktops: desktops.iter().map(|&desktop| desktop.to_owned()).collect(),
        locale: None,
    };
    let shown = |desktops: &[&str], name: &str| {
        names(&apps::installed(&dirs, &session(desktops))).contains(&name)
    };

    assert!(shown(&["GNOME"], "Extensions"));
    assert!(shown(&["ubuntu", "GNOME"], "Extensions"));
    assert!(!shown(&["GNOME"], "KDE Only"));
    assert!(!shown(&["GNOME"], "Not on GNOME"));
    assert!(shown(&["KDE"], "KDE Only"));
    assert!(shown(&["KDE"], "Not on GNOME"));
    assert!(!shown(&["KDE"], "Extensions"));
    // A session that names no desktop, such as gamescope's, shows only the entries for
    // every desktop.
    assert!(!shown(&[], "Extensions"));
    assert!(shown(&[], "Not on GNOME"));
}

#[test]
fn a_user_entry_overrides_or_hides_a_system_entry_of_the_same_id() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    assert_eq!(app(&apps, "org.example.Player").name, "Player");
    assert!(!names(&apps).contains(&"Player (system copy)"));
    assert!(!names(&apps).contains(&"Masked"));
}

#[test]
fn no_entry_the_library_shows_as_a_game_is_an_app() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    // A desktop game, a Steam game's shortcut and a Bottles program's.
    for name in ["Celeste", "Dota 2", "Epic Games Launcher"] {
        assert!(!names(&apps).contains(&name), "{name} is listed: {apps:?}");
    }
    // Nothing the desktop source imports is listed, in its own fixture either.
    let (_desktop, desktop_dirs) = fixture_dirs("desktop");
    for (dirs, session) in [(dirs, gnome()), (desktop_dirs, Session::default())] {
        let games = pindu_core::sources::desktop::import(&dirs);
        let apps = apps::installed(&dirs, &session);
        assert!(!games.games.is_empty());
        for game in &games.games {
            assert!(
                apps.iter().all(|app| app.id != game.native_id),
                "{} is a game and an app",
                game.native_id
            );
        }
    }
}

#[test]
fn the_launchers_the_library_imports_from_are_apps_and_pindu_itself_is_not() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    assert_eq!(app(&apps, "steam").name, "Steam");
    assert_eq!(
        app(&apps, "com.heroicgameslauncher.hgl").name,
        "Heroic Games Launcher"
    );
    assert!(
        apps.iter()
            .all(|app| !app.name.starts_with("Pindu launcher")),
        "{apps:?}"
    );
}

#[test]
fn the_desktop_source_imports_neither_the_launchers_nor_pindu_whatever_their_exec_line() {
    let (_dir, dirs) = apps_dirs();

    let games = pindu_core::sources::desktop::import(&dirs);

    // The development build's entry is a game by its category and runs no launcher
    // command: only its app ID keeps it out.
    let ids: Vec<&str> = games
        .games
        .iter()
        .map(|game| game.native_id.as_str())
        .collect();
    assert_eq!(ids, ["celeste"]);
}

#[test]
fn an_app_opens_with_its_exec_line_as_an_argument_list_without_field_codes() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    let player = &app(&apps, "org.example.Player").command;
    assert_eq!(player.argv, ["player", "--new-window", "--title=Player"]);
    assert_eq!(player.working_dir.as_deref(), Some(Path::new("/srv/media")));
    assert!(player.environment.is_empty());
    let heroic = &app(&apps, "com.heroicgameslauncher.hgl").command;
    assert_eq!(
        heroic.argv,
        [
            "/usr/bin/flatpak",
            "run",
            "--branch=stable",
            "--arch=x86_64",
            "--command=heroic-run",
            "com.heroicgameslauncher.hgl",
        ]
    );
    assert_eq!(heroic.working_dir, None);
}

#[test]
fn an_app_s_name_is_in_the_session_s_language_where_its_entry_has_one() {
    let (_dir, dirs) = apps_dirs();
    let name = |locale: Option<&str>| {
        let session = Session {
            desktops: Vec::new(),
            locale: locale.map(str::to_owned),
        };
        app(&apps::installed(&dirs, &session), "org.gnome.TextEditor")
            .name
            .clone()
    };

    assert_eq!(name(None), "Text Editor");
    assert_eq!(name(Some("en_US.UTF-8")), "Text Editor");
    assert_eq!(name(Some("de_DE.UTF-8")), "Texteditor");
    assert_eq!(name(Some("pt_BR.UTF-8")), "Editor de texto");
    assert_eq!(name(Some("sr_RS.UTF-8@latin")), "Uređivač teksta");
    assert_eq!(name(Some("pt_PT.UTF-8")), "Text Editor");
}

#[test]
fn an_app_s_icon_is_found_as_the_desktop_source_finds_one() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    assert_eq!(
        app(&apps, "org.gnome.TextEditor").icon,
        Some(
            dirs.host_usr
                .join("share/icons/hicolor/scalable/apps/org.gnome.TextEditor.svg")
        )
    );
    assert_eq!(app(&apps, "org.example.Player").icon, None);
}

#[test]
fn a_gnome_session_lists_exactly_these_apps() {
    let (_dir, dirs) = apps_dirs();

    let apps = apps::installed(&dirs, &gnome());

    assert_eq!(
        names(&apps),
        [
            "Absolute Tool",
            "Extensions",
            "Heroic Games Launcher",
            "Player",
            "Steam",
            "Text Editor",
            "Tool",
        ]
    );
}
