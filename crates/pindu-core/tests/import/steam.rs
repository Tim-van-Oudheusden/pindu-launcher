use std::path::{Path, PathBuf};

use pindu_core::model::{
    ArtLayout, ControllerSupport, DeckCompatibility, DeckRating, LaunchKind, Platform, Source,
};
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

/// Steam's Deck rating as `appinfo.vdf` stores it: Ints, the category from 0 to 3.
fn deck(category: i64, tested_at: Option<i64>) -> Value {
    let category = ("category", Value::Int(category));
    match tested_at {
        Some(time) => map([category, ("test_timestamp", Value::Int(time))]),
        None => map([category]),
    }
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
            // 2013-07-09 UTC.
            ("steam_release_date", Value::Int(1_373_328_000)),
            // Action, Strategy and Free To Play, which is a price model, not a genre.
            (
                "genres",
                map([
                    ("0", Value::Int(1)),
                    ("1", Value::Int(2)),
                    ("2", Value::Int(37)),
                ]),
            ),
            ("controller_support", text("full")),
            ("steam_deck_compatibility", deck(3, Some(1_700_000_000))),
        ]),
    )]);
    let cyberpunk = map([
        (
            "common",
            map([
                ("name", text("Cyberpunk 2077")),
                ("type", text("game")),
                ("oslist", text("windows")),
                // The original release, 2020-12-10 UTC, wins over the Steam one.
                ("original_release_date", Value::Int(1_607_558_400)),
                ("steam_release_date", Value::Int(1_700_000_000)),
                // RPG, Early Access (a release state) and an ID with no name.
                (
                    "genres",
                    map([
                        ("0", Value::Int(3)),
                        ("1", Value::Int(70)),
                        ("2", Value::Int(999)),
                    ]),
                ),
                ("controller_support", text("partial")),
                ("steam_deck_compatibility", deck(2, None)),
            ]),
        ),
        ("extended", map([("developer", text("CD PROJEKT RED"))])),
    ]);
    let apps = [
        (570, dota),
        (1091500, cyberpunk),
        (1493710, app("Proton Experimental", "Tool", "linux")),
        (
            1245620,
            map([(
                "common",
                map([
                    ("name", text("ELDEN RING")),
                    ("type", text("Game")),
                    ("oslist", text("windows")),
                    // Zero means Steam does not know the date.
                    ("steam_release_date", Value::Int(0)),
                    ("steam_deck_compatibility", deck(1, Some(1_650_000_000))),
                ]),
            )]),
        ),
        (
            292030,
            map([(
                "common",
                map([
                    ("name", text("The Witcher 3: Wild Hunt")),
                    ("type", text("Game")),
                    ("oslist", text("windows")),
                    ("steam_deck_compatibility", deck(0, Some(0))),
                ]),
            )]),
        ),
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
fn the_steam_deck_rating_comes_from_appinfo() {
    let (_dir, _root, snapshot) = import_fixture("steam", true);

    let rating = |id| game(&snapshot, id).deck_compatibility;
    assert_eq!(
        rating("570"),
        Some(DeckCompatibility {
            rating: DeckRating::Verified,
            tested_at: Some(1_700_000_000),
        })
    );
    assert_eq!(
        rating("1091500"),
        Some(DeckCompatibility {
            rating: DeckRating::Playable,
            tested_at: None,
        })
    );
    assert_eq!(
        rating("1245620"),
        Some(DeckCompatibility {
            rating: DeckRating::Unsupported,
            tested_at: Some(1_650_000_000),
        })
    );
    // Valve has not rated it yet; a zero test time means it was never tested.
    assert_eq!(
        rating("292030"),
        Some(DeckCompatibility {
            rating: DeckRating::Unknown,
            tested_at: None,
        })
    );
}

#[test]
fn without_appinfo_there_is_no_steam_deck_rating() {
    let (_dir, _root, snapshot) = import_fixture("steam", false);

    assert!(
        snapshot
            .games
            .iter()
            .all(|game| game.deck_compatibility.is_none())
    );
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
fn steams_login_files_are_never_opened() {
    let (_dir, root) = fixture("steam");
    let steam_root = Location::of(Source::Steam, &host_dirs(&root.join("home")))
        .unwrap()
        .resolve(None)
        .unwrap();
    write_appinfo(&steam_root);
    // Folders in the places of the files that hold Steam's sign-in tokens and accounts:
    // they exist, but reading them fails even for root, which CI's test user is.
    for credentials in ["config/config.vdf", "config/loginusers.vdf"] {
        std::fs::create_dir_all(steam_root.join(credentials)).unwrap();
    }
    let (_clean_dir, _clean_root, clean) = import_fixture("steam", true);

    let snapshot = steam::import(&steam_root).unwrap();

    assert_eq!(
        snapshot.errors.len(),
        clean.errors.len(),
        "{:?}",
        snapshot.errors
    );
    assert_eq!(ids(&snapshot), ids(&clean));
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

#[test]
fn release_year_genres_and_controller_support_come_from_appinfo_in_both_layouts() {
    for fixture_name in ["steam", "steam-flatpak"] {
        let (_dir, _root, snapshot) = import_fixture(fixture_name, true);

        let dota = game(&snapshot, "570");
        assert_eq!(dota.year, Some(2013), "{fixture_name}");
        assert_eq!(dota.genres, ["Action", "Strategy"], "{fixture_name}");
        assert_eq!(
            dota.controller_support,
            Some(ControllerSupport::Full),
            "{fixture_name}"
        );
    }

    let (_dir, _root, snapshot) = import_fixture("steam", true);
    let cyberpunk = game(&snapshot, "1091500");
    assert_eq!(cyberpunk.year, Some(2020));
    assert_eq!(cyberpunk.genres, ["RPG"]);
    assert_eq!(
        cyberpunk.controller_support,
        Some(ControllerSupport::Partial)
    );
    let elden_ring = game(&snapshot, "1245620");
    assert_eq!(elden_ring.year, None);
    assert!(elden_ring.genres.is_empty());
    // Not declared, which is not the same as no support.
    assert_eq!(elden_ring.controller_support, None);
}
