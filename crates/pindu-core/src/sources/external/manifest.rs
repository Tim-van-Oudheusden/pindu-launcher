//! An external importer's manifest, `importer.json`: who the helper is, how to run it,
//! where it runs, what it reads and which URI schemes its launches use.

use std::path::{Component, Path};

use serde_json::{Map, Value};

use super::PROTOCOLS;

/// The manifest's file name, at the top of the helper's folder.
pub const MANIFEST: &str = "importer.json";

/// Where a helper asks to run ([`super::runner::Tier`] is where it does).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runs {
    /// In a sandbox of its own inside the Flatpak, seeing only its folder and the folders
    /// it reads.
    SubSandbox,
    /// On the host, with the user's rights.
    Host,
}

/// A helper's checked manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    /// The protocol version the helper speaks, one of [`PROTOCOLS`].
    pub protocol: u64,
    /// The helper's ID: lowercase letters, digits and `-`, the prefix of its games'
    /// native IDs.
    pub id: String,
    /// The helper's name, as the user sees it.
    pub name: String,
    /// The helper's own version, as the user sees it.
    pub version: String,
    /// The program and its arguments; the first entry is a file inside the helper's
    /// folder, relative to it.
    pub program: Vec<String>,
    /// Where the helper asks to run.
    pub runs: Runs,
    /// The folders it reads, absolute or under the home folder (`~/…`), in the order
    /// the probe tries them.
    pub reads: Vec<String>,
    /// The URI schemes its launches use; never `file`, `http` or `https`.
    pub uri_schemes: Vec<String>,
}

/// URI schemes no helper may launch through: they open any file or page.
pub(super) const FORBIDDEN_SCHEMES: &[&str] = &["file", "http", "https"];

/// The longest helper ID.
const MAX_ID_LEN: usize = 64;

/// Reads and checks the manifest of the helper in `folder`. The error says what is
/// wrong, for the user.
pub fn read_manifest(folder: &Path) -> Result<Manifest, String> {
    let text = match std::fs::read_to_string(folder.join(MANIFEST)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(format!("it has no {MANIFEST}"));
        }
        Err(error) => return Err(format!("{MANIFEST} cannot be read: {error}")),
    };
    parse_manifest(&text, folder)
}

/// Checks the manifest `text` of the helper in `folder`.
pub fn parse_manifest(text: &str, folder: &Path) -> Result<Manifest, String> {
    let json: Value = serde_json::from_str(text)
        .map_err(|error| format!("{MANIFEST} is not valid JSON: {error}"))?;
    let object = json
        .as_object()
        .ok_or_else(|| format!("{MANIFEST} is not a JSON object"))?;
    let protocol = object
        .get("protocol")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("{MANIFEST} names no protocol version"))?;
    check_protocol(protocol, PROTOCOLS)?;
    let id = text_field(object, "id")?;
    if !is_helper_id(&id) {
        return Err(format!(
            "its ID “{id}” is not lowercase letters, digits and “-”"
        ));
    }
    let name = text_field(object, "name")?;
    let version = text_field(object, "version")?;
    let program = list_field(object, "program")?;
    match program.first() {
        Some(first) if is_file_inside(folder, first) => {}
        Some(first) => {
            return Err(format!(
                "its program “{first}” is not a file inside its folder"
            ));
        }
        None => return Err(format!("{MANIFEST} names no program")),
    }
    let runs = match object.get("runs").and_then(Value::as_str) {
        Some("sub-sandbox") => Runs::SubSandbox,
        Some("host") => Runs::Host,
        _ => {
            return Err(format!(
                "{MANIFEST} says neither “sub-sandbox” nor “host” for where it runs"
            ));
        }
    };
    let reads = optional_list_field(object, "reads")?;
    if let Some(read) = reads
        .iter()
        .find(|read| !(read.starts_with('/') || read.starts_with("~/")))
    {
        return Err(format!(
            "the folder “{read}” it reads is neither absolute nor under “~/”"
        ));
    }
    let uri_schemes = optional_list_field(object, "uri_schemes")?;
    for scheme in &uri_schemes {
        if FORBIDDEN_SCHEMES.contains(&scheme.as_str()) {
            return Err(format!("it may not launch through “{scheme}” URIs"));
        }
        if !is_scheme(scheme) {
            return Err(format!("“{scheme}” is not a URI scheme"));
        }
    }
    Ok(Manifest {
        protocol,
        id,
        name,
        version,
        program,
        runs,
        reads,
        uri_schemes,
    })
}

/// Whether `id` is a helper ID: lowercase ASCII letters, digits and `-`, not empty.
pub fn is_helper_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_LEN
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Checks that a helper speaking `protocol` can run in a build that speaks `spoken`, in
/// order. The error says which side needs updating, for the user.
fn check_protocol(protocol: u64, spoken: &[u64]) -> Result<(), String> {
    if spoken.last().is_some_and(|&newest| protocol > newest) {
        return Err(format!(
            "it speaks protocol {protocol}, which needs a newer version of Pindu; this version speaks {}",
            versions(spoken, "and")
        ));
    }
    if !spoken.contains(&protocol) {
        return Err(format!(
            "it speaks protocol {protocol}, which this version of Pindu no longer speaks; ask its author for an update that speaks {}",
            versions(spoken, "or")
        ));
    }
    Ok(())
}

/// The protocol versions `spoken`, for a sentence, the last joined with `conjunction`.
fn versions(spoken: &[u64], conjunction: &str) -> String {
    let mut versions: Vec<String> = spoken.iter().map(u64::to_string).collect();
    let last = versions.pop().unwrap_or_default();
    if versions.is_empty() {
        last
    } else {
        format!("{} {conjunction} {last}", versions.join(", "))
    }
}

/// Whether `relative` names a file inside `folder`, with no way out of it: no absolute
/// path, no `..`, and no link that leads outside.
fn is_file_inside(folder: &Path, relative: &str) -> bool {
    let path = Path::new(relative);
    let plain = path
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    if !plain {
        return false;
    }
    let (Ok(folder), Ok(file)) = (folder.canonicalize(), folder.join(path).canonicalize()) else {
        return false;
    };
    file.starts_with(&folder) && file.is_file()
}

/// Whether `scheme` is a lowercase URI scheme: a letter, then letters, digits, `+`, `-`
/// and `.`.
fn is_scheme(scheme: &str) -> bool {
    let mut bytes = scheme.bytes();
    bytes.next().is_some_and(|first| first.is_ascii_lowercase())
        && bytes.all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || b"+-.".contains(&byte)
        })
}

fn text_field(object: &Map<String, Value>, key: &str) -> Result<String, String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
        .ok_or_else(|| format!("{MANIFEST} has no “{key}”"))
}

fn list_field(object: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    strings(object.get(key)).ok_or_else(|| format!("“{key}” in {MANIFEST} is not a list of text"))
}

fn optional_list_field(object: &Map<String, Value>, key: &str) -> Result<Vec<String>, String> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(Vec::new()),
        value => {
            strings(value).ok_or_else(|| format!("“{key}” in {MANIFEST} is not a list of text"))
        }
    }
}

/// A JSON list of strings; `None` for anything else.
fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_protocol_newer_than_pindu_speaks_asks_for_a_newer_pindu() {
        assert_eq!(
            check_protocol(7, &[1, 2]),
            Err(
                "it speaks protocol 7, which needs a newer version of Pindu; this version speaks 1 and 2"
                    .into()
            )
        );
    }

    #[test]
    fn a_protocol_older_than_pindu_speaks_asks_for_an_update_from_its_author() {
        assert_eq!(
            check_protocol(0, &[1, 2]),
            Err(
                "it speaks protocol 0, which this version of Pindu no longer speaks; ask its author for an update that speaks 1 or 2"
                    .into()
            )
        );
    }

    #[test]
    fn each_protocol_pindu_speaks_runs() {
        assert_eq!(check_protocol(1, &[1, 2]), Ok(()));
        assert_eq!(check_protocol(2, &[1, 2]), Ok(()));
    }

    #[test]
    fn a_build_speaks_at_most_two_versions_oldest_first() {
        assert!((1..=2).contains(&PROTOCOLS.len()), "{PROTOCOLS:?}");
        assert!(PROTOCOLS.is_sorted(), "{PROTOCOLS:?}");
    }

    /// A helper folder holding `example-importer`.
    fn folder() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("example-importer"), "#!/bin/sh\n").unwrap();
        dir
    }

    fn manifest(changes: &[(&str, Value)]) -> String {
        let mut json = serde_json::json!({
            "protocol": 0,
            "id": "example-launcher",
            "name": "Example Launcher",
            "version": "0.3.0",
            "program": ["./example-importer", "--quiet"],
            "runs": "sub-sandbox",
            "reads": ["~/.local/share/example-launcher", "/srv/example"],
            "uri_schemes": ["example"],
        });
        for (key, value) in changes {
            if value.is_null() {
                json.as_object_mut().unwrap().remove(*key);
            } else {
                json[*key] = value.clone();
            }
        }
        json.to_string()
    }

    #[test]
    fn a_complete_manifest_reads_as_written() {
        let dir = folder();
        std::fs::write(dir.path().join(MANIFEST), manifest(&[])).unwrap();

        assert_eq!(
            read_manifest(dir.path()),
            Ok(Manifest {
                protocol: 0,
                id: "example-launcher".into(),
                name: "Example Launcher".into(),
                version: "0.3.0".into(),
                program: vec!["./example-importer".into(), "--quiet".into()],
                runs: Runs::SubSandbox,
                reads: vec![
                    "~/.local/share/example-launcher".into(),
                    "/srv/example".into()
                ],
                uri_schemes: vec!["example".into()],
            })
        );
    }

    #[test]
    fn reads_and_uri_schemes_may_be_left_out_and_host_is_a_place_to_run() {
        let dir = folder();
        let text = manifest(&[
            ("reads", Value::Null),
            ("uri_schemes", Value::Null),
            ("runs", "host".into()),
        ]);

        let manifest = parse_manifest(&text, dir.path()).unwrap();

        assert_eq!(manifest.runs, Runs::Host);
        assert!(manifest.reads.is_empty());
        assert!(manifest.uri_schemes.is_empty());
    }

    #[test]
    fn a_scheme_may_hold_digits_plus_minus_and_dots_after_its_first_letter() {
        let dir = folder();
        let text = manifest(&[("uri_schemes", serde_json::json!(["x-game+run.2"]))]);

        let manifest = parse_manifest(&text, dir.path()).unwrap();

        assert_eq!(manifest.uri_schemes, ["x-game+run.2"]);
    }

    #[test]
    fn a_folder_without_a_manifest_says_so() {
        let dir = folder();

        assert_eq!(
            read_manifest(dir.path()),
            Err("it has no importer.json".into())
        );
    }

    #[test]
    fn a_manifest_that_cannot_be_read_says_so_rather_than_that_it_is_missing() {
        let dir = folder();
        std::fs::create_dir(dir.path().join(MANIFEST)).unwrap();

        let error = read_manifest(dir.path()).unwrap_err();

        assert!(error.starts_with("importer.json cannot be read"), "{error}");
    }

    #[test]
    fn a_protocol_pindu_does_not_speak_names_the_ones_it_does() {
        let dir = folder();

        let error = parse_manifest(&manifest(&[("protocol", 7.into())]), dir.path());

        assert_eq!(
            error,
            Err(
                "it speaks protocol 7, which needs a newer version of Pindu; this version speaks 0"
                    .into()
            )
        );
    }

    #[test]
    fn each_broken_rule_refuses_the_manifest_with_its_reason() {
        let dir = folder();
        std::fs::create_dir(dir.path().join("lib")).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("tool"), "").unwrap();
        std::os::unix::fs::symlink(outside.path().join("tool"), dir.path().join("link")).unwrap();
        let cases: &[(&[(&str, Value)], &str)] = &[
            (&[("protocol", 1.into())], "protocol 1"),
            (&[("protocol", Value::Null)], "no protocol"),
            (&[("id", "Example".into())], "“Example”"),
            (&[("id", "example/launcher".into())], "“example/launcher”"),
            (&[("id", "".into())], "no “id”"),
            (&[("name", Value::Null)], "no “name”"),
            (&[("version", Value::Null)], "no “version”"),
            (&[("program", serde_json::json!([]))], "no program"),
            (&[("program", "./example-importer".into())], "“program”"),
            (
                &[("program", serde_json::json!(["./missing"]))],
                "“./missing”",
            ),
            (&[("program", serde_json::json!(["lib"]))], "“lib”"),
            (&[("program", serde_json::json!(["/bin/sh"]))], "“/bin/sh”"),
            (
                &[("program", serde_json::json!(["../example-importer"]))],
                "“../example-importer”",
            ),
            (&[("program", serde_json::json!(["link"]))], "“link”"),
            (&[("runs", "flatpak".into())], "neither"),
            (&[("runs", Value::Null)], "neither"),
            (&[("reads", serde_json::json!(["games"]))], "“games”"),
            (&[("uri_schemes", serde_json::json!(["https"]))], "“https”"),
            (&[("uri_schemes", serde_json::json!(["file"]))], "“file”"),
            (&[("uri_schemes", serde_json::json!(["http"]))], "“http”"),
            (
                &[("uri_schemes", serde_json::json!(["Example"]))],
                "“Example”",
            ),
            (&[("uri_schemes", serde_json::json!(["2game"]))], "“2game”"),
            (
                &[("uri_schemes", serde_json::json!(["ex_ample"]))],
                "“ex_ample”",
            ),
            (&[("uri_schemes", serde_json::json!([""]))], "“”"),
        ];

        for (changes, reason) in cases {
            let result = parse_manifest(&manifest(changes), dir.path());
            let error = result.expect_err(&format!("{changes:?} was accepted"));
            assert!(error.contains(reason), "{changes:?}: {error}");
        }
        assert!(
            parse_manifest("{", dir.path())
                .unwrap_err()
                .contains("not valid JSON")
        );
        assert!(
            parse_manifest("[]", dir.path())
                .unwrap_err()
                .contains("not a JSON object")
        );
    }
}
