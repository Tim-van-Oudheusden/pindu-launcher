# Fullscreen menus

Fullscreen mode has two menus: the menu drawer on the left (Start or Guide) and Quick Access on the right (Guide + A,
a right stick click, the Steam Deck's "…" button, or <kbd>Ctrl</kbd>+<kbd>2</kbd>). The menu drawer is where to go;
Quick Access is the state of things and the settings. This page says how both are arranged and what each part holds.
The maintainer chose this arrangement, design A of the fullscreen menus design pass, on 2026-11-22; it is built in the
issues of that series, starting with #717, and each one extends this page. It is the default; a design may arrange
the menus its own way (#725, [Arrangements by design](#arrangements-by-design)), never what they hold.

Which panel is open, at which level and section, and what the buttons do to them is `pindu_core::panels` (`Panels`);
what the drawer's main level lists is `pindu_core::panels::drawer_entries`, and what each Quick Access section lists,
and in which order, `pindu_core::panels::section_entries`. The drawer is `PinduMenuDrawer`
(`crates/pindu-ui/src/menu_drawer.rs`, `data/ui/menu_drawer.blp`), Quick Access `PinduQuickAccess`
(`crates/pindu-ui/src/quick_access.rs`, `data/ui/quick_access.blp`).

## Arrangements by design

Each design arranges the menus the way its own Home arranges games, through four layout options of its `theme.json`
(`panel-sections`, `quick-tiles`, `menu-entries` and `playing-card`, see
[`fullscreen-style.md`](fullscreen-style.md#layout-options)). The panels follow the design in force at once: switching
design in Quick Access's Design section rearranges the open panel, with focus staying on the design's card.

| | The same in every design | Differs |
|---|---|---|
| Entries | Which entries a menu and each section hold, and their order | — |
| Buttons | Start, Guide, R3, Guide + A, A, B, View or X, LB and RB | — |
| Directions | Up and Down always lead through a section in the entries' order | Whether two neighbouring tiles are a Left/Right or an Up/Down apart |
| Shape | — | Where the section strip sits, how the tiles and the menu's entries are laid out, how the game that runs is drawn |

| Design | `panel-sections` | `quick-tiles` | `menu-entries` | `playing-card` |
|---|---|---|---|---|
| Adwaita (and high contrast) | `"top"` | `"pills"` | `"list"` | `"compact"` |
| Lobby | `"top"` | `"grid"` | `"list"` | `"compact"` |
| Marquee | `"side"` | `"list"` | `"list"` | `"banner"` |
| Shelves | `"top"` | `"shelf"` | `"shelves"` | `"compact"` |

A user's design that names none of the options gets the first column of each: the arrangement this page describes
below. The arrangements apply in the main window and over a running game alike.

- **Sections at the side** (`"side"`): the strip is a rail of icons, about 60 px wide, down Quick Access's inner edge,
  and the shown section's name heads its page. The controller's focus never lands on the rail; LB and RB switch
  section as before.
- **Tiles** (`pindu_core::panels::step_arranged_tile`): as pills, two equal columns, each pill at least 56 px tall
  with its icon at its start, its name and second line beside it, and the small arrow at its end on a tile that leads
  to a level; as a list, one tile per row in the tiles' order, the second line at the row's end; as shelves, the
  system tiles (Sound Output, Bluetooth, Network) under **Devices** and the other three under **Library**, a shelf
  with no tile left out. In columns Left and Right move along a row and Up and Down keep the column; on shelves Left
  and Right move along a shelf and Up and Down change shelf, landing on the tile that shelf last had focused, as
  Home's shelves do. Tiles still lead or act; they never switch, and pills take no accent fill for "on".
- **The menu as shelves** (`"shelves"`, `pindu_core::panels::menu_shelves`): in place of the rows and the dock,
  three shelves of cards, each card its icon and name: **Go To** (Home, Library with its number of games, Search),
  **Collections** (every collection with its number of games, the row scrolling sideways, so there is no All
  Collections) and **Leave** (the dock's entries, with their names; one the system does not offer is dimmed and says
  why under its name). Left and Right move along a shelf, Up and Down change shelf as on Quick's shelves. Up from the
  main level's first item (a Playing Now card, else Home) still wraps to Suspend, now its card on Leave. View or X on
  a collection's card leads to its actions, and B back to that card. The Leave cards run the dock's actions, so Hold
  to Confirm, asking first while a game runs and the hint bar work on them as on the dock.
- **The Playing Now card as a banner** (`"banner"`): the game's hero art, else its cover, loaded as Home loads a
  game's hero, across the top of the drawer's main level and of Quick, with "Playing Now", the time, the title, Return
  and Stop over a scrim at its foot. Without art the banner is the card colour.

Every Text Size, Reduce Motion and high contrast keep each arrangement usable: tiles and cards grow with their text,
and high contrast shows Adwaita's arrangement, as it shows Adwaita for everything else.

## The menu drawer's main level

The drawer is a full-height panel that slides out from the left. Its main level is the places, top to bottom:

1. The Playing Now cards, one for each game that runs, and over a running game its windows when it has more than one.
2. **Home**.
3. **Library**, with the number of games at its end: the number Home's Library tile shows.
4. **Search**: closes the menu and starts a search with the on-screen keyboard, as Y does. From a page where Y does
   nothing, such as a game's page, it goes to the library first. The row ends with the glyph of the Search button on
   the controller in use, so it also teaches the shortcut; it follows a changed binding at once.
5. A **Collections** heading and the first five collections (`INLINE_COLLECTIONS`) in the user's order, each with its
   icon, its name and its number of games. A long name ends in an ellipsis; the number stays in view. A opens the
   collection's tab in the library. With no collections the heading and the rows are left out.
6. **All Collections**, only with more than five: leads to the Collections level, which lists every one.

Over a running game (`game_overlay`) the drawer leaves out Search and the power dock.

- View or X on a collection, in the main level or in the Collections level, leads to its actions: Rename…, Move Up,
  Move Down and Delete…. The hint bar shows View while a collection has focus. B goes back to where they were opened
  from, with focus on that collection (or on All Collections when a move took it past the first five).
- B from the Collections level goes back to the main level, with focus on All Collections.
- The rows follow the library as collections are added, renamed, reordered and deleted, and as the numbers of games
  change. Deleting the collection whose actions show goes back to where they were opened from.
- The menu has no Controls: what each button does and changing the buttons are in Quick Access's Controller section.

## The power dock

Power is the menu's dock: a row of round buttons at the bottom of the main level, under its scrolling list, so it
stays in view however many collections there are (`PinduPowerDock`, `crates/pindu-ui/src/fullscreen_power.rs`,
`data/ui/fullscreen_power.blp`). What it holds is `pindu_core::gamescope::session::power_dock`, in two groups:

- **Leaving the launcher**, at its start: **Exit Fullscreen Mode**, **Switch to Desktop** in a gamescope-session-plus
  session, and **Exit Pindu Launcher** outside the launcher's own session.
- **The machine**, at its end: **Suspend**, **Restart** and **Power Off**, as the system lists them.

Each button is an icon, with its name as its tooltip and its accessible label. A line over the dock names the focused
button, and is empty while focus is elsewhere. An action the system does not offer stays in the dock, dimmed, and
still takes focus: the line gives the reason (the system does not allow it, or in the Flatpak that Preferences gives
the access), A does nothing, and a screen reader hears it as disabled with the reason. The buttons run the same
actions the Power level's rows did, so Hold to Confirm, asking first while a game runs and an inhibitor's refusal
work as before; the hold's progress shows over the round button. The rules are in
[System panel](system-panel.md#power).

Focus:

- Up from the main level's first item (a Playing Now card's buttons while a game runs, else Home) wraps to the dock
  and lands on Suspend; without it, on the machine group's first button; without that group, on the dock's first
  button (`pindu_core::gamescope::session::dock_landing`). Suspend is Start, Up, A.
- Down from the list's last row enters the dock at its first button. Up from the dock goes back to the list's last
  row; Down stays.
- Left and Right move along the dock and stop at its ends. Power Off is Start, Up, Right, Right, A.
- The menu still opens on the first Playing Now card's Return, or on Home.
- The hint bar says what A does on the focused button: Select, Hold for Restart and Power Off while Hold to Confirm is
  on, and nothing on a dimmed one.

## Quick Access's sections

Under the status header (the time, Offline, the battery) Quick Access has a strip of six section buttons
(`.panel-sections`, each a `.panel-section`), the shown one checked and named, over the shown section's page, which
scrolls by itself. A level that an entry leads to replaces the strip and the section while it shows.

Quick Access opens on **Quick**, or on **Notifications** while one is unread. It does not remember the section it was
closed on.

What each section lists, top to bottom. "→" leads to the level of that name; "(when …)" is when the entry shows.

| Section | Entries |
|---|---|
| **Quick** | The Playing Now cards · the page cards: Filters → (when the library is under the panel) and Performance → (when the focused game is one Pindu runs) · the tiles: Sound Output →, Bluetooth →, Network → (each when available), Surprise Me, Refresh Library, Add a Game · the activity strip (while something runs) |
| **Notifications** | A **Running** heading and the background tasks, each with its progress and Cancel, while something runs · then the notifications, newest first, or the line that says there are none |
| **Design** | The designs, as a row of cards · Variant · then one list: the choice rows Background (when the design offers the choice), Background Blur, Background Dim, Cover Size, Text Size, then the Reduce Motion switch · My Stylesheet |
| **Controller** | The controllers · then one list: the choice rows Button Prompts and Swap Confirm and Back, the Hold to Confirm switch, the choice rows Repeat Delay, Repeat Rate and Stick Dead Zone · their note · then What Each Button Does → · Change Buttons → (not over a running game) |
| **System** | Network → · Bluetooth → · Sound Output → · System Update → (each when available) · then a list of their own: Navigation Sounds, Volume (insensitive while the sounds are off; Left and Right turn it), Exit After Launching · Diagnostics Report → |
| **Manage** | Sources → · Duplicates → · Library Health → · Proton → · Add a Store → · Add a Game · Refresh Library · an **Artwork** heading: SteamGridDB Key (A types it with the on-screen keyboard; its second line says whether a key is stored, or why the keyring cannot be used), Prefer SteamGridDB Art · a **Metadata** heading: Get Metadata From Wikidata, with Refresh Metadata right under it while it is on |

- Sound Output, Bluetooth, Network, Refresh Library and Add a Game are listed twice on purpose: once where they are
  quick to reach, once where they belong. The tile and the row do the same and show the same second line.
- Over a running game (`game_overlay`), Surprise Me, Add a Store, Add a Game, Diagnostics Report and Change Buttons are
  left out.
- A section with nothing to show is left out of the strip and of LB and RB's steps.
- Notifications marks the notifications read once it shows. When the last background task ends, the Running heading
  and its list go, and focus on a task moves to the first notification.
- Quick Access has no Settings level: each setting sits in the section it belongs to (#720), as the table says, and
  writes the same GSettings key Preferences does. Up and Down go on from one list of a section to the next.
- Manage reads whether a SteamGridDB key is stored each time it shows.

### Quick

Quick is what a player sees every time: the game that runs, what the page underneath offers, and six things to reach
in one or two presses. Top to bottom:

1. **The Playing Now cards**, one per game that runs, each with Return and Stop.
2. **The page cards** (`.page-card`), one per entry the page under the panel offers
   (`pindu_core::panels::page_entries`): a full-width card with the entry's icon in a round chip, **For This Page** over
   its name, and at its end a second line and an arrow. A leads to the entry's level.
   - **Filters**, while fullscreen mode's library is under the panel. Its second line names the filters that are on, as
     the Filters level calls them and in its order, joined with commas and cut with an ellipsis when they do not fit;
     nothing while none is on (`Filters::names_on`).
   - **Performance for {game}**, while the focused game is one Pindu runs, named as its level is.
3. **The tiles** (`.quick-tiles`), three equal columns of `.quick-tile` buttons 10 px apart, each about 100 px tall,
   with its icon at the top and its name at the bottom, and under the name a second line (`.quick-tile-state`) where
   the entry has one (`pindu_core::panels::quick_tiles`). The tiles that show fill the grid from its start: two rows of
   three with all three system entries, one row in the Flatpak, which has none. A tile that leads to a level carries a
   small arrow; the others act. A tile is insensitive when its row is (Network when the system does not allow changing
   it, Surprise Me with nothing installed, Refresh Library while a sync runs). A name wraps rather than being cut, and
   the tile grows taller at larger text sizes.

   | Tile | Second line | A |
   |---|---|---|
   | Sound Output | The output in use | Leads to Sound Output |
   | Bluetooth | The connected devices, or how the radio is | Leads to Bluetooth |
   | Network | The connection in use | Leads to Network |
   | Surprise Me | None | Shows a random installed game |
   | Refresh Library | None | Reads the sources again |
   | Add a Game | None | Opens Add a Game |

4. **The activity strip** (`.activity-strip`), while something runs in the background: the oldest task's label, its
   progress bar and percentage (a moving bar while there is no telling how far it is). A shows the Notifications
   section, which lists the tasks with Cancel.

Over a running game Quick shows the Playing Now cards, the page cards and the tiles that apply there (Surprise Me and
Add a Game are left out).

## Choice rows

A setting with a few fixed options is one row that changes in place (`PinduChoiceRow`,
`crates/pindu-ui/src/widgets/choice_row.rs`; the steps are `pindu_core::choice`): the setting's name at the start, the
chosen option between two arrows at the end, and under the name the option's second line where it has one (Repeat
Delay's "After 0.3 seconds"). The option's place is as wide as the widest option, so the arrows stay put; a long option
wraps rather than be cut. The row is bound to its setting's stateful action (`win.fullscreen-background-blur` and the
others) with a target per option, so choosing an option writes the setting at once, as Preferences does.

- Up and Down pass the row in one step.
- Left and Right choose the option before or after, with the move sound. They stop at the first and the last option,
  where the arrow that leads nowhere is dimmed and nothing sounds; focus never leaves the row sideways. In
  right-to-left text they swap, as they do for a slider.
- A chooses the next option, wrapping from the last to the first.
- The keyboard's Left, Right and Enter do the same as the D-pad and A, and a click on the row as A. The pointer and
  touch can press either arrow; the arrows are never focus stops.
- The hint bar shows the Left and Right pair with **Adjust** on a focused row, as on a slider.
- A screen reader hears the setting's name, the chosen option as its value, and the new option after a change.

The choice rows are Design's Background, Background Blur, Background Dim, Cover Size and Text Size, and Controller's
Button Prompts, Swap Confirm and Back, Repeat Delay, Repeat Rate and Stick Dead Zone. Lists whose length varies or
whose names are long stay lists of rows: the designs, Variant, Genre, the Proton builds and the sound outputs.

### Design's cards

Design opens on one row of cards (`.design-cards`), in the order `pindu_core::panels::design_choices` gives: Adwaita,
then each installed design by name. Each card shows the design's picture (its `preview`, see
[`fullscreen-style.md`](fullscreen-style.md); Adwaita's is built in), clipped to rounded corners, over its name; a
design without a picture shows an empty card in the card colour. The design in force is checked: a check over its
picture, and its card is `:checked`.

- A on a card applies its design at once, and Quick Access stays open on it.
- Left and Right move along the row and stop at its ends; the row scrolls so the focused card shows whole. It runs to
  the panel's edge, so a card cut off there shows there are more.
- Down leaves the row for Variant, or for the first setting under it when the design has no variants (Variant is then
  hidden); Up from there comes back to the design in force.
- A design for a newer Pindu launcher shows insensitive, with "Needs a newer Pindu launcher" under its name.
- A screen reader reads each card's name, and "In use" for the design in force or why a design cannot be used.

## The buttons

| Button | On a section | At a level |
|---|---|---|
| LB, RB, <kbd>Ctrl</kbd>+<kbd>Page Up</kbd>, <kbd>Ctrl</kbd>+<kbd>Page Down</kbd> | The section before or after, wrapping from the last to the first, with the move sound | Nothing, except that the diagnostics report turns its pages |
| B | Closes Quick Access | Back to the section that holds the level's entry: the one it was opened from, or else the one it belongs to (the games with a log lead back to the report) |
| Up, Down | Move through the section; Up from its first item stops there. In Quick, between the cards, the rows of tiles and the activity strip | Move through the level |
| Left, Right | In Quick, along a row of tiles, stopping at its ends, and between a Playing Now card's Return and Stop | Turn a slider |
| A | Activates the focused item | Activates the focused item |

The hint bar shows the LB and RB pair with **Section** while a section shows and more than one has anything to show,
in the main window and over a running game. Quick Access's What Each Button Does and the keyboard shortcuts dialog say
the same.

The pointer and touch can press a section button. The controller's focus never lands on the strip; a screen reader
hears it as tabs, each section page as the tab panel its button names, and the Notifications button carries the
unread count.

## The Controller section's levels

- **What Each Button Does** lists every button and key and what it does in fullscreen mode, in groups, with the
  glyphs of the controller in use. The Menu rows add that Power is the dock at the menu's bottom, a press of Up from
  its top.
- **Change Buttons** lists the functions a controller button can be given, each with its button, then **Reset to
  Defaults**. Choosing one shows "Press a button…" and takes the next press; Guide, or a few seconds without a press,
  leaves it as it was. The hints, the menu's Search and What Each Button Does follow a change at once.

Each level's title is its row's name. B leads back to the Controller section, with focus on the row that led there.

## Focus

- A section shown for the first time since Quick Access opened focuses its first item: in Quick the first page card,
  or else the first Playing Now card's Return, or else the first tile (`pindu_core::panels::quick_first_focus`); in
  Notifications the first task, or else the first notification; in Design the design in force; in Controller the
  first controller that can rumble, or else Button Prompts; in System its first entry, or Navigation Sounds while none
  of Network, Bluetooth, Sound Output and System Update is available; and in Manage Sources.
- In Quick, Up and Down from a row of tiles to the next keep the column, or land on the nearest tile of a shorter row.
  Down from the last row lands on the activity strip; Up from the first row on the last page card, or else the last
  Playing Now card's Return. Moving into the tiles from above lands on the first tile, from the strip on the first tile
  of the last row. The D-pad, the left stick and the arrow keys move alike.
- Coming back to a section while Quick Access stays open focuses the item it had focused.
- Coming back from a level focuses the entry that led to it.

## Motion

With Reduce Motion on, or the system's reduced motion or animations off, sections change without a crossfade.
