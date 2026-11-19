# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-19, roadmap run: the product analysis (`docs/product-analysis.md`) became milestones M6–M9 and issues
  #501–#559 (all labelled `hold`), plus follow-ups #565–#569, #591, #600, #606, #619, #622–#630, #638, #641. Every
  issue was built by parallel agents and admin-merged without waiting for CI, at the maintainer's request; #654 then
  made `main`'s CI pass. Migrations now run to 0011. Open: #546 (emulation) and #547 (external importers) wait on
  the maintainer's decision, with spec pages in `docs/`; none of the new UI has been checked on real hardware, and
  much of it was not looked at on screen at all. Still open from before: `hardware-check:passed` on #497.
- Gotcha: tests that make a file unreadable with `chmod 000` pass locally and fail in CI, which runs as root. Make
  the failure root-proof (a missing file, a symlink loop, a path past PATH_MAX). Check by running the SDK-built test
  binary on the host under `unshare -r`. The MSRV job can be run locally with CI's own rustup-init command in the SDK.
- Gotcha: for visual checks, gamescope headless (`gamescopectl screenshot`) is more faithful than Broadway, which
  draws stale frames during animations and crashes when text size changes live; run a second instance under
  `dbus-run-session` so it does not hand off to the running app.
- Gotcha: visual checks without hardware: install a prefix build (`meson setup <dir> --prefix=<dir>/inst
  -Dprofile=development`) and run it inside the devel app's sandbox (`flatpak run --devel --command=sh
  io.github.tim_van_oudheusden.pindu-launcher-devel`), so glycin's `flatpak-spawn --sandbox` works, on
  `gtk4-broadwayd :5 --port 8085` with `HOME`/`XDG_*`/`HOST_XDG_DATA_HOME` pointing at a fixture Faugus library;
  screenshot it with the browser. Puppeteer `page.screenshot({clip})` shrinks the Broadway window; key events stop
  reaching an `adw::Dialog` opened from the keyboard in fullscreen mode there, so drive the controller through
  `handle_intent` instead.
- Gotcha: parallel agents each get a `git worktree` under `~/Git/pindu-wt/<issue>` with a reflinked
  `_build/cargo-target` (`cp -a --reflink=always`, btrfs); they must use absolute paths, or edits land in the main
  checkout.
- Gotcha: the host has no development libraries; run cargo and meson through `~/.cache/pindu-gates.sh '<cmd>'` (GNOME
  51 SDK with rust-stable, `_build/cargo-target`), and the Flatpak with `flatpak run org.flatpak.Builder` (no
  `flatpak-builder` on the host). A killed build leaves a FUSE mount in `.flatpak-builder/rofiles`: `fusermount3 -u`.
- Gotcha: live gamescope checks without hardware: `gamescope --backend headless --steam -W 1280 -H 720 -- flatpak run
  --nosocket=wayland <app-id> --fullscreen`, a uinput Xbox 360 pad from python-evdev (the user has rw on
  `/dev/uinput`), `--launch desktop:<stem>` for a desktop-entry stand-in game, and `GAMESCOPECTRL_REQUEST_SCREENSHOT`
  set to 3 (full composition; 1 leaves overlays out), saved to `/tmp/gamescope.png`. Home's background animates, so
  compare a pixel, not the whole file. Real Steam runs there too: `org.gtk.Actions.Activate steam-big-picture` on
  `/io/github/tim_van_oudheusden/pindu_launcher[_devel]/window/1` opens Big Picture, `DISPLAY=:N steam -shutdown`
  exits it; the launcher needs `DBUS_SESSION_BUS_ADDRESS` in its environment. Remove stand-in `.desktop` files after:
  the user's library imports them.
- Gotcha: a child with `hexpand` makes its pane expand too; the overlay's panes set `hexpand: false`.
- Gotcha: parallel PRs that touch strings conflict on `po/pindu-launcher.pot` line references: take `main`'s copy,
  `meson compile -C _build pindu-launcher-pot`, and let `meson test` (check-pot) confirm. A new `gettext` call in a
  file means a `po/POTFILES.in` line (check-potfiles). CI runs tests as root.
