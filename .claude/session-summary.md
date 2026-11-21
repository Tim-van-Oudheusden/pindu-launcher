# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-21, #685: Left and Right stop at the ends of Home's row in the layouts with two screens
  (`home::row_step`, `HomePage::navigate_row`), for the controller and, through a key controller on `carousel_row`,
  the arrow keys. Checked by eye in Panorama, Marquee and Ribbon (both screens), D-pad, left stick and arrows.
- 2026-11-21, #684 (stacked on #685's branch, so the two summaries do not conflict): a Home card whose art loads
  none carries `.no-art` (`window::show_art_class`, the `on_loaded` of `CoverArt::show`), and Ribbon, Marquee,
  Lobby, Shelves and Panorama show `.jump-title` on it. Checked by eye in all five with the throwaway library's
  "A Game Without Art" games.
- Gotchas: `/tmp` is a tmpfs with a per-user quota: once it is full, every `flatpak run` fails with "Failed to export
  bpf: System failure beyond the control of libseccomp" (EDQUOT). Keep cargo targets out of `/tmp`
  (`~/.cache/pindu-sdk-target`). This host has no meson, blueprint-compiler or GTK headers; build and test in
  `org.gnome.Sdk//51` (`flatpak run --command=sh … -c 'PATH=/usr/lib/sdk/rust-stable/bin:$PATH …'`), with a
  `CARGO_HOME` that has no rustup shims. The SDK's binary runs on the host: `/tmp/pindu-check.sh <design> pad:…
  key:… shot:…` drives the dev install (`/tmp/pindu-dev`, prefix `/tmp/pindu-prefix`) in headless gamescope, and
  `/tmp/pad.py` sends D-pad, stick (`sleft`/`sright`) and button presses. Arrow keys are not intents in fullscreen
  mode (`keys::fullscreen_intent` leaves them to GTK); Home's rows catch them in a capture-phase key controller.
  The local Flatpak build skips `ui-smoke` (no xvfb); CI runs it.
- Still true from #682: GTK measures an ellipsizing label with `letter-spacing` too narrow
  (`widgets/spaced_label.rs`), and a wrapping label asked for its width at a fixed height answers with one long line
  (`widgets/tile_frame.rs`). A scrolled list's adjustment emits `changed` only when its size changes, so
  `HomePage::settle_shelves` is also called after a rebuild and on map.
