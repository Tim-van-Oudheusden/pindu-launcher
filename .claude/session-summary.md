# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-23 (late night), #788 (this commit): 0.7.0-rc.3, after #810 (#811): the first-sync summary no longer
  covers a new console's set-up checklist (`health::summary_owed`, `first_sync_summary` takes the `SetupState`).
  The harness presets `first-sync-summary-shown=true`; pass `--set first-sync-summary-shown=false` to see the
  summary. Before that, 0.7.0-rc.2: #793 (#796), #791 (#798), #792 (#802, replacing #795's decoder probe with a
  frame-rate step-down, the maintainer's call), #794 (#804) and #797 (#805). #792 stays open for the laptop
  measurement; #791, #794, #797 and #810 each end in a person's Pindu OS device check. Filed pindu-os#54 (VA-API
  packages).
- Other agents work this repo at the same time (a "hive": issue comments trigger them, and the merge queue merges
  their PRs). An early decision comment on #792 was implemented as #795 and merged before the maintainer's later
  choice; #803 duplicated #791. Post a decision comment only once it is final, and check `gh pr list` before
  opening a PR. cargo-mutants in CI misses `10 * 60` style constants and untested edges: write literals, and test
  thresholds at their boundary.
- Harness gained `--offline` (the app in a network namespace with no network, `unshare -rn`) and `--bare`
  (`harness/bare.sh`: the host's application folders bind-mounted empty, so with `--empty` the app sees a new
  console with no games and no store). No rust-analyzer on this host: `lsp rename` fails, use `ast_edit` (it only
  rewrites expression paths; fix type positions by hand).
- Harness `--gnome-buttons` (close-only title buttons through `gtk-4.0/settings.ini`): without it the desktop window
  at 360 px always logs `exceeds … width`, as CI's `ui-smoke-narrow` pins the same layout.
- /var/home filled up once (ENOSPC mid-build): each `target-<N>`/`prefix-<N>.build` is 4–9 GB. Delete them once an
  issue merges.
- Harness (outside the repo, `~/.cache/pindu-work/harness/`, README there): `install.sh SOURCE PREFIX` builds and
  installs through `~/.cache/pindu-work/sdk.sh` (runs a command in `org.gnome.Sdk//51` with rust-stable);
  `run.sh PREFIX [--design ID] ACTION...` runs fullscreen at 1280×720 in headless gamescope with a throwaway profile,
  a fake Faugus library, a uinput pad (`pad:r3`, `pad:rb`, `pad:guide+a`, `pad:down*3`, `pad:a@1.5`), keys
  (`key:ctrl+Page_Down`) and `shot:PATH.png`. The host's own desktop games and Flatpaks show up in its library, and
  its first notification is "Added 20 games", so Quick Access opens on Notifications there. The pad is a real host
  device while it runs. Pass `--env GTK_A11Y=none`: without an accessibility bus the first `announce()` crashes the
  app (#729, also on `main` with the search keyboard). The app gets no system bus (`DBUS_SYSTEM_BUS_ADDRESS` points
  nowhere): before that, two runs suspended the real PC from the power dock. A fake logind needs its own private bus.
- Gotchas: `/tmp` is a tmpfs with a per-user quota; keep cargo targets and prefixes in `~/.cache/pindu-work/`, and
  run cargo-mutants (installed in the SDK cargo home) with `TMPDIR` there, `--gitignore true` and
  `CARGO_TARGET_DIR` unset (`sdk.sh` sets one, and parallel workers sharing it test each other's mutants), or it
  copies `_build/` into `/tmp` and fills the quota. CI's mutants check covers every changed pindu-core line. This
  host has no meson, blueprint-compiler, GTK headers or sqlite3 dev library: host `cargo check -p pindu-core --tests`
  works, everything else runs in the SDK. Give parallel agents their own `CARGO_TARGET_DIR`. GTK measures an
  ellipsizing label with `letter-spacing` too narrow (`widgets/spaced_label.rs`); a wrapping label asked for its width
  at a fixed height answers with one long line (`widgets/tile_frame.rs`). Light style:
  `ADW_DEBUG_COLOR_SCHEME=prefer-light`; high contrast: `ADW_DEBUG_HIGH_CONTRAST=1`.
