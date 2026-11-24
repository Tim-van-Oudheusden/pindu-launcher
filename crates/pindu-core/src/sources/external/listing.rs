//! What the Sources lists of both shells show for each external importer.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::manifest::Manifest;
use super::runner::{self, Place, Tier};
use super::{Helper, PROTOCOLS};
use crate::model::{ExternalImporterState, Timestamp};

/// Where a listed helper runs, as the user is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Running {
    /// In its own sandbox, seeing only what it reads, without network.
    InItsSandbox,
    /// With the user's permissions: on the host, or as Pindu's child natively.
    WithYourPermissions,
    /// On the host, which needs host access Pindu does not have, so it does not run.
    NeedsHostAccess,
    /// Unknown: its manifest cannot be read.
    Unknown,
}

/// One helper as the Sources lists show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// The helper's ID.
    pub id: String,
    /// Its name: its manifest's, else its ID.
    pub name: String,
    /// Its own version, when its manifest can be read.
    pub version: Option<String>,
    /// Whether it comes with Pindu.
    pub bundled: bool,
    /// Whether the user turned it on.
    pub enabled: bool,
    /// Where it runs.
    pub running: Running,
    /// The folders it reads.
    pub reads: Vec<PathBuf>,
    /// The URI schemes its launches use.
    pub uri_schemes: Vec<String>,
    /// Whether it speaks a protocol that may still change, which the lists mark.
    pub experimental: bool,
    /// Whether it speaks a deprecated protocol, one a later version of Pindu stops
    /// speaking, so it needs an update from its author; the lists mark it.
    pub deprecated: bool,
    /// The folder its last probe found its source in.
    pub found: Option<PathBuf>,
    /// When it was last synced, if ever.
    pub last_sync: Option<Timestamp>,
    /// Why it cannot run, or the problems its last sync ran into.
    pub problems: Vec<String>,
}

/// The helpers Pindu found, in order, with their stored `states`, as they are listed where
/// Pindu runs at `place` with the home folder `home`. A helper with an ID listed before
/// (a user's folder with a bundled helper's ID) is left out, as it never runs, and so is a
/// bundled helper that serves a built-in source ([`super::BUILT_IN`]), which is listed as
/// that source.
pub fn listing(
    helpers: &[Helper],
    states: &[ExternalImporterState],
    place: Place,
    home: &Path,
) -> Vec<Listed> {
    listing_in(helpers, states, place, home, PROTOCOLS)
}

/// [`listing`] in a build that speaks the protocol versions `spoken`, oldest first.
fn listing_in(
    helpers: &[Helper],
    states: &[ExternalImporterState],
    place: Place,
    home: &Path,
    spoken: &[u64],
) -> Vec<Listed> {
    let mut seen = HashSet::new();
    helpers
        .iter()
        .filter(|helper| !(helper.bundled && super::built_in(&helper.id).is_some()))
        .filter(|helper| seen.insert(helper.id.as_str()))
        .map(|helper| {
            let state = states
                .iter()
                .find(|state| state.helper_id == helper.id)
                .cloned()
                .unwrap_or_else(|| ExternalImporterState::new(&helper.id));
            let mut listed = Listed {
                id: helper.id.clone(),
                name: helper.id.clone(),
                version: None,
                bundled: helper.bundled,
                enabled: state.enabled,
                running: Running::Unknown,
                reads: Vec::new(),
                uri_schemes: Vec::new(),
                experimental: true,
                deprecated: false,
                found: state.root,
                last_sync: state.last_sync,
                problems: state.errors,
            };
            match &helper.manifest {
                Ok(manifest) => {
                    listed.name.clone_from(&manifest.name);
                    listed.version = Some(manifest.version.clone());
                    listed.running = running(manifest, place);
                    listed.reads = runner::reads(manifest, home);
                    listed.uri_schemes.clone_from(&manifest.uri_schemes);
                    // Protocol 0 may change in any release, until it is promised as 1.
                    listed.experimental = manifest.protocol == 0;
                    // Older than the newest version spoken: deprecated, by the compatibility
                    // policy. A bundled helper comes with the release and needs no update.
                    listed.deprecated = !helper.bundled
                        && spoken
                            .last()
                            .is_some_and(|&newest| manifest.protocol < newest);
                }
                Err(problem) => listed.problems = vec![problem.clone()],
            }
            listed
        })
        .collect()
}

/// Where the helper of `manifest` runs where Pindu runs at `place`, as the user is told.
pub fn running(manifest: &Manifest, place: Place) -> Running {
    match runner::tier(manifest.runs, place) {
        Ok(Tier::SubSandbox) => Running::InItsSandbox,
        Ok(Tier::Host | Tier::Native) => Running::WithYourPermissions,
        Err(_) => Running::NeedsHostAccess,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::external::manifest::{Manifest, Runs};

    fn helper(id: &str, runs: Runs) -> Helper {
        Helper {
            id: id.into(),
            folder: PathBuf::from("/importers").join(id),
            bundled: false,
            manifest: Ok(Manifest {
                protocol: 0,
                id: id.into(),
                name: format!("{id} launcher"),
                version: "1.2".into(),
                program: vec!["./helper".into()],
                runs,
                reads: vec!["~/.local/share/example".into()],
                uri_schemes: vec!["example".into()],
            }),
        }
    }

    const FLATPAK: Place = Place {
        in_flatpak: true,
        host_spawn: false,
    };

    #[test]
    fn a_helper_on_the_older_of_two_spoken_protocols_is_deprecated_unless_it_is_bundled() {
        let on = |protocol, bundled| {
            let mut helper = helper("alpha", Runs::SubSandbox);
            helper.bundled = bundled;
            helper.manifest.as_mut().unwrap().protocol = protocol;
            listing_in(&[helper], &[], FLATPAK, Path::new("/home/me"), &[1, 2])[0].deprecated
        };

        assert!(on(1, false));
        assert!(!on(2, false));
        assert!(!on(1, true));
    }

    #[test]
    fn a_helper_shows_what_it_reads_and_launches_and_is_experimental_and_off_at_first() {
        let listed = listing(
            &[helper("alpha", Runs::SubSandbox)],
            &[],
            FLATPAK,
            Path::new("/home/me"),
        );

        assert_eq!(
            listed,
            [Listed {
                id: "alpha".into(),
                name: "alpha launcher".into(),
                version: Some("1.2".into()),
                bundled: false,
                enabled: false,
                running: Running::InItsSandbox,
                reads: vec!["/home/me/.local/share/example".into()],
                uri_schemes: vec!["example".into()],
                experimental: true,
                deprecated: false,
                found: None,
                last_sync: None,
                problems: Vec::new(),
            }]
        );
    }

    #[test]
    fn its_stored_state_says_whether_it_is_on_where_it_found_its_source_and_its_problems() {
        let state = ExternalImporterState {
            helper_id: "alpha".into(),
            enabled: true,
            root: Some("/home/me/.local/share/example".into()),
            watch: Vec::new(),
            last_sync: Some(100),
            errors: vec!["Entry 2 is broken".into()],
        };

        let listed = listing(
            &[helper("alpha", Runs::SubSandbox)],
            &[ExternalImporterState::new("beta"), state.clone()],
            FLATPAK,
            Path::new("/home/me"),
        );

        assert!(listed[0].enabled);
        assert_eq!(listed[0].found, state.root);
        assert_eq!(listed[0].last_sync, Some(100));
        assert_eq!(listed[0].problems, state.errors);
    }

    #[test]
    fn a_host_helper_runs_with_your_permissions_or_needs_host_access() {
        let with_host = Place {
            host_spawn: true,
            ..FLATPAK
        };
        let native = Place {
            in_flatpak: false,
            host_spawn: true,
        };
        let home = Path::new("/home/me");
        let running = |runs, place| listing(&[helper("alpha", runs)], &[], place, home)[0].running;

        assert_eq!(running(Runs::Host, FLATPAK), Running::NeedsHostAccess);
        assert_eq!(running(Runs::Host, with_host), Running::WithYourPermissions);
        assert_eq!(
            running(Runs::SubSandbox, native),
            Running::WithYourPermissions
        );
        assert_eq!(running(Runs::Host, native), Running::WithYourPermissions);
    }

    #[test]
    fn a_helper_whose_manifest_cannot_be_read_shows_why_by_its_id() {
        let broken = Helper {
            manifest: Err("it has no importer.json".into()),
            ..helper("alpha", Runs::SubSandbox)
        };
        let mut state = ExternalImporterState::new("alpha");
        state.errors = vec!["an old problem".into()];

        let listed = listing(&[broken], &[state], FLATPAK, Path::new("/home/me"));

        assert_eq!(listed[0].name, "alpha");
        assert_eq!(listed[0].version, None);
        assert_eq!(listed[0].running, Running::Unknown);
        assert!(listed[0].experimental);
        assert_eq!(listed[0].problems, ["it has no importer.json"]);
    }

    #[test]
    fn a_users_helper_with_a_bundled_helpers_id_is_not_listed_again() {
        let bundled = Helper {
            bundled: true,
            ..helper("alpha", Runs::SubSandbox)
        };

        let listed = listing(
            &[
                bundled,
                helper("alpha", Runs::Host),
                helper("beta", Runs::Host),
            ],
            &[],
            FLATPAK,
            Path::new("/home/me"),
        );

        let ids: Vec<_> = listed
            .iter()
            .map(|listed| (listed.id.as_str(), listed.bundled))
            .collect();
        assert_eq!(ids, [("alpha", true), ("beta", false)]);
    }

    #[test]
    fn a_bundled_helper_that_serves_a_built_in_source_is_that_source_not_an_importer() {
        let bottles = Helper {
            bundled: true,
            ..helper("bottles", Runs::SubSandbox)
        };

        let listed = listing(
            &[bottles, helper("beta", Runs::Host)],
            &[],
            FLATPAK,
            Path::new("/home/me"),
        );

        let ids: Vec<_> = listed.iter().map(|listed| listed.id.as_str()).collect();
        assert_eq!(ids, ["beta"]);
    }
}
