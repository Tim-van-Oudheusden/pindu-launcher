//! The requests Pindu sends a helper and the checks on its answers.
//!
//! Everything a helper returns is data: each entry is checked against the rules of
//! `docs/external-importers.md`, an entry that breaks one is left out with a problem
//! naming it, and no field is run, opened or shown as markup.

use std::collections::{BTreeMap, HashSet};
use std::path::{Component, Path, PathBuf};

use serde_json::{Map, Value, json};

use super::manifest::{FORBIDDEN_SCHEMES, Manifest};
use crate::category;
use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source};
use crate::sources::{ImportedGame, ServiceRef, Snapshot, SourceFlags};

/// What a helper's probe found: the folder to import from, and what to watch below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The source's folder: one of the candidates or below one.
    pub root: PathBuf,
    /// The files and folders below `root` whose change means the source changed.
    pub watch: Vec<PathBuf>,
}

/// The longest native ID a helper may give, in bytes.
const MAX_NATIVE_ID_LEN: usize = 256;

/// The `probe` request, offering the candidate folders.
pub fn probe_request(protocol: u64, candidates: &[PathBuf]) -> String {
    json!({"protocol": protocol, "request": "probe", "candidates": candidates}).to_string()
}

/// The `import` request for the source at `root`; `utc_offset` (seconds east of UTC)
/// lets the helper convert local times.
pub fn import_request(protocol: u64, root: &Path, utc_offset: i64) -> String {
    json!({"protocol": protocol, "request": "import", "root": root, "utc_offset": utc_offset})
        .to_string()
}

/// Checks a probe's `answer`: `None` when the source is not on this machine.
pub fn parse_probe(
    answer: &[u8],
    manifest: &Manifest,
    candidates: &[PathBuf],
) -> Result<Option<Probe>, String> {
    let object = answer_object(answer, manifest)?;
    let found = object
        .get("found")
        .and_then(Value::as_bool)
        .ok_or("its probe did not say whether it found the source")?;
    if !found {
        return Ok(None);
    }
    let root_text = object
        .get("root")
        .and_then(Value::as_str)
        .ok_or("it found the source but named no root folder")?;
    let root = normalize(Path::new(root_text))
        .filter(|root| {
            candidates
                .iter()
                .filter_map(|candidate| normalize(candidate))
                .any(|candidate| root.starts_with(candidate))
        })
        .ok_or_else(|| {
            format!("it found the source at “{root_text}”, outside the folders it reads")
        })?;
    let watch = object
        .get("watch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter_map(|path| normalize(Path::new(path)))
        .filter(|path| path.starts_with(&root))
        .collect();
    Ok(Some(Probe { root, watch }))
}

/// Checks an import's `answer` from the helper of `manifest` into a snapshot of
/// [`Source::External`] games. `host_launches` says whether the helper runs where it may
/// describe a command on the host.
pub fn parse_import(
    answer: &[u8],
    manifest: &Manifest,
    host_launches: bool,
) -> Result<Snapshot, String> {
    let object = answer_object(answer, manifest)?;
    let games = object
        .get("games")
        .and_then(Value::as_array)
        .ok_or("its answer has no list of games")?;
    let mut snapshot = Snapshot::new(Source::External);
    snapshot.errors.extend(
        object
            .get("problems")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned),
    );
    let mut seen = HashSet::new();
    for (index, entry) in games.iter().enumerate() {
        let label = label(entry, index);
        match game(entry, manifest, host_launches) {
            Ok((mut game, problems)) => {
                snapshot.errors.extend(
                    problems
                        .into_iter()
                        .map(|problem| format!("{label}: {problem}")),
                );
                if !seen.insert(game.native_id.clone()) {
                    snapshot.errors.push(format!(
                        "{label}: its native ID “{}” is listed twice",
                        game.native_id
                    ));
                    continue;
                }
                game.native_id = format!("{}/{}", manifest.id, game.native_id);
                snapshot.games.push(game);
            }
            Err(problem) => snapshot.errors.push(format!("{label}: {problem}")),
        }
    }
    Ok(snapshot)
}

/// The answer's JSON object, once it is one in the helper's protocol (which its manifest
/// was checked to name one of [`super::PROTOCOLS`]).
fn answer_object(answer: &[u8], manifest: &Manifest) -> Result<Map<String, Value>, String> {
    let json: Value = serde_json::from_slice(answer)
        .map_err(|error| format!("its answer is not valid JSON: {error}"))?;
    let Value::Object(object) = json else {
        return Err("its answer is not a JSON object".to_owned());
    };
    match object.get("protocol").and_then(Value::as_u64) {
        Some(protocol) if protocol == manifest.protocol => Ok(object),
        Some(protocol) => Err(format!(
            "it answered in protocol {protocol}, and its manifest says {}",
            manifest.protocol
        )),
        None => Err("its answer names no protocol version".to_owned()),
    }
}

/// How a problem names an entry: its title, else its native ID, else its place.
fn label(entry: &Value, index: usize) -> String {
    let text = |key| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .filter(|text| !text.is_empty())
    };
    match text("title").or_else(|| text("native_id")) {
        Some(name) => format!("“{name}”"),
        None => format!("Entry {}", index + 1),
    }
}

/// One entry as a game, with the problems that only dropped part of it (artwork); an
/// error when the entry breaks a rule and is left out.
fn game(
    entry: &Value,
    manifest: &Manifest,
    host_launches: bool,
) -> Result<(ImportedGame, Vec<String>), String> {
    let entry = Entry(entry.as_object().ok_or("it is not a JSON object")?);
    let native_id = entry
        .text("native_id")
        .ok()
        .flatten()
        .filter(|id| !id.is_empty() && id.len() <= MAX_NATIVE_ID_LEN)
        .ok_or("its native ID is not text of 1 to 256 bytes")?;
    let title = entry
        .text("title")?
        .filter(|title| !title.trim().is_empty())
        .ok_or("it has no title")?;
    let installed = entry
        .0
        .get("installed")
        .and_then(Value::as_bool)
        .ok_or("“installed” is not true or false")?;
    let launch = entry
        .0
        .get("launch")
        .and_then(Value::as_object)
        .ok_or("it has no launch")?;
    let (launch_kind, launch_uri, launch_config) = self::launch(launch, manifest, host_launches)?;
    let mut game = ImportedGame::new(native_id, title, launch_kind);
    game.installed = installed;
    game.launch_uri = launch_uri;
    game.launch_config = launch_config;
    game.developer = entry.text("developer")?;
    game.description = entry.text("description")?;
    game.platform = entry
        .0
        .get("platform")
        .and_then(Value::as_str)
        .and_then(|platform| platform.parse::<Platform>().ok());
    game.install_path = entry
        .text("install_path")?
        .map(|path| {
            Some(PathBuf::from(&path))
                .filter(|path| path.is_absolute())
                .ok_or(format!("its install path “{path}” is not absolute"))
        })
        .transpose()?;
    game.size_bytes = entry.count("size_bytes", "its size")?;
    game.playtime_secs = entry.count("playtime_secs", "its playtime")?;
    game.last_played_at = entry.count("last_played_at", "its last played time")?;
    game.genres = entry
        .texts("genres")
        .ok_or("its genres are not a list of text")?
        .into_iter()
        .filter(|genre| category::is_category(genre))
        .collect();
    game.source_flags = flags(entry.0.get("flags")).ok_or("its flags are not valid")?;
    game.service = match entry.0.get("service") {
        None | Some(Value::Null) => None,
        Some(service) => Some(self::service(service).ok_or("its service is not valid")?),
    };
    game.runner = entry.text("runner")?;
    game.year = match entry.0.get("year") {
        None | Some(Value::Null) => None,
        Some(year) => Some(
            year.as_i64()
                .and_then(|year| i32::try_from(year).ok())
                .ok_or("its year is not a whole number")?,
        ),
    };
    let (artwork, problems) = artwork(entry.0.get("artwork"))?;
    game.artwork = artwork;
    Ok((game, problems))
}

/// One entry's JSON object, with typed reads of its optional fields.
struct Entry<'a>(&'a Map<String, Value>);

impl Entry<'_> {
    /// An optional text field; an error when it is there and not text.
    fn text(&self, key: &str) -> Result<Option<String>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(text)) => Ok(Some(text.clone())),
            Some(_) => Err(format!("its {} is not text", key.replace('_', " "))),
        }
    }

    /// An optional whole number, 0 or more, that `what` names.
    fn count(&self, key: &str, what: &str) -> Result<Option<i64>, String> {
        match self.0.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_i64()
                .filter(|count| *count >= 0)
                .map(Some)
                .ok_or(format!("{what} is not a whole number, 0 or more")),
        }
    }

    /// An optional list of text; `None` when it is there and not one.
    fn texts(&self, key: &str) -> Option<Vec<String>> {
        match self.0.get(key) {
            None | Some(Value::Null) => Some(Vec::new()),
            value => strings(value),
        }
    }
}

/// The launch kind, URI and command an entry's `launch` describes.
fn launch(
    launch: &Map<String, Value>,
    manifest: &Manifest,
    host_launches: bool,
) -> Result<(LaunchKind, Option<String>, Option<LaunchConfig>), String> {
    let launch = Entry(launch);
    match launch.0.get("kind").and_then(Value::as_str) {
        Some("uri") => {
            let uri = launch.text("uri")?.ok_or("its launch has no uri")?;
            let scheme = uri
                .split_once(':')
                .map(|(scheme, _)| scheme.to_ascii_lowercase())
                .unwrap_or_default();
            if FORBIDDEN_SCHEMES.contains(&scheme.as_str())
                || !manifest.uri_schemes.contains(&scheme)
            {
                return Err(format!("its URI scheme “{scheme}” is not one it declares"));
            }
            Ok((LaunchKind::StoreUri, Some(uri), None))
        }
        Some("flatpak") => {
            let app_id = launch.text("app_id")?.unwrap_or_default();
            if !is_flatpak_app_id(&app_id) {
                return Err(format!("“{app_id}” is not a Flatpak app ID"));
            }
            let args = launch
                .texts("args")
                .ok_or("“args” of its launch is not a list of text")?;
            let mut arguments = vec!["run".to_owned()];
            if let Some(command) = launch.text("command")? {
                if command.is_empty() || command.starts_with('-') {
                    return Err(format!("its Flatpak command “{command}” is not a command"));
                }
                arguments.push(format!("--command={command}"));
            }
            arguments.push(app_id);
            arguments.extend(args);
            Ok((
                LaunchKind::HostCommand,
                None,
                Some(LaunchConfig {
                    executable: "flatpak".into(),
                    arguments,
                    ..LaunchConfig::default()
                }),
            ))
        }
        Some("host") => {
            if !host_launches {
                return Err(
                    "it asks to run a command on the host, which a helper in its own sandbox \
                     may not describe"
                        .to_owned(),
                );
            }
            let mut argv = launch
                .texts("argv")
                .filter(|argv| !argv.is_empty())
                .ok_or("its command is not a list of text")?
                .into_iter();
            let executable = argv
                .next()
                .map(PathBuf::from)
                .filter(|program| program.is_absolute())
                .ok_or("its command does not start with an absolute program path")?;
            let working_dir = launch
                .text("working_dir")?
                .map(PathBuf::from)
                .map(|dir| {
                    Some(dir)
                        .filter(|dir| dir.is_absolute())
                        .ok_or("its command's folder is not absolute")
                })
                .transpose()?;
            let environment = match launch.0.get("env") {
                None | Some(Value::Null) => BTreeMap::new(),
                Some(env) => env
                    .as_object()
                    .and_then(|env| {
                        env.iter()
                            .map(|(key, value)| Some((key.clone(), value.as_str()?.to_owned())))
                            .collect()
                    })
                    .ok_or("its command's environment is not text")?,
            };
            Ok((
                LaunchKind::HostCommand,
                None,
                Some(LaunchConfig {
                    executable,
                    arguments: argv.collect(),
                    working_dir,
                    environment,
                    ..LaunchConfig::default()
                }),
            ))
        }
        Some(kind) => Err(format!(
            "launch kind “{kind}” is not one a helper may describe"
        )),
        None => Err("its launch names no kind".to_owned()),
    }
}

/// Whether `id` is a Flatpak app ID: three or more dot-separated parts of letters, digits,
/// `_` and `-`, none starting with a digit or `-`.
fn is_flatpak_app_id(id: &str) -> bool {
    id.len() <= 255
        && id.split('.').count() >= 3
        && id.split('.').all(|part| {
            part.bytes()
                .next()
                .is_some_and(|first| first.is_ascii_alphabetic() || first == b'_')
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
        })
}

fn flags(value: Option<&Value>) -> Option<SourceFlags> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Some(SourceFlags::default());
    };
    let flags = Entry(value.as_object()?);
    let flag = |key| match flags.0.get(key) {
        None | Some(Value::Null) => Some(false),
        Some(value) => value.as_bool(),
    };
    Some(SourceFlags {
        hidden: flag("hidden")?,
        favourite: flag("favourite")?,
        categories: flags.texts("categories")?,
    })
}

fn service(value: &Value) -> Option<ServiceRef> {
    let text = |key| {
        value
            .get(key)?
            .as_str()
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned)
    };
    Some(ServiceRef {
        service: text("service")?,
        service_id: text("service_id")?,
    })
}

/// The artwork Pindu can read, and a problem for each image it cannot.
#[expect(
    clippy::type_complexity,
    reason = "a pair of the kept images and the problems, used once"
)]
fn artwork(value: Option<&Value>) -> Result<(Vec<(ArtLayout, PathBuf)>, Vec<String>), String> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok((Vec::new(), Vec::new()));
    };
    let images = value.as_object().ok_or("its artwork is not an object")?;
    let mut artwork = Vec::new();
    let mut problems = Vec::new();
    for &layout in ArtLayout::ALL {
        let Some(image) = images.get(layout.as_str()) else {
            continue;
        };
        match image.as_str().map(PathBuf::from) {
            Some(path) if path.is_absolute() && path.is_file() => artwork.push((layout, path)),
            _ => problems.push(format!(
                "its {layout} image {image} is not a file Pindu can read"
            )),
        }
    }
    Ok((artwork, problems))
}

/// A JSON list of strings; `None` for anything else.
fn strings(value: Option<&Value>) -> Option<Vec<String>> {
    value?
        .as_array()?
        .iter()
        .map(|item| item.as_str().map(str::to_owned))
        .collect()
}

/// `path` with `.` and `..` worked out, without reading the file system; `None` unless it
/// is absolute.
fn normalize(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normal = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::Normal(part) => normal.push(part),
            Component::ParentDir => {
                normal.pop();
            }
            Component::RootDir | Component::CurDir | Component::Prefix(_) => {}
        }
    }
    Some(normal)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use serde_json::{Value, json};

    use super::*;
    use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source};
    use crate::sources::external::manifest::Runs;
    use crate::sources::{ImportedGame, ServiceRef, SourceFlags};

    fn manifest() -> Manifest {
        Manifest {
            protocol: 0,
            id: "example-launcher".into(),
            name: "Example Launcher".into(),
            version: "0.3.0".into(),
            program: vec!["./example-importer".into()],
            runs: Runs::SubSandbox,
            reads: vec!["~/.local/share/example-launcher".into()],
            uri_schemes: vec!["example".into()],
        }
    }

    fn answer(games: Value) -> Vec<u8> {
        json!({"protocol": 0, "games": games, "problems": []})
            .to_string()
            .into_bytes()
    }

    fn game(changes: Value) -> Value {
        let mut game = json!({
            "native_id": "1234",
            "title": "Example Game",
            "installed": true,
            "launch": {"kind": "uri", "uri": "example://run/1234"},
        });
        for (key, value) in changes.as_object().unwrap() {
            if value.is_null() {
                game.as_object_mut().unwrap().remove(key);
            } else {
                game[key] = value.clone();
            }
        }
        game
    }

    /// The snapshot of one game's answer, from a sub-sandboxed helper.
    fn import_one(changes: Value) -> Snapshot {
        parse_import(&answer(json!([game(changes)])), &manifest(), false).unwrap()
    }

    #[test]
    fn requests_are_one_json_document_naming_the_protocol() {
        let probe: Value = serde_json::from_str(&probe_request(
            0,
            &["/home/me/.local/share/example-launcher".into()],
        ))
        .unwrap();
        let import: Value = serde_json::from_str(&import_request(
            0,
            Path::new("/home/me/.local/share/example-launcher"),
            3600,
        ))
        .unwrap();

        assert_eq!(
            probe,
            json!({"protocol": 0, "request": "probe",
                   "candidates": ["/home/me/.local/share/example-launcher"]})
        );
        assert_eq!(
            import,
            json!({"protocol": 0, "request": "import",
                   "root": "/home/me/.local/share/example-launcher", "utc_offset": 3600})
        );
    }

    #[test]
    fn a_probe_finds_a_root_below_a_candidate_and_only_watches_below_it() {
        let candidates = vec![PathBuf::from("/home/me/.local/share/example-launcher")];
        let text = json!({
            "protocol": 0, "found": true,
            "root": "/home/me/.local/share/example-launcher/main",
            "watch": [
                "/home/me/.local/share/example-launcher/main/library.json",
                "/home/me/.config/elsewhere.json",
                "/home/me/.local/share/example-launcher/main/../../elsewhere",
            ],
        });

        let probe = parse_probe(text.to_string().as_bytes(), &manifest(), &candidates);

        assert_eq!(
            probe,
            Ok(Some(Probe {
                root: "/home/me/.local/share/example-launcher/main".into(),
                watch: vec!["/home/me/.local/share/example-launcher/main/library.json".into()],
            }))
        );
    }

    #[test]
    fn a_probe_that_finds_nothing_is_a_source_not_on_this_machine() {
        let text = json!({"protocol": 0, "found": false}).to_string();

        assert_eq!(parse_probe(text.as_bytes(), &manifest(), &[]), Ok(None));
    }

    #[test]
    fn a_probe_root_outside_the_candidates_or_a_broken_answer_is_a_problem() {
        let candidates = vec![PathBuf::from("/home/me/.local/share/example-launcher")];
        for (text, reason) in [
            (
                json!({"protocol": 0, "found": true, "root": "/etc"}),
                "“/etc”",
            ),
            (
                json!({"protocol": 0, "found": true,
                       "root": "/home/me/.local/share/example-launcher/../../.ssh"}),
                ".ssh",
            ),
            (json!({"protocol": 0, "found": true}), "no root"),
            (json!({"protocol": 1, "found": false}), "protocol 1"),
            (json!({"protocol": 0}), "whether"),
            (json!([]), "not a JSON object"),
        ] {
            let error = parse_probe(text.to_string().as_bytes(), &manifest(), &candidates)
                .expect_err(&text.to_string());
            assert!(error.contains(reason), "{text}: {error}");
        }
        let error = parse_probe(b"found", &manifest(), &candidates).unwrap_err();
        assert!(error.contains("not valid JSON"), "{error}");
    }

    #[test]
    fn a_complete_entry_becomes_an_external_game_with_the_helpers_id_in_front() {
        let dir = tempfile::tempdir().unwrap();
        let cover = dir.path().join("1234.jpg");
        std::fs::write(&cover, "jpg").unwrap();

        let snapshot = import_one(json!({
            "developer": "Example Studio",
            "description": "A game.",
            "platform": "linux",
            "install_path": "/games/example",
            "size_bytes": 1000,
            "playtime_secs": 3600,
            "last_played_at": 1_763_500_000,
            "genres": ["Action", "Roguelite"],
            "artwork": {"cover": cover},
            "flags": {"hidden": true, "favourite": true, "categories": ["Couch"]},
            "service": {"service": "steam", "service_id": "220"},
            "runner": "linux",
            "year": 2024,
            "unknown_field": "is ignored",
        }));

        let mut expected = ImportedGame::new(
            "example-launcher/1234",
            "Example Game",
            LaunchKind::StoreUri,
        );
        expected.installed = true;
        expected.launch_uri = Some("example://run/1234".into());
        expected.developer = Some("Example Studio".into());
        expected.description = Some("A game.".into());
        expected.platform = Some(Platform::Linux);
        expected.install_path = Some("/games/example".into());
        expected.size_bytes = Some(1000);
        expected.playtime_secs = Some(3600);
        expected.last_played_at = Some(1_763_500_000);
        expected.genres = vec!["Action".into()];
        expected.artwork = vec![(ArtLayout::Cover, cover)];
        expected.source_flags = SourceFlags {
            hidden: true,
            favourite: true,
            categories: vec!["Couch".into()],
        };
        expected.service = Some(ServiceRef {
            service: "steam".into(),
            service_id: "220".into(),
        });
        expected.runner = Some("linux".into());
        expected.year = Some(2024);
        assert_eq!(snapshot.source, Source::External);
        assert_eq!(snapshot.games, vec![expected]);
        assert_eq!(snapshot.errors, Vec::<String>::new());
    }

    #[test]
    fn the_helpers_own_problems_are_kept() {
        let text = json!({"protocol": 0, "games": [],
                          "problems": ["Entry 17 in library.json has no title"]});

        let snapshot = parse_import(text.to_string().as_bytes(), &manifest(), false).unwrap();

        assert_eq!(snapshot.errors, ["Entry 17 in library.json has no title"]);
    }

    #[test]
    fn an_unknown_platform_is_unknown_rather_than_a_problem() {
        let snapshot = import_one(json!({"platform": "macos"}));

        assert_eq!(snapshot.games[0].platform, None);
        assert!(snapshot.errors.is_empty());
    }

    #[test]
    fn a_flatpak_launch_runs_flatpak_run_with_the_app_and_its_arguments() {
        let snapshot = import_one(json!({"launch": {
            "kind": "flatpak", "app_id": "com.example.Launcher",
            "command": "example-cli", "args": ["--run", "1234"],
        }}));

        let game = &snapshot.games[0];
        assert_eq!(game.launch_kind, LaunchKind::HostCommand);
        assert_eq!(
            game.launch_config,
            Some(LaunchConfig {
                executable: "flatpak".into(),
                arguments: vec![
                    "run".into(),
                    "--command=example-cli".into(),
                    "com.example.Launcher".into(),
                    "--run".into(),
                    "1234".into(),
                ],
                ..LaunchConfig::default()
            })
        );
    }

    #[test]
    fn a_host_launch_from_a_helper_that_may_describe_one_keeps_its_command() {
        let entry = game(json!({"launch": {
            "kind": "host", "argv": ["/usr/bin/example", "--run", "1234"],
            "working_dir": "/games/example", "env": {"EXAMPLE_MODE": "fast"},
        }}));

        let snapshot = parse_import(&answer(json!([entry])), &manifest(), true).unwrap();

        let game = &snapshot.games[0];
        assert_eq!(game.launch_kind, LaunchKind::HostCommand);
        assert_eq!(
            game.launch_config,
            Some(LaunchConfig {
                executable: "/usr/bin/example".into(),
                arguments: vec!["--run".into(), "1234".into()],
                working_dir: Some("/games/example".into()),
                environment: BTreeMap::from([("EXAMPLE_MODE".into(), "fast".into())]),
                ..LaunchConfig::default()
            })
        );
    }

    #[test]
    fn a_host_launch_from_a_sub_sandboxed_helper_is_dropped_naming_the_game() {
        let snapshot =
            import_one(json!({"launch": {"kind": "host", "argv": ["/usr/bin/example"]}}));

        assert!(snapshot.games.is_empty());
        assert_eq!(snapshot.errors.len(), 1);
        assert!(
            snapshot.errors[0].contains("Example Game"),
            "{:?}",
            snapshot.errors
        );
    }

    #[test]
    fn an_entry_that_breaks_a_rule_is_left_out_with_a_problem_naming_it() {
        let cases = [
            (json!({"native_id": ""}), "native ID"),
            (json!({"native_id": "x".repeat(257)}), "native ID"),
            (json!({"native_id": 1234}), "native ID"),
            (json!({"title": ""}), "title"),
            (json!({"title": null}), "title"),
            (json!({"installed": "yes"}), "installed"),
            (json!({"launch": null}), "launch"),
            (json!({"launch": {"kind": "proton"}}), "proton"),
            (
                json!({"launch": {"kind": "runtime-native"}}),
                "runtime-native",
            ),
            (
                json!({"launch": {"kind": "uri", "uri": "other://run/1"}}),
                "other",
            ),
            (
                json!({"launch": {"kind": "uri", "uri": "https://example.com"}}),
                "https",
            ),
            (
                json!({"launch": {"kind": "uri", "uri": "file:///bin/sh"}}),
                "file",
            ),
            (json!({"launch": {"kind": "uri"}}), "uri"),
            (
                json!({"launch": {"kind": "flatpak", "app_id": "--filesystem=host"}}),
                "app",
            ),
            (
                json!({"launch": {"kind": "flatpak", "app_id": "example"}}),
                "app",
            ),
            (
                json!({"launch": {"kind": "flatpak", "app_id": "com.example.App",
                                  "command": "--talk-name=x"}}),
                "command",
            ),
            (
                json!({"launch": {"kind": "flatpak", "app_id": "com.example.App",
                                  "args": "--run"}}),
                "args",
            ),
            (json!({"install_path": "games/example"}), "install path"),
            (json!({"size_bytes": -1}), "size"),
            (json!({"playtime_secs": -1}), "playtime"),
            (json!({"playtime_secs": 1.5}), "playtime"),
            (json!({"last_played_at": "yesterday"}), "last played"),
            (json!({"developer": 7}), "developer"),
            (json!({"description": []}), "description"),
            (json!({"genres": "Action"}), "genres"),
            (json!({"flags": {"hidden": "no"}}), "flags"),
            (json!({"service": {"service": "steam"}}), "service"),
            (json!({"runner": 1}), "runner"),
            (json!({"year": "2024"}), "year"),
            (json!({"artwork": ["cover"]}), "artwork"),
        ];

        for (changes, reason) in cases {
            let snapshot = import_one(changes.clone());
            assert!(snapshot.games.is_empty(), "{changes} was accepted");
            assert_eq!(snapshot.errors.len(), 1, "{changes}");
            assert!(
                snapshot.errors[0].contains(reason),
                "{changes}: {}",
                snapshot.errors[0]
            );
        }
    }

    #[test]
    fn host_argv_must_start_with_an_absolute_program_and_run_in_an_absolute_folder() {
        for launch in [
            json!({"kind": "host", "argv": []}),
            json!({"kind": "host", "argv": ["example"]}),
            json!({"kind": "host", "argv": ["/usr/bin/example"], "working_dir": "games"}),
            json!({"kind": "host", "argv": ["/usr/bin/example"], "env": {"A": 1}}),
            json!({"kind": "host", "argv": "/usr/bin/example --run"}),
        ] {
            let entry = game(json!({ "launch": launch.clone() }));
            let snapshot = parse_import(&answer(json!([entry])), &manifest(), true).unwrap();
            assert!(snapshot.games.is_empty(), "{launch} was accepted");
            assert_eq!(snapshot.errors.len(), 1, "{launch}");
        }
    }

    #[test]
    fn a_second_entry_with_the_same_native_id_is_left_out() {
        let games = json!([game(json!({})), game(json!({"title": "Again"}))]);

        let snapshot = parse_import(&answer(games), &manifest(), false).unwrap();

        assert_eq!(snapshot.games.len(), 1);
        assert_eq!(snapshot.games[0].title, "Example Game");
        assert!(snapshot.errors[0].contains("1234"), "{:?}", snapshot.errors);
    }

    #[test]
    fn artwork_pindu_cannot_read_is_dropped_and_the_game_stays() {
        let snapshot = import_one(json!({"artwork": {
            "cover": "/nowhere/cover.jpg", "hero": "relative.jpg", "banner": "/x.jpg",
        }}));

        assert_eq!(snapshot.games.len(), 1);
        assert!(snapshot.games[0].artwork.is_empty());
        assert_eq!(snapshot.errors.len(), 2, "{:?}", snapshot.errors);
    }

    #[test]
    fn an_import_answer_without_games_or_in_another_protocol_is_a_problem() {
        for (text, reason) in [
            (json!({"protocol": 0}), "games"),
            (json!({"protocol": 0, "games": {}}), "games"),
            (json!({"protocol": 1, "games": []}), "protocol 1"),
            (json!({"games": []}), "protocol"),
        ] {
            let error = parse_import(text.to_string().as_bytes(), &manifest(), false)
                .expect_err(&text.to_string());
            assert!(error.contains(reason), "{text}: {error}");
        }
    }

    #[test]
    fn a_flatpak_app_id_is_three_or_more_parts_of_letters_digits_underscores_and_dashes() {
        for id in [
            "com.example.App",
            "org._private.app-name2",
            "a.b.c.d",
            "com.ex_ample.a-b",
        ] {
            assert!(is_flatpak_app_id(id), "{id} was refused");
        }
        for id in [
            "com.example",
            "com.example.1app",
            "com.example.-app",
            "com..app",
            "com.ex ample.app",
            "com.example.app$",
            "com.exa+mple.app",
            "--filesystem=host",
        ] {
            assert!(!is_flatpak_app_id(id), "{id} was accepted");
        }
        assert!(is_flatpak_app_id(&format!("a.b.{}", "c".repeat(251))));
        assert!(!is_flatpak_app_id(&format!("a.b.{}", "c".repeat(252))));
    }
}
