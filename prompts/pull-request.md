# Pull Request Prompt

Create a pull request body for `<ISSUE>` (title: `<TITLE>`) implemented by commits `<COMMIT_RANGE>` in
`pindu-launcher`.

Rules: Conventional Commits title ending in the issue number (`type(scope): summary (#<number>)`), one commit per
issue, issue referenced in the commit, body starts with `Fixes #<number>` (or `Refs #<number>` and what remains when the
issue cannot be fully closed).

## Summary

- **What:** one or two sentences: what changes for the user.
- **Why:** the issue's goal; note any verified deviations from the issue or the spec (the issue wins over the spec).

## Testing

- Failing-before and passing-after output for the changed path.
- For UI changes: how the UI was smoke-tested (desktop or Broadway backend) and that keyboard and controller navigation
  were checked where they apply.

## Quality gates

- The exact commands run and their results:
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

## Notes for the reviewer

- Documented tradeoffs, harness limitations, or pre-existing issues touched.
- Any kind of AI use that `AI_DISCLOSURE.md` does not list yet.
- Any feature that seems to need a new Flatpak permission, filed as an issue for the maintainer instead of added.

## Dependencies

- Only if changed: crate, exact version, why, and the approval.
