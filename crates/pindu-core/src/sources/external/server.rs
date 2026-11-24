//! The helper's side of the protocol, for a built-in importer that runs as a bundled
//! helper ([`super::BUILT_IN`]): answering one request with what the importer found.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value, json};

use super::PROTOCOLS;
use super::protocol::Probe;
use crate::model::{LaunchConfig, LaunchKind};
use crate::sources::{ImportError, ImportedGame, Snapshot};

/// Answers one `request` for a built-in importer: a probe with what `probe` finds among
/// the candidates offered, an import with the snapshot `import` reads from the root it is
/// given (with the request's UTC offset, seconds east of UTC). The error, for the
/// helper's standard error, says why it could not answer: a request it does not
/// understand, or a source it could not read.
pub fn serve(
    request: &[u8],
    probe: impl FnOnce(&[PathBuf]) -> Option<Probe>,
    import: impl FnOnce(&Path, i64) -> Result<Snapshot, ImportError>,
) -> Result<String, String> {
    let request: Value = serde_json::from_slice(request)
        .map_err(|error| format!("the request is not valid JSON: {error}"))?;
    let protocol = request
        .get("protocol")
        .and_then(Value::as_u64)
        .filter(|protocol| PROTOCOLS.contains(protocol))
        .ok_or("the request is not in a protocol this importer speaks")?;
    match request.get("request").and_then(Value::as_str) {
        Some("probe") => {
            let candidates: Vec<PathBuf> = request
                .get("candidates")
                .and_then(Value::as_array)
                .ok_or("the probe offers no candidates")?
                .iter()
                .filter_map(Value::as_str)
                .map(PathBuf::from)
                .collect();
            Ok(probe_answer(protocol, probe(&candidates).as_ref()))
        }
        Some("import") => {
            let root = request
                .get("root")
                .and_then(Value::as_str)
                .ok_or("the import names no root folder")?;
            let utc_offset = request
                .get("utc_offset")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            let snapshot =
                import(Path::new(root), utc_offset).map_err(|error| error.to_string())?;
            Ok(import_answer(protocol, &snapshot))
        }
        _ => Err("the request is neither a probe nor an import".to_owned()),
    }
}

/// The answer to a `probe` request in `protocol`: where the source is, or that it is not
/// on this machine.
pub fn probe_answer(protocol: u64, probe: Option<&Probe>) -> String {
    match probe {
        Some(Probe { root, watch }) => {
            let watch: Vec<String> = watch.iter().map(|path| text(path)).collect();
            json!({"protocol": protocol, "found": true, "root": text(root), "watch": watch})
        }
        None => json!({"protocol": protocol, "found": false}),
    }
    .to_string()
}

/// The answer to an `import` request in `protocol` that lists `snapshot`: its games and
/// its problems written down, which [`super::protocol::parse_import_as`] reads back as
/// the same snapshot. What the protocol cannot carry is left out: a launch inside Pindu's
/// sandbox (Proton, the Steam Linux Runtime) or of a desktop entry is written as its kind,
/// which Pindu leaves out with a problem naming the game, and a command's Proton, prefix
/// and performance settings are not written.
pub fn import_answer(protocol: u64, snapshot: &Snapshot) -> String {
    let games: Vec<Value> = snapshot.games.iter().map(game).collect();
    json!({"protocol": protocol, "games": games, "problems": snapshot.errors}).to_string()
}

/// One game as an entry of the import answer.
fn game(game: &ImportedGame) -> Value {
    let mut entry = Map::new();
    let mut put = |key: &str, value: Value| {
        if !value.is_null() {
            entry.insert(key.to_owned(), value);
        }
    };
    put("native_id", json!(game.native_id));
    put("title", json!(game.title));
    put("installed", json!(game.installed));
    put("launch", launch(game));
    put("developer", json!(game.developer));
    put("description", json!(game.description));
    put(
        "platform",
        json!(game.platform.map(|platform| platform.as_str())),
    );
    put(
        "install_path",
        json!(game.install_path.as_deref().map(text)),
    );
    put("size_bytes", json!(game.size_bytes));
    put("playtime_secs", json!(game.playtime_secs));
    put("last_played_at", json!(game.last_played_at));
    put("genres", json!(game.genres));
    let artwork: Map<String, Value> = game
        .artwork
        .iter()
        .map(|(layout, path)| (layout.as_str().to_owned(), json!(text(path))))
        .collect();
    put("artwork", Value::Object(artwork));
    put(
        "flags",
        json!({
            "hidden": game.source_flags.hidden,
            "favourite": game.source_flags.favourite,
            "categories": game.source_flags.categories,
        }),
    );
    put(
        "service",
        json!(game.service.as_ref().map(|service| json!({
            "service": service.service,
            "service_id": service.service_id,
        }))),
    );
    put("runner", json!(game.runner));
    put("year", json!(game.year));
    Value::Object(entry)
}

/// How the game starts, as the protocol's launch kinds say it.
fn launch(game: &ImportedGame) -> Value {
    match (game.launch_kind, &game.launch_uri, &game.launch_config) {
        (LaunchKind::StoreUri, Some(uri), _) => json!({"kind": "uri", "uri": uri}),
        (LaunchKind::StoreUri, None, _) => json!({"kind": "none"}),
        (LaunchKind::HostCommand, _, Some(config)) => {
            flatpak_run(config).unwrap_or_else(|| host_command(config))
        }
        (kind, ..) => json!({"kind": kind.as_str()}),
    }
}

/// A `flatpak run [--command=<command>] <app ID> <arguments…>` command as a `flatpak`
/// launch; `None` for any other command.
fn flatpak_run(config: &LaunchConfig) -> Option<Value> {
    if config.executable != Path::new("flatpak") || config.working_dir.is_some() {
        return None;
    }
    let mut arguments = config.arguments.iter();
    if arguments.next().map(String::as_str) != Some("run") {
        return None;
    }
    let mut next = arguments.next()?;
    let command = match next.strip_prefix("--command=") {
        Some(command) => {
            next = arguments.next()?;
            Some(command)
        }
        None => None,
    };
    Some(json!({
        "kind": "flatpak",
        "app_id": next,
        "command": command,
        "args": arguments.collect::<Vec<_>>(),
    }))
}

/// A command run on the host as a `host` launch.
fn host_command(config: &LaunchConfig) -> Value {
    let argv: Vec<String> = std::iter::once(text(&config.executable))
        .chain(config.arguments.iter().cloned())
        .collect();
    json!({
        "kind": "host",
        "argv": argv,
        "working_dir": config.working_dir.as_deref().map(text),
        "env": config.environment,
    })
}

/// A path as JSON text; a name that is not UTF-8 is written with replacement characters,
/// which Pindu then cannot find.
fn text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::model::{ArtLayout, LaunchConfig, LaunchKind, Platform, Source};
    use crate::sources::external::manifest::{Manifest, Runs};
    use crate::sources::external::protocol::{
        import_request, parse_import_as, parse_probe, probe_request,
    };
    use crate::sources::{ImportedGame, ServiceRef, Snapshot, SourceFlags};

    fn manifest() -> Manifest {
        Manifest {
            protocol: 0,
            id: "bottles".into(),
            name: "Bottles".into(),
            version: "1.0".into(),
            program: vec!["./pindu-import-bottles".into()],
            runs: Runs::SubSandbox,
            reads: vec!["~/.local/share/bottles".into()],
            uri_schemes: vec!["bottles".into()],
        }
    }

    #[test]
    fn a_probe_offers_the_candidates_and_answers_where_the_source_is() {
        let candidates = vec![
            PathBuf::from("/home/me/.local/share/bottles"),
            PathBuf::from("/home/me/.var/app/com.usebottles.bottles/data/bottles"),
        ];
        let found = Probe {
            root: candidates[1].clone(),
            watch: vec![candidates[1].join("library.yml")],
        };
        let request = probe_request(0, &candidates);

        let answer = serve(
            request.as_bytes(),
            |offered| {
                assert_eq!(offered, candidates);
                Some(found.clone())
            },
            |_, _| unreachable!("a probe imports nothing"),
        )
        .unwrap();

        assert_eq!(
            parse_probe(answer.as_bytes(), &manifest(), &candidates),
            Ok(Some(found))
        );
    }

    #[test]
    fn an_import_answer_written_from_a_snapshot_reads_back_as_that_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let cover = dir.path().join("cover.png");
        let icon = dir.path().join("icon.png");
        std::fs::write(&cover, "png").unwrap();
        std::fs::write(&icon, "png").unwrap();
        let mut complete = ImportedGame::new("1234", "Example Game", LaunchKind::StoreUri);
        complete.installed = true;
        complete.launch_uri = Some("bottles:run/Gaming/Example Game".into());
        complete.developer = Some("Example Studio".into());
        complete.description = Some("A game.".into());
        complete.platform = Some(Platform::Windows);
        complete.install_path = Some("/games/example".into());
        complete.size_bytes = Some(1000);
        complete.playtime_secs = Some(3600);
        complete.last_played_at = Some(1_763_500_000);
        complete.genres = vec!["Action".into()];
        complete.artwork = vec![(ArtLayout::Cover, cover), (ArtLayout::Icon, icon)];
        complete.source_flags = SourceFlags {
            hidden: true,
            favourite: true,
            categories: vec!["Couch".into()],
        };
        complete.service = Some(ServiceRef {
            service: "steam".into(),
            service_id: "220".into(),
        });
        complete.runner = Some("wine".into());
        complete.year = Some(2024);
        let unstartable = ImportedGame::new("5678", "AC/DC Live", LaunchKind::StoreUri);
        let mut command = ImportedGame::new("9", "A Command", LaunchKind::HostCommand);
        command.launch_config = Some(LaunchConfig {
            executable: "/usr/bin/example".into(),
            arguments: vec!["--run".into(), "9".into()],
            working_dir: Some("/games/example".into()),
            environment: BTreeMap::from([("EXAMPLE_MODE".into(), "fast".into())]),
            ..LaunchConfig::default()
        });
        let mut flatpak = ImportedGame::new("10", "A Flatpak", LaunchKind::HostCommand);
        flatpak.launch_config = Some(LaunchConfig {
            executable: "flatpak".into(),
            arguments: vec![
                "run".into(),
                "--command=example-cli".into(),
                "com.example.Launcher".into(),
                "--run".into(),
                "10".into(),
            ],
            ..LaunchConfig::default()
        });
        let snapshot = Snapshot {
            source: Source::Bottles,
            games: vec![complete, unstartable, command, flatpak],
            errors: vec!["“AC/DC Live” cannot be started: its name has a “/”".into()],
        };

        let answer = import_answer(0, &snapshot);

        assert_eq!(
            parse_import_as(answer.as_bytes(), &manifest(), true, Source::Bottles),
            Ok(snapshot)
        );
    }

    #[test]
    fn an_import_reads_the_root_it_is_given_and_answers_its_snapshot() {
        let root = Path::new("/home/me/.local/share/bottles");
        let mut snapshot = Snapshot::new(Source::Bottles);
        let mut game = ImportedGame::new("1234", "Celeste", LaunchKind::StoreUri);
        game.launch_uri = Some("bottles:run/Gaming/Celeste".into());
        snapshot.games.push(game);
        let request = import_request(0, root, 3600);

        let answer = serve(
            request.as_bytes(),
            |_| unreachable!("an import probes nothing"),
            |at, utc_offset| {
                assert_eq!((at, utc_offset), (root, 3600));
                Ok(snapshot.clone())
            },
        );

        assert_eq!(answer, Ok(import_answer(0, &snapshot)));
    }

    #[test]
    fn a_request_it_cannot_answer_or_a_source_it_cannot_read_is_an_error() {
        let unreadable = || {
            Err(ImportError {
                path: "/home/me/.local/share/bottles/library.yml".into(),
                message: "broken".into(),
            })
        };
        for (request, reason) in [
            ("probe".to_owned(), "not valid JSON"),
            (
                json!({"protocol": 1, "request": "probe", "candidates": []}).to_string(),
                "protocol",
            ),
            (
                json!({"protocol": 0, "request": "launch"}).to_string(),
                "neither",
            ),
            (
                json!({"protocol": 0, "request": "probe"}).to_string(),
                "candidates",
            ),
            (
                json!({"protocol": 0, "request": "import"}).to_string(),
                "root",
            ),
            (
                import_request(0, Path::new("/home/me/.local/share/bottles"), 0),
                "/home/me/.local/share/bottles/library.yml: broken",
            ),
        ] {
            let error =
                serve(request.as_bytes(), |_| None, |_, _| unreadable()).expect_err(&request);
            assert!(error.contains(reason), "{request}: {error}");
        }
    }

    #[test]
    fn only_a_plain_flatpak_run_is_written_as_a_flatpak_launch() {
        let flatpak = |arguments: &[&str], working_dir: Option<&str>| LaunchConfig {
            executable: "flatpak".into(),
            arguments: arguments
                .iter()
                .map(|argument| (*argument).to_owned())
                .collect(),
            working_dir: working_dir.map(Into::into),
            ..LaunchConfig::default()
        };
        let kind =
            |config: &LaunchConfig| flatpak_run(config).map(|launch| launch["app_id"].clone());

        assert_eq!(
            kind(&flatpak(&["run", "com.example.App"], None)),
            Some(json!("com.example.App"))
        );
        assert_eq!(
            kind(&flatpak(&["run", "com.example.App"], Some("/games"))),
            None
        );
        assert_eq!(kind(&flatpak(&["install", "com.example.App"], None)), None);
        assert_eq!(kind(&flatpak(&["run", "--command=x"], None)), None);
        let other = LaunchConfig {
            executable: "/usr/bin/flatpak-wrapper".into(),
            ..flatpak(&["run", "com.example.App"], None)
        };
        assert_eq!(kind(&other), None);
    }
}
