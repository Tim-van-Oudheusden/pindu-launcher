//! logind on the host through `busctl`, for the Flatpak: its sandbox has no system bus, but
//! with the host-spawn override it may run `busctl --system call` on the host. These are
//! the commands, as argument lists, and what their output means; the UI's host backend
//! runs them through `flatpak-spawn --host`. `--json=short` makes the answers JSON, so a
//! reason or a program's name with quotes or spaces in it reads back as it was.

use std::collections::BTreeMap;

use serde_json::Value;

use super::{Facts, Inhibitor, PowerAction};
use crate::launch::Command;
use crate::system::Permission;

/// What a failed call writes before logind's reason.
const CALL_FAILED: &str = "Call failed:";

/// The command that asks logind whether the session may do `action` (`CanSuspend`,
/// `CanReboot` or `CanPowerOff`).
pub fn can_command(action: PowerAction) -> Command {
    let method = match action {
        PowerAction::Suspend => "CanSuspend",
        PowerAction::Restart => "CanReboot",
        PowerAction::PowerOff => "CanPowerOff",
    };
    manager_call(method, &[])
}

/// The command that asks logind to do `action` (`Suspend`, `Reboot` or `PowerOff`), with
/// `interactive` on, so polkit may ask where the session has an agent for it.
pub fn run_command(action: PowerAction) -> Command {
    let method = match action {
        PowerAction::Suspend => "Suspend",
        PowerAction::Restart => "Reboot",
        PowerAction::PowerOff => "PowerOff",
    };
    manager_call(method, &["b", "true"])
}

/// The command that lists logind's inhibitors (`ListInhibitors`).
pub fn inhibitors_command() -> Command {
    manager_call("ListInhibitors", &[])
}

/// `busctl` calling `method` of logind's manager on the system bus with `arguments`, its
/// signature first.
fn manager_call(method: &str, arguments: &[&str]) -> Command {
    let call = [
        "busctl",
        "--system",
        "--json=short",
        "call",
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
        method,
    ];
    Command {
        argv: call
            .iter()
            .chain(arguments)
            .map(|arg| (*arg).to_owned())
            .collect(),
        environment: BTreeMap::new(),
        working_dir: None,
    }
}

/// What a `Can*` call answered, from its output; `None` when it is not an answer.
pub fn parse_answer(stdout: &str) -> Option<Permission> {
    let reply: Value = serde_json::from_str(stdout).ok()?;
    if reply.get("type")?.as_str()? != "s" {
        return None;
    }
    let answer = reply.get("data")?.get(0)?.as_str()?;
    Some(Permission::parse(answer))
}

/// What logind allows, from the answers to `CanSuspend`, `CanReboot` and `CanPowerOff`
/// in [`PowerAction::ALL`]'s order, each `None` where the call failed. A call that failed
/// counts as `na`, which leaves its entry out, and with every call failed logind is taken
/// not to be there.
pub fn facts(answers: [Option<Permission>; 3]) -> Facts {
    let reached = answers.iter().any(Option::is_some);
    let [can_suspend, can_reboot, can_power_off] =
        answers.map(|answer| answer.unwrap_or(Permission::Na));
    Facts {
        system_bus: reached,
        logind: reached,
        can_suspend,
        can_reboot,
        can_power_off,
        sandboxed: false,
    }
}

/// The inhibitors `ListInhibitors` listed, from its output; none when it cannot be read.
pub fn parse_inhibitors(stdout: &str) -> Vec<Inhibitor> {
    let Ok(reply) = serde_json::from_str::<Value>(stdout) else {
        return Vec::new();
    };
    let Some(listed) = reply
        .get("data")
        .and_then(|data| data.get(0))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    listed
        .iter()
        .filter_map(|inhibitor| {
            let field = |index: usize| Some(inhibitor.get(index)?.as_str()?.to_owned());
            Some(Inhibitor {
                what: field(0)?,
                who: field(1)?,
                why: field(2)?,
                mode: field(3)?,
            })
        })
        .collect()
}

/// Why a call failed, from its errors (`stderr`) and its exit `status`: logind's reason
/// where busctl gives one, else the last line written, else the status.
pub fn error_message(stderr: &str, status: i32) -> String {
    let mut lines = stderr
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty());
    if let Some(reason) = lines
        .clone()
        .find_map(|line| line.strip_prefix(CALL_FAILED))
    {
        return reason.trim().to_owned();
    }
    match lines.next_back() {
        Some(line) => line.to_owned(),
        None => format!("busctl exited with status {status}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANAGER: [&str; 7] = [
        "busctl",
        "--system",
        "--json=short",
        "call",
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    ];

    fn argv(method: &str, arguments: &[&str]) -> Vec<String> {
        MANAGER
            .iter()
            .chain([method].iter())
            .chain(arguments)
            .map(|arg| (*arg).to_owned())
            .collect()
    }

    #[test]
    fn each_action_asks_and_runs_its_logind_method_as_an_argument_list() {
        for (action, can, run) in [
            (PowerAction::Suspend, "CanSuspend", "Suspend"),
            (PowerAction::Restart, "CanReboot", "Reboot"),
            (PowerAction::PowerOff, "CanPowerOff", "PowerOff"),
        ] {
            let asked = can_command(action);
            assert_eq!(asked.argv, argv(can, &[]), "{action:?}");
            assert!(asked.environment.is_empty());
            assert_eq!(asked.working_dir, None);
            assert_eq!(
                run_command(action).argv,
                argv(run, &["b", "true"]),
                "{action:?}"
            );
        }
        assert_eq!(inhibitors_command().argv, argv("ListInhibitors", &[]));
    }

    #[test]
    fn an_answer_reads_as_logind_s_permission() {
        assert_eq!(
            parse_answer("{\"type\":\"s\",\"data\":[\"yes\"]}\n"),
            Some(Permission::Yes)
        );
        assert_eq!(
            parse_answer(r#"{"type":"s","data":["challenge"]}"#),
            Some(Permission::Challenge)
        );
        assert_eq!(
            parse_answer(r#"{"type":"s","data":["no"]}"#),
            Some(Permission::No)
        );
        assert_eq!(
            parse_answer(r#"{"type":"s","data":["na"]}"#),
            Some(Permission::Na)
        );
    }

    #[test]
    fn output_that_is_not_an_answer_reads_as_none() {
        for output in [
            "",
            "s \"yes\"",
            r#"{"type":"b","data":[true]}"#,
            r#"{"type":"s","data":[]}"#,
            r#"{"type":"s"}"#,
        ] {
            assert_eq!(parse_answer(output), None, "{output:?}");
        }
    }

    #[test]
    fn the_answers_make_the_facts_and_a_failed_call_counts_as_not_possible() {
        assert_eq!(
            facts([Some(Permission::Yes), None, Some(Permission::No)]),
            Facts {
                system_bus: true,
                logind: true,
                can_suspend: Permission::Yes,
                can_reboot: Permission::Na,
                can_power_off: Permission::No,
                sandboxed: false,
            }
        );
    }

    #[test]
    fn every_call_failed_means_no_logind() {
        let facts = facts([None, None, None]);
        assert!(!facts.logind);
        assert!(!super::super::PowerState::from_facts(&facts).shown());
    }

    #[test]
    fn inhibitors_read_what_who_why_and_mode() {
        let listed = concat!(
            r#"{"type":"a(ssssuu)","data":[[["sleep","GNOME Shell","GNOME needs to lock "#,
            r#"the screen","delay",1000,5659],["shutdown:sleep","Steam","Updating \"Game\"","#,
            r#""block",1000,4242]]]}"#,
        );
        assert_eq!(
            parse_inhibitors(listed),
            [
                Inhibitor {
                    what: "sleep".to_owned(),
                    who: "GNOME Shell".to_owned(),
                    why: "GNOME needs to lock the screen".to_owned(),
                    mode: "delay".to_owned(),
                },
                Inhibitor {
                    what: "shutdown:sleep".to_owned(),
                    who: "Steam".to_owned(),
                    why: "Updating \"Game\"".to_owned(),
                    mode: "block".to_owned(),
                },
            ]
        );
    }

    #[test]
    fn inhibitors_that_cannot_be_read_are_none() {
        for output in [
            "",
            "a(ssssuu) 0",
            r#"{"type":"a(ssssuu)","data":[]}"#,
            r#"{"type":"a(ssssuu)","data":[[["sleep","Steam"]]]}"#,
        ] {
            assert!(parse_inhibitors(output).is_empty(), "{output:?}");
        }
        assert_eq!(parse_inhibitors(r#"{"type":"a(ssssuu)","data":[[]]}"#), []);
    }

    #[test]
    fn a_failed_call_says_logind_s_reason() {
        assert_eq!(
            error_message("Call failed: Access denied\n", 1),
            "Access denied"
        );
        let inhibited = "Call failed: Operation inhibited by \"Steam\" (PID 4242 \"steam\", \
                         user tim), reason is \"Updating\".\n";
        assert_eq!(
            error_message(inhibited, 1),
            "Operation inhibited by \"Steam\" (PID 4242 \"steam\", user tim), reason is \
             \"Updating\"."
        );
    }

    #[test]
    fn a_failure_without_logind_s_reason_says_what_there_is() {
        assert_eq!(
            error_message("Portal call failed\nbusctl: not found\n", 127),
            "busctl: not found"
        );
        assert_eq!(error_message("  \n", 1), "busctl exited with status 1");
    }
}
