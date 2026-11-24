# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-24, #833 XP-5 (this commit, stacked on #832 XP-3 and #831 XP-2): install, replace and remove an
  external importer from Preferences (`external::install`: lists the `.tar.gz` with `tar --list --absolute-names`
  before unpacking, unpacks into `pindu-launcher/.importer-install-*`, checks the tree, one rename). Harness:
  GTK's own file chooser (no portal) takes `ctrl+l`, the folder path, Return, then `ctrl+l ctrl+a`, one letter and
  End to accept its completion; in Preferences the selectable property subtitles take Tab stops. XP-3:
  `SyncService::sources` reads states and helpers in one call (two awaits let Quick Access focus the copy order
  first); `run.sh --seed DIR` copies DIR into the run's XDG data home. Pre-existing: after a Sources switch's sync
  rebuilds the fullscreen list, focus is lost. XP-4 #834 is a subagent's worktree (`wt-834`).
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
