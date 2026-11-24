# Apps

On Pindu OS the launcher is the whole user interface, so a program that is not a game has no other door. Apps is that
door: a plain list of the applications installed on the system, each opened with one press. It is part of the Pindu OS
work package in the session-shell hierarchy ([`docs/spec-amendments.md`](spec-amendments.md#session-and-os-shell-revised)),
issue #830; the OS half is Tim-van-Oudheusden/pindu-os#66. Apps installs and removes nothing: that is Get Games.

## Where it shows

- **Fullscreen mode:** an Apps tile on Home, after Installed and Categories, in every design. Adwaita, Lobby and
  Marquee have it in the first screen's row of section tiles, the Explore layout in its row of tiles, and Shelves, whose
  sections are rows of games, in its last row beside the Library and Steam Big Picture tiles. Its art is
  `data/home/apps.svg`; designs can tint it through `.section-apps`. The tile leads to the Apps page, a grid of app
  icons with their names: A opens the focused app, B goes back to Home. Home has no Apps tile while there are no apps.
- **Desktop window:** an Apps item in the sidebar, under the library's views, with the number of apps. It shows the same
  page in the library's place; a click opens an app. With no apps, the page says what Apps lists.
- **Not** in Quick Access, the library, search, counts or exports.

## What counts as an app

Apps are read from the folders the desktop source already reads (`sources::desktop::application_dirs`): the user's
`~/.local/share/applications`, the user's and the system's Flatpak exports, and the host's `/usr/share/applications`,
in that order. The first entry of a desktop ID wins, so a user's entry overrides a system one of the same ID, and a
user's entry with `Hidden=true` hides it.

An app is a desktop entry that:

- has `Type=Application`, a `Name` and an `Exec` line that parses;
- is not `Hidden`, `NoDisplay` or `Terminal=true`;
- passes `OnlyShowIn` and `NotShowIn` for the session's desktops (`XDG_CURRENT_DESKTOP`). A session that names no
  desktop shows only the entries meant for every desktop;
- passes `TryExec`: the program is an executable file at its path (a path under `/usr` is looked up under the host's
  `/usr`, as the sandbox sees it), or by name in the host's folders of programs (`~/.local/bin`, `/usr/local/bin`,
  `/usr/bin`, `/usr/sbin`, `/usr/games` and the Flatpak exports' `bin`).

Its name is `Name` in the session's language (`LC_ALL`, `LC_MESSAGES` or `LANG`, matched as the Desktop Entry
Specification says), its icon is found the way the desktop source finds icons (an absolute path, or the name in the
hicolor theme of each data folder, then the host's pixmaps), and the list is sorted by name. The code is
`crates/pindu-core/src/apps.rs`, tested against `crates/pindu-core/tests/fixtures/apps`.

## What is left out, and why

- **Games.** An entry the library already shows as a game is not an app, so nothing is listed twice: a
  `Categories=Game` entry the desktop source imports, and a launcher's game shortcut, an entry whose `Exec` runs Steam,
  Heroic, Lutris, Bottles, Faugus or itch, or opens one of their URIs, which that launcher's source lists.
- **The launcher's own entry**, release or development build: opening it would open the launcher again.
- **Kept in:** the launchers the library imports from (Steam, Heroic, Lutris, Bottles, Faugus, itch). The desktop
  source skips their own entries, as they are not games; they are the main reason for this list.

Apps are not games: they are not rows in the game library, get no playtime, artwork lookups or duplicates grouping, and
never show in Library, Installed, Recently Played, search results, counts or exports.

## Opening an app

An app opens through the launch service (`launch_service::open_app`) from its `Exec` line as an argument list, never
a shell string, with `Path` as its folder. Field codes such as `%u` and `%f` are dropped, along with Flatpak's
file-forwarding markers, as the desktop source drops them. Inside the Flatpak it runs on the host through
`flatpak-spawn --host`.

In a gamescope `--steam` session the app is followed the way an opened store is (#791, `add_store::opened_id`): its
windows come to the front, the launcher comes back when the last one closes, and the menu drawer and Quick Access show
it with Return and Exit while it is open. Exit asks its windows to close, then ends them. Elsewhere the app's window
takes focus, and with it the controller. An app may need a keyboard or mouse; it opens as it is.

## Keeping the list current

The launcher watches the application folders, there yet or not, and reads the list again a second after they last
changed (`apps_service.rs`). An app that Get Games installs or removes while the launcher runs appears or disappears
without a restart or Refresh Library.

## Open question

On a full desktop the list holds every installed application, not just a store and a few launchers, so the desktop
window's Apps shows them all. Whether to keep that, or offer a switch, is the maintainer's call (#830); the list is not
filtered by guesswork.
