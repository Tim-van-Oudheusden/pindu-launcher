//! The `importer.json` Meson installs beside the helper agrees with the built-in Bottles
//! source it serves.

use std::path::Path;

use pindu_core::model::Source;
use pindu_core::sources::external::manifest::{Runs, parse_manifest};
use pindu_core::sources::external::{built_in, runner};
use pindu_core::sources::location::{HostDirs, Location};

#[test]
fn the_manifest_reads_where_pindu_looks_for_bottles_and_runs_in_its_sub_sandbox() {
    let folder = tempfile::tempdir().unwrap();
    std::fs::write(folder.path().join("pindu-import-bottles"), "").unwrap();
    let text = include_str!("../importer.json.in").replace("@VERSION@", "1.0");
    let home = Path::new("/home/me");
    let dirs = HostDirs {
        home: home.to_owned(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: "/usr".into(),
        system_flatpak: "/var/lib/flatpak".into(),
    };

    let manifest = parse_manifest(&text, folder.path()).unwrap();

    assert_eq!(built_in(&manifest.id), Some(Source::Bottles));
    assert_eq!(manifest.name, "Bottles");
    assert_eq!(manifest.runs, Runs::SubSandbox);
    assert_eq!(manifest.uri_schemes, ["bottles"]);
    assert_eq!(
        runner::reads(&manifest, home),
        Location::of(Source::Bottles, &dirs).unwrap().candidates()
    );
}
