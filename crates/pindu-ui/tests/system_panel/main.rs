//! The system panel's sections (`docs/system-panel.md`) over their real backends, against
//! python-dbusmock's `logind`, `networkmanager` and `bluez5` templates on a private system
//! bus: a renamed method, a wrong signature or a missed signal in a backend fails here,
//! where the core's own tests with fake backends cannot see it.
//!
//! The suite has its own harness. GIO reads the system bus address from the environment,
//! which is process-wide and only safe to set before any thread starts, and the sections
//! run on the main thread's main loop, so the scenarios run one after the other on it.
//! Without `dbus-daemon` or python-dbusmock the suite says so and passes. Arguments that
//! are not options pick the scenarios whose names contain them, as libtest's filters do.

use std::process::ExitCode;

use gtk::glib;

mod bluetooth;
mod bus;
mod network;
mod power;

/// Every scenario, by name.
const SCENARIOS: &[(&str, fn())] = &[
    (
        "power::suspend_restart_and_power_off_call_logind",
        power::suspend_restart_and_power_off_call_logind,
    ),
    (
        "power::what_logind_answers_picks_the_entries",
        power::what_logind_answers_picks_the_entries,
    ),
    (
        "power::a_refusal_names_who_inhibits_it",
        power::a_refusal_names_who_inhibits_it,
    ),
    (
        "power::the_section_shows_once_logind_appears",
        power::the_section_shows_once_logind_appears,
    ),
    (
        "network::devices_and_networks_are_listed_and_scanned",
        network::devices_and_networks_are_listed_and_scanned,
    ),
    (
        "network::an_open_network_is_joined",
        network::an_open_network_is_joined,
    ),
    (
        "network::a_wrong_passphrase_is_asked_again_and_leaves_nothing_saved",
        network::a_wrong_passphrase_is_asked_again_and_leaves_nothing_saved,
    ),
    (
        "network::wifi_is_switched_off_and_on",
        network::wifi_is_switched_off_and_on,
    ),
    (
        "network::polkit_saying_no_leaves_the_entry_denied",
        network::polkit_saying_no_leaves_the_entry_denied,
    ),
    (
        "network::the_section_shows_once_networkmanager_appears",
        network::the_section_shows_once_networkmanager_appears,
    ),
    (
        "bluetooth::the_radio_discovery_pairing_and_connections_go_through_bluez",
        bluetooth::the_radio_discovery_pairing_and_connections_go_through_bluez,
    ),
    (
        "bluetooth::a_refused_confirmation_ends_the_pairing_quietly",
        bluetooth::a_refused_confirmation_ends_the_pairing_quietly,
    ),
    (
        "bluetooth::the_section_shows_once_bluez_has_an_adapter",
        bluetooth::the_section_shows_once_bluez_has_an_adapter,
    ),
];

fn main() -> ExitCode {
    if let Some(reason) = bus::missing_tool() {
        println!("Skipping the system panel's dbusmock tests: {reason}.");
        return ExitCode::SUCCESS;
    }
    let filters: Vec<String> = std::env::args()
        .skip(1)
        .filter(|argument| !argument.starts_with('-'))
        .collect();

    let bus = bus::Bus::start();
    // SAFETY: no thread has started yet, and GIO has not read the environment.
    unsafe { std::env::set_var("DBUS_SYSTEM_BUS_ADDRESS", bus.address()) };
    assert!(
        pindu_ui::permission_service::assume_native(),
        "nothing has asked whether this is the Flatpak yet"
    );
    let context = glib::MainContext::default();
    let _owner = context.acquire().expect("the main context is free");
    // GIO drops its shared connection once nobody holds it: hold it, so the backends and
    // the scenarios use one connection, with one unique name, throughout.
    let _system_bus = bus::system_bus();

    let chosen: Vec<_> = SCENARIOS
        .iter()
        .filter(|(name, _)| filters.is_empty() || filters.iter().any(|f| name.contains(f.as_str())))
        .collect();
    println!("\nrunning {} tests", chosen.len());
    for (name, scenario) in &chosen {
        println!("test {name} ...");
        scenario();
        println!("test {name} ... ok");
    }
    println!("\ntest result: ok. {} passed\n", chosen.len());
    ExitCode::SUCCESS
}
