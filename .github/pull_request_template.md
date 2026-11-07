<!--
Title follows Conventional Commits and names the issue, e.g. `feat(import): import Steam libraries (#8)`.
The branch holds one commit that references the issue.
-->
## Summary

**What:** <!-- one or two sentences: the change -->

**Why:** <!-- the problem it solves; say so here if the issue and the spec disagree (the issue wins) -->

Fixes #<!-- issue number; use "Refs #N" and say what remains if the issue cannot be closed yet -->

## Changes

- <!-- one line per change -->

## Testing

<!-- Evidence, not claims. Paste failing-before/passing-after output for the changed path. -->

- [ ] New or changed behaviour is covered by a test written first (TDD), one at a time, each confirmed failing before
  the implementation
- [ ] Tests assert observable behaviour, not implementation internals; parsers run against fixture trees
- [ ] UI changes checked in the development Flatpak, keyboard-only and in a narrow window included

## Quality gates

From AGENTS.md, Session Completion. Tick the ones you ran; say why if one does not apply.

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`
- [ ] `cargo test --workspace`
- [ ] `meson setup _build && meson test -C _build` (`meson setup --reconfigure _build` after the first time)
- [ ] `flatpak-builder --user --install --force-clean _flatpak build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml`
- [ ] `flatpak-builder-lint --exceptions --user-exceptions build-aux/flatpak/lint-exceptions.json` on that build

## Review rubric

- [ ] Adwaita first: stock libadwaita widgets and style classes, layout in Blueprint, no custom theme
- [ ] User-facing strings go through gettext, and `po/pindu-launcher.pot` is refreshed
- [ ] Rust for Linux lints respected: docs on every public item, `// SAFETY:` comments, `#[expect(..., reason = "...")]`
  rather than `#[allow]`
- [ ] No new `finish-args` or bundled modules, and the Flathub manifest is untouched
- [ ] No new, upgraded or removed dependency, or it is listed under Dependencies with the maintainer's approval

## AI disclosure

<!--
AI_DISCLOSURE.md names the AI-assisted parts of the project. If this change adds a kind of AI use it does not list,
say so here so the maintainer can update it; otherwise leave "None".
-->
- None

## Notes for the reviewer

<!-- Anything risky, out of scope, or needing a second look; follow-up issues filed. -->

## Dependencies

<!-- Only if changed: crate, exact version, why, and where the maintainer approved it. -->
- None
