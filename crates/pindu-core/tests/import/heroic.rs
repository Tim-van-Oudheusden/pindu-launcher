use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use pindu_core::model::{ArtLayout, LaunchKind, Platform, Source};
use pindu_core::sources::location::{HostDirs, Location};
use pindu_core::sources::{ImportedGame, Snapshot, heroic};

use crate::support::fixture_into;

/// Copies the Heroic fixture to `<home>/<at>` and returns the temp dir, home and root.
fn heroic_home(at: &str) -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let heroic_root = home.join(at);
    fixture_into("heroic", &heroic_root);
    (dir, home, heroic_root)
}
fn dirs(home: &Path) -> HostDirs {
    HostDirs {
        home: home.to_owned(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
    }
}

fn import(root: &Path, source: Source) -> Snapshot {
    heroic::import(root, source).unwrap()
}

fn game<'a>(snapshot: &'a Snapshot, native_id: &str) -> &'a ImportedGame {
    snapshot
        .games
        .iter()
        .find(|game| game.native_id == native_id)
        .unwrap_or_else(|| panic!("no game {native_id} in {:?}", snapshot.games))
}

fn ids(snapshot: &Snapshot) -> Vec<&str> {
    snapshot
        .games
        .iter()
        .map(|game| game.native_id.as_str())
        .collect()
}

fn cache_file(url: &str) -> String {
    match url {
        "ut2004_square" => "72d1b2456bf895c2f8d907bbe4938452c7c49938578a82f3ae6d1501d3eaf1cd",
        "ut2004_wide" => "c1dec405583ad51fd89724007cac482748a1002d59fa5391c82f78f0eb74db44",
        "kinglet_tall" => "7cdc0ec148d6702309331f7e3ce71a572964fede033a3e4acb7b4da0157b948c",
        "sugar_logo" => "ed047fe587887f821ca037e7dda8f740503bf682544246c4e3221090bde5528c",
        _ => unreachable!(),
    }
    .to_owned()
}

#[test]
fn epic_library_is_joined_with_its_installed_json() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicEpic);

    assert_eq!(snapshot.source, Source::HeroicEpic);
    assert_eq!(ids(&snapshot), ["Sugar", "Kinglet"]);
    let sugar = game(&snapshot, "Sugar");
    assert!(sugar.installed);
    assert_eq!(
        sugar.install_path.as_deref(),
        Some(Path::new("/games/Heroic/RocketRacing"))
    );
    assert_eq!(sugar.size_bytes, Some(2_147_483_648));
    assert_eq!(sugar.platform, Some(Platform::Windows));
    assert_eq!(sugar.launch_kind, LaunchKind::StoreUri);
    let kinglet = game(&snapshot, "Kinglet");
    assert!(!kinglet.installed);
    assert_eq!(kinglet.platform, None);
    assert_eq!(kinglet.install_path, None);
}

#[test]
fn gog_platform_comes_from_install_info() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicGog);

    let ut = game(&snapshot, "1207658924");
    assert!(ut.installed);
    assert_eq!(ut.platform, Some(Platform::Linux));
    assert_eq!(
        ut.install_path.as_deref(),
        Some(Path::new("/games/Heroic/UT2004"))
    );
    assert!(!game(&snapshot, "1495134320").installed);
}

#[test]
fn amazon_games_are_joined_by_id_and_run_on_windows() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicAmazon);

    let amazon = game(&snapshot, "amzn1.adg.product.0001");
    assert!(amazon.installed);
    assert_eq!(amazon.platform, Some(Platform::Windows));
    assert_eq!(
        amazon.install_path.as_deref(),
        Some(Path::new("/games/Heroic/AmazonGame"))
    );
    assert_eq!(amazon.size_bytes, Some(1_000_000));
}

#[test]
fn sideloaded_games_use_their_own_install_info_and_local_art() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicSideload);

    let side = game(&snapshot, "aB3dE");
    assert!(side.installed);
    assert_eq!(side.platform, Some(Platform::Linux));
    assert_eq!(side.artwork.len(), 1);
    assert_eq!(side.artwork[0].0, ArtLayout::Cover);
    assert!(side.artwork[0].1.ends_with("sideload_square.png"));
}

#[test]
fn game_overrides_replace_title_and_cover() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicEpic);

    let sugar = game(&snapshot, "Sugar");
    assert_eq!(sugar.title, "Rocket Racing (renamed)");
    assert!(sugar.artwork.contains(&(
        ArtLayout::Cover,
        root.join("game_overrides_images/Sugar-square.png")
    )));
}

#[test]
fn art_comes_from_heroics_images_cache() {
    let (_dir, _home, root) = heroic_home(".config/heroic");
    let cache = root.join("images-cache");

    let gog = import(&root, Source::HeroicGog);
    let epic = import(&root, Source::HeroicEpic);

    assert_eq!(
        game(&gog, "1207658924").artwork,
        [
            (ArtLayout::Cover, cache.join(cache_file("ut2004_square"))),
            (ArtLayout::Hero, cache.join(cache_file("ut2004_wide"))),
        ]
    );
    // Heroic resizes Epic art through URL parameters, which are part of the cache key.
    assert_eq!(
        game(&epic, "Kinglet").artwork,
        [(ArtLayout::Cover, cache.join(cache_file("kinglet_tall")))]
    );
    assert!(
        game(&epic, "Sugar")
            .artwork
            .contains(&(ArtLayout::Logo, cache.join(cache_file("sugar_logo"))))
    );
    assert!(game(&gog, "1495134320").artwork.is_empty());
}

#[test]
fn metadata_comes_from_game_info() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let snapshot = import(&root, Source::HeroicEpic);

    let sugar = game(&snapshot, "Sugar");
    assert_eq!(sugar.developer.as_deref(), Some("Psyonix"));
    assert_eq!(sugar.genres, ["Racing", "Sports"]);
    assert_eq!(sugar.description.as_deref(), Some("Arcade racing."));
}

#[test]
fn hidden_favourite_and_category_flags_come_from_store_config() {
    let (_dir, _home, root) = heroic_home(".config/heroic");

    let epic = import(&root, Source::HeroicEpic);
    let gog = import(&root, Source::HeroicGog);

    assert!(game(&epic, "Kinglet").source_flags.hidden);
    assert!(!game(&epic, "Sugar").source_flags.hidden);
    let ut = &game(&gog, "1207658924").source_flags;
    assert!(ut.favourite);
    assert_eq!(ut.categories, ["Shooters"]);
    assert_eq!(
        game(&epic, "Sugar").source_flags.categories,
        ["Racing", "Shooters"]
    );
}

#[test]
fn credential_files_are_never_opened() {
    let (_dir, _home, root) = heroic_home(".config/heroic");
    for credentials in [
        "gog_store/auth.json",
        "legendaryConfig/legendary/user.json",
        "nile_config/nile/user.json",
    ] {
        std::fs::set_permissions(
            root.join(credentials),
            std::fs::Permissions::from_mode(0o000),
        )
        .unwrap();
    }

    for source in [
        Source::HeroicEpic,
        Source::HeroicGog,
        Source::HeroicAmazon,
        Source::HeroicSideload,
    ] {
        let snapshot = heroic::import(&root, source).unwrap();
        assert!(
            snapshot.errors.is_empty(),
            "{source}: {:?}",
            snapshot.errors
        );
    }
}

#[test]
fn a_broken_installed_file_is_reported_and_the_library_still_imports() {
    let (_dir, _home, root) = heroic_home(".config/heroic");
    std::fs::write(root.join("gog_store/installed.json"), "{ not json").unwrap();

    let snapshot = import(&root, Source::HeroicGog);

    assert_eq!(ids(&snapshot), ["1207658924", "1495134320"]);
    assert!(!game(&snapshot, "1207658924").installed);
    assert_eq!(snapshot.errors.len(), 1);
    assert!(
        snapshot.errors[0].contains("installed.json"),
        "{:?}",
        snapshot.errors
    );
}

#[test]
fn native_and_flatpak_heroic_both_import() {
    for at in [
        ".config/heroic",
        ".var/app/com.heroicgameslauncher.hgl/config/heroic",
    ] {
        let (_dir, home, root) = heroic_home(at);

        let resolved = Location::of(Source::HeroicGog, &dirs(&home))
            .unwrap()
            .resolve(None)
            .unwrap();
        let snapshot = import(&resolved, Source::HeroicGog);

        assert_eq!(resolved, root);
        assert!(game(&snapshot, "1207658924").installed, "{at}");
    }
}
