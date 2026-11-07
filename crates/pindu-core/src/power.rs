//! The batteries the kernel reports under `/sys/class/power_supply`: the machine's own
//! battery, which Quick Access's header shows, and the batteries of peripherals such as
//! controllers (`scope` `Device`). Plain reads of sysfs, no system bus.

use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where the kernel lists the power supplies.
pub const SYSFS_POWER_SUPPLY: &str = "/sys/class/power_supply";

/// How often the batteries are read again while the app runs: a charge changes slowly.
pub const REFRESH_INTERVAL: Duration = Duration::from_secs(60);

/// A battery's charging state, from its `status` attribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Charge {
    /// Charging (`Charging`).
    Charging,
    /// Running on the battery (`Discharging`).
    Discharging,
    /// Plugged in, but not charging (`Not charging`).
    NotCharging,
    /// Fully charged (`Full`).
    Full,
    /// Any other status, or none.
    #[default]
    Unknown,
}

impl Charge {
    /// The state a `status` value names: `Charging`, `Discharging`, `Not charging`,
    /// `Full`, or else unknown.
    pub fn from_status(status: &str) -> Self {
        match status {
            "Charging" => Self::Charging,
            "Discharging" => Self::Discharging,
            "Not charging" => Self::NotCharging,
            "Full" => Self::Full,
            _ => Self::Unknown,
        }
    }
}

/// One battery, with what sysfs tells about it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Battery {
    /// The supply's name in `/sys/class/power_supply`, such as `BAT0`.
    pub name: String,
    /// The charge in percent (`capacity`); some peripherals only report a coarse level.
    pub capacity: Option<u8>,
    /// Its charging state (`status`).
    pub charge: Charge,
    /// The model's name (`model_name`), when the supply reports one.
    pub model_name: Option<String>,
    /// The maker's name (`manufacturer`), when the supply reports one.
    pub manufacturer: Option<String>,
    /// The serial number (`serial_number`), when the supply reports one.
    pub serial_number: Option<String>,
    /// Where the supply's `device` link leads, resolved: the device the battery sits in.
    pub device: Option<PathBuf>,
}

impl Battery {
    /// The icon for the charge, from the icon theme's `battery-level-*` set: the charge
    /// rounded down to tens, marked when charging or plugged in, and charged when full.
    /// None without a charge in percent.
    pub fn icon_name(&self) -> Option<String> {
        let level = self.capacity?.min(100) / 10 * 10;
        Some(match self.charge {
            Charge::Full => "battery-level-100-charged-symbolic".to_owned(),
            Charge::Charging if level == 100 => "battery-level-100-charged-symbolic".to_owned(),
            Charge::Charging => format!("battery-level-{level}-charging-symbolic"),
            Charge::NotCharging => format!("battery-level-{level}-plugged-in-symbolic"),
            Charge::Discharging | Charge::Unknown => format!("battery-level-{level}-symbolic"),
        })
    }
}

/// The batteries found: the machine's own, and the peripherals'.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Batteries {
    /// The machine's own battery: the first by name that is not a peripheral's.
    pub system: Option<Battery>,
    /// The peripherals' batteries (`scope` `Device`), by name.
    pub peripherals: Vec<Battery>,
}

/// Reads the batteries under `root`, normally [`SYSFS_POWER_SUPPLY`]. A battery whose
/// `scope` is `Device` belongs to a peripheral; any other is the machine's own, and the
/// first of those by name is the system battery. Bays without a battery are left out.
pub fn read_batteries(root: &Path) -> Batteries {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    let mut batteries = Batteries::default();
    for name in names {
        let supply = root.join(&name);
        let read = |attribute: &str| {
            std::fs::read_to_string(supply.join(attribute))
                .ok()
                .map(|value| value.trim().to_owned())
        };
        if read("type").as_deref() != Some("Battery") || read("present").as_deref() == Some("0") {
            continue;
        }
        let peripheral = read("scope").as_deref() == Some("Device");
        let battery = Battery {
            capacity: read("capacity").and_then(|capacity| capacity.parse().ok()),
            charge: Charge::from_status(&read("status").unwrap_or_default()),
            model_name: read("model_name"),
            manufacturer: read("manufacturer"),
            serial_number: read("serial_number"),
            device: std::fs::canonicalize(supply.join("device")).ok(),
            name,
        };
        if peripheral {
            batteries.peripherals.push(battery);
        } else if batteries.system.is_none() {
            batteries.system = Some(battery);
        }
    }
    batteries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/power_supply")
            .join(name)
            .join("sys/class/power_supply")
    }

    #[test]
    fn a_laptop_has_its_battery_as_the_system_battery_not_its_empty_bay() {
        let system = read_batteries(&fixture("laptop")).system.unwrap();
        assert_eq!(system.name, "BAT1");
        assert_eq!(system.capacity, Some(57));
        assert_eq!(system.charge, Charge::Discharging);
        assert_eq!(system.model_name.as_deref(), Some("5B10W13930"));
        assert_eq!(system.manufacturer.as_deref(), Some("SMP"));
        assert_eq!(system.serial_number.as_deref(), Some("1234"));
    }

    #[test]
    fn a_laptop_mouse_is_a_peripheral_named_by_its_device() {
        let root = fixture("laptop");
        let peripherals = read_batteries(&root).peripherals;
        assert_eq!(
            peripherals,
            [Battery {
                name: "hidpp_battery_0".into(),
                capacity: None,
                charge: Charge::Discharging,
                model_name: Some("MX Master 3".into()),
                manufacturer: Some("Logitech".into()),
                serial_number: Some("4089a1b2".into()),
                device: Some(
                    root.join("../../devices/pci0000:00/0000:00:14.0/usb1/1-2/1-2:1.2/0003:046D:C52B.0003/0003:046D:4082.0005")
                        .canonicalize()
                        .unwrap()
                ),
            }]
        );
    }

    #[test]
    fn a_steam_deck_has_its_battery_without_a_scope_as_the_system_battery() {
        let batteries = read_batteries(&fixture("steam-deck"));
        assert_eq!(
            batteries,
            Batteries {
                system: Some(Battery {
                    name: "BAT1".into(),
                    capacity: Some(82),
                    charge: Charge::Charging,
                    ..Battery::default()
                }),
                peripherals: Vec::new(),
            }
        );
    }

    #[test]
    fn a_desktop_has_no_system_battery_and_a_dualsense_as_a_peripheral() {
        let root = fixture("desktop-dualsense");
        let batteries = read_batteries(&root);
        assert_eq!(
            batteries,
            Batteries {
                system: None,
                peripherals: vec![Battery {
                    name: "ps-controller-battery-a0:ab:51:12:34:56".into(),
                    capacity: Some(70),
                    charge: Charge::Charging,
                    device: Some(
                        root.join("../../devices/virtual/misc/uhid/0005:054C:0CE6.0007")
                            .canonicalize()
                            .unwrap()
                    ),
                    ..Battery::default()
                }],
            }
        );
    }

    #[test]
    fn an_empty_or_missing_tree_has_no_batteries() {
        assert_eq!(read_batteries(&fixture("empty")), Batteries::default());
        assert_eq!(read_batteries(&fixture("missing")), Batteries::default());
    }

    #[test]
    fn the_status_names_the_charging_state() {
        assert_eq!(Charge::from_status("Charging"), Charge::Charging);
        assert_eq!(Charge::from_status("Discharging"), Charge::Discharging);
        assert_eq!(Charge::from_status("Not charging"), Charge::NotCharging);
        assert_eq!(Charge::from_status("Full"), Charge::Full);
        assert_eq!(Charge::from_status("Unknown"), Charge::Unknown);
        assert_eq!(Charge::from_status(""), Charge::Unknown);
    }

    #[test]
    fn the_icon_shows_the_charge_in_tens_and_the_charging_state() {
        let icon = |capacity, charge| {
            Battery {
                capacity,
                charge,
                ..Battery::default()
            }
            .icon_name()
        };
        let name = |name: &str| Some(name.to_owned());
        assert_eq!(
            icon(Some(57), Charge::Discharging),
            name("battery-level-50-symbolic")
        );
        assert_eq!(
            icon(Some(3), Charge::Unknown),
            name("battery-level-0-symbolic")
        );
        assert_eq!(
            icon(Some(82), Charge::Charging),
            name("battery-level-80-charging-symbolic")
        );
        assert_eq!(
            icon(Some(100), Charge::Charging),
            name("battery-level-100-charged-symbolic")
        );
        assert_eq!(
            icon(Some(98), Charge::Full),
            name("battery-level-100-charged-symbolic")
        );
        assert_eq!(
            icon(Some(60), Charge::NotCharging),
            name("battery-level-60-plugged-in-symbolic")
        );
        assert_eq!(icon(None, Charge::Discharging), None);
    }
}
