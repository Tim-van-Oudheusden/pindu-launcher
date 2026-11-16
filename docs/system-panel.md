# System panel

Fullscreen mode's system panel lets the owner of a console run the machine with a controller only: power off, restart
and suspend, join a Wi-Fi network, pair a Bluetooth controller or headset, choose the audio output, and see, apply and
roll back an OS update. It is work package **L-03** of the
[Pindu OS spec](https://github.com/Tim-van-Oudheusden/pindu-os/blob/main/docs/spec.md#phase-3-ready-for-games), where
the launcher is the whole user interface: Pindu OS has no desktop, no shell, no sudo and, on a console, no keyboard.

This page is the short spec issue #317 asks for: what each section does, which system service it talks to, how the
launcher finds out what is available, and how the work splits into issues. The launcher's product spec does not cover
the panel; where this page and an issue disagree, the issue wins.

## Where it lives

| Section | Place | Why |
| --- | --- | --- |
| Power | A **Power** entry in the menu drawer, after **Quit**, leading to **Suspend**, **Restart** and **Power off** | The drawer already holds the ways out (Exit Fullscreen, Quit); Steam's Main Menu puts Power in the same place |
| Network | A **Network** entry in Quick Access, subtitled with the connection (`Wired`, the Wi-Fi network's name, `Offline`) | Quick Access holds the settings a player changes mid-session |
| Bluetooth | A **Bluetooth** entry in Quick Access, subtitled with the connected devices | As above |
| Audio output | A **Sound Output** entry in Quick Access, subtitled with the output in use | As above |
| OS updates | A **System Update** entry in Quick Access, subtitled with the installed version, or `Update ready` | As above |

Each Quick Access entry leads to a level of its own (`QuickAccessLevel::Network`, `Bluetooth`, `SoundOutput`,
`SystemUpdate`), and B steps back to the entries, as the existing levels do. The drawer's Power entry leads to
`DrawerLevel::Power`. A passphrase or PIN is typed in a dialog with the on-screen keyboard, which takes the buttons
first, as dialogs do today. Every entry uses stock libadwaita rows; nothing is styled beyond what the designs already
style.

## What is available

The panel shows a section only when the service behind it is there and the session may use it. A missing service or
permission hides the section or says why in place. It is never shown as an error, the same rule as
[Honest sandbox](../AGENTS.md#architecture).

The launcher checks each section once when fullscreen mode starts, and again when the service appears or disappears
on the bus (`NameOwnerChanged`):

1. **In the Flatpak**, the sandbox has no system bus (`finish-args` grants none, and this panel does not ask for it).
   Power, Network, Bluetooth and System Update are hidden. Sound Output is hidden too, because the sandbox does not
   have the tools it uses (see [Audio output](#audio-output)). With every section hidden, the drawer has no Power entry
   and Quick Access shows no system entries. A desktop's own settings do this job there.
2. **Natively**, a section whose service has no owner on the system bus and cannot be activated
   (`ListActivatableNames`) is hidden: a system without BlueZ shows no Bluetooth entry.
3. **The service is there, but the hardware is not**: no Bluetooth adapter hides Bluetooth; no Wi-Fi device leaves
   Network with the wired state only.
4. **The service is there, but polkit answers `no`** for the action (logind's `CanPowerOff` and friends,
   NetworkManager's `GetPermissions`): the entry stays, insensitive, with a subtitle saying the system does not allow
   it. `challenge` (a password would be asked) counts as allowed: on a desktop the desktop's agent asks; on Pindu OS
   the rules below make every action a plain `yes`.
5. **The hardware is switched off** (a Bluetooth adapter blocked by rfkill, Wi-Fi radio off): the section shows the
   state and a switch to turn it on, or says it is blocked when the switch is a hardware one.

OS updates only exist on Pindu OS: System Update shows when `/etc/os-release` says `ID=pindu-os` and the update status
file the OS writes (see [Pindu OS side](#pindu-os-side)) exists.

## Sections

All system bus calls go through `gio::DBusProxy` and `gio::DBusConnection`, which the launcher already uses for the
session bus. No crate is added.

### Power

Service: logind, `org.freedesktop.login1` on the system bus.

- **Suspend**, **Restart** and **Power off** call `Manager.Suspend`, `Manager.Reboot` and `Manager.PowerOff` with
  `interactive = true`. Each shows only when the matching `CanSuspend`, `CanReboot` or `CanPowerOff` answers `yes` or
  `challenge` (rule 4 above for `no`; `na` hides it).
- While a game the launcher started is running, Restart and Power off first ask in a dialog naming the game ("Power
  off while *Game* is running? Unsaved progress is lost"). Suspend does not ask.
- A block inhibitor that stops the action (logind refuses the call) is reported as an in-app notification naming who
  holds it (`ListInhibitors`), not as a failure of the launcher.

### Network

Service: NetworkManager, `org.freedesktop.NetworkManager` on the system bus.

- **State.** One row per wired and Wi-Fi device (`GetDevices`, `DeviceType` 1 and 2) with its state: connected (and
  to which network), connecting, disconnected, cable unplugged. The Quick Access subtitle follows `PrimaryConnection`.
- **Wi-Fi list.** Opening the level asks for a scan (`Device.Wireless.RequestScan`) and lists the access points
  (`GetAllAccessPoints`) grouped by SSID, strongest first, with a signal icon and a lock for secured networks. The
  network in use is checked; networks with a saved connection are marked **Saved**.
- **Join.** A saved network is activated (`ActivateConnection`). A new one opens the passphrase dialog for WPA/WPA2
  personal (`key-mgmt` `wpa-psk`) and WPA3 personal (`sae`); an open network joins without one. The launcher then
  calls `AddAndActivateConnection2` with a system connection (`connection.permissions` empty), so it comes up at the
  next boot before anyone is logged in, with the passphrase stored in the connection (`psk-flags` 0). A wrong
  passphrase (device state reason `NO_SECRETS` or `SUPPLICANT_DISCONNECT`) reopens the dialog with "Wrong passphrase"
  and deletes the new connection, so no broken one is left behind.
- **Forget.** A saved network has **Forget**, which deletes its connection (`Settings.Connection.Delete`).
- **Not offered:** enterprise Wi-Fi (802.1X), hidden networks, static addresses, VPN and proxies. An enterprise network
  is listed but insensitive, subtitled "Set up on another device"; the others are not listed.

### Bluetooth

Service: BlueZ, `org.bluez` on the system bus.

- **State.** The adapter (`org.bluez.Adapter1` from `ObjectManager.GetManagedObjects`) with a switch for `Powered`;
  `PowerState` `off-blocked` says the radio is blocked rather than offering the switch.
- **Devices.** Paired devices first, then the ones found, each with its `Icon` (`input-gaming`, `audio-headset`,
  `audio-headphones`, …), name, and Connected, Paired or nothing. Discovery (`StartDiscovery`) runs only while the
  level is open, and stops when it closes.
- **Pair and connect.** A found device is paired (`Device1.Pair`), trusted (`Trusted = true`, so it reconnects by
  itself) and connected (`Connect`). A paired device toggles between Connect and Disconnect.
- **Agent.** While the level is open the launcher registers a pairing agent (`org.bluez.Agent1`, capability
  `KeyboardDisplay`) and asks to be the default agent. Numeric comparison (`RequestConfirmation`) shows the code with
  A to confirm and B to refuse; a displayed passkey (`DisplayPasskey`) shows the code to type on the device; a PIN
  (`RequestPinCode`) opens the PIN dialog with the on-screen keyboard. Controllers and headsets usually pair with none
  of these ("Just Works").
- **Forget.** A paired device has **Forget** (`Adapter1.RemoveDevice`).

### Audio output

Service: PipeWire with WirePlumber, on the session's own sockets. Neither has a D-Bus interface for this.

- **Outputs.** The core reads the `Audio/Sink` nodes and the default sink (`default.audio.sink` in the `default`
  metadata) from `pw-dump`'s JSON, which the core parses and tests against fixture dumps. Each output shows its
  `node.description`; the default one is checked. The level reads the dump again whenever it opens, as the batteries
  are read again when Quick Access opens.
- **Choose.** Choosing an output runs `wpctl set-default <id>`. WirePlumber stores the choice, so it outlasts a reboot.
  Streams that follow the default (games do) move to it. One runs at a time: a choice made while one runs waits for
  it, only the latest of those is sent, and once none is left the level checks the output last written, so the last
  choice made is the default that stays.
- **Not offered:** volume (the controller's volume keys and the game's own setting do that), card profiles (WirePlumber
  picks one per card), inputs.
- `pw-dump` comes with `pipewire-utils` on Fedora and `pipewire-bin` on Debian and Ubuntu; `wpctl` with
  `wireplumber`. The native packages require them. In the Flatpak neither exists, so the section is hidden there (rule
  1); see [Decisions](#decisions) for the alternative that would work in the sandbox too.

### OS updates

Service: Pindu OS's update status file and rollback unit (below), and logind for the restart.

- **Version.** The installed version is `IMAGE_VERSION` from `/etc/os-release` (for example `45.20261101.0`).
- **Update ready.** The status file holds `bootc status --format=json --format-version=1`. A `status.staged`
  deployment means an update is downloaded and waits for the next start: the level shows its version and **Restart to
  Update**, which is logind's `Reboot` (bootc applies a staged update when the machine shuts down).
- **Roll back.** A `status.rollback` deployment shows **Roll Back to *version***, with a dialog that says the console
  restarts into that version and that the current one stays available to go back to. Confirming starts
  `pindu-os-rollback.service` through systemd (`org.freedesktop.systemd1.Manager.StartUnit`, mode `replace`), waits
  for the job to finish, then restarts through logind. `status.rollbackQueued` true shows "The previous version starts
  next time" and **Restart** instead.
- The level reads the file again when it opens and when the file changes (`gio::FileMonitor`).

## Code layout

Each section is a service in `pindu-ui` over a backend trait in `pindu-core`, as the gamepad is
(`GamepadBackend` in `pindu-core`, `ManetteBackend` in `pindu-ui`). The shells observe the services' models and call
the services; they never talk to the bus or spawn a process themselves.

| Section | Core module (pure logic and the trait) | UI backend | UI service | UI level |
| --- | --- | --- | --- | --- |
| Availability | `system` (the rules in [What is available](#what-is-available), from facts the backends report) | | | |
| Power | `system::power` (`PowerBackend`) | `logind_backend.rs` | `system_power_service.rs` | not yet moved (#338) |
| Network | `system::network` (`NetworkBackend`) | `networkmanager_backend.rs` | `network_service.rs` | `PinduNetworkLevel` (`fullscreen_network.rs`), `data/ui/fullscreen_network.blp` |
| Bluetooth | `system::bluetooth` (`BluetoothBackend`) | `bluez_backend.rs` | `bluetooth_service.rs` | not yet moved (#338) |
| Audio output | `system::audio` (`AudioBackend`, `pw-dump` parser) | `wireplumber_backend.rs` | `audio_output_service.rs` | not yet moved (#338) |
| OS updates | `system::update` (`UpdateBackend`, `bootc status` parser) | `pindu_os_backend.rs` | `system_update_service.rs` | not yet moved (#338) |

- The backends report results through callbacks on the main thread, like `GamepadBackend::start`'s sink, so the core
  stays free of GLib and every flow (join a network, pair a device, roll back) is tested in `pindu-core` with a fake
  backend.
- The existing `power` module (batteries from sysfs) keeps its name; the new power actions live in `system::power`.
- The panel's levels and entries extend `panels.rs` (`DrawerLevel`, `QuickAccessLevel`, `drawer_entries`).
- Each section's level is its own composite widget with its own Blueprint (the UI level column), a single child of its
  page in `window.blp`, as `HomePage` is. The section's entry stays in `window.blp`, in the panel's shared list. Moving
  a level keeps its action names (`win.*`), the widget names controller focus finds its rows by, and the style classes
  in [Fullscreen style](fullscreen-style.md); its rows stay direct children of their list boxes.

## Pindu OS side

Pindu OS ships three things for this panel. They are tracked in the Pindu OS issue listed under
[Issues](#issues).

1. **`pindu-os-update-status.service`**, a root oneshot that writes `bootc status --format=json --format-version=1` to
   `/run/pindu-os/update-status.json` (mode 0644, written to a temporary file and renamed). It runs at boot, after
   every run of `bootc-fetch-apply-updates.service` and after a rollback. `bootc status` needs root
   ([bootc#409](https://github.com/bootc-dev/bootc/issues/409)); the file lets the launcher read it without any.
2. **`pindu-os-rollback.service`**, a root oneshot that runs `bootc rollback` and then the status unit. It is never
   enabled, only started on demand. What `bootc rollback` does to an update that is staged at the time is checked
   there, and the status file must show the result.
3. **A polkit rule** (`/usr/share/polkit-1/rules.d/`) that lets the `pindu` user, in an active local session, start
   `pindu-os-rollback.service` (action `org.freedesktop.systemd1.manage-units`, `unit` `pindu-os-rollback.service`,
   `verb` `start`) and nothing else through systemd. The same file grants anything this page uses that Fedora's
   defaults do not already allow the active session without a password. The gamescope session has no polkit agent,
   so an action that would ask for one cannot work there; NetworkManager's
   `org.freedesktop.NetworkManager.settings.modify.system` is the one to check
   (`pkaction --verbose --action-id …`). logind, BlueZ and the rest of NetworkManager allow the active session by
   default.

A unit plus a rule instead of a helper program keeps the root side to one `bootc` call per unit, with no argument the
launcher can pass in.

## Flatpak

No `finish-args` change. The Flatpak has no system bus, so its fullscreen mode shows none of the system sections, and
on a desktop the desktop's settings cover them. Pindu OS installs the native package. Should the maintainer ever want
the panel in the Flatpak, it would need `--system-talk-name=` for `org.freedesktop.login1`,
`org.freedesktop.NetworkManager` and `org.bluez`; that is the maintainer's call, outside this work.

## Out of scope

- The Pindu OS side itself (above), which is Pindu OS work.
- gamescope's `--steam` window protocol (Pindu OS work package L-02).
- Display settings: resolution, HDR and VRR.
- A desktop-mode version of these settings.
- The top bar's status icons for Wi-Fi and Bluetooth.

## Issues

| Issue | Covers | Depends on |
| --- | --- | --- |
| #321 Add the system panel's availability rules and the Power section to fullscreen mode | The `system` module and availability rules, logind, the drawer's Power level | This page |
| #322 Let the on-screen keyboard type passphrases and PINs | Capitals and symbols layers, a hidden-text entry with a show toggle, the passphrase and PIN dialog | This page |
| #323 Add a Network section to fullscreen mode's Quick Access | NetworkManager: state, Wi-Fi list, join with a passphrase, forget | #321, #322 |
| #324 Add a Bluetooth section to fullscreen mode's Quick Access | BlueZ: discovery, pairing agent, pair, connect, forget | #321, #322 |
| #325 Add a Sound Output section to fullscreen mode's Quick Access | `pw-dump` and `wpctl`, the RPM and DEB requirements | #321 |
| #326 Add a System Update section to fullscreen mode's Quick Access | Version, staged update, restart into it, roll back | #321, Tim-van-Oudheusden/pindu-os#17 |
| Tim-van-Oudheusden/pindu-os#17 Ship the update status and rollback units and the polkit rule for the launcher's system panel | The [Pindu OS side](#pindu-os-side) | This page |

## Decisions

Choices this page makes that the maintainer may want to turn around before the issues start:

- **Power in the drawer, not Quick Access.** The issue calls Quick Access the natural home; this page puts Power beside
  Quit because the drawer is where fullscreen mode is left. Moving it into Quick Access changes only #321.
- **Audio through `pw-dump` and `wpctl`.** No new crate, and native only, which is where Pindu OS runs. The
  alternative is the `libpulse-binding` crate (2.30.1), talking to PipeWire's PulseAudio server through the
  `--socket=pulseaudio` the Flatpak already has, so Sound Output would work in the Flatpak too; it is a new
  dependency, so it needs the maintainer's approval.
- **Rollback through a systemd unit and a polkit rule** rather than a helper program run with `pkexec`: nothing on the
  root side takes input from the launcher.
