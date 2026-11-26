# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-26, Get Games (#884–#889, this commit under #889): GG-1 spike merged (#910); GG-2 #912, then GG-3, GG-4,
  GG-6 and GG-5 as a stack of branches (`feat/886-…` on the GG-2 commit, each next one on the previous), each PR
  opened once its base merged and rebased onto `main`. Maintainer decisions: `quick-xml` 0.42.0 only (no gzip crate:
  Flatpak deploys an uncompressed `appstream.xml`), installs without `--noninteractive` (it prints no progress; Add a
  Store too, stdin closed), the 62-entry blocklist in the launcher's default and pindu-os#77, Kodi and Firefox as
  extras, the first-catalogue time recorded in the hardware checklist's Get Games row.
- Harness for Get Games: `~/.cache/pindu-work/fakebin/flatpak` fakes the host's `flatpak` (put it first in `PATH` for
  `run.sh` only: exported in the persistent shell, `sdk.sh`'s `flatpak run` hit the fake and every build silently
  did nothing). It links the real Flathub catalogue on `update --appstream`, replays GG-1's install output, deploys a
  fake app and desktop entry, touches `.changed`, and takes `FAKE_FLATPAK_OFFLINE/FAIL/HUGE/STEP/REFRESH` and
  `FAKE_FLATPAK_EXTERNAL=<id>` (another program installs it after `FAKE_FLATPAK_EXTERNAL_AFTER` seconds).
  `run.sh` takes `click:X,Y` and `move:X,Y` for the desktop window (`--desktop --size 1280x1000` fits long menus).
- Gotchas: never share a `CARGO_TARGET_DIR` between worktrees: cargo keys a path dependency's build by its path
  relative to the workspace, so one worktree's tests ran the other's `pindu-core` without rebuilding. cargo-mutants
  in CI times out (and fails) on mutants that make code quadratic: the AppStream parser cleared its text as elements
  opened, and a wrong clear let it grow over the whole catalogue; each element now reads its text from where it
  started. A pushed PR gets fix-up commits, not an amend and force-push. `land.sh` loops left running with `nohup`
  never saw #910 merge; check and merge by hand. The sentinel also flags `Cargo.toml` changes.
- Still true from before: regenerate `po/pindu-launcher.pot` on conflicts, never hand-merge; hive's reviewer threads
  block merges until resolved; `--env GTK_A11Y=none` for every harness run; keep targets and mutants' `TMPDIR` in
  `~/.cache/pindu-work/`, and delete each `target-<N>`/`prefix-<N>.build` (4–9 GB) once its issue merges. The harness
  (`~/.cache/pindu-work/harness/`, README there: `install.sh SOURCE PREFIX`, `run.sh PREFIX [--design ID] [--bare]
  [--empty] [--offline] [--desktop] ACTION...`) and every cargo or meson command run in the SDK through `sdk.sh`:
  this host has no meson, blueprint-compiler, GTK headers or sqlite3 dev library.
