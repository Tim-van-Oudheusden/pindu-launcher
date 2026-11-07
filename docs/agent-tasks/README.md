# Agent task traceability

Every change an agent makes traces back to the GitHub issue that asked for it, and every issue leads forward to the
change that resolved it. GitHub issues are the task records (`AGENTS.md`, "Issue Tracking"); this directory does not
copy them. It describes the chain of links from an issue to the code on `main`, which links are enforced, and how to
follow the chain either way.

## The chain

| Link | Carries the issue number as | Example | Enforced by |
| --- | --- | --- | --- |
| Issue | The task itself, with a milestone, labels and acceptance criteria | #233 | — |
| Claim | The assignee; Hive claims with its own comment and label | `gh issue edit 233 --add-assignee @me` | Convention (`AGENTS.md`, "Sessions with GitHub Issues") |
| Branch | `<type>/<number>-<slug>` off an up-to-date `main` | `feat/233-native-installs` | Convention (`AGENTS.md`) |
| Commit | One Conventional Commit per issue, `(#<number>)` at the end of the subject | `feat(launch): explain a missing umu-run and hide Flatpak-only settings in native installs (#233)` | Convention (`AGENTS.md`, "Git and Version Management") |
| Pull request | Title `type(scope): summary (#<number>)`, body starting `Fixes #<number>` | #260 | Convention (`AGENTS.md`); `main` only changes through a pull request (the `main` ruleset) |
| Merge | Merge commit `Merge pull request #<PR> from Tim-van-Oudheusden/<branch>`, made by the maintainer | `216cd11` | The `main` ruleset allows merge commits only, after the required CI checks pass |
| Close | The merge closes the issue through `Fixes #<number>` | #233 closed by #260 | GitHub |

The ruleset guarantees that every change on `main` arrived in a merged pull request with green CI; the issue link in
that pull request is what ties it to a task. Branch names and commit subjects carry the number too, which makes them
quick to search, but the pull request's `Fixes #<number>` is the authoritative link: it is what closes the issue and
what `closedByPullRequestsReferences` reports. Use `Refs #<number>` instead when a pull request covers only part of an
issue; it links without closing.

One commit may close two issues when they cannot land apart, as `feat(packaging): … (#235, #236)` did; its pull
request then has one `Fixes` line per issue.

**Exempt:** Dependabot and other bot pull requests are named for their task (`dependabot/<ecosystem>/<package>`) and
need no issue. A release is its own issue ("Release X.Y.Z") on a `release/<issue>-vX.Y.Z` branch
([`releasing.md`](../releasing.md)).

## Following the chain

From an issue to the change that resolved it:

```sh
gh issue view <number> --json state,assignees,labels,closedByPullRequestsReferences
gh pr list --state all --search "<number>"
git log --oneline --grep "#<number>" origin/main
```

From a line of code back to its issue:

```sh
git blame -L <line>,<line> <file>              # the commit
git log -1 --format=%s <commit>                # its subject ends in (#<number>)
gh pr list --state merged --search "<commit>"  # the pull request, whose body says Fixes #<number>
```

From a pull request to its issue: `gh pr view <PR> --json title,body,closingIssuesReferences`.

From an agent to everything it did: filter pull requests by author or branch, for example
`gh pr list --state all --author app/hive-worker-bee` for Hive. Local CLI agents push as the maintainer, so their pull
requests are found through their issues.

## When the chain breaks

- **Work with no issue:** file one (`gh issue create`) before opening the pull request. An agent that finds work outside
  its issue files a new issue rather than widening the pull request (`AGENTS.md`, "Session Completion").
- **A pull request that links the wrong issue:** edit its body; nothing else needs to change.
- **An issue closed with no linked pull request:** reopen it, or comment with the pull request or commit that resolved
  it so the record is complete.
- **Agent work that must be undone:** revert its merge commit (`git revert -m 1 <merge>`) on a new branch, open a pull
  request that says `Refs #<number>`, and reopen the issue if its work is still wanted.
