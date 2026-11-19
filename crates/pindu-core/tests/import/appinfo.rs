use pindu_core::sources::vdf::{Value, read_appinfo};

use crate::support::{appinfo_bytes, appinfo_file, map, text};

fn dota() -> Value {
    map([(
        "common",
        map([
            ("name", text("Dota 2")),
            ("type", text("Game")),
            ("oslist", text("windows,linux")),
            ("controller_support", text("full")),
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
fn version_27_entries_parse() {
    // Format 27 has no binary SHA-1 in the entry header, so the header is 20 bytes shorter.
    let bytes = appinfo_bytes(27, &[(570, dota())]);

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

/// One binary KeyValues entry with its key inline (formats 27 and 28).
fn binary_entry(kind: u8, key: &str, value: &[u8]) -> Vec<u8> {
    [&[kind], key.as_bytes(), &[0], value].concat()
}

#[test]
fn pointer_colour_and_64_bit_values_keep_their_widths() {
    // A string follows each value, so the string reads back only if the value took
    // exactly its own width.
    let typed: [(u8, &str, Vec<u8>); 4] = [
        (0x04, "pointer", 0x0102_0304i32.to_le_bytes().into()),
        (0x06, "colour", 0x7F80_90A0i32.to_le_bytes().into()),
        (
            0x07,
            "uint64",
            76_561_197_960_287_930u64.to_le_bytes().into(),
        ),
        (0x0A, "int64", (-5_000_000_000i64).to_le_bytes().into()),
    ];
    let mut appinfo = Vec::new();
    for (kind, key, value) in &typed {
        appinfo.extend(binary_entry(*kind, key, value));
        appinfo.extend(binary_entry(0x01, &format!("after_{key}"), b"next\0"));
    }
    appinfo.push(0x08);
    let root = [binary_entry(0x00, "appinfo", &appinfo), vec![0x08]].concat();
    let bytes = appinfo_file(28, &[(570, root)], &[]);

    let apps = read_appinfo(&bytes, |_| true).unwrap();

    assert_eq!(
        apps.get(&570),
        Some(&map([
            ("pointer", Value::Int(0x0102_0304)),
            ("after_pointer", text("next")),
            ("colour", Value::Int(0x7F80_90A0)),
            ("after_colour", text("next")),
            ("uint64", Value::Int(76_561_197_960_287_930)),
            ("after_uint64", text("next")),
            ("int64", Value::Int(-5_000_000_000)),
            ("after_int64", text("next")),
        ]))
    );
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

#[test]
fn maps_nested_too_deeply_are_an_error_not_a_stack_overflow() {
    let depth = 100_000;
    // Built by hand: the support encoder recurses once per level, as the parser did.
    let mut body = Vec::new();
    body.extend_from_slice(&[0; 4 + 4 + 8 + 20 + 4 + 20]); // entry header, format 28 and up
    for _ in 0..depth {
        body.push(0x00); // map
        body.extend_from_slice(&0u32.to_le_bytes()); // key "a", string table index 0
    }
    body.extend(std::iter::repeat_n(0x08, depth + 1)); // end of each map and of the root
    let mut bytes = vec![0x29, 0x44, 0x56, 0x07];
    bytes.extend_from_slice(&1u32.to_le_bytes()); // universe
    let table_offset = 4 + 4 + 8 + 4 + 4 + body.len() + 4;
    bytes.extend_from_slice(&(table_offset as i64).to_le_bytes());
    bytes.extend_from_slice(&570u32.to_le_bytes());
    bytes.extend_from_slice(&(body.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&body);
    bytes.extend_from_slice(&0u32.to_le_bytes()); // end of apps
    bytes.extend_from_slice(&1u32.to_le_bytes()); // one string
    bytes.extend_from_slice(b"a\0");

    let error = read_appinfo(&bytes, |_| true).unwrap_err();

    assert_eq!(error.message, "maps nested too deeply");
}
