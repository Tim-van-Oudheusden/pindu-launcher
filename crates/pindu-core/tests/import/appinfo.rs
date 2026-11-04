use pindu_core::sources::vdf::{Value, read_appinfo};

use crate::support::{appinfo_bytes, map, text};

fn dota() -> Value {
    map([(
        "common",
        map([
            ("name", text("Dota 2")),
            ("type", text("Game")),
            ("oslist", text("windows,linux")),
            ("controller_support", Value::Int(1)),
        ]),
    )])
}

#[test]
fn version_28_entries_parse() {
    let bytes = appinfo_bytes(28, &[(570, dota())]);

    let apps = read_appinfo(&bytes, |_| true).unwrap();

    assert_eq!(apps.get(&570), Some(&dota()));
}

#[test]
fn version_29_keys_come_from_the_string_table() {
    let bytes = appinfo_bytes(
        29,
        &[
            (570, dota()),
            (730, map([("common", map([("name", text("CS2"))]))])),
        ],
    );

    let apps = read_appinfo(&bytes, |_| true).unwrap();

    assert_eq!(apps.get(&570), Some(&dota()));
    assert_eq!(
        apps[&730].path(&["common", "name"]).and_then(Value::as_str),
        Some("CS2")
    );
}

#[test]
fn unwanted_apps_are_skipped() {
    let bytes = appinfo_bytes(29, &[(570, dota()), (730, dota())]);

    let apps = read_appinfo(&bytes, |app_id| app_id == 730).unwrap();

    assert_eq!(apps.keys().copied().collect::<Vec<_>>(), [730]);
}

#[test]
fn an_unknown_format_version_is_an_error() {
    let mut bytes = appinfo_bytes(29, &[(570, dota())]);
    bytes[0] = 0x30;

    assert!(read_appinfo(&bytes, |_| true).is_err());
}

#[test]
fn a_truncated_file_is_an_error() {
    let bytes = appinfo_bytes(28, &[(570, dota())]);

    assert!(read_appinfo(&bytes[..bytes.len() - 10], |_| true).is_err());
}
