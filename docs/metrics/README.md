# Metrics

The repository's metrics: the gates every change passes, what each one measures and what a failure means, where its
result is published, and how to reproduce it locally. Every metric here comes from a command, script or workflow in the
repository; nothing is collected from the app or its users, which sends no telemetry.

Results are published as GitHub Actions **checks** on each pull request and push to `main` (open the run on the
[Actions tab](https://github.com/Tim-van-Oudheusden/pindu-launcher/actions) for the logs) and, where noted, as a
downloadable **run artifact**. The gates and their settings live in version control (`Cargo.toml`'s
`[workspace.lints]`, `clippy.toml`, `rustfmt.toml`, the Meson tests, `build-aux/flatpak/lint-exceptions.json`), so a
change to one is reviewed like code. [`CONTRIBUTING.md`](../../CONTRIBUTING.md#quality-gates) says which to run before
opening a pull request.

## Index

| Metric | Measures | Source | Cadence | Published as |
| --- | --- | --- | --- | --- |
| [Formatting](#formatting) | Code matches `rustfmt.toml` | `cargo fmt --all --check` in `ci.yml` (`checks`) | Every PR and push to `main` | `Rust and Meson checks` |
| [Lints](#lints) | The Rust for Linux lint set and clippy, warnings as errors | `cargo clippy` in `ci.yml` (`checks`) | Same | `Rust and Meson checks` |
| [Documentation](#documentation) | Rustdoc lints and docs on every public item | `cargo doc` in `ci.yml` (`checks`) | Same | `Rust and Meson checks` |
| [Unit and fixture tests](#unit-and-fixture-tests) | Behaviour of the core, the importers against fixtures, and the UI crate's logic | `cargo test --workspace` in `ci.yml` (`checks`), and the `cargo` Meson suite | Same | `Rust and Meson checks` |
| [Data files](#data-files) | Desktop file, MetaInfo and GSettings schema are valid | `data` Meson suite | Same | `Rust and Meson checks`; inside both Flatpak builds |
| [Translations](#translations) | Every file with strings is in `POTFILES.in`; the template matches the code | `po` Meson suite, `po/check-pot.sh` | Same | `Rust and Meson checks` |
| [Versions](#versions) | Meson, Cargo, MetaInfo, RPM and DEB name one version | `versions` Meson test (`build-aux/check-version.sh`) | Every PR and push; on a tag against the tag | `Rust and Meson checks`; Release's `Check the tag against the versions` |
| [Manifest parity](#manifest-parity) | Release manifest grants exactly the development manifest's permissions and modules | `manifests` Meson test (`build-aux/check-manifests.sh`) | Every PR and push | `Rust and Meson checks` |
| [Tooling tests](#tooling-tests) | The version, manifest and release-notes scripts catch what they are meant to catch | `tooling` Meson suite (`tests/tooling/`) | Every PR and push | `Rust and Meson checks`; inside both Flatpak builds |
| [UI smoke test](#ui-smoke-test) | The installed app starts and stays up without a panic or GLib critical | `ui-smoke` Meson test (`tests/e2e/ui-smoke.sh`) | Every PR and push, in both Flatpak builds and every native package | `Flatpak build, …` and `Native package install and UI smoke test` checks |
| [Flatpak build and lint](#flatpak-build-and-lint) | Both manifests build, test and pass `flatpak-builder-lint` | `ci.yml` (`flatpak`), `release.yml` (`flatpak`) | Every PR and push; every release tag | `Flatpak build, UI smoke test and lint (development/release)`; bundle artifacts |
| [Source tarball](#source-tarball) | `meson dist` vendors the crates and builds and tests offline | `ci.yml` and `release.yml` (`sources`) | Same | `Source tarball with vendored crates`; `sources` artifact |
| [Native packages](#native-packages) | RPM and DEBs build offline from the tarball, install in clean containers and start | `ci.yml` and `release.yml` (`packages`, `packages-smoke`) | Same | `Native package (…)` and smoke checks; `package-*` artifacts |

## Formatting

`rustfmt` with the settings in `rustfmt.toml`. A failure means a file differs from what `cargo fmt --all` writes.

```sh
cargo fmt --all --check
```

## Lints

The workspace lint set (`[workspace.lints]` in `Cargo.toml`, from the Linux kernel's Rust flags) plus clippy with
`clippy.toml`, on every target, with warnings as errors. A failure is a real finding to fix; a lint that is wrong for
one item gets `#[expect(lint, reason = "...")]`, never `#[allow]` ([Lints](../../AGENTS.md#lints)). The generated
libmanette bindings are exempt.

```sh
cargo clippy --workspace --all-targets -- -D warnings
```

## Documentation

Rustdoc with warnings as errors: broken intra-doc links and the rustdoc lints. With `missing_docs` in the lint set,
every public item, field, variant and constant carries documentation.

```sh
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
```

## Unit and fixture tests

The workspace's tests: `pindu-core`'s unit tests need no display, and its importer tests in
`crates/pindu-core/tests/import/` read fixture copies of each launcher's data (`crates/pindu-core/tests/fixtures/`) in
native and Flatpak layouts. A failure means behaviour changed; fix the code, never the test
([Testing](../../AGENTS.md#testing)). Meson runs the same tests as its `cargo-test` test in the `cargo` suite, so they
also run inside the Flatpak builds and the tarball check.

```sh
cargo test --workspace
meson test -C _build --suite cargo
```

## Data files

The `data` Meson suite: `validate-desktop` (`desktop-file-validate` on the desktop file), `validate-metainfo`
(`appstreamcli validate --no-net` on the MetaInfo, which also carries the release notes) and `validate-schema`
(`glib-compile-schemas --strict` on the GSettings schema). The Blueprint files are compiled by the build itself, so a
broken one fails `meson compile`.

```sh
meson test -C _build --suite data
```

## Translations

`check-potfiles` (the `po` suite, `po/check-potfiles.py`) fails when a file with translatable strings is missing from
`po/POTFILES.in`. `po/check-pot.sh` fails when `po/pindu-launcher.pot` misses a string the code has or keeps one it no
longer has, ignoring source locations and dates; refresh it with `meson compile -C _build pindu-launcher-pot`.

```sh
meson test -C _build --suite po
po/check-pot.sh _build
```

## Versions

`build-aux/check-version.sh` compares the version in `meson.build`, `Cargo.toml`, the MetaInfo's newest `<release>`,
the RPM spec and the newest Debian changelog entry. With a tag argument, as the Release workflow passes, the tag must be
that version with a `v` prefix, and `build-aux/release-notes.py` must find notes for it. A failure means a release bump
missed a file ([`docs/releasing.md`](../releasing.md)).

```sh
meson test -C _build versions
build-aux/check-version.sh vX.Y.Z
```

## Manifest parity

`build-aux/check-manifests.sh` compares everything from `finish-args:` on in the two manifests in
`build-aux/flatpak/`, minus the lines that differ on purpose (the development build's debug environment, profile and
build type). A failure means the release manifest's permissions or modules drifted from the development manifest's;
permissions themselves are the maintainer's call ([`SECURITY-AI.md`](../security/SECURITY-AI.md)).

```sh
meson test -C _build manifests
```

## Tooling tests

The `tooling` Meson suite runs the Python `unittest` files in `tests/tooling/` against the scripts in `build-aux/`:
`check-version.sh` and `check-manifests.sh` must pass on the repository and fail on temporary copies with a mismatched
version, a mismatched tag, or a release manifest that gains a permission or a module, and `release-notes.py` must print
the newest MetaInfo release's notes and fail for an unknown version. A failure means a script no longer does what CI
and the Release workflow rely on it to do.

```sh
meson test -C _build --suite tooling
python3 tests/tooling/test_check_version.py   # one file, without Meson
```

## UI smoke test

`tests/e2e/ui-smoke.sh` starts the installed app with an empty library and settings, on its own D-Bus session, with
`G_DEBUG=fatal-criticals`, and fails if it is not still running five seconds later. It needs a display and the
installed resources, so a plain `meson test` skips it (exit 77); it runs under xvfb in both Flatpak builds and, through
`build-aux/package-smoke.sh`, in every native package.

```sh
meson test -C _build ui-smoke   # after meson install, with a display
```

## Flatpak build and lint

The `flatpak` job builds the development and the release manifest with `flatpak-builder` and `run-tests`, so the Meson
tests, the UI smoke test included, run inside the GNOME 51 SDK after the install. It then lints the manifest and the
exported repository (MetaInfo, desktop file, icons) with `flatpak-builder-lint`; the exceptions in
`build-aux/flatpak/lint-exceptions.json` each say why. The development bundle is uploaded as an artifact
([Installing](../../README.md#installing)).

```sh
flatpak-builder --user --install --force-clean _flatpak \
  build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml
flatpak-builder-lint --exceptions --user-exceptions build-aux/flatpak/lint-exceptions.json manifest \
  build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml
```

## Source tarball

`meson dist` runs `build-aux/dist-vendor.sh`, which vendors the locked crates into the tarball, then builds and tests
the tarball offline, as a distribution's package builder would. A failure means the release tarball could not build
without network.

```sh
meson setup _dist && meson dist -C _dist --formats xztar
```

## Native packages

`build-aux/rpm/build.sh` and `build-aux/deb/build.sh` build an RPM for Fedora 45 and DEBs for Debian testing and
Ubuntu 26.10 offline from the tarball, each in its distribution's container. `build-aux/package-smoke.sh` then installs
each package in a clean container with only the dependencies it declares and runs the UI smoke test under Xvfb. A
failure means a package misses a dependency or a file. When a new distribution release qualifies, add it to both
workflows' matrices ([`docs/releasing.md`](../releasing.md)).

```sh
podman run --rm -v "$PWD:/src:Z" -w /src registry.fedoraproject.org/fedora:45 \
  build-aux/rpm/build.sh _dist/meson-dist/pindu-launcher-*.tar.xz packages
```

## Adding a metric

1. Compute it with a command or a script under `build-aux/` that runs the same locally and in CI, and give it a Meson
   test where it can run offline.
2. Run it from a workflow, with only the token permissions it needs, and publish the result as a check, plus an
   artifact if it is worth downloading.
3. Add a row to the index and a section saying what it measures, what a failure means and how to reproduce it.
