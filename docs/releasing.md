# Releasing

GitHub Releases are the release channel. A `v*` tag runs `.github/workflows/release.yml`, which builds the stable
Flatpak from `build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher.yml`, a source tarball with the crates
vendored (`meson dist`), and from that tarball an RPM for Fedora 45 and `.deb`s for Debian testing and Ubuntu 26.10.
The bundle and the packages are built for x86_64 and aarch64, each natively on a GitHub runner of that architecture.
It publishes them with checksums and notes, and signs build provenance for the RPMs. Nothing is uploaded by hand.

```mermaid
flowchart LR
  tag[push tag vX.Y.Z] --> version[check tag = meson = Cargo = MetaInfo = RPM = DEB]
  version --> flatpak[build bundle, meson test, flatpak-builder-lint]
  version --> sources[meson dist: vendor crates, build and test offline]
  sources --> packages[RPM and DEBs, built offline]
  packages --> smoke[install in clean containers, UI smoke test, cover decoded through glycin]
  smoke --> provenance[RPM build provenance, signed through Sigstore]
  flatpak --> publish[GitHub release: bundle, tarball, packages, SHA256SUMS, notes]
  sources --> publish
  provenance --> publish
  publish -->|final release only| recipes[pull request: AUR and nixpkgs recipe bump]
```

## Cut a release

1. **Open an issue** ("Release X.Y.Z") from the [Release issue form](../.github/ISSUE_TEMPLATE/release.yml), which
   carries the [hardware checklist](#hardware-checklist), in the M5 milestone, or whichever one is current, and branch
   `release/<issue>-vX.Y.Z` off an up-to-date `main`.
2. **Bump the version** in `meson.build` (`project(version:)`), `Cargo.toml` (`[workspace.package] version`) and
   `build-aux/rpm/pindu-launcher.spec` (`Version:`, plus a `%changelog` entry), add an entry at the top of
   `build-aux/deb/debian/changelog` (`dch -v X.Y.Z-1`, or by hand), then run `cargo check` so `Cargo.lock` follows.
3. **Write the notes** as a new `<release version="X.Y.Z" date="YYYY-MM-DD">` at the top of `<releases>` in
   `data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in`: one `<p>` and a `<ul>` of user-visible changes
   since the last tag (`git log vPREVIOUS..`). The same text becomes the release notes on GitHub and in software
   centres, so write it for users. Preview it with `build-aux/release-notes.py X.Y.Z`.
4. **Refresh the translation template**, which carries the MetaInfo strings: `meson compile -C _build
   pindu-launcher-pot`. CI's `po/check-pot.sh` fails otherwise.
5. **Check** with `meson test -C _build`: its `versions` test fails while the versions disagree.
6. **Land it** through a pull request as usual.
7. **Tag the merge commit** on `main` and push the tag:

   ```sh
   git switch main && git pull
   git tag -a vX.Y.Z -m "Pindu launcher X.Y.Z"
   git push origin vX.Y.Z
   ```

8. **Watch the [Release workflow](https://github.com/Tim-van-Oudheusden/pindu-launcher/actions/workflows/release.yml)**.
   When it is green, the [release](https://github.com/Tim-van-Oudheusden/pindu-launcher/releases) has, for each
   architecture, `pindu-launcher-X.Y.Z-<x86_64|aarch64>.flatpak`, `pindu-launcher-X.Y.Z-1.fc45.<x86_64|aarch64>.rpm`,
   `pindu-launcher-X.Y.Z-debian-testing-<amd64|arm64>.deb` and `pindu-launcher-X.Y.Z-ubuntu-26.10-<amd64|arm64>.deb`,
   plus `pindu-launcher-X.Y.Z.tar.xz` and `SHA256SUMS`. Install the bundle (the notes say how), start it, and check
   that it opens and imports a library. Check an RPM's [build provenance](#verify-an-rpms-build-provenance).
9. **Run the [hardware checklist](#hardware-checklist)** on the release's files, on one real machine, and record the
   result on the "Release X.Y.Z" issue. Run it on a pre-release, then tag the final version, so a failed check never
   reaches a final release.

The native packages target the distros that ship GTK 4.24 and libadwaita 1.10 (#232). When a new Fedora, Debian or
Ubuntu release qualifies, add it to the `packages` and `packages-smoke` matrices in `packaging.yml` and `release.yml`
and to the table in `build-aux/release-notes.py`.

## Soak a pre-release

A final release first soaks as a pre-release on real machines. Pindu OS follows only final releases, so nothing
reaches its users before the soak ends.

1. **Cut a pre-release** with the steps above, under the same "Release X.Y.Z" issue, with the version written the way
   each tool sorts a pre-release below the final version (`build-aux/check-version.sh` checks all of them):

   | Where | First pre-release of 0.5.0 |
   | --- | --- |
   | `meson.build`, `Cargo.toml`, the tag | `0.5.0-rc.1`, tag `v0.5.0-rc.1` |
   | RPM spec `Version:` and `%changelog` | `0.5.0~rc1`, changelog entry `0.5.0~rc1-1` |
   | Debian changelog | `0.5.0~rc1-1` (`dch -v 0.5.0~rc1-1`) |
   | MetaInfo | `<release version="0.5.0~rc1" type="development" date="YYYY-MM-DD">` |

   The Release workflow publishes a `vX.Y.Z-rc.N` tag as a GitHub pre-release that never becomes "latest". Its files
   are named after the tag (`pindu-launcher-0.5.0-rc.1-x86_64.flatpak`, `pindu-launcher-0.5.0-rc.1-1.fc45.x86_64.rpm`
   and so on), because GitHub renames files with a `~`; the packages inside are `0.5.0~rc1`, which `dnf` and `apt`
   upgrade to `0.5.0`.
2. **Run the hardware checklist** on the pre-release's files, on one real machine, and record the result on the
   "Release X.Y.Z" issue.
3. **Wait a few days** while the pre-release runs on real machines. A bug that needs a fix goes through a pull request
   and a new pre-release (`rc.2`), whose soak starts again with step 2.
4. **Tag the final version** once the soak passes, with steps 2 to 8 above: a pull request sets `X.Y.Z` in
   `meson.build`, `Cargo.toml` and the RPM spec, adds `X.Y.Z-1` entries to the RPM and Debian changelogs, and turns the
   MetaInfo's pre-release `<release>` into `<release version="X.Y.Z" date="YYYY-MM-DD">` without `type`; then tag its
   merge commit `vX.Y.Z`. The final must ship the code that soaked: if anything but that version bump has landed on
   `main` since the pre-release, cut another pre-release instead and soak that.

## Distribution recipes

COPR, the AUR and nixpkgs build the release tarball, `pindu-launcher-X.Y.Z.tar.xz`, whose crates are vendored, so
each builds offline (#382). They follow final releases only, as Pindu OS does: a `vX.Y.Z-rc.N` pre-release is never
built or bumped there, so their users only get a version once its soak has passed. The tarball's checksum exists only
once the final release is published, so the AUR and nixpkgs recipes are bumped after step 8 of the final release,
through a pull request of their own; `meson test`'s `versions` check leaves them out.

**The Release workflow opens that pull request by itself.** Once a final release is published, its `recipes` job runs
`scripts/recipe-bump.py`, which takes the tarball's checksum from the release's `SHA256SUMS` and sets the AUR and
nixpkgs lines below, then pushes `chore/recipes-X.Y.Z` and opens the pull request, which refers to the "Release X.Y.Z"
issue. A pre-release skips the job, and a rerun does nothing once the pull request is open or `main`'s recipes build
the version. A pull request opened with the workflow's token starts no workflow: **close and reopen it** to run the
checks, try the recipes as below, and merge it (it is high risk, `build-aux/**`). Then copy the PKGBUILD and a fresh
`.SRCINFO` to the AUR repository. When the job fails, run the script by hand:
`python3 scripts/recipe-bump.py X.Y.Z SHA256SUMS`, with the release's `SHA256SUMS` downloaded.

- **COPR** (`.copr/Makefile`, the make_srpm method): nothing to bump. It builds `build-aux/rpm/pindu-launcher.spec`,
  already bumped in step 2, downloads the spec's `Source0` and checks it against the release's `SHA256SUMS`, so start
  the COPR build only once the final release is published. While `main`'s spec holds a pre-release version
  (`X.Y.Z~rcN`), `make srpm` refuses to run: the tarball of tag `vX.Y.Z-rc.N` is not where `Source0` points, and COPR
  ships final releases only. Try it in a `fedora:45` container with
  `make -f .copr/Makefile srpm outdir=/tmp/srpm`, then `dnf builddep` and `rpmbuild --rebuild` on the `.src.rpm`.
  The COPR project builds for the Fedora releases that ship GTK 4.24 and libadwaita 1.10: Fedora 45 and rawhide,
  x86_64 and aarch64.
- **AUR** (`build-aux/aur/PKGBUILD`): set `pkgver`, reset `pkgrel=1` and set `sha256sums`, or run `updpkgsums`. Then
  regenerate the metadata with `makepkg --printsrcinfo > .SRCINFO`, both on Arch, and try it with `makepkg -s` as a
  non-root user in an `archlinux` container. The AUR repository gets these two files at its root.
- **nixpkgs** (`build-aux/nix/package.nix`): set `version` and the `src` hash, the tarball's SHA-256 in SRI form:
  `nix hash convert --hash-algo sha256 --to sri <sha256>`. There is no `cargoHash`: the build uses the tarball's
  vendored crates. Try it in a `nixos/nix` container with
  `nix-build -E 'with import <nixpkgs> { }; callPackage ./build-aux/nix/package.nix { }'`.

## Verify an RPM's build provenance

The Release workflow's `provenance` job signs a [build provenance
attestation](https://docs.github.com/en/actions/security-for-github-actions/using-artifact-attestations) for each RPM
with `actions/attest-build-provenance`, before the release is published. It records which workflow run built the file,
from which commit, and is signed through Sigstore with the run's identity, so no one can forge it without this
repository's workflow. Pindu OS asks for it, so it can check a release RPM before it installs one. Check one with the
GitHub CLI:

```sh
gh release download vX.Y.Z -R Tim-van-Oudheusden/pindu-launcher -p 'pindu-launcher-*.fc45.x86_64.rpm'
gh attestation verify pindu-launcher-X.Y.Z-1.fc45.x86_64.rpm -R Tim-van-Oudheusden/pindu-launcher \
  --signer-workflow Tim-van-Oudheusden/pindu-launcher/.github/workflows/release.yml
```

It prints `Verification succeeded!` and the workflow run that built the file; a changed file, or one built anywhere
else, fails. A rerun of the workflow by hand signs the rebuilt files again; that attestation names the branch the run
was started from as its source, and the tag is the run's input.

## When something goes wrong

- **The version check fails:** the tag does not match the tree. Delete the tag (`git push origin :vX.Y.Z` and
  `git tag -d vX.Y.Z`), fix the versions through a pull request, and tag again.
- **The build, a test or the lint fails:** nothing is published. Fix it through a pull request and release a new patch
  version; a published tag is never moved.
- **The publish step fails** (a network or permissions error): run the Release workflow by hand from the Actions tab
  with the tag as input. It rebuilds the tag and replaces the files of an existing release.
- **A published release is broken:** mark it as a pre-release or delete it on GitHub, and release a fixed patch
  version. Keep the tag, so the version is never reused.

## Hardware checklist

Fullscreen mode's system panel, controllers, gamescope and Proton only fail on real hardware, so every release runs
this checklist once, by hand, on one real machine. Run it on the pre-release before the final tag; Pindu OS waits for a
recorded pass before it promotes the new launcher from `:testing` to `:stable`
([pindu-os#30](https://github.com/Tim-van-Oudheusden/pindu-os/issues/30)).

**The machine** runs Pindu OS with the build under test installed, because the system panel's Network, Bluetooth,
Power and System Update sections only show there (see [System panel](system-panel.md)). It needs a Bluetooth adapter,
Wi-Fi, a Bluetooth controller, and a Steam library with one native game and one Windows game set to run through Proton.

| Check | Passes when |
| --- | --- |
| Fullscreen mode in gamescope | The launcher starts fullscreen in the gamescope session, the controller drives it, a game takes the focus and the launcher gets it back when the game quits |
| Controller over Bluetooth | **Quick Access → Bluetooth** pairs the controller, which then moves through the library, opens a game's page and goes back |
| Wi-Fi with the on-screen keyboard | **Quick Access → Network** joins a WPA2 network, with the passphrase typed on the on-screen keyboard with the controller |
| Suspend and resume | **Power → Suspend** puts the machine to sleep; after waking, the launcher, the controller and the network work again |
| Steam game | A native Steam game starts from the launcher, and its playtime counts once it quits |
| Proton game | A Windows game through Proton starts from the launcher, and its playtime counts once it quits |
| System Update | **Quick Access → System Update** shows the installed version, and the update state and actions it offers work |

**Record it on the "Release X.Y.Z" issue**, which the [Release issue form](../.github/ISSUE_TEMPLATE/release.yml)
creates with the checklist in it. The title is exactly `Release X.Y.Z`, the final version without `v` or `-rc.N`, so
one issue holds the checks for every pre-release of that version:

1. Fill in **Hardware tested** (the machine, the Pindu OS image, the controller, the Wi-Fi network) and **Build tested**
   (the tag and the file installed, for example `v0.5.0-rc.1` and `pindu-launcher-0.5.0-rc.1-1.fc45.x86_64.rpm`).
2. Tick each check that passes. Comment on a check that fails, with what happened, and file a bug for it.
3. When every check is ticked for one build, the maintainer adds the label **`hardware-check:passed`** to the issue.
   That label on that issue is the recorded pass; nothing else counts. The issue may be open or closed.

A failed check leaves the label off. The fix ships in a new pre-release, and the checklist runs again on it: untick the
boxes and update **Build tested**. If the label was already added and a later build is the one that ships, remove the
label until the new build passes.

Pindu OS looks the pass up with:

```sh
gh issue list -R Tim-van-Oudheusden/pindu-launcher --state all --label hardware-check:passed \
  --search '"Release X.Y.Z" in:title' --json title,url --jq '.[] | select(.title == "Release X.Y.Z") | .url'
```
