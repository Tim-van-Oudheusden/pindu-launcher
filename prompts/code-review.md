# Code Review Prompt

Review the diff between `<FIXED_POINT>` and `<HEAD>` in the `pindu-launcher` repository (native GNOME game library in
Rust on GTK4 and libadwaita: `crates/pindu-core` GTK-free core, `crates/pindu-ui` desktop and fullscreen shells,
Blueprint UI in `data/ui/`, Meson driving cargo, Flatpak).

Check two axes and report them separately:

## Standards

- TDD compliance: tests were written one at a time for the behaviour being changed, assert observable outputs (not
  private state or widget internals), and the full suite passes (`cargo test --workspace`).
- Logic that needs no GTK lives in `crates/pindu-core` with plain `cargo test` tests; `crates/pindu-ui` stays a thin
  shell that observes models and calls services, and never reads files or spawns processes.
- Parsers and location resolvers are tested against fixture trees, in both native and Flatpak layouts.
- No test weakened, deleted or marked `#[ignore]` to make CI pass.
- Rust for Linux lints: every public item documented in plain sentences; every `unsafe` block has a `// SAFETY:`
  comment and every `unsafe fn` a `# Safety` section; `pub(crate)` unless reachable from the crate root;
  `#[expect(lint, reason = "...")]`, never `#[allow]`, and never to silence a real finding; no `dbg!`, no raw pointer
  `as` casts.
- Adwaita first: stock libadwaita widgets and style classes, system accent colour and typography, no custom theme; UI
  layout in Blueprint files; no console branding.
- User-facing strings go through gettext, in British English, the file is listed in `po/POTFILES.in`, and the template
  is refreshed (`meson compile -C _build pindu-launcher-pot`; CI runs `po/check-pot.sh _build`).
- No new, upgraded or removed dependency without explicit approval (crate, exact version, why).
- No new `finish-args` permissions or bundled modules in `build-aux/flatpak/*.yml`; no Flathub manifest or submission
  changes; `AI_DISCLOSURE.md` untouched unless the maintainer edits it.
- Secrets stay in the keyring (oo7); game launches use argv lists, never shell strings, and never `--watch-bus`.
- Gates green:
  ```sh
  cargo fmt --all --check
  cargo clippy --workspace --all-targets -- -D warnings
  RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
  cargo test --workspace
  meson setup _build && meson test -C _build
  ```
  `meson setup` refuses an existing `_build`; use `meson setup --reconfigure _build` after the first time. Also build
  the development manifest with `flatpak-builder --user --install --force-clean _flatpak
  build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml` and run `flatpak-builder-lint --exceptions
  --user-exceptions build-aux/flatpak/lint-exceptions.json` on it; CI (`.github/workflows/ci.yml`) runs the same.
- Conventional Commits, one commit per issue, issue number referenced.

## Spec

- Every acceptance criterion named in `<ISSUE>` is implemented and tested.
- Where the issue and the spec section it links disagree, the issue wins and the PR says so.
- No unrequested behaviour (scope creep); no partially implemented criteria.

Quote the offending line and source rule for each finding. Under 400 words.
