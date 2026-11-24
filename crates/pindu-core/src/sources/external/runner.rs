//! Where a helper runs, the command that runs it there, and running it once with a
//! request on standard input, a time limit and a size limit on its answer.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use super::manifest::{Manifest, Runs};
use crate::launch::Command;

/// Where a helper runs on this machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// In a sandbox of its own inside the Flatpak (`flatpak-spawn --sandbox`): no network,
    /// and only its own folder and the folders it reads, read-only.
    SubSandbox,
    /// On the host from inside the Flatpak (`flatpak-spawn --host`), with the user's
    /// rights.
    Host,
    /// As Pindu's child, outside any sandbox, with the user's rights: every helper of a
    /// native package.
    Native,
}

impl Tier {
    /// Whether a helper here may describe a command run on the host: one that runs with the
    /// user's rights anyway. A sub-sandboxed helper may not, or a helper trusted with
    /// nothing would get Pindu to run any command.
    pub fn host_launches(self) -> bool {
        self != Self::SubSandbox
    }
}

/// What Pindu can offer a helper where it runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Place {
    /// Whether Pindu runs inside Flatpak.
    pub in_flatpak: bool,
    /// Whether Pindu may run commands on the host (always, outside Flatpak).
    pub host_spawn: bool,
}

/// How long a helper may take to answer, and how large its answer may be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// How long a helper may run before it is killed.
    pub timeout: Duration,
    /// The largest answer accepted, in bytes.
    pub answer_bytes: usize,
}

impl Limits {
    /// The protocol's limits: 30 seconds and 32 MiB.
    pub const PROTOCOL: Self = Self {
        timeout: Duration::from_secs(30),
        answer_bytes: 33_554_432,
    };
}

/// Where a helper that asks to run as `runs` runs here; an error, for the user, when it
/// cannot run.
pub fn tier(runs: Runs, place: Place) -> Result<Tier, String> {
    match (place.in_flatpak, runs) {
        (false, _) => Ok(Tier::Native),
        (true, Runs::SubSandbox) => Ok(Tier::SubSandbox),
        (true, Runs::Host) if place.host_spawn => Ok(Tier::Host),
        (true, Runs::Host) => Err(
            "it runs on the host, which needs host access; Preferences explains how to allow it"
                .to_owned(),
        ),
    }
}

/// The folders a helper's manifest says it reads, with `~/` in front of each read as the
/// home folder `home`.
pub fn reads(manifest: &Manifest, home: &Path) -> Vec<PathBuf> {
    manifest
        .reads
        .iter()
        .map(|read| match read.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => PathBuf::from(read),
        })
        .collect()
}

/// The command that runs the helper in `folder` at `tier`: its program, inside the
/// sub-sandbox with `folder` and `reads` read-only (`flatpak-spawn` refuses a folder that
/// does not exist, so `reads` holds only existing ones), through host spawn, or as a plain
/// child. Through `flatpak-spawn`, `timeout` kills the helper where it runs, as killing
/// `flatpak-spawn` alone would leave it running.
pub fn command(
    folder: &Path,
    manifest: &Manifest,
    tier: Tier,
    reads: &[PathBuf],
    timeout: Duration,
) -> Command {
    let program = manifest
        .program
        .split_first()
        .map(|(first, rest)| {
            std::iter::once(folder.join(first).display().to_string())
                .chain(rest.iter().cloned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let spawn = |options: Vec<String>| {
        let mut argv = vec!["flatpak-spawn".to_owned()];
        argv.extend(options);
        argv.push(format!("--directory={}", folder.display()));
        argv.extend(["--", "timeout", "--signal=KILL"].map(str::to_owned));
        argv.push(format!("{}s", timeout.as_secs().max(1)));
        argv.extend(program.iter().cloned());
        Command {
            argv,
            environment: BTreeMap::new(),
            working_dir: None,
        }
    };
    match tier {
        Tier::SubSandbox => spawn(
            ["--sandbox", "--no-network"]
                .map(str::to_owned)
                .into_iter()
                .chain(
                    std::iter::once(folder)
                        .chain(reads.iter().map(PathBuf::as_path))
                        .map(|path| format!("--sandbox-expose-path-ro={}", path.display())),
                )
                .collect(),
        ),
        Tier::Host => spawn(vec!["--host".to_owned()]),
        Tier::Native => Command {
            argv: program,
            environment: BTreeMap::new(),
            working_dir: Some(folder.to_owned()),
        },
    }
}

/// Runs `command` once with `request` on its standard input, and returns what it wrote to
/// standard output. Its standard error goes to ours, Pindu's log. An error, for the user,
/// when it cannot start, runs longer than the limit, answers more than the limit or exits
/// non-zero.
pub fn run(command: &Command, request: &str, limits: Limits) -> Result<Vec<u8>, String> {
    let deadline = Instant::now() + limits.timeout;
    let (program, arguments) = command.argv.split_first().ok_or("it names no program")?;
    let mut process = std::process::Command::new(program);
    process
        .args(arguments)
        .envs(&command.environment)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if let Some(dir) = &command.working_dir {
        process.current_dir(dir);
    }
    let mut child = retry_busy(|| process.spawn())
        .map_err(|error| format!("its program could not start: {error}"))?;
    // A helper may answer without reading the request; the write then fails, and only
    // this thread waits on it.
    let mut stdin = child.stdin.take().expect("standard input is piped");
    let request = request.as_bytes().to_vec();
    std::thread::spawn(move || stdin.write_all(&request));
    let mut stdout = child.stdout.take().expect("standard output is piped");
    let limit = limits.answer_bytes;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut answer = Vec::new();
        let read = (&mut stdout)
            .take(u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1))
            .read_to_end(&mut answer);
        sender.send(read.map(|_| answer))
    });
    let too_long = || {
        format!(
            "it did not answer within {} seconds",
            limits.timeout.as_secs_f32()
        )
    };
    let answer = match receiver.recv_timeout(limits.timeout) {
        Ok(Ok(answer)) if answer.len() <= limit => answer,
        Ok(Ok(_)) => {
            kill(&mut child);
            return Err(format!("its answer is larger than {}", size(limit)));
        }
        Ok(Err(error)) => {
            kill(&mut child);
            return Err(format!("its answer could not be read: {error}"));
        }
        Err(_) => {
            kill(&mut child);
            return Err(too_long());
        }
    };
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if deadline.saturating_duration_since(Instant::now()).is_zero() => {
                kill(&mut child);
                return Err(too_long());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => return Err(format!("it could not be waited for: {error}")),
        }
    };
    if status.success() {
        Ok(answer)
    } else {
        Err(format!("it could not answer ({status})"))
    }
}

/// Starts a program with `spawn`, trying again for up to 200 ms while it is busy. A
/// program written a moment ago stays busy while another thread's child, forked before
/// the writer closed it, has not yet exec'd; that lasts milliseconds.
fn retry_busy<T>(mut spawn: impl FnMut() -> std::io::Result<T>) -> std::io::Result<T> {
    let mut spawned = spawn();
    for _ in 0..20 {
        match &spawned {
            Err(error) if error.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(Duration::from_millis(10));
                spawned = spawn();
            }
            _ => break,
        }
    }
    spawned
}

/// Kills a helper that broke a limit, and reaps it.
fn kill(child: &mut Child) {
    // It may have exited already; either way nothing is left to do.
    let _ = child.kill();
    let _ = child.wait();
}

/// A byte count as the user reads it: whole MiB where it is one.
fn size(bytes: usize) -> String {
    const MIB: usize = 1024 * 1024;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    fn manifest(runs: Runs) -> Manifest {
        Manifest {
            protocol: 0,
            id: "example-launcher".into(),
            name: "Example Launcher".into(),
            version: "0.3.0".into(),
            program: vec!["./example-importer".into(), "--quiet".into()],
            runs,
            reads: vec![
                "~/.local/share/example-launcher".into(),
                "/srv/example".into(),
            ],
            uri_schemes: Vec::new(),
        }
    }

    const FLATPAK: Place = Place {
        in_flatpak: true,
        host_spawn: false,
    };

    #[test]
    fn a_helper_runs_in_its_sub_sandbox_in_the_flatpak_and_as_a_child_natively() {
        let native = Place {
            in_flatpak: false,
            host_spawn: true,
        };
        let with_host = Place {
            host_spawn: true,
            ..FLATPAK
        };

        assert_eq!(tier(Runs::SubSandbox, FLATPAK), Ok(Tier::SubSandbox));
        assert_eq!(tier(Runs::SubSandbox, with_host), Ok(Tier::SubSandbox));
        assert_eq!(tier(Runs::Host, with_host), Ok(Tier::Host));
        assert_eq!(tier(Runs::SubSandbox, native), Ok(Tier::Native));
        assert_eq!(tier(Runs::Host, native), Ok(Tier::Native));
    }

    #[test]
    fn a_host_helper_without_host_spawn_does_not_run_and_says_it_needs_host_access() {
        let error = tier(Runs::Host, FLATPAK).unwrap_err();

        assert!(error.contains("host access"), "{error}");
    }

    #[test]
    fn only_a_sub_sandboxed_helper_may_not_describe_host_commands() {
        assert!(!Tier::SubSandbox.host_launches());
        assert!(Tier::Host.host_launches());
        assert!(Tier::Native.host_launches());
    }

    #[test]
    fn reads_under_the_home_folder_start_from_it() {
        assert_eq!(
            reads(&manifest(Runs::SubSandbox), Path::new("/home/me")),
            [
                PathBuf::from("/home/me/.local/share/example-launcher"),
                PathBuf::from("/srv/example"),
            ]
        );
    }

    #[test]
    fn a_sub_sandboxed_helper_sees_only_its_folder_and_its_reads_read_only_without_network() {
        let folder = Path::new("/data/pindu-launcher/importers/example-launcher");

        let command = command(
            folder,
            &manifest(Runs::SubSandbox),
            Tier::SubSandbox,
            &["/home/me/.local/share/example-launcher".into()],
            Duration::from_secs(30),
        );

        assert_eq!(
            command.argv,
            [
                "flatpak-spawn",
                "--sandbox",
                "--no-network",
                "--sandbox-expose-path-ro=/data/pindu-launcher/importers/example-launcher",
                "--sandbox-expose-path-ro=/home/me/.local/share/example-launcher",
                "--directory=/data/pindu-launcher/importers/example-launcher",
                "--",
                "timeout",
                "--signal=KILL",
                "30s",
                "/data/pindu-launcher/importers/example-launcher/./example-importer",
                "--quiet",
            ]
        );
        assert_eq!(command.working_dir, None);
    }

    #[test]
    fn a_host_helper_runs_through_host_spawn_in_its_folder() {
        let folder = Path::new("/data/pindu-launcher/importers/example-launcher");

        let command = command(
            folder,
            &manifest(Runs::Host),
            Tier::Host,
            &[],
            Duration::from_secs(30),
        );

        assert_eq!(
            command.argv,
            [
                "flatpak-spawn",
                "--host",
                "--directory=/data/pindu-launcher/importers/example-launcher",
                "--",
                "timeout",
                "--signal=KILL",
                "30s",
                "/data/pindu-launcher/importers/example-launcher/./example-importer",
                "--quiet",
            ]
        );
    }

    #[test]
    fn natively_a_helper_is_a_plain_child_in_its_folder() {
        let folder = Path::new("/home/me/.local/share/pindu-launcher/importers/example-launcher");

        let command = command(
            folder,
            &manifest(Runs::SubSandbox),
            Tier::Native,
            &["/srv/example".into()],
            Duration::from_secs(30),
        );

        assert_eq!(
            command,
            Command {
                argv: vec![
                    folder.join("./example-importer").display().to_string(),
                    "--quiet".into(),
                ],
                environment: BTreeMap::new(),
                working_dir: Some(folder.to_owned()),
            }
        );
    }

    /// A native command running the shell script `body` from a fresh folder.
    fn script(body: &str) -> (tempfile::TempDir, Command) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("helper");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        let command = Command {
            argv: vec![path.display().to_string()],
            environment: BTreeMap::new(),
            working_dir: Some(dir.path().to_owned()),
        };
        (dir, command)
    }

    const QUICK: Limits = Limits {
        timeout: Duration::from_secs(5),
        answer_bytes: 1024,
    };

    #[test]
    fn a_helper_reads_the_request_and_its_answer_comes_back() {
        let (_dir, command) = script("cat");

        assert_eq!(
            run(&command, r#"{"request": "probe"}"#, QUICK),
            Ok(br#"{"request": "probe"}"#.to_vec())
        );
    }

    #[test]
    fn a_helper_that_ignores_the_request_still_answers() {
        let (_dir, command) = script("echo '{}'");

        assert_eq!(
            run(&command, &"x".repeat(1 << 20), QUICK),
            Ok(b"{}\n".to_vec())
        );
    }

    #[test]
    fn a_helper_that_runs_too_long_is_killed() {
        let (_dir, command) = script("sleep 10");
        let limits = Limits {
            timeout: Duration::from_millis(200),
            ..QUICK
        };
        let started = std::time::Instant::now();

        let error = run(&command, "{}", limits).unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.contains("did not answer within"), "{error}");
    }

    #[test]
    fn an_answer_over_the_limit_is_refused() {
        let (_dir, command) = script("yes");
        let started = std::time::Instant::now();

        let error = run(&command, "{}", QUICK).unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.contains("larger than"), "{error}");
    }

    #[test]
    fn an_answer_at_the_limit_is_kept() {
        let (_dir, command) = script("head -c 1024 /dev/zero");

        assert_eq!(
            run(&command, "{}", QUICK).map(|answer| answer.len()),
            Ok(1024)
        );
    }

    #[test]
    fn a_helper_that_exits_with_an_error_could_not_answer() {
        let (_dir, command) = script("echo '{}'; exit 3");

        let error = run(&command, "{}", QUICK).unwrap_err();

        assert!(error.contains('3'), "{error}");
    }

    #[test]
    fn a_program_that_cannot_start_says_so() {
        let command = Command {
            argv: vec!["/nonexistent/helper".into()],
            environment: BTreeMap::new(),
            working_dir: None,
        };

        let error = run(&command, "{}", QUICK).unwrap_err();

        assert!(error.contains("could not start"), "{error}");
    }

    #[test]
    fn a_limit_reads_in_whole_mib_where_it_is_one_and_in_bytes_otherwise() {
        assert_eq!(size(33_554_432), "32 MiB");
        assert_eq!(size(1_048_576), "1 MiB");
        assert_eq!(size(1_048_575), "1048575 bytes");
        assert_eq!(size(1_572_864), "1572864 bytes");
        assert_eq!(size(0), "0 bytes");
    }

    #[test]
    fn a_helper_that_answers_but_keeps_running_is_killed_at_the_deadline() {
        let (dir, command) = script("echo '{}'; exec >&-; echo $$ > pid; exec sleep 10");
        let limits = Limits {
            timeout: Duration::from_millis(300),
            ..QUICK
        };
        let started = std::time::Instant::now();

        let error = run(&command, "{}", limits).unwrap_err();

        assert!(started.elapsed() < Duration::from_secs(5));
        assert!(error.contains("did not answer within"), "{error}");
        let pid = std::fs::read_to_string(dir.path().join("pid")).unwrap();
        assert!(
            !Path::new("/proc").join(pid.trim()).exists(),
            "still running"
        );
    }

    #[test]
    fn a_helper_past_its_time_is_killed_not_left_running() {
        let (dir, command) = script("echo $$ > pid; exec sleep 10");
        let limits = Limits {
            timeout: Duration::from_millis(300),
            ..QUICK
        };

        run(&command, "{}", limits).unwrap_err();

        let pid = std::fs::read_to_string(dir.path().join("pid")).unwrap();
        assert!(
            !Path::new("/proc").join(pid.trim()).exists(),
            "still running"
        );
    }

    #[test]
    fn a_busy_program_is_tried_again_and_any_other_error_is_not() {
        let busy = || std::io::Error::from(std::io::ErrorKind::ExecutableFileBusy);
        let mut calls = 0;
        let started = retry_busy(|| {
            calls += 1;
            if calls < 4 { Err(busy()) } else { Ok(calls) }
        });
        assert_eq!(started.ok(), Some(4));

        let mut calls = 0;
        let always_busy = retry_busy::<()>(|| {
            calls += 1;
            Err(busy())
        });
        assert_eq!(
            always_busy.map_err(|error| error.kind()),
            Err(std::io::ErrorKind::ExecutableFileBusy)
        );
        assert_eq!(calls, 21);

        let mut calls = 0;
        let missing = retry_busy::<()>(|| {
            calls += 1;
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        });
        assert!(missing.is_err());
        assert_eq!(calls, 1);
    }

    #[test]
    fn the_protocols_limits_are_30_seconds_and_32_mib() {
        assert_eq!(Limits::PROTOCOL.timeout, Duration::from_secs(30));
        assert_eq!(Limits::PROTOCOL.answer_bytes, 33_554_432);
    }
}
