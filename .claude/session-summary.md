# Session summary

Hand-off notes for agent work in this repository, so the next session starts from where the last one stopped instead
of cold. Keep it short: overwrite the "Latest session" section at the end of each session, in the issue's own commit;
durable lessons belong in `.claude/memory/corrections.tsv`, not here.

Format: date, issue/PR worked, outcome, open follow-ups, gotchas for next time.

## Latest session

- 2026-11-22, #717 (first of the menus series #717 → #718 → #719 → #720 → #721 → #722 → #723 → #725 → #724, each
  pull request stacked on the one before so the summaries do not conflict): Quick Access is six sections under a
  strip (`QuickAccessSection`, `section_entries`, `sections`, `Shown` in `panels.rs`; `Panels::section`,
  `show_section`; `hints::Action::Sections`). The four former levels (Notifications, Activity, Designs, Controllers)
  are section content; LB/RB and Ctrl+Page Up/Down switch sections; `win.quick-access-section`. Row visibility
  comes only from `section_entries`; the Quick copies of Sound Output, Bluetooth, Network, Refresh Library and Add a
  Game bind their second line, icon and availability to the System/Manage originals. Checked with the harness below
  in Lobby, Adwaita, Marquee and Shelves at every Text Size: "Notifications" ellipsizes in the strip at Larger and
  Largest (tooltip and accessible name keep it whole). Not seen: the panel over a running game, the pointer on the
  strip, a screen reader.
- 2026-11-22, #718 (stacked on #717): Quick's page cards (Filters with the filters that are on,
  `library::Filters::names_on`; Performance for the focused game), the six tiles (`panels::quick_tiles`, laid out
  three to a row from the start by `quick_section.rs`'s `PinduQuickSection`, whose `focus` vfunc takes both the arrow
  keys and the controller's moves through `panels::step_tile`) and the activity strip. Tile names wrap without a line
  limit, so "Refresh Library" takes three lines at Largest rather than being cut. Not seen: the Playing Now focus
  paths, Quick over a running game, the Sound Output tile and the Flatpak's single row (no running game or system
  services in the harness).
- 2026-11-22, #719 (stacked on #718): `widgets/choice_row.rs`'s `PinduChoiceRow` replaces the ten radio groups
  (Background, Background Blur, Background Dim; Button Prompts, Swap Confirm and Back, Repeat Delay, Repeat Rate, Stick
  Dead Zone; Settings' Cover Size and Text Size), bound to the same stateful actions (`win.tile-size` is new, same
  key). `controller_navigation::adjust_slider` turns sliders and choice rows for both hosts (`Adjusted`); steps are
  `pindu_core::choice`. Seen in the harness at Largest and in Hebrew under high contrast. Not seen: the pointer on the
  arrows, a screen reader. Right sent within a second of changing Swap Confirm and Back was once lost.
- Harness (outside the repo, `~/.cache/pindu-work/harness/`, README there): `install.sh SOURCE PREFIX` builds and
  installs through `~/.cache/pindu-work/sdk.sh` (runs a command in `org.gnome.Sdk//51` with rust-stable);
  `run.sh PREFIX [--design ID] ACTION...` runs fullscreen at 1280×720 in headless gamescope with a throwaway profile,
  a fake Faugus library, a uinput pad (`pad:r3`, `pad:rb`, `pad:guide+a`, `pad:down*3`, `pad:a@1.5`), keys
  (`key:ctrl+Page_Down`) and `shot:PATH.png`. The host's own desktop games and Flatpaks show up in its library, and
  its first notification is "Added 20 games", so Quick Access opens on Notifications there. The pad is a real host
  device while it runs. Pass `--env GTK_A11Y=none`: without an accessibility bus the first `announce()` crashes the
  app (#729, also on `main` with the search keyboard).
- Gotchas: `/tmp` is a tmpfs with a per-user quota; keep cargo targets and prefixes in `~/.cache/pindu-work/`, and
  run cargo-mutants (installed in the SDK cargo home) with `TMPDIR` there and `--gitignore true`, or it copies
  `_build/` into `/tmp` and fills the quota. CI's mutants check covers every changed pindu-core line. This
  host has no meson, blueprint-compiler, GTK headers or sqlite3 dev library: host `cargo check -p pindu-core --tests`
  works, everything else runs in the SDK. Give parallel agents their own `CARGO_TARGET_DIR`. GTK measures an
  ellipsizing label with `letter-spacing` too narrow (`widgets/spaced_label.rs`); a wrapping label asked for its width
  at a fixed height answers with one long line (`widgets/tile_frame.rs`). Light style:
  `ADW_DEBUG_COLOR_SCHEME=prefer-light`; high contrast: `ADW_DEBUG_HIGH_CONTRAST=1`.
