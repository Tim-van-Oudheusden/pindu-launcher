# Support policy

This page says, for each way of installing Pindu launcher, which versions get fixes, how an update arrives, which
runtime or distribution release it runs on and until when, and where to report a problem. It is a draft for 1.0
(product analysis, [section 9.5](product-analysis.md#95-what-10-should-promise)); the choices it makes are listed at
the end for the maintainer to approve or change.

Pindu launcher is a one-person project, so the dates below are when support ends at the latest, not promises of a
response time. Security reports follow [`SECURITY.md`](../SECURITY.md).

## Which versions get fixes

Only the latest final release gets fixes. A fix lands on `main` and ships in the next release, usually a patch
version; older releases are not patched and fixes are not backported. This is the same rule as
[`SECURITY.md`](../SECURITY.md#supported-versions).

- **Pre-releases** (`vX.Y.Z-rc.N`) are there to be tried before a final release
  ([Soak a pre-release](releasing.md#soak-a-pre-release)). A bug found in one is fixed in the next pre-release or in
  the final release, never in that pre-release.
- **`main`** and the development bundles CI builds from it get fixes as they land, but are not releases: use them to
  try a fix, not to stay on.
- **A broken release** is marked as a pre-release on GitHub, so it is no longer "latest", and a fixed patch version
  replaces it ([When something goes wrong](releasing.md#when-something-goes-wrong)).

Every install type below gets the same launcher version from the same tag. They differ in how the update reaches you
and in what the launcher runs on.

## Install types

| Install type | Architectures | How an update arrives | Runs on | Supported until |
| --- | --- | --- | --- | --- |
| Flatpak bundle from a GitHub release | x86_64, aarch64 | You install the next release's bundle | GNOME 51 runtime | The runtime's end of life, expected in September 2027 |
| RPM for Fedora 45 | x86_64, aarch64 | You install the next release's RPM, or `dnf upgrade` from COPR | Fedora 45 | 8 December 2027 |
| DEB for Ubuntu 26.10 | amd64, arm64 | You install the next release's DEB | Ubuntu 26.10 | 1 July 2027 |
| DEB for Debian testing | amd64, arm64 | You install the next release's DEB | Debian testing at the time of the release | No fixed date; see below |
| Pindu OS | x86_64 | With the system image, after the hardware check | Pindu OS (Fedora 45) | As long as Pindu OS ships the launcher |
| AUR and nixpkgs recipes | x86_64, aarch64 | Your package manager, after the recipe bump | Arch Linux, NixOS | Each distribution's own rolling or release schedule |
| Built from source | Any | You build the next tag or `main` | GTK 4.24 and libadwaita 1.10 or newer | As long as those are what the latest release needs |

### Flatpak bundle from a GitHub release

Each [release](https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/latest) has a bundle for x86_64 and one
for aarch64 ([Installing](../README.md#flatpak-bundle-from-a-release)).

- **Updates:** a bundle does not update itself, and `flatpak update` does not find a newer one. Install the next
  release's bundle the same way; the library and settings stay. **Watch → Custom → Releases** on the repository page
  tells you when a release is out.
- **Runtime:** the bundle runs on `org.gnome.Platform//51`, which Flatpak installs and updates from Flathub. A GNOME
  runtime branch is usually supported for a year and reaches its end of life when a newer one is released
  ([Flatpak documentation](https://docs.flatpak.org/en/latest/available-runtimes.html#gnome)): GNOME 48's ended on
  24 March 2026, a year after GNOME 48 came out
  ([Ubuntu Discourse](https://discourse.ubuntu.com/t/the-gnome-48-runtime-is-no-longer-supported-as-of-march-24-2026-ubuntu-24-04-4-lts-flatpak/80598)).
  GNOME 51 came out on 16 September 2026 ([GNOME Discourse](https://discourse.gnome.org/t/gnome-51-released/38820)),
  so its runtime is expected to reach its end of life around September 2027, when GNOME 53 is released. The launcher
  moves to a newer runtime before then, in a release of its own; from that release on, the bundle needs the new
  runtime, which Flatpak installs with it.
- **Not supported:** a bundle older than the latest release, and any bundle once its runtime is past its end of life.

### Native packages: Fedora, Debian and Ubuntu

Each release has an RPM for Fedora 45 and DEBs for Debian testing and Ubuntu 26.10, for both architectures
([Installing](../README.md#native-package)). They run without the Flatpak sandbox.

- **Updates:** a package downloaded from a release does not update itself. Install the next release's package with
  `sudo dnf install ./<file>` or `sudo apt install ./<file>`. The COPR project builds the same spec for Fedora 45 and
  rawhide after each final release ([Distribution recipes](releasing.md#distribution-recipes)), so a system that uses
  it gets updates with `dnf upgrade`.
- **Fedora 45:** supported until Fedora 45's end of life on 8 December 2027
  ([Fedora 45 schedule](https://fedorapeople.org/groups/schedule/f-45/f-45-key-tasks.html),
  [endoflife.date](https://endoflife.date/fedora)).
- **Ubuntu 26.10:** an interim release with nine months of support, until 1 July 2027
  ([Ubuntu release cycle](https://ubuntu.com/about/release-cycle), [endoflife.date](https://endoflife.date/ubuntu)).
- **Debian testing:** testing moves all the time and has no end-of-life date. Each DEB is built against testing as it
  is on the day of the release, and supported on testing until the next release; a newer testing that breaks the
  package is fixed in a new release, not in the old package. Debian stable is not supported until a stable release
  ships GTK 4.24 and libadwaita 1.10.
- **When a distribution release reaches its end of life,** the first launcher release after that date no longer
  builds a package for it, and the release notes say so. Adding a newer distribution release (Fedora 46,
  Ubuntu 27.04, a new Debian stable) is a decision of its own, made in an issue, not by this policy.
- **Not supported:** a package older than the latest release, and a package installed on a distribution release it
  was not built for.

### Pindu OS

[Pindu OS](https://github.com/Tim-van-Oudheusden/pindu-os) is an x86_64 console image on Fedora 45 that installs the
launcher's release RPM, pinned by version and checksum. The app reports it as its own install type ("Installed as:
Pindu OS" in its debugging information), because it updates differently from a native package. Pindu OS is early and
has published no `:stable` image yet, so what follows describes how it works once it does.

- **Updates:** the launcher arrives with the system image, never on its own. Pindu OS follows final releases only.
  Its `:testing` channel gets a final release once Pindu OS pins that release's RPM, and its `:stable` channel
  promotes it only once the release's hardware check has passed (`hardware-check:passed` on the "Release X.Y.Z"
  issue, [Hardware checklist](releasing.md#hardware-checklist)). Until then `:stable` keeps the previous launcher.
  The console downloads an update in the background and applies it the next time it starts, and **Roll Back** in
  Quick Access → System returns to the previous image ([System Update](../README.md#system-update)).
- **Versions fixed:** the launcher in the latest `:stable` image. Because `:stable` waits for the hardware check, it
  may be one final release behind for a few days; a bug that the latest release fixes is fixed on the console once
  that release reaches `:stable`.
- **Runs on:** whatever Pindu OS's image is built from (Fedora 45 today). How long an image is supported, and when it
  moves to a newer Fedora, is Pindu OS's policy, not this one's.

### AUR and nixpkgs recipes

`build-aux/aur/PKGBUILD` and `build-aux/nix/package.nix` build the release tarball offline. They follow final
releases only, and are bumped by a pull request that the Release workflow opens after each final release
([Distribution recipes](releasing.md#distribution-recipes)).

- **Updates:** through `pacman` with an AUR helper, or `nix`, once the bumped recipe reaches the AUR or nixpkgs. That
  can take a few days after the release.
- **Runs on:** Arch Linux and NixOS as they are when the recipe is bumped. Both move on their own schedule, so the
  recipe is supported on the current state of the distribution, not on old snapshots.
- **Not supported:** the CI release workflow does not build or test these recipes. A recipe is checked by hand when
  it is bumped, and fixed in the next bump.

### Built from source

[Building from source](../README.md#building-from-source) is for development and for distributions that package the
app themselves. The latest tag and `main` are supported on a system with what the latest release needs (today GTK
4.24, libadwaita 1.10 and the Rust version in the README).

## Once the app is on Flathub

The Flathub listing (#63) is the maintainer's work. Once it exists:

- Flathub becomes the recommended way to install the Flatpak. Updates then arrive with `flatpak update` or the
  software centre, and the runtime moves on Flathub's schedule.
- GitHub releases keep their bundles, built by the same Release workflow, for machines without Flathub and for trying
  a pre-release. They keep the support above: the latest release only, updated by hand.
- A bundle and the Flathub app share the app ID, so they cannot be installed side by side. To switch, uninstall the
  bundle with `flatpak uninstall io.github.tim_van_oudheusden.pindu-launcher` (without `--delete-data`, which keeps the
  library and settings) and install from Flathub.

## Where to report a problem

| Problem | Where |
| --- | --- |
| A bug in the launcher, whatever the install type | A [bug report](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/new?template=bug.yml) in this repository; the form asks how you installed it |
| A package or recipe from this repository that does not install or build | A bug report here, with **Installing or packaging** as the area |
| The console itself: installing, updating or rolling back Pindu OS, or a console that does not start | [Pindu OS's issues](https://github.com/Tim-van-Oudheusden/pindu-os/issues/new) |
| A security problem | Privately, as [`SECURITY.md`](../SECURITY.md#reporting-a-vulnerability) says |
| A package built by someone else (another distribution's package or repository) | That package's maintainer first |

Report a problem against the latest release where you can. A report from an older version may already be fixed: the
first answer is usually to update.

## Choices for the maintainer

This draft makes these choices; approve or change them in review.

1. **Latest release only.** Only the latest final release gets fixes, with no backports and no long-term branch, as
   `SECURITY.md` already says. A 1.x line with patch releases on an older minor version is not offered.
2. **Distribution releases end with their own end of life.** Fedora 45 until 8 December 2027 and Ubuntu 26.10 until
   1 July 2027; the first release after that drops the package. Which newer releases replace them is left to an issue
   at the time.
3. **Debian testing has no end date.** Each DEB is supported on testing until the next launcher release; Debian stable
   is not supported until it ships GTK 4.24 and libadwaita 1.10.
4. **The runtime moves before its end of life.** The bundle stays on GNOME 51 until a release moves it to a newer
   runtime, at the latest when GNOME 53 comes out (expected September 2027).
5. **Pindu OS is its own install type.** The launcher there is supported as the version in `:stable`, which waits for
   the hardware check, and the bug form lists Pindu OS as an option. Console problems go to Pindu OS's tracker.
6. **AUR and nixpkgs recipes are best effort.** They are bumped after each final release but not built by CI, and they
   are listed as their own option in the bug form.
7. **GitHub bundles continue after Flathub.** Flathub becomes the recommended Flatpak, and the Release workflow keeps
   attaching bundles with the same support.
