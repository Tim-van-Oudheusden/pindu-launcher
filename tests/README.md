# Tests

The crates keep their own Rust tests: `#[cfg(test)]` modules and `crates/*/tests/`, run by `cargo test --workspace`
and the `cargo` Meson suite. This tree holds the tests that span the repository:

- `tooling/`: Python `unittest` files for the scripts in `build-aux/` (`check-version.sh`, `check-manifests.sh`,
  `release-notes.py`). Each runs its script on the repository and on broken temporary copies of the files it reads.
  `tests/meson.build` registers them as the `tooling` suite; each file also runs on its own with `python3`.
- `e2e/`: `ui-smoke.sh`, the end-to-end UI smoke test, registered in `tests/meson.build`. It starts the installed app,
  so it needs a display and the install, and skips (exit 77) without them. It runs under xvfb in both Flatpak builds
  and, through `build-aux/package-smoke.sh`, on every native package.
- `scripts/`: Python `unittest` files for the compliance scripts in `scripts/`, which run on the CI runner rather than
  in the GNOME SDK, so CI's `Script tests` job runs them instead of Meson.

```sh
meson test -C _build --suite tooling
meson test -C _build ui-smoke   # after meson install, with a display
python3 -m unittest discover -s tests/scripts
```

[`docs/metrics/README.md`](../docs/metrics/README.md) says what each check measures and where CI runs it.
