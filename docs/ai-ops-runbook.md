# AI operations runbook

How to watch the agents working in this repository, and how to stop them, undo their work and recover when they or a
workflow misbehave. [`AGENTS.md`](../AGENTS.md) says what agents may do; this page is the day-to-day side: where to
look, what to do when something fails, and how to pause everything.

There is no alerting backend. Every signal below is a pull request, an issue, a label or a GitHub Actions run, read with
`gh` or on the Actions tab. The launcher itself runs no AI and sends nothing anywhere; the agents only work on this
repository.

## The automations

| Automation | Runs as | Starts on | Can it change `main`? |
| --- | --- | --- | --- |
| CI (`.github/workflows/ci.yml`) | `GITHUB_TOKEN`, `contents: read` | Every pull request and every push to `main` | No; it only reads and uploads artifacts |
| Release (`.github/workflows/release.yml`) | `GITHUB_TOKEN`; only its `publish` job has `contents: write` | A pushed `v*` tag, or a manual run for an existing tag | No; it publishes a GitHub release for the tag ([`releasing.md`](releasing.md)) |
| Hive agents | The `hive-worker-bee` GitHub App, outside this repository | Hive's scheduler and the Hive dashboard, at the operational level the maintainer set (L2–L3 to start, #237) | Only through a pull request that passes the ruleset |
| Local CLI agents (Claude Code, Copilot, Cursor and others) | The maintainer's own `gh` login and SSH key | A session the maintainer starts | Only through a pull request (`AGENTS.md`, "Sessions with GitHub Issues") |

Each Hive readiness issue that adds an automation (for example Claude Code in CI, #246) adds its row to this table and
to [Stopping the agents](#stopping-the-agents).

## Where to look

| Question | Signal | Command |
| --- | --- | --- |
| What are agents working on? | Open pull requests by head branch: `<type>/<number>-<slug>` from CLI agents and Hive | `gh pr list --json number,title,headRefName,author,labels` |
| Who holds an issue? | Its assignee, or Hive's claim comment and label | `gh issue view <N> --json assignees,labels,comments` |
| What has Hive done? | Pull requests and issues by its App | `gh pr list --state all --author app/hive-worker-bee` |
| Is a pull request ready to merge? | The ruleset's required checks and the merge state | `gh pr checks <N>`, `gh pr view <N> --json mergeStateStatus` |
| Is `main` healthy? | CI runs on `main` | `gh run list --workflow ci.yml --branch main --limit 10` |
| Did a release publish? | Release runs and the release's files | `gh run list --workflow release.yml`, `gh release view vX.Y.Z` |
| How does an issue link to its change? | The issue → branch → commit → pull request chain | `gh issue view <N> --json closedByPullRequestsReferences` |

## Playbooks

### CI fails on a pull request

`gh pr checks <N>` names the failing job; `gh run view <run-id> --log-failed` shows why. Every CI job is a required
check on `main`, so the pull request cannot merge until it passes.

- **`Rust and Meson checks`:** formatting, clippy, rustdoc, unit tests or Meson tests. Run the same gates locally
  (`AGENTS.md`, "Session Completion") and fix the code. Never weaken a test or add `#[allow]` to get green.
- **`Source tarball with vendored crates`:** `meson dist` vendors the crates and builds the tarball offline. A
  dependency change that does not vendor, or a file `meson dist` misses, fails here.
- **`Native package (…)` and `Native package install and UI smoke test (…)`:** the RPM or a DEB does not build from the
  tarball, or the installed package does not start in a clean container (`build-aux/package-smoke.sh`).
- **`Flatpak build, UI smoke test and lint (…)`:** the manifest does not build, `meson test` fails inside the build
  (the UI smoke test runs there), or `flatpak-builder-lint` reports a new problem. Fix the cause; an entry in
  `build-aux/flatpak/lint-exceptions.json` is the maintainer's call, like any change to `finish-args` (`AGENTS.md`,
  "Safety Boundaries").
- **A job fails for a reason outside the change** (a network error, a runner out of disk): `gh run rerun <run-id>
  --failed`. If it fails the same way twice, treat it as a real failure and file an issue.

### CI fails on `main`

Something merged that does not pass on top of a change merged in the meantime, or the platform moved. File an issue
(`gh issue create`, label `type:chore`), fix it through a pull request, or revert the merge that broke it
([Reverting agent work](#reverting-agent-work)). Do not tag a release from a red `main`.

### The Release workflow fails

Follow [`releasing.md` § When something goes wrong](releasing.md#when-something-goes-wrong): a version mismatch means
deleting the unpublished tag and fixing the versions through a pull request; a build, test or lint failure publishes
nothing and is fixed in a new patch version; a failed upload is rerun by hand with the tag as input; a broken published
release is marked as a pre-release or deleted, and its tag is kept. Agents never move or reuse a published tag.

### An agent acted outside its instructions

Ignored rules, a change outside the issue's scope, instructions taken from imported data or an issue body (`AGENTS.md`,
"Safety Boundaries"). Stop the agent first ([Stopping the agents](#stopping-the-agents)), close or put on hold its open
pull requests, and file an issue that says what happened. A leaked secret is rotated before anything else: the
SteamGridDB key lives in the user's keyring, but a token an agent used (Hive's App, a `gh` login) is revoked and
re-issued. Fix the instructions that let it happen (`AGENTS.md`) in their own pull request. Work that already merged is
reverted or fixed forward ([Reverting agent work](#reverting-agent-work)).

### Two agents worked the same issue

Agents claim an issue by assigning it, or Hive with its claim comment and label. Find the competing pull requests with
`gh pr list --state all --search "<N>"`, keep the one from the agent that held the claim, and close the other with a
comment linking it.

## Stopping the agents

To pause agent work, for example during an incident or while a bad instruction gets fixed, stop each entry point:

| Stop | How | Undo |
| --- | --- | --- |
| Hive, pause | Pause the repository in the Hive dashboard, or lower its operational level there so Hive only proposes work | Resume it, or raise the level again |
| Hive, hard stop | Revoke the `hive-worker-bee` App's access to this repository in the repository's GitHub App settings | Reinstall the App |
| Hive on one issue | Remove its claim label and unassign the issue (`gh issue edit <N> --remove-label <label> --remove-assignee <login>`) | Hive can claim it again |
| A labelled hand-off | Remove the label that hands issues to an agent (for example `ai-fix-requested`, once #246 adds it): `gh issue edit <N> --remove-label <label>` | Add it again |
| A workflow | `gh workflow disable <file>` | `gh workflow enable <file>` |
| One pull request | Convert it to a draft (`gh pr ready <N> --undo`) and say why in a comment | `gh pr ready <N>` |
| A local CLI agent | End the session | — |

Never disable `ci.yml` to pause agents: CI keeps running on every pull request either way, and nothing reaches `main`
without its required checks. Never disable `release.yml` to stop a release; just do not push a tag. A workflow disabled
by hand stays disabled until someone enables it, so write down what you paused in the incident's issue.

## Reverting agent work

Every agent change is one Conventional Commit in one merged pull request, so undoing it is one revert:

```sh
git switch main && git pull
git switch -c revert/<issue>-<slug>
git revert -m 1 <merge-commit>
git push -u origin HEAD
gh pr create --title "revert: <original subject>" --body "Reverts #<PR>; Refs #<issue>"
```

Find the merge commit with `gh pr view <PR> --json mergeCommit`. Reopen the issue (`gh issue reopen <N>`) if its work is
still wanted. The revert passes CI and lands like any other pull request. Never force-push or rewrite `main`, and never
revert a release by moving its tag: release a fixed patch version instead ([`releasing.md`](releasing.md)).

## The `main` ruleset

The `main` ruleset (`gh api repos/Tim-van-Oudheusden/pindu-launcher/rulesets`) is what keeps every agent behind a pull
request:

- no deletion of `main` and no force pushes;
- changes only through a pull request, merged with a merge commit, with every review thread resolved;
- every CI job is a required check: `Rust and Meson checks`, `Source tarball with vendored crates`, both
  `Flatpak build, UI smoke test and lint` jobs and the three `Native package install and UI smoke test` jobs.

Repository admins can bypass it (#237). Only the maintainer is an admin; agent tokens and Apps never get the admin role.
The bypass is for emergencies the checks cannot pass by design, such as a broken runner image; when the maintainer
uses it, the pull request or issue says why, and the next run on `main` must still be green.
