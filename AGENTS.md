## Issue Tracking

GitHub issues are the **primary source of truth** for issue information. All issue content (requirements, acceptance
criteria, discussion, links to PRs/commits) lives in GitHub. Access, read, address, and implement issues **exclusively
through the GitHub API**.

- Repository: `Tim-van-Oudheusden/Dailies` (remote `origin`, git@github.com)
- Use the `gh` CLI when available; otherwise call the REST API with `GITHUB_TOKEN`
- Track work in GitHub issues, not markdown TODO lists

```bash
gh issue list -S "is:open"      # Find available work
gh issue view <number>          # Read issue, comments, acceptance criteria
gh issue create --title "Summary" --body "Why and what"   # File remaining/follow-up work
gh issue comment <number> --body "Progress / links"       # Report progress
gh pr create --title "type(scope): summary (#<number>)" --body-file <file>   # Land work; body says "Fixes #<number>"
```

## Git and Version Management

- Commits follow the Conventional Commits specification; one commit per issue, referencing the issue number.
- Pull requests are merged with merge commits only; the branch is deleted on merge.
- A release is a pushed `vX.Y.Z` tag that matches the Meson project version and the newest metainfo release; only the
  maintainer pushes tags.
- The ruleset in `.github/rulesets/main.json` is the source of truth for branch protection; change it by PR, then apply
  it: the first time with `gh api -X POST repos/Tim-van-Oudheusden/Dailies/rulesets --input .github/rulesets/main.json`,
  afterwards with `gh api -X PUT repos/Tim-van-Oudheusden/Dailies/rulesets/<id> --input .github/rulesets/main.json`.
  `tests/tooling/test_ruleset.py` fails if a required check is not the `name:` of a workflow job.

## Testing

- **TDD must be used** — write tests before implementation code
- **One test at a time** — write one failing test, make it pass, refactor, then write the next
- **Confirm the test fails first** — run each new test and verify it fails for the expected reason
- **Test behaviour, not implementation** — assert on observable outputs, return values, and side effects
- **Put logic where it is testable** — pure logic belongs in `dailies/core` (never imports `gi`; tested with plain
  pytest, no display); keep `dailies/data` and `dailies/ui` a thin GTK shell over it
- **Run the full suite after every change** — `pytest`
- **Never modify tests to make them pass** — never delete, skip (`pytest.mark.skip`), `xfail`, or weaken assertions,
  and never add `# noqa` to silence failures
- **Don't over-implement** — if the test passes, the implementation is done
