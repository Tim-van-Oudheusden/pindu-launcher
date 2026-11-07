//! Shared helpers for the importer tests.

use std::path::{Path, PathBuf};

use pindu_core::sources::vdf::Value;

/// Copies `tests/fixtures/<name>` into a fresh temporary folder, replacing `@ROOT@` in
/// every text file with the copy's path so absolute paths in launcher files point inside it.
pub(crate) fn fixture(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_owned();
    fixture_into(name, &root);
    (dir, root)
}

/// Copies `tests/fixtures/<name>` into `dest`, replacing `@ROOT@` with `dest`.
pub(crate) fn fixture_into(name: &str, dest: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    copy_tree(&source, dest, dest);
}

fn copy_tree(from: &Path, to: &Path, root: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_tree(&entry.path(), &target, root);
        } else {
            let bytes = std::fs::read(entry.path()).unwrap();
            let bytes = match String::from_utf8(bytes) {
                Ok(text) => text.replace("@ROOT@", root.to_str().unwrap()).into_bytes(),
                Err(error) => error.into_bytes(),
            };
            std::fs::write(target, bytes).unwrap();
        }
    }
}

/// Encodes apps as a Steam `appinfo.vdf` of format version 28 or 29 (29 keeps keys in a
/// string table at the end of the file).
pub(crate) fn appinfo_bytes(version: u8, apps: &[(u32, Value)]) -> Vec<u8> {
    let mut strings: Vec<String> = Vec::new();
    let mut out = Vec::new();
    // The magic is 0x075644_NN with the version written as hex digits: 28 → 0x28.
    let version_tag = (version / 10) * 16 + version % 10;
    out.extend_from_slice(&[version_tag, 0x44, 0x56, 0x07]);
    out.extend_from_slice(&1u32.to_le_bytes());
    let table_offset_at = out.len();
    if version >= 29 {
        out.extend_from_slice(&0i64.to_le_bytes());
    }
    for (app_id, value) in apps {
        let mut body = Vec::new();
        body.extend_from_slice(&2u32.to_le_bytes()); // info state
        body.extend_from_slice(&1_700_000_000u32.to_le_bytes()); // last updated
        body.extend_from_slice(&0u64.to_le_bytes()); // PICS token
        body.extend_from_slice(&[0; 20]); // text SHA-1
        body.extend_from_slice(&1u32.to_le_bytes()); // change number
        body.extend_from_slice(&[0; 20]); // binary SHA-1
        let root = Value::Map(vec![("appinfo".into(), value.clone())]);
        encode_map(&root, version, &mut strings, &mut body);
        out.extend_from_slice(&app_id.to_le_bytes());
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend_from_slice(&body);
    }
    out.extend_from_slice(&0u32.to_le_bytes());
    if version >= 29 {
        let offset = out.len() as i64;
        out[table_offset_at..table_offset_at + 8].copy_from_slice(&offset.to_le_bytes());
        out.extend_from_slice(&(strings.len() as u32).to_le_bytes());
        for string in &strings {
            out.extend_from_slice(string.as_bytes());
            out.push(0);
        }
    }
    out
}

fn encode_map(value: &Value, version: u8, strings: &mut Vec<String>, out: &mut Vec<u8>) {
    for (key, child) in value.entries() {
        let kind = match child {
            Value::Map(_) => 0u8,
            Value::Str(_) => 1,
            Value::Int(_) => 2,
            Value::Float(_) => 3,
        };
        out.push(kind);
        if version >= 29 {
            let index = strings.iter().position(|s| s == key).unwrap_or_else(|| {
                strings.push(key.clone());
                strings.len() - 1
            });
            out.extend_from_slice(&(index as u32).to_le_bytes());
        } else {
            out.extend_from_slice(key.as_bytes());
            out.push(0);
        }
        match child {
            Value::Map(_) => encode_map(child, version, strings, out),
            Value::Str(text) => {
                out.extend_from_slice(text.as_bytes());
                out.push(0);
            }
            Value::Int(number) => out.extend_from_slice(&(*number as i32).to_le_bytes()),
            Value::Float(number) => out.extend_from_slice(&number.to_le_bytes()),
        }
    }
    out.push(8);
}

/// A map from `(key, value)` pairs, for building appinfo entries.
pub(crate) fn map<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Map(
        entries
            .into_iter()
            .map(|(key, value)| (key.to_owned(), value))
            .collect(),
    )
}

pub(crate) fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}
