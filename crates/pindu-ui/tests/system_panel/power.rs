//! The Power section over [`LogindBackend`], against dbusmock's `logind` template.

use std::cell::RefCell;
use std::rc::Rc;

use gtk::glib::prelude::*;
use pindu_core::system::power::{Failure, PowerAction, PowerEntry, SystemPower};
use pindu_ui::logind_backend::LogindBackend;

use crate::bus::{Mock, settle, wait_until};

const LOGIND: &str = "org.freedesktop.login1";
const PATH: &str = "/org/freedesktop/login1";
const MANAGER: &str = "org.freedesktop.login1.Manager";

/// The section read for the first time, with the failures it reports.
fn section() -> (SystemPower, Rc<RefCell<Vec<Failure>>>) {
    let power = SystemPower::new(Box::new(LogindBackend::new()));
    let failures = Rc::new(RefCell::new(Vec::new()));
    power.connect_failed({
        let failures = failures.clone();
        move |failure| failures.borrow_mut().push(failure.clone())
    });
    power.refresh();
    (power, failures)
}

pub(crate) fn suspend_restart_and_power_off_call_logind() {
    let logind = Mock::start("logind", LOGIND, None);
    for (action, method) in [
        (PowerAction::Suspend, "Suspend"),
        (PowerAction::Restart, "Reboot"),
        (PowerAction::PowerOff, "PowerOff"),
    ] {
        let (power, failures) = section();
        wait_until("the Power entries", || power.state().shown());
        assert_eq!(
            power.state().entries,
            PowerAction::ALL.map(|action| PowerEntry {
                action,
                allowed: true
            })
        );

        power.run(action);
        wait_until(&format!("logind's {method}"), || {
            !logind.calls(PATH, method).is_empty()
        });
        settle();
        // Interactive, so polkit may ask where the session has an agent.
        assert_eq!(logind.calls(PATH, method), [[true.to_variant()]]);
        assert!(failures.borrow().is_empty(), "{:?}", failures.borrow());
    }
}

pub(crate) fn what_logind_answers_picks_the_entries() {
    let _logind = Mock::start(
        "logind",
        LOGIND,
        Some(r#"{"CanSuspend": "na", "CanReboot": "no", "CanPowerOff": "challenge"}"#),
    );
    let (power, _) = section();
    wait_until("the Power entries", || power.state().shown());
    assert_eq!(
        power.state().entries,
        [
            PowerEntry {
                action: PowerAction::Restart,
                allowed: false
            },
            PowerEntry {
                action: PowerAction::PowerOff,
                allowed: true
            },
        ]
    );
}

pub(crate) fn a_refusal_names_who_inhibits_it() {
    let logind = Mock::start("logind", LOGIND, None);
    logind.add_method(
        PATH,
        MANAGER,
        "PowerOff",
        ("b", ""),
        "raise dbus.exceptions.DBusException('Operation inhibited by \"Steam\"', \
         name='org.freedesktop.login1.OperationInhibited')",
    );
    logind.add_method(
        PATH,
        MANAGER,
        "ListInhibitors",
        ("", "a(ssssuu)"),
        "ret = [('shutdown:sleep', 'Steam', 'Downloading a game', 'block', 1000, 4242), \
         ('shutdown', 'GNOME Shell', 'Saving the session', 'delay', 1000, 2121)]",
    );
    let (power, failures) = section();
    wait_until("the Power entries", || power.state().shown());

    power.run(PowerAction::PowerOff);
    wait_until("the refusal", || !failures.borrow().is_empty());
    assert_eq!(
        *failures.borrow(),
        [Failure::Inhibited {
            action: PowerAction::PowerOff,
            holders: vec!["Steam".to_owned()],
        }]
    );
}

pub(crate) fn the_section_shows_once_logind_appears() {
    let (power, _) = section();
    settle();
    assert!(!power.state().shown(), "no logind, no Power entry");

    let _logind = Mock::start("logind", LOGIND, None);
    wait_until("the Power entries", || power.state().shown());
    assert!(power.state().allows(PowerAction::Suspend));
}
