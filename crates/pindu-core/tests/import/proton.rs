use std::path::{Path, PathBuf};

use pindu_core::proton;
use pindu_core::sources::location::HostDirs;

use crate::support::fixture;

/// Our tools folder inside the sandbox: the app's own data folder.
const OURS: &str =
    ".var/app/io.github.tim_van_oudheusden.pindu-launcher/data/pindu-launcher/compatibilitytools.d";

fn host_dirs(home: &Path) -> HostDirs {
    HostDirs {
        home: home.to_owned(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
    }
}

fn home() -> (tempfile::TempDir, PathBuf) {
    let (dir, root) = fixture("proton");
    (dir, root.join("home"))
}

#[test]
fn proton_is_found_in_native_and_flatpak_steam_and_our_tools_folder_newest_first() {
    let (_dir, home) = home();

    let builds = proton::discover(&proton::tool_dirs(&host_dirs(&home), &home.join(OURS)));

    let names: Vec<(&str, &str)> = builds
        .iter()
        .map(|build| (build.name.as_str(), build.display_name.as_str()))
        .collect();
    assert_eq!(
        names,
        [
            ("GE-Proton11-3", "GE-Proton11-3"),
            ("Proton-Ours-1", "Proton-Ours-1"),
            (
                "proton-cachyos-10.0-20260101-slr-x86_64",
                "Proton-CachyOS 10.0 (2026-01-01)"
            ),
            ("GE-Proton10-34", "GE-Proton10-34"),
        ]
    );
    // Native Steam's copy wins over the same build in Flatpak Steam.
    assert_eq!(
        builds[0].path,
        home.join(".local/share/Steam/compatibilitytools.d/GE-Proton11-3")
    );
    // Each build's wineserver, which Stop asks to end a game's prefix: under `files`, or
    // under `dist` in older builds that unpack there.
    assert_eq!(
        builds[0].wineserver,
        builds[0].path.join("files/bin/wineserver")
    );
    assert_eq!(
        builds[1].wineserver,
        home.join(OURS).join("Proton-Ours-1/dist/bin/wineserver")
    );
}
