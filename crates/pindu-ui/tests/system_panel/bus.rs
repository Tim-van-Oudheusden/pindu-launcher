//! A private system bus, python-dbusmock's templates on it, and waiting on the main loop.

use std::fs;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use gtk::gio::prelude::*;
use gtk::{gio, glib};

/// dbusmock's own interface, on every object it serves.
pub(crate) const MOCK: &str = "org.freedesktop.DBus.Mock";

/// How long anything may take to happen.
const TIMEOUT: Duration = Duration::from_secs(10);

/// What lets everyone on the bus own any name and call anything, with no service
/// directories, so nothing on it can be activated.
const CONFIG: &str = r#"<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>system</type>
  <listen>unix:dir=@DIR@</listen>
  <auth>EXTERNAL</auth>
  <policy context="default">
    <allow user="*"/>
    <allow own="*"/>
    <allow send_type="method_call"/>
    <allow send_type="method_return"/>
    <allow send_type="error"/>
    <allow send_type="signal"/>
    <allow receive_type="method_call"/>
    <allow receive_type="method_return"/>
    <allow receive_type="error"/>
    <allow receive_type="signal"/>
  </policy>
</busconfig>
"#;

/// Why the suite cannot run here, if it cannot.
pub(crate) fn missing_tool() -> Option<&'static str> {
    let runs = |program: &str, args: &[&str]| {
        Command::new(program)
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    };
    if !runs("dbus-daemon", &["--version"]) {
        return Some("dbus-daemon is not installed");
    }
    if !runs("python3", &["-c", "import dbusmock"]) {
        return Some(
            "python-dbusmock is not installed for python3 (python3 -m pip install python-dbusmock)",
        );
    }
    None
}

/// A `dbus-daemon` of our own, as the system bus.
pub(crate) struct Bus {
    daemon: Child,
    dir: PathBuf,
    address: String,
}

impl Bus {
    /// Starts the bus, with its socket in a fresh folder.
    pub(crate) fn start() -> Self {
        let dir = std::env::temp_dir().join(format!("pindu-system-bus-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("a folder for the bus");
        let config = dir.join("system.conf");
        let listen = dir.to_str().expect("a UTF-8 temporary folder");
        fs::write(&config, CONFIG.replace("@DIR@", listen)).expect("the bus's configuration");
        let mut daemon = Command::new("dbus-daemon")
            .arg(format!("--config-file={}", config.display()))
            .args(["--nofork", "--print-address=1"])
            .stdout(Stdio::piped())
            .spawn()
            .expect("dbus-daemon starts");
        let mut address = String::new();
        BufReader::new(daemon.stdout.take().expect("dbus-daemon's output"))
            .read_line(&mut address)
            .expect("dbus-daemon prints its address");
        Self {
            daemon,
            dir,
            address: address.trim().to_owned(),
        }
    }

    /// The bus's address.
    pub(crate) fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for Bus {
    fn drop(&mut self) {
        let _ = self.daemon.kill();
        let _ = self.daemon.wait();
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The system bus, as GIO, and so the backends, have it.
pub(crate) fn system_bus() -> gio::DBusConnection {
    gio::bus_get_sync(gio::BusType::System, gio::Cancellable::NONE).expect("the private bus")
}

/// Whether `name` has an owner on the bus.
fn has_owner(name: &str) -> bool {
    system_bus()
        .call_sync(
            Some("org.freedesktop.DBus"),
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "NameHasOwner",
            Some(&(name,).to_variant()),
            None,
            gio::DBusCallFlags::NONE,
            -1,
            gio::Cancellable::NONE,
        )
        .ok()
        .and_then(|reply| reply.get::<(bool,)>())
        .is_some_and(|(owned,)| owned)
}

/// A dbusmock template serving its service on the bus, until it is dropped.
pub(crate) struct Mock {
    process: Child,
    name: &'static str,
}

impl Mock {
    /// Starts dbusmock's `template`, which owns `name`, with the template's `parameters`
    /// as JSON, and waits until it is on the bus.
    pub(crate) fn start(template: &str, name: &'static str, parameters: Option<&str>) -> Self {
        let mut command = Command::new("python3");
        command
            .args(["-m", "dbusmock", "--system", "--template", template])
            .stdout(Stdio::null());
        if let Some(parameters) = parameters {
            command.args(["--parameters", parameters]);
        }
        let process = command.spawn().expect("dbusmock starts");
        let mock = Self { process, name };
        wait_until(&format!("{name} on the bus"), || has_owner(name));
        mock
    }

    /// Calls `method` of `interface` on the mock's object at `path`.
    pub(crate) fn call(
        &self,
        path: &str,
        interface: &str,
        method: &str,
        parameters: Option<glib::Variant>,
    ) -> glib::Variant {
        system_bus()
            .call_sync(
                Some(self.name),
                path,
                interface,
                method,
                parameters.as_ref(),
                None,
                gio::DBusCallFlags::NONE,
                -1,
                gio::Cancellable::NONE,
            )
            .unwrap_or_else(|error| panic!("{interface}.{method} on {path}: {error}"))
    }

    /// Gives the object at `path` the method `name` of `interface`, running dbusmock's
    /// Python `code`, in place of the template's if it has one.
    pub(crate) fn add_method(
        &self,
        path: &str,
        interface: &str,
        name: &str,
        signatures: (&str, &str),
        code: &str,
    ) {
        let (input, output) = signatures;
        self.call(
            path,
            MOCK,
            "AddMethod",
            Some((interface, name, input, output, code).to_variant()),
        );
    }

    /// The arguments of each call of `method` on the object at `path`, oldest first.
    pub(crate) fn calls(&self, path: &str, method: &str) -> Vec<Vec<glib::Variant>> {
        self.call(path, MOCK, "GetMethodCalls", Some((method,).to_variant()))
            .get::<(Vec<(u64, Vec<glib::Variant>)>,)>()
            .expect("GetMethodCalls answers a(tav)")
            .0
            .into_iter()
            .map(|(_, arguments)| arguments)
            .collect()
    }
}

impl Drop for Mock {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
        // The next scenario starts without the service, so wait for the bus to see it go;
        // never panic here, which would abort a scenario that is failing already.
        let deadline = Instant::now() + TIMEOUT;
        while has_owner(self.name) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Runs the main loop until `done` holds, failing after [`TIMEOUT`].
pub(crate) fn wait_until(what: &str, mut done: impl FnMut() -> bool) {
    let context = glib::MainContext::default();
    let deadline = Instant::now() + TIMEOUT;
    loop {
        while context.iteration(false) {}
        if done() {
            return;
        }
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Runs the main loop for a moment, for what should not happen to have the chance.
pub(crate) fn settle() {
    let context = glib::MainContext::default();
    let until = Instant::now() + Duration::from_millis(500);
    while Instant::now() < until {
        while context.iteration(false) {}
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// `path` as an object path value.
pub(crate) fn object_path(path: &str) -> glib::Variant {
    glib::variant::ObjectPath::try_from(path.to_owned())
        .expect("an object path")
        .to_variant()
}
