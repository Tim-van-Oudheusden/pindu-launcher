# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-15, #249: added the empty `.acmm.yml` waiver file and a test in `tests/scripts/test_acmm_self_check.py`
  that checks every waiver names a real criterion with `id`, `satisfied_by` and `reason`. Self-check: L6, 12/12.
- Open: the rest of #249 is the maintainer's (Hive dashboard, GitHub App, operational level, ACMM Eval screenshot), so
  the pull request says `Refs #249` and leaves the issue open.
- Gotcha: the waiver test parses YAML with `yq`, which GitHub's Ubuntu runners preinstall; no Python YAML dependency.
