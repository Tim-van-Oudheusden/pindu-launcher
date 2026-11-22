# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-22, #747 (first of a stack, #747 → … → #744; six issues worked by parallel agents in worktrees
  `~/.cache/pindu-work/wt-<N>` and stacked at the end so the summaries, the .pot and shared docs do not conflict):
  the starting screen shows the game's logo (`launch_logo`, `.launch-logo`, 480×160 contained) from the header's
  texture, the title hidden while it shows; no banner. Without a design the overlay stays dark in the light style
  (a white logo vanished on the light one). Seen in the harness with a seeded `library.db` of hand-added games that
  run `sleep 40` (`~/.cache/pindu-work/harness-747/`). Not seen: a screen reader, `--steam` gamescope, held B.
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
