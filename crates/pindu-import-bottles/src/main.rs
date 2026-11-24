//! `pindu-import-bottles`: Pindu's Bottles importer as a bundled helper speaking the
//! external importer protocol (`docs/external-importers.md`). Pindu installs it in
//! `<libexecdir>/pindu-launcher/importers/bottles/` beside its `importer.json` and runs it
//! once per request, in its own sub-sandbox in the Flatpak. It reads the request on
//! standard input and writes the answer on standard output; when it cannot answer, it
//! says why on standard error, which goes to Pindu's log, and exits with 1.

use std::io::{Read, Write};
use std::process::ExitCode;

use pindu_core::sources::bottles;
use pindu_core::sources::external::server;

fn main() -> ExitCode {
    let mut request = Vec::new();
    let answered = std::io::stdin()
        .read_to_end(&mut request)
        .map_err(|error| format!("the request could not be read: {error}"))
        .and_then(|_| server::serve(&request, bottles::probe, |root, _| bottles::import(root)))
        .and_then(|answer| {
            std::io::stdout()
                .write_all(answer.as_bytes())
                .map_err(|error| format!("the answer could not be written: {error}"))
        });
    match answered {
        Ok(()) => ExitCode::SUCCESS,
        Err(problem) => {
            eprintln!("pindu-import-bottles: {problem}");
            ExitCode::FAILURE
        }
    }
}
