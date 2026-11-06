//! Quick Access's Controllers level: each connected pad with the glyphs it shows under the
//! settings, whether it can rumble to identify itself, and its battery where one of the
//! peripheral batteries the kernel reports can be told to be its.

use std::path::Path;

use crate::gamepad::{ControllerSettings, DeviceId, Glyphs, HidIds, Layout, Pad};
use crate::power::Battery;

/// A connected pad as the Controllers level shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Controller {
    pub device: DeviceId,
    pub name: String,
    /// The glyphs the hints show for it.
    pub glyphs: Glyphs,
    /// Whether it can rumble, which Identify needs.
    pub rumble: bool,
    /// Its battery, when exactly one peripheral battery sits on a device with its bus,
    /// vendor and product and no other pad shares them.
    pub battery: Option<Battery>,
}

/// The pads, in their order, as the Controllers level shows them under `settings`, each
/// with its battery from `peripherals` where it can be told apart. Under Steam Input every
/// pad is Steam's virtual one, which has no battery of its own, so it shows none.
pub fn controllers<'a>(
    pads: impl IntoIterator<Item = (DeviceId, &'a Pad)>,
    settings: ControllerSettings,
    peripherals: &[Battery],
) -> Vec<Controller> {
    let pads: Vec<_> = pads.into_iter().collect();
    let battery_ids: Vec<_> = peripherals.iter().map(device_ids).collect();
    pads.iter()
        .map(|&(device, pad)| {
            let battery = pad.ids.and_then(|ids| {
                let one_pad = pads
                    .iter()
                    .filter(|(_, other)| other.ids == Some(ids))
                    .count()
                    == 1;
                let mut matching = peripherals
                    .iter()
                    .zip(&battery_ids)
                    .filter(|(_, battery)| **battery == Some(ids));
                let battery = matching.next().map(|(battery, _)| battery);
                (one_pad && matching.next().is_none())
                    .then(|| battery.cloned())
                    .flatten()
            });
            Controller {
                device,
                name: pad.name.clone(),
                glyphs: Layout::resolve(pad.kind, settings).glyphs,
                rumble: pad.rumble,
                battery,
            }
        })
        .collect()
}

/// The bus, vendor and product of the HID device a battery sits on, from the kernel's name
/// for it, `BBBB:VVVV:PPPP.NNNN`, nearest first along its device path.
fn device_ids(battery: &Battery) -> Option<HidIds> {
    battery
        .device
        .as_deref()?
        .ancestors()
        .filter_map(Path::file_name)
        .find_map(|name| hid_ids(name.to_str()?))
}

/// The IDs in a HID device name such as `0005:054C:0CE6.0007`.
fn hid_ids(name: &str) -> Option<HidIds> {
    let (ids, instance) = name.split_once('.')?;
    let mut parts = ids.split(':');
    let mut hex = || {
        let part = parts.next().filter(|part| part.len() == 4)?;
        u16::from_str_radix(part, 16).ok()
    };
    let ids = HidIds {
        bus: hex()?,
        vendor: hex()?,
        product: hex()?,
    };
    let whole = parts.next().is_none() && u16::from_str_radix(instance, 16).is_ok();
    whole.then_some(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gamepad::{DeviceKind, Prompts};
    use crate::power;

    /// A desktop with a DualSense over Bluetooth, whose battery sits on its HID device
    /// `0005:054C:0CE6.0007`.
    fn dualsense_batteries() -> Vec<Battery> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/power_supply/desktop-dualsense/sys/class/power_supply");
        power::read_batteries(&root).peripherals
    }

    fn pad(name: &str, kind: DeviceKind, guid: &str, rumble: bool) -> Pad {
        Pad {
            name: name.into(),
            kind,
            ids: HidIds::from_guid(guid),
            rumble,
        }
    }

    const DUALSENSE_BLUETOOTH: &str = "050000004c050000e60c000000810000";
    const DUALSENSE_USB: &str = "030000004c050000e60c000011810000";
    const STEAM_PAD: &str = "03000000de280000ff11000001000000";

    #[test]
    fn each_pad_shows_its_glyphs_rumble_and_the_battery_on_its_device() {
        let dualsense = pad(
            "DualSense Wireless Controller",
            DeviceKind::PlayStation,
            DUALSENSE_BLUETOOTH,
            true,
        );
        let steam = pad("Steam Virtual Gamepad", DeviceKind::Xbox, STEAM_PAD, false);
        let pads = [(DeviceId(1), &dualsense), (DeviceId(2), &steam)];

        let shown = controllers(pads, ControllerSettings::default(), &dualsense_batteries());

        assert_eq!(shown.len(), 2);
        assert_eq!(shown[0].device, DeviceId(1));
        assert_eq!(shown[0].name, "DualSense Wireless Controller");
        assert_eq!(shown[0].glyphs, Glyphs::Shapes);
        assert!(shown[0].rumble);
        assert_eq!(
            shown[0].battery.as_ref().map(|battery| battery.capacity),
            Some(Some(70))
        );
        assert_eq!(shown[1].glyphs, Glyphs::Letters);
        assert!(!shown[1].rumble);
        assert_eq!(shown[1].battery, None);
    }

    #[test]
    fn the_glyphs_follow_the_button_prompts_setting() {
        let steam = pad("Steam Virtual Gamepad", DeviceKind::Xbox, STEAM_PAD, true);
        let settings = ControllerSettings {
            prompts: Prompts::Shapes,
            ..ControllerSettings::default()
        };

        let shown = controllers([(DeviceId(1), &steam)], settings, &[]);

        assert_eq!(shown[0].glyphs, Glyphs::Shapes);
    }

    #[test]
    fn a_battery_on_another_bus_is_not_the_pad_s() {
        let dualsense = pad("DualSense", DeviceKind::PlayStation, DUALSENSE_USB, true);

        let shown = controllers(
            [(DeviceId(1), &dualsense)],
            ControllerSettings::default(),
            &dualsense_batteries(),
        );

        assert_eq!(shown[0].battery, None);
    }

    #[test]
    fn two_pads_of_one_model_cannot_tell_their_batteries_apart() {
        let first = pad(
            "DualSense",
            DeviceKind::PlayStation,
            DUALSENSE_BLUETOOTH,
            true,
        );
        let second = first.clone();

        let shown = controllers(
            [(DeviceId(1), &first), (DeviceId(2), &second)],
            ControllerSettings::default(),
            &dualsense_batteries(),
        );

        assert_eq!(shown[0].battery, None);
        assert_eq!(shown[1].battery, None);
    }

    #[test]
    fn a_pad_without_ids_has_no_battery() {
        let unknown = Pad {
            name: "Pad".into(),
            kind: DeviceKind::Generic,
            ids: None,
            rumble: false,
        };

        let shown = controllers(
            [(DeviceId(1), &unknown)],
            ControllerSettings::default(),
            &dualsense_batteries(),
        );

        assert_eq!(shown[0].battery, None);
    }
}
