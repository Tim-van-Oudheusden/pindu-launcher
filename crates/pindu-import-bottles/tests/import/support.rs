//! What the Bottles tests need here: pindu-core's fixtures, and the importer they run,
//! which is the helper program.

use std::path::Path;

/// Copies pindu-core's `tests/fixtures/<name>` into `dest`, replacing `@ROOT@` with `dest`,
/// as pindu-core's own tests do.
pub(crate) fn fixture_into(name: &str, dest: &Path) {
    let source = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../pindu-core/tests/fixtures")
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

/// The Bottles importer as the `pindu-import-bottles` program, spoken to over the
/// protocol.
pub(crate) mod bottles {
    use std::io::Write;
    use std::path::{Path, PathBuf};
    use std::process::{Command, Stdio};

    use pindu_core::model::Source;
    use pindu_core::sources::external::manifest::{Manifest, parse_manifest};
    use pindu_core::sources::external::protocol::{
        import_request, parse_import_as, parse_probe, probe_request,
    };
    use pindu_core::sources::external::runner::{self, Limits, Tier};
    use pindu_core::sources::{ImportError, Snapshot};

    /// The built program; its folder stands in for the helper's installed folder.
    const PROGRAM: &str = env!("CARGO_BIN_EXE_pindu-import-bottles");

    /// The helper's folder and its manifest as Meson installs it.
    fn helper() -> (PathBuf, Manifest) {
        let folder = Path::new(PROGRAM).parent().unwrap().to_owned();
        let text =
            include_str!("../../importer.json.in").replace("@VERSION@", env!("CARGO_PKG_VERSION"));
        let manifest = parse_manifest(&text, &folder).unwrap();
        (folder, manifest)
    }

    /// Runs the helper once with `request`, as Pindu runs it natively: its answer, or what
    /// it wrote on standard error when it could not answer.
    fn ask(folder: &Path, manifest: &Manifest, request: &str) -> Result<Vec<u8>, String> {
        let command = runner::command(
            folder,
            manifest,
            Tier::Native,
            &[],
            Limits::PROTOCOL.timeout,
        );
        let (program, arguments) = command.argv.split_first().unwrap();
        let mut child = Command::new(program)
            .args(arguments)
            .current_dir(command.working_dir.unwrap())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(request.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        if output.status.success() {
            Ok(output.stdout)
        } else {
            Err(String::from_utf8(output.stderr).unwrap())
        }
    }

    /// Imports the Bottles folder at `root` through the helper: a probe offering `root`,
    /// then an import of the folder it found, read back as from a sub-sandboxed helper. A
    /// folder it cannot read comes back as the importer's error, which the helper writes
    /// on standard error as `pindu-import-bottles: <path>: <message>`.
    pub(crate) fn import(root: &Path) -> Result<Snapshot, ImportError> {
        let (folder, manifest) = helper();
        let candidates = [root.to_owned()];
        let probe = ask(
            &folder,
            &manifest,
            &probe_request(manifest.protocol, &candidates),
        )
        .unwrap();
        let found = parse_probe(&probe, &manifest, &candidates)
            .unwrap()
            .expect("the probe finds the Bottles folder");
        let request = import_request(manifest.protocol, &found.root, 0);
        match ask(&folder, &manifest, &request) {
            Ok(answer) => Ok(parse_import_as(&answer, &manifest, false, Source::Bottles).unwrap()),
            Err(error) => {
                let (path, message) = error
                    .trim_end()
                    .strip_prefix("pindu-import-bottles: ")
                    .and_then(|error| error.split_once(": "))
                    .unwrap_or_else(|| panic!("not the importer's error: {error}"));
                Err(ImportError {
                    path: path.into(),
                    message: message.to_owned(),
                })
            }
        }
    }
}
