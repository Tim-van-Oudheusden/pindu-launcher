# Prompts Catalog

Reusable prompt templates for this repository. They encode the conventions from `AGENTS.md` (authoritative) so agent
and human reviews stay consistent.

## Templates

- [`code-review.md`](./code-review.md): two-axis review checklist (standards and spec) matching the repo's quality
  gates and rules.
- [`pull-request.md`](./pull-request.md): pull request body prepared from an issue and its commit.

Quality gates both templates refer to (AGENTS.md "Session Completion"):

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

Usage: copy a template into your prompt, fill the bracketed fields, and follow `AGENTS.md` for issue tracking and the
session completion workflow. On a pull request, ask Claude Code for a review with a comment such as:

```text
@claude review this PR using prompts/code-review.md against issue #<number>
```
