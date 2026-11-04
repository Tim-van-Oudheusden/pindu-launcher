use std::path::{Path, PathBuf};

use pindu_core::model::{ArtLayout, LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::vdf::Value;
use pindu_core::sources::{ImportedGame, Snapshot, steam};

use crate::support::{appinfo_bytes, fixture, map, text};

fn host_dirs(home: &Path) -> HostDirs {
    HostDirs {
        home: home.to_owned(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
    }
}

fn app(name: &str, kind: &str, oslist: &str) -> Value {
    map([(
        "common",
        map([
            ("name", text(name)),
            ("type", text(kind)),
            ("oslist", text(oslist)),
        ]),
    )])
}

fn write_appinfo(steam_root: &Path) {
    let dota = map([(
        "common",
        map([
            ("name", text("Dota 2")),
            ("type", text("Game")),
            ("oslist", text("windows,linux")),
            ("icon", text("0bbb630d63262dd66d2fdd0f7d37e8661a410075")),
            (
                "associations",
                map([(
                    "0",
                    map([("type", text("developer")), ("name", text("Valve"))]),
                )]),
            ),
        ]),
    )]);
    let cyberpunk = map([
        (
            "common",
            map([
                ("name", text("Cyberpunk 2077")),
                ("type", text("game")),
                ("oslist", text("windows")),
            ]),
        ),
        ("extended", map([("developer", text("CD PROJEKT RED"))])),
    ]);
    let apps = [
        (570, dota),
        (1091500, cyberpunk),
        (1493710, app("Proton Experimental", "Tool", "linux")),
        (1245620, app("ELDEN RING", "Game", "windows")),
        (292030, app("The Witcher 3: Wild Hunt", "Game", "windows")),
    ];
    std::fs::create_dir_all(steam_root.join("appcache")).unwrap();
    std::fs::write(
        steam_root.join("appcache/appinfo.vdf"),
        appinfo_bytes(29, &apps),
    )
    .unwrap();
}

/// Copies a fixture, writes its appinfo.vdf, resolves Steam in it and imports.
fn import_fixture(name: &str, with_appinfo: bool) -> (tempfile::TempDir, PathBuf, Snapshot) {
    let (dir, root) = fixture(name);
    let home = root.join("home");
    let steam_root = Location::of(Source::Steam, &host_dirs(&home))
        .unwrap()
        .resolve(None)
        .unwrap();
    if with_appinfo {
        write_appinfo(&steam_root);
    }
    let snapshot = steam::import(&steam_root).unwrap();
    (dir, root, snapshot)
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
        .map(|game| game.native_id.as_str())
        .collect();
    ids.sort();
    ids
}

#[test]
fn every_library_in_libraryfolders_is_read() {
    let (_dir, _root, snapshot) = import_fixture("steam", true);

    assert_eq!(snapshot.source, Source::Steam);
    assert_eq!(ids(&snapshot), ["1091500", "1245620", "292030", "570"]);
}

#[test]
fn install_state_comes_from_the_manifest_state_flags() {
    let (_dir, root, snapshot) = import_fixture("steam", true);

    let dota = game(&snapshot, "570");
    assert!(dota.installed);
    assert_eq!(dota.title, "Dota 2");
    assert_eq!(
        dota.install_path.as_deref(),
        Some(
            root.join("home/.local/share/Steam/steamapps/common/dota 2 beta")
                .as_path()
        )
    );
    assert_eq!(dota.size_bytes, Some(40_000_000_000));
    assert_eq!(dota.last_played_at, Some(1_757_613_311));
    assert_eq!(dota.launch_kind, LaunchKind::StoreUri);

    let cyberpunk = game(&snapshot, "1091500");
    assert!(!cyberpunk.installed);
    assert_eq!(cyberpunk.last_played_at, None);
}

#[test]
fn a_library_outside_granted_paths_is_read_from_its_apps_list() {
    let (_dir, _root, snapshot) = import_fixture("steam", true);

    let witcher = game(&snapshot, "292030");
    assert!(witcher.installed);
    assert_eq!(witcher.title, "The Witcher 3: Wild Hunt");
    assert_eq!(witcher.size_bytes, Some(50_000_000_000));
    assert_eq!(witcher.install_path, None);
}

#[test]
fn non_games_are_filtered_and_platform_and_developer_come_from_appinfo() {
    let (_dir, _root, snapshot) = import_fixture("steam", true);

    assert!(
        snapshot
            .games
            .iter()
            .all(|game| game.native_id != "1493710")
    );
    let dota = game(&snapshot, "570");
    assert_eq!(dota.platform, Some(Platform::Linux));
    assert_eq!(dota.developer.as_deref(), Some("Valve"));
    let cyberpunk = game(&snapshot, "1091500");
    assert_eq!(cyberpunk.platform, Some(Platform::Windows));
    assert_eq!(cyberpunk.developer.as_deref(), Some("CD PROJEKT RED"));
}

#[test]
fn librarycache_art_is_used_in_every_layout() {
    let (_dir, root, snapshot) = import_fixture("steam", true);
    let cache = root.join("home/.local/share/Steam/appcache/librarycache");

    assert_eq!(
        game(&snapshot, "570").artwork,
        [
            (ArtLayout::Cover, cache.join("570/library_600x900.jpg")),
            (ArtLayout::Hero, cache.join("570/library_hero.jpg")),
            (ArtLayout::Logo, cache.join("570/logo.png")),
            (
                ArtLayout::Icon,
                cache.join("570/0bbb630d63262dd66d2fdd0f7d37e8661a410075.jpg")
            ),
        ]
    );
    assert_eq!(
        game(&snapshot, "1091500").artwork,
        [(
            ArtLayout::Cover,
            cache.join("1091500/0a1b2c/library_600x900.jpg")
        )]
    );
    assert_eq!(
        game(&snapshot, "1245620").artwork,
        [
            (ArtLayout::Cover, cache.join("1245620_library_600x900.jpg")),
            (ArtLayout::Hero, cache.join("1245620_library_hero.jpg")),
        ]
    );
}

#[test]
fn a_broken_manifest_is_reported_and_the_rest_imported() {
    let (_dir, _root, snapshot) = import_fixture("steam", true);

    assert_eq!(snapshot.errors.len(), 1, "{:?}", snapshot.errors);
    assert!(
        snapshot.errors[0].contains("appmanifest_999.acf"),
        "{:?}",
        snapshot.errors
    );
}

#[test]
fn without_appinfo_games_still_import_without_platform() {
    let (_dir, _root, snapshot) = import_fixture("steam", false);

    let dota = game(&snapshot, "570");
    assert_eq!(dota.title, "Dota 2");
    assert_eq!(dota.platform, None);
    // Apps known only from an apps list have no name without appinfo.vdf.
    assert!(snapshot.games.iter().all(|game| game.native_id != "292030"));
}

#[test]
fn flatpak_steam_imports_its_main_library_whatever_path_it_records() {
    let (_dir, root, snapshot) = import_fixture("steam-flatpak", true);

    assert_eq!(ids(&snapshot), ["570"]);
    let dota = game(&snapshot, "570");
    assert!(dota.installed);
    let steam_root = root.join("home/.var/app/com.valvesoftware.Steam/data/Steam");
    assert_eq!(
        dota.install_path,
        Some(steam_root.join("steamapps/common/dota 2 beta"))
    );
    assert_eq!(
        dota.artwork,
        [(
            ArtLayout::Cover,
            steam_root.join("appcache/librarycache/570/library_600x900.jpg")
        )]
    );
}

#[test]
fn a_portrait_capsule_is_the_cover_when_there_is_no_600x900_art() {
    let (_dir, root) = fixture("steam");
    let steam_root = root.join("home/.local/share/Steam");
    write_appinfo(&steam_root);
    let capsule = steam_root.join("appcache/librarycache/292030/9f2c/library_capsule.jpg");
    std::fs::create_dir_all(capsule.parent().unwrap()).unwrap();
    std::fs::write(&capsule, "capsule").unwrap();

    let snapshot = steam::import(&steam_root).unwrap();

    assert_eq!(
        game(&snapshot, "292030").artwork,
        [(ArtLayout::Cover, capsule)]
    );
}

/// Makes the account folder `id` the most recently written one.
fn touch_account(steam_root: &Path, id: &str, age_secs: u64) {
    let config = steam_root.join(format!("userdata/{id}/config/localconfig.vdf"));
    let time = std::time::SystemTime::now() - std::time::Duration::from_secs(age_secs);
    std::fs::File::options()
        .write(true)
        .open(config)
        .unwrap()
        .set_modified(time)
        .unwrap();
}

#[test]
fn playtime_comes_from_the_most_recent_accounts_localconfig() {
    let (_dir, root) = fixture("steam");
    let steam_root = root.join("home/.local/share/Steam");
    touch_account(&steam_root, "111", 3600);
    touch_account(&steam_root, "222", 60);
    touch_account(&steam_root, "0", 0);
    write_appinfo(&steam_root);

    let snapshot = steam::import(&steam_root).unwrap();

    let dota = game(&snapshot, "570");
    // Steam keeps minutes.
    assert_eq!(dota.playtime_secs, Some(574 * 60));
    // The newer of the manifest's and the account's last-played times.
    assert_eq!(dota.last_played_at, Some(1_757_613_311));
    assert_eq!(
        game(&snapshot, "1245620").last_played_at,
        Some(1_800_000_000)
    );
    // Listed without any playtime.
    assert_eq!(game(&snapshot, "1091500").playtime_secs, None);
}

#[test]
fn the_other_account_counts_when_it_was_used_last() {
    let (_dir, root) = fixture("steam");
    let steam_root = root.join("home/.local/share/Steam");
    touch_account(&steam_root, "111", 60);
    touch_account(&steam_root, "222", 3600);
    write_appinfo(&steam_root);

    let snapshot = steam::import(&steam_root).unwrap();

    assert_eq!(game(&snapshot, "570").playtime_secs, Some(10 * 60));
}
