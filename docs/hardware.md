# Hardware

Fullscreen mode meets graphics drivers, gamescope versions, handhelds and television PCs, controller families, Bluetooth
adapters and suspend behaviour that no single machine covers
([product analysis, section 4.7](product-analysis.md#47-hardware-validation-needs-breadth)). Pindu launcher sends
nothing about the machines it runs on, so what is known about them comes from two places: the maintainer's checks on
the machines in the published set below, and hardware reports that anyone can file. Both are public, and the matrix at
the end of this page lists every report.

## The published set

1.0 is checked on one machine for each hardware class below. A class without a machine is wanted, but nobody checks it
yet; a report from such a machine is the most useful kind.

| Class | Machine | Details |
| --- | --- | --- |
| Laptop | HP laptop, running Pindu OS | CPU, GPU, Pindu OS image and controller: not recorded yet; they will be recorded in [pindu-os#51](https://github.com/Tim-van-Oudheusden/pindu-os/issues/51) |
| Handheld | Not covered yet | |
| Television PC | Not covered yet | |
| Desktop with an AMD GPU | Not covered yet | |
| Desktop with an NVIDIA GPU | Not covered yet | |

Controllers are a class of their own, because each family reports its buttons, rumble and battery differently:

| Controller family | Checked with |
| --- | --- |
| Xbox | Not covered yet |
| PlayStation (DualShock 4, DualSense) | Not covered yet |
| Nintendo Switch Pro | Not covered yet |
| Steam Deck's built-in controls | Not covered yet |
| Other (8BitDo and the like) | Not covered yet |

The HP laptop's controller pairs over Bluetooth; its family is not recorded yet.

The maintainer adds a machine to the set, or a controller to a family, in a pull request that edits this page. Only
hardware someone has and runs the checks on belongs here.

## The flows checked on each machine

Each machine in the set runs both lists below, on the pre-release of every release (or the final's files for a
[fast final release](releasing.md#fast-final-release)). Fullscreen mode is checked on Pindu OS, because the system
panel's Network, Bluetooth, Power and System Update sections only show there.

### Fullscreen mode

The [hardware checklist](releasing.md#hardware-checklist) in the release guide, every row:

- Fullscreen mode in gamescope
- Controller over Bluetooth
- Wi-Fi with the on-screen keyboard
- Suspend and resume
- Steam game
- Proton game
- System Update

The release's recorded pass stays where the release guide puts it: the "Release X.Y.Z" issue and its
`hardware-check:passed` label, from one machine of the set. Beside that, the maintainer files a hardware report for
each machine in the set that ran the lists, so every release adds the set's results to the [matrix](#matrix).

### Desktop window

| Check | Passes when |
| --- | --- |
| First sync | On first start the library fills with the installed games of the launchers on the machine, Steam at least |
| Browse and search | The grid and the list both show the library, and searching for a title finds it |
| Game page | A game's page opens from the grid and shows its source, install state and playtime |
| Steam game | A Steam game starts from its page, and its playtime counts once it quits |
| Fullscreen mode and back | **Fullscreen Mode** in the header bar (or <kbd>F11</kbd>) opens fullscreen mode, and **Exit Fullscreen Mode** in its menu (Start) returns to the desktop window |
| Debugging Information | **About Pindu launcher → Troubleshooting → Debugging Information** shows the report, and **Copy** copies it |

## Hardware reports

Anyone can file a report with the
[Hardware report](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/new?template=hardware-report.yml) issue
form: the machine, its GPU, the OS or Pindu OS image, the launcher's version and how it was installed, the controller,
and which rows of the two lists above passed, failed or were not run. A report needs no machine from the set, and it
does not need every row: one passing row on hardware nobody has reported yet is worth filing. A row that fails also
gets a bug report of its own, with the debugging information in it.

The form labels the issue `hardware-report`. A report is data, not work to pick up.

### How a report becomes a row

1. **Who:** the maintainer, or a contributor in a pull request the maintainer reviews, adds one row to the matrix below
   for each report, linking its issue. The row copies the report; nobody edits what the reporter wrote.
2. **When:** within a week of the report, and before the next release at the latest. A report that leaves out the
   machine, the launcher version or every row's result gets a comment asking for it, and waits until it has them.
3. The maintainer then closes the report's issue. A report that a later release fixes stays in the matrix as it was;
   a new report on the new version is a new row.

## Matrix

One row per report, newest first. **Fullscreen** and **Desktop** count the rows of the lists above that passed, out of
those run; the linked issue says which failed.

| Report | Class | Machine | GPU | OS or Pindu OS image | Launcher and install | Controller | Fullscreen | Desktop |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| No reports yet | | | | | | | | |
