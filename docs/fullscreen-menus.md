# Fullscreen menus

Fullscreen mode has two menus: the menu drawer on the left (Start or Guide) and Quick Access on the right (Guide + A,
a right stick click, the Steam Deck's "…" button, or <kbd>Ctrl</kbd>+<kbd>2</kbd>). This page says how Quick Access is
arranged and what each part holds. The maintainer chose this arrangement, design A of the fullscreen menus design
pass, on 2026-11-22; it is built in the issues of that series, starting with #717, and each one extends this page.

Which panel is open, at which level and section, and what the buttons do to them is `pindu_core::panels` (`Panels`);
what each section lists, and in which order, is `pindu_core::panels::section_entries`. The panel itself is
`PinduQuickAccess` (`crates/pindu-ui/src/quick_access.rs`, `data/ui/quick_access.blp`).

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
| **Design** | The designs, Variant · then one list of choice rows: Background (when the design offers the choice), Background Blur, Background Dim · My Stylesheet |
| **Controller** | The controllers · then one list of choice rows: Button Prompts, Swap Confirm and Back, Repeat Delay, Repeat Rate, Stick Dead Zone · their note · then Controls → |
| **System** | Network → · Bluetooth → · Sound Output → · System Update → (each when available) · Settings → · Diagnostics Report → |
| **Manage** | Sources → · Duplicates → · Library Health → · Proton → · Add a Store → · Add a Game · Refresh Library · Refresh Metadata (when Wikidata metadata is on) |

- Sound Output, Bluetooth, Network, Refresh Library and Add a Game are listed twice on purpose: once where they are
  quick to reach, once where they belong. The tile and the row do the same and show the same second line.
- Over a running game (`game_overlay`), Surprise Me, Add a Store, Add a Game and Diagnostics Report are left out.
- A section with nothing to show is left out of the strip and of LB and RB's steps.
- Notifications marks the notifications read once it shows. When the last background task ends, the Running heading
  and its list go, and focus on a task moves to the first notification.

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

The choice rows are Design's Background, Background Blur and Background Dim; Controller's Button Prompts, Swap Confirm
and Back, Repeat Delay, Repeat Rate and Stick Dead Zone; and Settings' Cover Size and Text Size, in one list of their
own at the top. Lists whose length varies or whose names are long stay lists of rows: the
designs, Variant, Genre, the Proton builds and the sound outputs.

## The buttons

| Button | On a section | At a level |
|---|---|---|
| LB, RB, <kbd>Ctrl</kbd>+<kbd>Page Up</kbd>, <kbd>Ctrl</kbd>+<kbd>Page Down</kbd> | The section before or after, wrapping from the last to the first, with the move sound | Nothing, except that the diagnostics report turns its pages |
| B | Closes Quick Access | Back to the section that holds the level's entry: the one it was opened from, or else the one it belongs to (the games with a log lead back to the report) |
| Up, Down | Move through the section; Up from its first item stops there. In Quick, between the cards, the rows of tiles and the activity strip | Move through the level |
| Left, Right | In Quick, along a row of tiles, stopping at its ends, and between a Playing Now card's Return and Stop | Turn a slider |
| A | Activates the focused item | Activates the focused item |

The hint bar shows the LB and RB pair with **Section** while a section shows and more than one has anything to show,
in the main window and over a running game. Quick Access's Controls and the keyboard shortcuts dialog say the same.

The pointer and touch can press a section button. The controller's focus never lands on the strip; a screen reader
hears it as tabs, each section page as the tab panel its button names, and the Notifications button carries the
unread count.

## Focus

- A section shown for the first time since Quick Access opened focuses its first item: in Quick the first page card,
  or else the first Playing Now card's Return, or else the first tile (`pindu_core::panels::quick_first_focus`); in
  Notifications the first task, or else the first notification; in Design the design in force; in Controller the
  first controller that can rumble, or else Button Prompts.
- In Quick, Up and Down from a row of tiles to the next keep the column, or land on the nearest tile of a shorter row.
  Down from the last row lands on the activity strip; Up from the first row on the last page card, or else the last
  Playing Now card's Return. Moving into the tiles from above lands on the first tile, from the strip on the first tile
  of the last row. The D-pad, the left stick and the arrow keys move alike.
- Coming back to a section while Quick Access stays open focuses the item it had focused.
- Coming back from a level focuses the entry that led to it.

## Motion

With Reduce Motion on, or the system's reduced motion or animations off, sections change without a crossfade.
