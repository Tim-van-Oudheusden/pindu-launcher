use std::os::unix::fs::PermissionsExt;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;
use crate::model::GameId;
use crate::sources::location::HostDirs;
use crate::sources::sync::sync_all;

struct Fixture {
    dir: tempfile::TempDir,
    store: Store,
    environment: Environment,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(&dir.path().join("library.db")).unwrap();
    let environment = Environment {
        installed: dir.path().join("importers"),
        bundled: Some(dir.path().join("bundled")),
        place: Place {
            in_flatpak: false,
            host_spawn: true,
        },
        home: dir.path().to_owned(),
        limits: Limits {
            timeout: Duration::from_secs(5),
            answer_bytes: 1024 * 1024,
        },
    };
    Fixture {
        dir,
        store,
        environment,
    }
}

impl Fixture {
    /// The launcher folder the helper `id` reads, created.
    fn launcher(&self, id: &str) -> PathBuf {
        let folder = self.dir.path().join(format!("launcher-{id}"));
        std::fs::create_dir_all(&folder).unwrap();
        folder
    }

    /// Writes the helper `id` into `parent`: a script answering a probe with `probe` and
    /// an import with `import`, and noting each run in a `runs` file.
    fn helper(&self, parent: &Path, id: &str, probe: &Value, import: &Value) -> PathBuf {
        let manifest = json!({
            "protocol": 0,
            "id": id,
            "name": format!("{id} launcher"),
            "version": "1.0",
            "program": ["./helper"],
            "runs": "sub-sandbox",
            "reads": [format!("~/launcher-{id}")],
            "uri_schemes": ["example"],
        });
        self.helper_with(parent, id, &manifest, probe, import)
    }

    fn helper_with(
        &self,
        parent: &Path,
        id: &str,
        manifest: &Value,
        probe: &Value,
        import: &Value,
    ) -> PathBuf {
        let folder = parent.join(id);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("importer.json"), manifest.to_string()).unwrap();
        answer(&folder, probe, import);
        let script = folder.join("helper");
        std::fs::write(
            &script,
            "#!/bin/sh\n\
             here=$(dirname \"$0\")\n\
             request=$(cat)\n\
             echo ran >> \"$here/runs\"\n\
             case \"$request\" in\n\
             *'\"probe\"'*) cat \"$here/probe.json\" ;;\n\
             *) cat \"$here/import.json\" ;;\n\
             esac\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        folder
    }

    /// An installed helper `id` that finds its launcher and lists `games`.
    fn installed(&self, id: &str, games: Value) -> PathBuf {
        let root = self.launcher(id);
        self.helper(
            &self.environment.installed,
            id,
            &found(&root),
            &json!({"protocol": 0, "games": games}),
        )
    }

    fn turn_on(&self, id: &str) {
        let mut state = self
            .store
            .external_importer(id)
            .unwrap()
            .unwrap_or_else(|| ExternalImporterState::new(id));
        state.enabled = true;
        self.store.set_external_importer(&state).unwrap();
    }

    fn sync(&self, now: Timestamp) -> Vec<HelperSync> {
        sync_helpers(&self.store, &self.environment, now, 0).unwrap()
    }

    fn in_library(&self, id: &str) -> bool {
        self.store
            .game(&id.parse::<GameId>().unwrap())
            .unwrap()
            .is_some_and(|game| game.in_library())
    }
}

/// Rewrites the answers of the helper in `folder`.
fn answer(folder: &Path, probe: &Value, import: &Value) {
    std::fs::write(folder.join("probe.json"), probe.to_string()).unwrap();
    std::fs::write(folder.join("import.json"), import.to_string()).unwrap();
}

fn found(root: &Path) -> Value {
    json!({"protocol": 0, "found": true, "root": root,
           "watch": [root.join("library.json")]})
}

fn game(native_id: &str, title: &str) -> Value {
    json!({"native_id": native_id, "title": title, "installed": true,
           "launch": {"kind": "uri", "uri": format!("example://run/{native_id}")}})
}

fn ran(folder: &Path) -> bool {
    folder.join("runs").exists()
}

#[test]
fn bundled_helpers_come_first_and_each_folder_is_checked() {
    let fixture = fixture();
    let bundled = fixture.environment.bundled.clone().unwrap();
    let installed = &fixture.environment.installed;
    let probe = json!({});
    fixture.helper(&bundled, "bottles", &probe, &probe);
    fixture.helper(installed, "beta", &probe, &probe);
    fixture.helper(installed, "alpha", &probe, &probe);
    fixture.helper(installed, "bottles", &probe, &probe);
    let renamed = fixture.helper(installed, "renamed", &probe, &probe);
    std::fs::rename(&renamed, installed.join("moved")).unwrap();
    std::fs::create_dir(installed.join("Not A Helper")).unwrap();
    std::fs::write(installed.join("notes.txt"), "").unwrap();

    let helpers = discover(installed, Some(&bundled));

    let found: Vec<_> = helpers
        .iter()
        .map(|helper| (helper.id.as_str(), helper.bundled, helper.manifest.is_ok()))
        .collect();
    assert_eq!(
        found,
        [
            ("bottles", true, true),
            ("alpha", false, true),
            ("beta", false, true),
            ("bottles", false, false),
            ("moved", false, false),
        ]
    );
    assert!(
        helpers[3]
            .manifest
            .as_ref()
            .unwrap_err()
            .contains("comes with Pindu")
    );
    assert!(
        helpers[4]
            .manifest
            .as_ref()
            .unwrap_err()
            .contains("“renamed”")
    );
}

#[test]
fn missing_helper_folders_hold_no_helpers() {
    let fixture = fixture();

    assert_eq!(discover(&fixture.environment.installed, None), []);
}

#[test]
fn a_new_helper_is_stored_off_and_does_not_run() {
    let fixture = fixture();
    let folder = fixture.installed("alpha", json!([game("1", "One")]));

    let synced = fixture.sync(100);

    assert_eq!(synced, []);
    assert!(!ran(&folder));
    assert_eq!(
        fixture.store.external_importer("alpha").unwrap(),
        Some(ExternalImporterState::new("alpha"))
    );
}

#[test]
fn a_helper_turned_on_adds_its_games_and_records_where_it_found_them() {
    let fixture = fixture();
    fixture.installed("alpha", json!([game("1", "One")]));
    fixture.turn_on("alpha");

    let synced = fixture.sync(100);

    assert_eq!(synced.len(), 1);
    assert_eq!(synced[0].name, "alpha launcher");
    assert_eq!(
        synced[0].sync.changes.added,
        [GameId::new(Source::External, "alpha/1")]
    );
    let game = fixture
        .store
        .game(&GameId::new(Source::External, "alpha/1"))
        .unwrap()
        .unwrap();
    assert_eq!(game.title, "One");
    assert_eq!(game.launch_uri.as_deref(), Some("example://run/1"));
    let root = fixture.dir.path().join("launcher-alpha");
    assert_eq!(
        fixture.store.external_importer("alpha").unwrap(),
        Some(ExternalImporterState {
            helper_id: "alpha".into(),
            enabled: true,
            root: Some(root.clone()),
            watch: vec![root.join("library.json")],
            last_sync: Some(100),
            errors: Vec::new(),
        })
    );
}

#[test]
fn a_helper_that_fails_leaves_its_games_as_they_were() {
    let fixture = fixture();
    let folder = fixture.installed("alpha", json!([game("1", "One")]));
    fixture.turn_on("alpha");
    fixture.sync(100);
    answer(
        &folder,
        &found(&fixture.launcher("alpha")),
        &json!("not an answer"),
    );

    let synced = fixture.sync(200);

    assert_eq!(synced[0].sync.problems.len(), 1, "{:?}", synced[0].sync);
    assert!(fixture.in_library("external:alpha/1"));
    let state = fixture.store.external_importer("alpha").unwrap().unwrap();
    assert_eq!(state.errors, synced[0].sync.problems);
}

#[test]
fn a_helper_that_finds_nothing_leaves_its_games_missing_and_watches_where_it_would_appear() {
    let fixture = fixture();
    let folder = fixture.installed("alpha", json!([game("1", "One")]));
    fixture.turn_on("alpha");
    fixture.sync(100);
    answer(&folder, &json!({"protocol": 0, "found": false}), &json!({}));

    let synced = fixture.sync(200);

    assert_eq!(
        synced[0].sync.changes.missing,
        [GameId::new(Source::External, "alpha/1")]
    );
    assert!(!fixture.in_library("external:alpha/1"));
    let state = fixture.store.external_importer("alpha").unwrap().unwrap();
    assert_eq!(state.root, None);
    assert_eq!(state.watch, [fixture.dir.path().join("launcher-alpha")]);
}

#[test]
fn one_helpers_sync_leaves_the_other_helpers_games_alone() {
    let fixture = fixture();
    let alpha = fixture.installed("alpha", json!([game("1", "One")]));
    fixture.installed("beta", json!([game("1", "Other One")]));
    fixture.turn_on("alpha");
    fixture.turn_on("beta");
    fixture.sync(100);
    answer(
        &alpha,
        &found(&fixture.launcher("alpha")),
        &json!({"protocol": 0, "games": []}),
    );

    let synced = fixture.sync(200);

    let missing: Vec<_> = synced
        .iter()
        .flat_map(|helper| helper.sync.changes.missing.clone())
        .collect();
    assert_eq!(missing, [GameId::new(Source::External, "alpha/1")]);
    assert!(fixture.in_library("external:beta/1"));
}

#[test]
fn a_broken_manifest_is_a_problem_of_that_helper_only() {
    let fixture = fixture();
    let broken = fixture.installed("alpha", json!([]));
    std::fs::write(broken.join("importer.json"), "{").unwrap();
    fixture.installed("beta", json!([game("1", "One")]));
    fixture.turn_on("alpha");
    fixture.turn_on("beta");

    let synced = fixture.sync(100);

    assert_eq!(synced.len(), 2);
    assert_eq!(synced[0].helper_id, "alpha");
    assert!(synced[0].sync.problems[0].contains("not valid JSON"));
    assert!(!ran(&broken));
    let state = fixture.store.external_importer("alpha").unwrap().unwrap();
    assert_eq!(state.errors, synced[0].sync.problems);
    assert!(fixture.in_library("external:beta/1"));
}

#[test]
fn a_host_helper_without_host_access_does_not_run() {
    let mut fixture = fixture();
    fixture.environment.place = Place {
        in_flatpak: true,
        host_spawn: false,
    };
    let manifest = json!({
        "protocol": 0, "id": "alpha", "name": "Alpha", "version": "1",
        "program": ["./helper"], "runs": "host",
    });
    let folder = fixture.helper_with(
        &fixture.environment.installed,
        "alpha",
        &manifest,
        &json!({}),
        &json!({}),
    );
    fixture.turn_on("alpha");

    let synced = fixture.sync(100);

    assert!(synced[0].sync.problems[0].contains("host access"));
    assert!(!ran(&folder));
}

#[test]
fn a_helper_that_is_gone_leaves_its_games_missing_and_is_forgotten() {
    let fixture = fixture();
    let folder = fixture.installed("alpha", json!([game("1", "One")]));
    fixture.turn_on("alpha");
    fixture.sync(100);
    std::fs::remove_dir_all(folder).unwrap();

    fixture.sync(200);

    let game = fixture
        .store
        .game(&GameId::new(Source::External, "alpha/1"))
        .unwrap()
        .unwrap();
    assert_eq!(game.missing_since, Some(200));
    assert_eq!(fixture.store.external_importer("alpha").unwrap(), None);
}

#[test]
fn a_full_sync_runs_the_helpers_after_the_built_in_sources_and_names_them_in_problems() {
    let fixture = fixture();
    let folder = fixture.installed("alpha", json!([game("1", "One")]));
    answer(
        &folder,
        &found(&fixture.launcher("alpha")),
        &json!({"protocol": 0, "games": [game("1", "One")], "problems": ["Entry 2 is broken"]}),
    );
    fixture.turn_on("alpha");
    let home = fixture.dir.path();
    let dirs = HostDirs {
        home: home.to_owned(),
        data_home: home.join(".local/share"),
        config_home: home.join(".config"),
        host_usr: home.join("usr"),
        system_flatpak: home.join("var/lib/flatpak"),
    };

    let summary = sync_all(&fixture.store, &dirs, &fixture.environment, 100, 0).unwrap();

    assert_eq!(
        summary.problems,
        [(
            Source::External,
            "alpha launcher: Entry 2 is broken".to_owned()
        )]
    );
    assert_eq!(
        summary.added().games,
        [GameId::new(Source::External, "alpha/1")]
    );
}
