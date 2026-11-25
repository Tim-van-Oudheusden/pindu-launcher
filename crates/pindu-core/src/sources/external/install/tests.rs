use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::{Value, json};

use super::*;
use crate::model::{ExternalImporterState, GameId, LaunchKind, Source};
use crate::sources::external::manifest::Runs;
use crate::sources::{ImportedGame, Snapshot};

struct Fixture {
    dir: tempfile::TempDir,
    store: Store,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("pindu-launcher/library.db")).unwrap();
        Self { dir, store }
    }

    /// `$XDG_DATA_HOME/pindu-launcher/importers`, not created.
    fn importers(&self) -> PathBuf {
        self.dir.path().join("pindu-launcher/importers")
    }

    /// The files left beside the importers folder, other than the library.
    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self.dir.path().join("pindu-launcher"))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| !name.starts_with("library.db") && name != "importers")
            .collect()
    }

    /// A fresh folder to build an archive's tree in.
    fn tree(&self, name: &str) -> PathBuf {
        let tree = self.dir.path().join("trees").join(name);
        std::fs::create_dir_all(&tree).unwrap();
        tree
    }

    /// `tree`'s contents as `<name>.tar.gz`, with `options` given to tar before the names.
    fn archive(&self, tree: &Path, name: &str, options: &[&str]) -> PathBuf {
        let archive = self.dir.path().join(format!("{name}.tar.gz"));
        let entries: Vec<String> = std::fs::read_dir(tree)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        let status = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(tree)
            .args(options)
            .args(&entries)
            .status()
            .unwrap();
        assert!(status.success());
        archive
    }

    /// An archive of a valid helper `id` inside one top-level folder, with `changes` to
    /// its manifest.
    fn helper_archive(&self, id: &str, changes: &[(&str, Value)]) -> PathBuf {
        let tree = self.tree(&format!("{id}-{}", changes.len()));
        write_helper(&tree.join(format!("{id}-1.0")), id, changes);
        self.archive(&tree, &format!("{id}-{}", changes.len()), &[])
    }

    /// Prepares `archive` in a build whose one bundled helper, `example-bundled`, serves no
    /// built-in source.
    fn prepare(&self, archive: &Path) -> Result<Prepared, String> {
        prepare(
            archive,
            &self.importers(),
            &["example-bundled".to_owned()],
            LIMITS,
        )
    }
}

const LIMITS: InstallLimits = InstallLimits {
    archive_bytes: 64 * 1024,
    unpacked_bytes: 64 * 1024,
    entries: 20,
};

/// Writes a helper `id` into `folder`: its manifest with `changes`, and its program.
fn write_helper(folder: &Path, id: &str, changes: &[(&str, Value)]) {
    std::fs::create_dir_all(folder).unwrap();
    let mut manifest = json!({
        "protocol": 0, "id": id, "name": "Example Launcher", "version": "1.0",
        "program": ["./helper"], "runs": "sub-sandbox",
        "reads": ["~/.local/share/example"], "uri_schemes": ["example"],
    });
    for (key, value) in changes {
        manifest[*key] = value.clone();
    }
    std::fs::write(folder.join("importer.json"), manifest.to_string()).unwrap();
    let program = folder.join("helper");
    std::fs::write(&program, "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
}

#[test]
fn a_helper_in_one_folder_installs_into_its_id_and_starts_off() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("example-launcher", &[]);

    let prepared = fixture.prepare(&archive).unwrap();
    assert_eq!(prepared.manifest.id, "example-launcher");
    assert_eq!(prepared.manifest.runs, Runs::SubSandbox);
    assert_eq!(prepared.replaces, None);
    install(prepared, &fixture.importers(), &fixture.store).unwrap();

    let folder = fixture.importers().join("example-launcher");
    assert!(folder.join("importer.json").is_file());
    let program = folder.join("helper");
    assert_ne!(program.metadata().unwrap().permissions().mode() & 0o111, 0);
    assert!(
        !fixture
            .store
            .external_importer("example-launcher")
            .unwrap()
            .is_some_and(|state| state.enabled)
    );
    assert_eq!(fixture.leftovers(), Vec::<String>::new());
}

#[test]
fn a_helper_at_the_top_of_the_archive_installs_too() {
    let fixture = Fixture::new();
    let tree = fixture.tree("top");
    write_helper(&tree, "example-launcher", &[]);
    let archive = fixture.archive(&tree, "top", &[]);

    install(
        fixture.prepare(&archive).unwrap(),
        &fixture.importers(),
        &fixture.store,
    )
    .unwrap();

    assert!(
        fixture
            .importers()
            .join("example-launcher/importer.json")
            .is_file()
    );
}

#[test]
fn cancelling_leaves_nothing_behind() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("example-launcher", &[]);

    drop(fixture.prepare(&archive).unwrap());

    assert!(!fixture.importers().join("example-launcher").exists());
    assert_eq!(fixture.leftovers(), Vec::<String>::new());
}

/// Asserts `archive` is refused for a reason containing `reason`, leaving no work folder
/// and no helper behind.
fn assert_refused(fixture: &Fixture, archive: &Path, reason: &str) {
    let error = fixture.prepare(archive).expect_err(reason);
    assert!(error.contains(reason), "{reason}: {error}");
    assert_eq!(fixture.leftovers(), Vec::<String>::new(), "{reason}");
    assert!(
        std::fs::read_dir(fixture.importers()).map_or(true, |mut entries| entries.next().is_none()),
        "{reason}"
    );
}

#[test]
fn a_link_in_the_archive_is_refused() {
    let fixture = Fixture::new();
    let tree = fixture.tree("link");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    std::os::unix::fs::symlink("/etc/passwd", tree.join("helper-1.0/passwd")).unwrap();
    let archive = fixture.archive(&tree, "link", &[]);

    assert_refused(&fixture, &archive, "link");
}

#[test]
fn a_special_file_in_the_archive_is_refused() {
    let fixture = Fixture::new();
    let tree = fixture.tree("fifo");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    let status = Command::new("mkfifo")
        .arg(tree.join("helper-1.0/pipe"))
        .status()
        .unwrap();
    assert!(status.success());
    let archive = fixture.archive(&tree, "fifo", &[]);

    assert_refused(&fixture, &archive, "special file");
}

#[test]
fn a_path_out_of_the_work_folder_is_refused_and_nothing_is_written_there() {
    let fixture = Fixture::new();
    let tree = fixture.tree("dotdot");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    let archive = fixture.archive(
        &tree,
        "dotdot",
        &["--transform", "s,^helper-1.0,../escaped,"],
    );

    assert_refused(&fixture, &archive, "“../escaped");
    assert!(!fixture.dir.path().join("escaped").exists());
}

#[test]
fn an_absolute_path_is_refused() {
    let fixture = Fixture::new();
    let tree = fixture.tree("absolute");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    let archive = fixture.dir.path().join("absolute.tar.gz");
    let status = Command::new("tar")
        .arg("-czPf")
        .arg(&archive)
        .arg(tree.join("helper-1.0"))
        .status()
        .unwrap();
    assert!(status.success());

    assert_refused(&fixture, &archive, "absolute");
}

#[test]
fn an_archive_over_the_limits_is_refused() {
    let fixture = Fixture::new();
    let tree = fixture.tree("large");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    std::fs::write(tree.join("helper-1.0/data"), vec![0; 64 * 1024]).unwrap();
    let large = fixture.archive(&tree, "large", &[]);
    let tree = fixture.tree("many");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    for n in 0..20 {
        std::fs::write(tree.join(format!("helper-1.0/{n}")), "").unwrap();
    }
    let many = fixture.archive(&tree, "many", &[]);
    let tree = fixture.tree("packed");
    write_helper(&tree.join("helper-1.0"), "example-launcher", &[]);
    // Pseudo-random bytes (xorshift) do not compress, so the archive itself is over its
    // limit.
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let noise: Vec<u8> = (0..200_000)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state.to_le_bytes()[0]
        })
        .collect();
    std::fs::write(tree.join("helper-1.0/noise"), noise).unwrap();
    let packed = fixture.archive(&tree, "packed", &[]);

    assert_refused(&fixture, &large, "unpacks to more than");
    assert_refused(&fixture, &many, "more than 20");
    assert_refused(&fixture, &packed, "larger than");
}

#[test]
fn an_archive_at_the_limits_installs() {
    let fixture = Fixture::new();
    let tree = fixture.tree("full");
    write_helper(&tree.join("h"), "example-launcher", &[]);
    let used: u64 = ["importer.json", "helper"]
        .iter()
        .map(|file| tree.join("h").join(file).metadata().unwrap().len())
        .sum();
    let fill = usize::try_from(LIMITS.unpacked_bytes - used).unwrap();
    std::fs::write(tree.join("h/data"), vec![0; fill]).unwrap();
    // The folder, its manifest, program and data: 4 of 20 entries.
    for n in 0..16 {
        std::fs::write(tree.join(format!("h/{n}")), "").unwrap();
    }
    let archive = fixture.archive(&tree, "full", &[]);

    assert!(fixture.prepare(&archive).is_ok());
}

#[test]
fn an_archive_without_one_helper_folder_is_refused() {
    let fixture = Fixture::new();
    let tree = fixture.tree("two");
    write_helper(&tree.join("one"), "example-launcher", &[]);
    write_helper(&tree.join("two"), "example-launcher", &[]);
    let two = fixture.archive(&tree, "two", &[]);
    let tree = fixture.tree("none");
    std::fs::write(tree.join("README"), "").unwrap();
    let none = fixture.archive(&tree, "none", &[]);
    let not_an_archive = fixture.dir.path().join("notes.tar.gz");
    std::fs::write(&not_an_archive, "not gzip").unwrap();

    assert_refused(&fixture, &two, "importer.json");
    assert_refused(&fixture, &none, "importer.json");
    assert_refused(&fixture, &not_an_archive, "cannot be read");
}

#[test]
fn each_kind_of_invalid_manifest_is_refused_with_its_reason() {
    let fixture = Fixture::new();
    let cases: &[(&str, Value, &str)] = &[
        ("protocol", json!(7), "protocol 7"),
        ("id", json!("Example"), "“Example”"),
        ("runs", json!("anywhere"), "neither"),
        ("program", json!(["./missing"]), "“./missing”"),
        ("uri_schemes", json!(["https"]), "“https”"),
        ("uri_schemes", json!(["file"]), "“file”"),
        ("uri_schemes", json!(["http"]), "“http”"),
    ];

    for (key, value, reason) in cases {
        let archive = fixture.helper_archive("example-launcher", &[(key, value.clone())]);
        assert_refused(&fixture, &archive, reason);
    }
    let tree = fixture.tree("broken");
    write_helper(&tree.join("h"), "example-launcher", &[]);
    std::fs::write(tree.join("h/importer.json"), "{").unwrap();
    let broken = fixture.archive(&tree, "broken", &[]);
    assert_refused(&fixture, &broken, "not valid JSON");
}

#[test]
fn a_helper_with_a_bundled_helpers_id_is_refused() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("example-bundled", &[]);

    assert_refused(&fixture, &archive, "comes with Pindu");
}

#[test]
fn a_helper_with_a_built_in_sources_id_is_refused_though_its_helper_is_not_bundled() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("bottles", &[]);

    assert_refused(&fixture, &archive, "comes with Pindu");
}

fn turn_on(store: &Store, id: &str) {
    let mut state = ExternalImporterState::new(id);
    state.enabled = true;
    store.set_external_importer(&state).unwrap();
}

#[test]
fn a_replacement_that_runs_and_reads_the_same_stays_on() {
    let fixture = Fixture::new();
    let first = fixture.helper_archive("example-launcher", &[]);
    install(
        fixture.prepare(&first).unwrap(),
        &fixture.importers(),
        &fixture.store,
    )
    .unwrap();
    turn_on(&fixture.store, "example-launcher");
    let second = fixture.helper_archive("example-launcher", &[("version", json!("2.0"))]);

    let prepared = fixture.prepare(&second).unwrap();
    assert_eq!(
        prepared
            .replaces
            .as_ref()
            .map(|previous| previous.as_ref().map(|m| m.version.clone())),
        Some(Some("1.0".to_owned()))
    );
    install(prepared, &fixture.importers(), &fixture.store).unwrap();

    let manifest =
        std::fs::read_to_string(fixture.importers().join("example-launcher/importer.json"))
            .unwrap();
    assert!(manifest.contains("2.0"));
    assert!(
        fixture
            .store
            .external_importer("example-launcher")
            .unwrap()
            .unwrap()
            .enabled
    );
    assert_eq!(fixture.leftovers(), Vec::<String>::new());
}

#[test]
fn a_replacement_that_runs_reads_or_launches_otherwise_is_turned_off() {
    for (key, value) in [
        ("runs", json!("host")),
        ("reads", json!(["~/.local/share/elsewhere"])),
        ("uri_schemes", json!(["other"])),
    ] {
        let fixture = Fixture::new();
        let first = fixture.helper_archive("example-launcher", &[]);
        install(
            fixture.prepare(&first).unwrap(),
            &fixture.importers(),
            &fixture.store,
        )
        .unwrap();
        turn_on(&fixture.store, "example-launcher");
        let second = fixture.helper_archive("example-launcher", &[(key, value)]);

        install(
            fixture.prepare(&second).unwrap(),
            &fixture.importers(),
            &fixture.store,
        )
        .unwrap();

        assert!(
            !fixture
                .store
                .external_importer("example-launcher")
                .unwrap()
                .unwrap()
                .enabled,
            "{key}"
        );
    }
}

#[test]
fn removing_a_helper_deletes_it_and_its_state_and_leaves_its_games_missing() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("example-launcher", &[]);
    install(
        fixture.prepare(&archive).unwrap(),
        &fixture.importers(),
        &fixture.store,
    )
    .unwrap();
    turn_on(&fixture.store, "example-launcher");
    let mut snapshot = Snapshot::new(Source::External);
    snapshot.games.push(ImportedGame::new(
        "example-launcher/1",
        "One",
        LaunchKind::StoreUri,
    ));
    snapshot
        .games
        .push(ImportedGame::new("other/1", "Other", LaunchKind::StoreUri));
    crate::sources::sync::apply_snapshot(&fixture.store, &snapshot, 100).unwrap();

    remove(
        &fixture.store,
        &fixture.importers(),
        "example-launcher",
        200,
    )
    .unwrap();

    assert!(!fixture.importers().join("example-launcher").exists());
    assert_eq!(
        fixture.store.external_importer("example-launcher").unwrap(),
        None
    );
    let game = |id: &str| {
        fixture
            .store
            .game(&GameId::new(Source::External, id))
            .unwrap()
            .unwrap()
    };
    assert_eq!(game("example-launcher/1").missing_since, Some(200));
    assert_eq!(game("other/1").missing_since, None);
}

#[test]
fn removing_a_helper_that_is_not_installed_says_so() {
    let fixture = Fixture::new();

    let error = remove(&fixture.store, &fixture.importers(), "gone", 100).unwrap_err();

    assert!(error.contains("gone"), "{error}");
}

#[test]
fn an_archive_exactly_at_its_size_limit_is_accepted_and_one_byte_over_is_not() {
    let fixture = Fixture::new();
    let archive = fixture.helper_archive("example-launcher", &[]);
    let length = archive.metadata().unwrap().len();
    let limits = |archive_bytes| InstallLimits {
        archive_bytes,
        ..LIMITS
    };

    assert!(prepare(&archive, &fixture.importers(), &[], limits(length)).is_ok());
    let error = prepare(&archive, &fixture.importers(), &[], limits(length - 1)).unwrap_err();
    assert!(error.contains("larger than"), "{error}");
}

#[test]
fn the_unpacked_tree_is_checked_again_for_links_special_files_and_limits() {
    let fixture = Fixture::new();
    let check = |tree: &Path, limits: InstallLimits| {
        let (mut entries, mut bytes) = (0, 0);
        check_tree(tree, tree, limits, &mut entries, &mut bytes).map(|()| (entries, bytes))
    };
    let tree = fixture.tree("plain");
    std::fs::create_dir(tree.join("folder")).unwrap();
    std::fs::write(tree.join("folder/a"), "12345").unwrap();
    std::fs::write(tree.join("b"), "123").unwrap();
    assert_eq!(check(&tree, LIMITS), Ok((3, 8)));
    let tight = InstallLimits {
        entries: 2,
        ..LIMITS
    };
    assert!(check(&tree, tight).unwrap_err().contains("more than 2"));
    let small = InstallLimits {
        unpacked_bytes: 7,
        ..LIMITS
    };
    assert!(
        check(&tree, small)
            .unwrap_err()
            .contains("more than 7 bytes")
    );

    let linked = fixture.tree("linked");
    std::os::unix::fs::symlink("/etc", linked.join("etc")).unwrap();
    assert!(
        check(&linked, LIMITS)
            .unwrap_err()
            .contains("“etc” in the archive is a link")
    );

    let piped = fixture.tree("piped");
    let status = Command::new("mkfifo")
        .arg(piped.join("pipe"))
        .status()
        .unwrap();
    assert!(status.success());
    assert!(check(&piped, LIMITS).unwrap_err().contains("special file"));
}

#[test]
fn a_single_folder_without_a_manifest_is_not_a_helper() {
    let fixture = Fixture::new();
    let tree = fixture.tree("folder");
    std::fs::create_dir(tree.join("helper-1.0")).unwrap();
    std::fs::write(tree.join("helper-1.0/README"), "").unwrap();
    let archive = fixture.archive(&tree, "folder", &[]);

    let error = fixture.prepare(&archive).unwrap_err();

    assert_eq!(
        error,
        "the archive holds neither importer.json nor one folder with it"
    );
}
