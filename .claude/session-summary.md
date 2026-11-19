# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-20, #664: prepared `v0.6.0-rc.1` from main, including the fullscreen background picker and README galleries.
- Candidate tags use `v0.6.0-rc.1`; RPM, Debian and MetaInfo use the corresponding `0.6.0~rc1` version.
  Tag the release PR's merge commit on main and let the Release workflow publish it.
- Keep #664 open for the hardware checklist, the candidate soak and the final release. No hardware pass is recorded.
