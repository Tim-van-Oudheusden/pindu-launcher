//! External importers: helper programs outside Pindu that read a launcher Pindu has no
//! importer for, speaking the protocol in `docs/external-importers.md`.
//!
//! A helper is a folder holding its manifest ([`manifest::MANIFEST`]) and its program.
//! Pindu runs the program once per request, with the request as one JSON document on
//! standard input, and reads one JSON document from standard output: `probe` says whether
//! the source is on this machine, `import` lists its games. Everything a helper returns is
//! data and is checked entry by entry; its games are [`Source::External`] games with
//! native IDs `<helper ID>/<the helper's own ID>`.

pub mod install;
pub mod listing;
pub mod manifest;
pub mod protocol;
pub mod runner;

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use self::manifest::{Manifest, is_helper_id, read_manifest};
use self::runner::{Limits, Place};
use super::Snapshot;
use super::sync::{Changes, SourceSync, apply_snapshot_within};
use crate::model::{ExternalImporterState, Source, Timestamp};
use crate::store::{self, Store};

/// The protocol versions this build speaks. `0` is unstable: it may change in any release
/// until two integrations from outside exist.
pub const PROTOCOLS: &[u64] = &[0];

/// A helper folder Pindu found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Helper {
    /// The helper's ID: its folder's name.
    pub id: String,
    /// The helper's folder.
    pub folder: PathBuf,
    /// Whether it comes with Pindu, in the package, rather than from the user.
    pub bundled: bool,
    /// Its checked manifest, or why it cannot run, for the user.
    pub manifest: Result<Manifest, String>,
}

/// Where helpers live and what Pindu can offer them on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// The user's helpers: `$XDG_DATA_HOME/pindu-launcher/importers`.
    pub installed: PathBuf,
    /// The helpers that come with Pindu, in the package; `None` for none.
    pub bundled: Option<PathBuf>,
    /// Whether Pindu runs in Flatpak and may run commands on the host.
    pub place: Place,
    /// The host's home folder, which `~/` in a manifest's reads stands for.
    pub home: PathBuf,
    /// How long a helper may take and how much it may answer.
    pub limits: Limits,
}

/// What a sync of one helper could not read and how it changed the library.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperSync {
    /// The helper's ID.
    pub helper_id: String,
    /// The helper's name, for the user: its manifest's name, else its ID.
    pub name: String,
    /// Its problems and changes.
    pub sync: SourceSync,
}

/// The helpers in the bundled folder, then the user's, each in order of ID. A folder whose
/// name is not a helper ID is not a helper.
pub fn discover(installed: &Path, bundled: Option<&Path>) -> Vec<Helper> {
    let mut helpers = bundled.map_or_else(Vec::new, |dir| helpers_in(dir, true));
    let bundled_ids: HashSet<String> = helpers.iter().map(|helper| helper.id.clone()).collect();
    for mut helper in helpers_in(installed, false) {
        if bundled_ids.contains(&helper.id) {
            helper.manifest = Err(format!(
                "a helper that comes with Pindu has its ID “{}”",
                helper.id
            ));
        }
        helpers.push(helper);
    }
    helpers
}

/// The helper folders in `dir`, in order of ID.
fn helpers_in(dir: &Path, bundled: bool) -> Vec<Helper> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut helpers: Vec<Helper> = entries
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let id = entry.file_name().into_string().ok()?;
            is_helper_id(&id).then(|| (id, entry.path()))
        })
        .map(|(id, folder)| {
            let manifest = read_manifest(&folder).and_then(|manifest| {
                if manifest.id == id {
                    Ok(manifest)
                } else {
                    Err(format!(
                        "its manifest calls it “{}”, not the name of its folder",
                        manifest.id
                    ))
                }
            });
            Helper {
                id,
                folder,
                bundled,
                manifest,
            }
        })
        .collect();
    helpers.sort_by(|a, b| a.id.cmp(&b.id));
    helpers
}

/// Syncs every helper the user turned on, and records each helper's state: a new helper
/// is stored off, and a helper that cannot run records why. The games of a helper that
/// is gone become missing and its state is forgotten. Returns the helpers that ran or
/// could not run while on.
pub fn sync_helpers(
    store: &Store,
    environment: &Environment,
    now: Timestamp,
    utc_offset: i64,
) -> store::Result<Vec<HelperSync>> {
    let mut synced = Vec::new();
    let mut seen = HashSet::new();
    for helper in discover(&environment.installed, environment.bundled.as_deref()) {
        // A user's folder with a bundled helper's ID never runs; the bundled one owns it.
        if !seen.insert(helper.id.clone()) {
            continue;
        }
        let mut state = store
            .external_importer(&helper.id)?
            .unwrap_or_else(|| ExternalImporterState::new(&helper.id));
        let sync = match &helper.manifest {
            Err(problem) => {
                state.errors = vec![problem.clone()];
                state.enabled.then(|| cannot_run(problem.clone()))
            }
            Ok(_) if !state.enabled => {
                state.errors.clear();
                None
            }
            Ok(manifest) => {
                let sync = sync_helper(
                    store,
                    &helper,
                    manifest,
                    environment,
                    &mut state,
                    now,
                    utc_offset,
                )?;
                state.last_sync = Some(now);
                state.errors.clone_from(&sync.problems);
                Some(sync)
            }
        };
        store.set_external_importer(&state)?;
        if let Some(sync) = sync {
            synced.push(HelperSync {
                name: helper
                    .manifest
                    .as_ref()
                    .map_or_else(|_| helper.id.clone(), |manifest| manifest.name.clone()),
                helper_id: helper.id,
                sync,
            });
        }
    }
    for gone in store.external_importers()? {
        if !seen.contains(&gone.helper_id) {
            let prefix = format!("{}/", gone.helper_id);
            apply_snapshot_within(store, &Snapshot::new(Source::External), now, &prefix)?;
            store.delete_external_importer(&gone.helper_id)?;
        }
    }
    Ok(synced)
}

/// Probes, imports and applies one helper that is on, noting in `state` where it found
/// its source. A helper that cannot run or answer leaves its games as they were.
fn sync_helper(
    store: &Store,
    helper: &Helper,
    manifest: &Manifest,
    environment: &Environment,
    state: &mut ExternalImporterState,
    now: Timestamp,
    utc_offset: i64,
) -> store::Result<SourceSync> {
    let tier = match runner::tier(manifest.runs, environment.place) {
        Ok(tier) => tier,
        Err(problem) => return Ok(cannot_run(problem)),
    };
    let reads = runner::reads(manifest, &environment.home);
    let visible: Vec<PathBuf> = reads.iter().filter(|read| read.exists()).cloned().collect();
    let command = runner::command(
        &helper.folder,
        manifest,
        tier,
        &visible,
        environment.limits.timeout,
    );
    let limits = environment.limits;
    let probe = runner::run(
        &command,
        &protocol::probe_request(manifest.protocol, &reads),
        limits,
    )
    .and_then(|answer| protocol::parse_probe(&answer, manifest, &reads));
    let snapshot = match probe {
        Err(problem) => return Ok(cannot_run(problem)),
        Ok(None) => {
            state.root = None;
            state.watch = reads;
            Snapshot::new(Source::External)
        }
        Ok(Some(probe)) => {
            let request = protocol::import_request(manifest.protocol, &probe.root, utc_offset);
            state.root = Some(probe.root);
            state.watch = probe.watch;
            match runner::run(&command, &request, limits)
                .and_then(|answer| protocol::parse_import(&answer, manifest, tier.host_launches()))
            {
                Ok(snapshot) => snapshot,
                Err(problem) => return Ok(cannot_run(problem)),
            }
        }
    };
    Ok(SourceSync {
        changes: apply_snapshot_within(store, &snapshot, now, &format!("{}/", helper.id))?,
        problems: snapshot.errors,
    })
}

/// The sync of a helper that could not run or answer: its problem, and no change.
fn cannot_run(problem: String) -> SourceSync {
    SourceSync {
        problems: vec![problem],
        changes: Changes::default(),
    }
}

#[cfg(test)]
mod tests;
