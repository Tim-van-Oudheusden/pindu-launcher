# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-21, #691 (filed this session at the maintainer's request): Lobby is the default design and Ribbon and
  Panorama are gone; a setting that names either is switched to `lobby` (`themes::replacement`,
  `StyleService::follow_design`) unless the user has a design with that id. The shipped-designs test no longer asks
  for a design with variants (approved). README's fullscreen screenshots 1, 2 (now `fullscreen-2-adwaita.webp`) and 5
  were retaken at 1280×720 from the throwaway library. `meson install` into an old prefix keeps removed theme folders:
  delete them from the prefix by hand. `/tmp/pindu-check.sh` with an empty first argument now leaves the design
  setting alone; set `fullscreen-design=''` with `/tmp/pindu-set.sh` for Adwaita.
- 2026-11-21, #685: Left and Right stop at the ends of Home's row in the layouts with two screens
  (`home::row_step`, `HomePage::navigate_row`), for the controller and, through a key controller on `carousel_row`,
  the arrow keys. Checked by eye in Panorama, Marquee and Ribbon (both screens), D-pad, left stick and arrows.
- 2026-11-21, #684 (stacked on #685's branch, so the two summaries do not conflict): a Home card whose art loads
  none carries `.no-art` (`window::show_art_class`, the `on_loaded` of `CoverArt::show`), and Ribbon, Marquee,
  Lobby, Shelves and Panorama show `.jump-title` on it. Checked by eye in all five with the throwaway library's
  "A Game Without Art" games.
- 2026-11-21, #683 (stacked on #684's branch): Adwaita, fullscreen mode with no design, has its own layout
  (`ThemeLayout::adwaita`, `"home": "explore"`, `top-bar-tabs`), the window class `no-design`
  (`StyleState::shows_no_design`), an Explore Home (`home::explore_rows`, `home::featured_step`; the carousel, tiles
  and shelves are rows of `HomePage`'s one-screen machinery, keyed by `RowId`) and CSS-only Quick Access and drawer
  looks scoped to `window.fullscreen-mode.no-design`. Down from the top bar's tabs now lands on the remembered row in
  both one-screen layouts (`HomePage::enter_from_top`). The Quick Access pane's selector is the full child chain
  `bottom-sheet > widget > overlay-split-view > widget > overlay-split-view > widget.overlay-pane`.
- Not seen by hand: Play starting a game from the banner (no launchable game in the test library), the Playing Now
  card and the battery/Offline chips (no running game, battery or offline state), Reduce Motion's lack of animation.
  Light style: `PINDU_WRAP="env ADW_DEBUG_COLOR_SCHEME=prefer-light"`; high contrast: `ADW_DEBUG_HIGH_CONTRAST=1`.
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
