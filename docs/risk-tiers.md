# Risk tiers

Every pull request is classified into one of three risk tiers from the paths it changes.
`.github/workflows/tier-classifier.yml` runs `scripts/tier-classify.sh` on each pull request and posts the tier, plus
every changed file with its own tier, to the `Risk tier` job summary. The classification never fails the run: it tells
reviewers how closely to look. The maintainer merges every pull request (AGENTS.md); a high-risk one only after the
review below.

The **highest tier of any changed file wins**, so one workflow edit makes the whole pull request high risk. A path that
matches no rule is **medium**, so a new kind of file that configures a gate, the build or the sandbox needs a high rule
in the same pull request that adds it.

The workflow runs the **base branch's** script and risk assessment (a second checkout of the base commit) against the
pull request's diff, so a pull request cannot lower its own tier by editing either. A change to the rules takes effect
for the pull requests after it merges.

Run it locally against your branch:

```sh
git -c core.quotePath=false diff --name-only --no-renames origin/main...HEAD | scripts/tier-classify.sh
```

`--no-renames` matters: with rename detection on, a moved file is listed only under its new path, so moving
`.github/workflows/ci.yml` to `docs/ci.yml` would read as a low-risk docs change. Without it, the old path is listed
too. `core.quotePath=false` lists non-ASCII names as they are; git still quotes a name holding `"`, `\` or a control
character (`"docs/a\tb.md"`), and the script rates such a line high, since it cannot match it reliably.

## Tier: high

Changes that alter how the app is built, gated, packaged or released, what stored data it reads, or how agents behave.
A mistake here can bypass every other check or ship to every user.

| Paths | Why |
| --- | --- |
| `.github/**` | CI gates, the review rubric, the release workflow, rulesets, the token policy, these risk rules |
| `scripts/**` | Gate tooling: the review rubric, this classifier, the token policy check |
| `build-aux/**` | Flatpak manifests (sandbox permissions in `finish-args`, runtime, sources), lint exceptions, RPM and DEB packaging, release and smoke-test scripts |
| `meson.build`, `*/meson.build`, `meson.options` | The build, the version a release is cut from, the install layout, the cargo options, and the `test()` registrations `meson test` runs: a deleted registration leaves every check green |
| `Cargo.toml`, `*/Cargo.toml`, `Cargo.lock` | Dependencies, which need the maintainer's approval, and the workspace lint set |
| `clippy.toml`, `rustfmt.toml` | Lint and formatting configuration the CI gates read |
| `data/*.metainfo.xml.in` | Release notes, licence, branding and content rating shown to users |
| `data/*.gschema.xml.in` | Stored settings: a removed or retyped key breaks existing installs |
| `AGENTS.md`, `CLAUDE.md`, `.claude/**`, `.cursor/**`, `prompts/**` | Agent instructions, permissions and governance |

Review: a human reviews the full diff line by line. Dependency changes follow the rules in `AGENTS.md` (approved,
exact version, real repository). Workflow changes give every workflow least-privilege `permissions:`, keep event fields
such as pull request titles in `env:`, never inline in `run:`, and update `.github/policies/workflow-permissions.yml`
when they add a workflow or change a token grant. Manifest changes never grow `finish-args` or the bundled modules, and
never touch the Flathub manifest.

## Tier: medium

Application code, UI and data files: `crates/**`, `data/**` and `po/**`, plus anything not otherwise classified, such
as root files like `.gitignore`.

Review: the rubric in the `PR review rubric` job summary (`.github/workflows/review.yml`): TDD evidence,
behaviour-level tests, no weakened tests, Adwaita first and HIG, keyboard usable, quality gates green.

## Tier: low

Prose that does not execute: `docs/**`, and Markdown files (`*.md`) outside the high-risk paths.

Review: a read-through for accuracy against the code and workflows it describes.

## Changing the tiers

The rules are policy as code in the risk assessment config, `.github/risk-assessment.yml`: an ordered `rules:` list of
`- <tier> <glob>` items, one per line, where the **first matching rule wins**, so high-risk rules sit above broad globs
such as `data/**` and `*.md` (`AGENTS.md` is high even though `*.md` is low, the MetaInfo is high even though `data/**`
is medium). `*` matches across `/`, so the file writes a directory rule as `dir/*`; this document shows it as `dir/**`.
`scripts/tier-classify.sh` evaluates the rules and fails without a tier if a line is not a blank line, a whole-line `#`
comment, the `rules:` line or such an item, if an item names an unknown tier, or if the file holds no rule at all; set
`RISK_TIER_POLICY` to try a different file locally.

`scripts/tier-classify.test.sh` covers the engine and the shipped rules, and checks that every rule appears under its
tier in this document; CI runs it. Update the rules, their tests and this document in the same pull request.
