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
| **Quick** | The Playing Now cards · Filters → (when the library is under the panel) · Performance → (when the focused game is one Pindu runs) · Sound Output → · Bluetooth → · Network → (each when available) · Surprise Me · Refresh Library · Add a Game |
| **Notifications** | A **Running** heading and the background tasks, each with its progress and Cancel, while something runs · then the notifications, newest first, or the line that says there are none |
| **Design** | The designs, Variant, Background, Background Blur, Background Dim, the cover sizes (Small, Medium and Large Covers), the text sizes (Default, Large, Larger and Largest Text), Reduce Motion, My Stylesheet |
| **Controller** | The controllers, Button Prompts, Swap Confirm and Back, Hold to Confirm, Repeat Delay, Repeat Rate, Stick Dead Zone and its note · then Controls → |
| **System** | Network → · Bluetooth → · Sound Output → · System Update → (each when available) · then, in a list of their own, Navigation Sounds, its Volume and Exit After Launching · then Diagnostics Report → |
| **Manage** | Sources → · Duplicates → · Library Health → · Proton → · Add a Store → · Add a Game · Refresh Library · then **Artwork**: SteamGridDB Key and Prefer SteamGridDB Art · then **Metadata**: Get Metadata From Wikidata, and under it Refresh Metadata while it is on |

- Sound Output, Bluetooth, Network, Refresh Library and Add a Game are listed twice on purpose: once where they are
  quick to reach, once where they belong. Both rows do the same and show the same second line.
- Quick Access has no Settings level: each setting sits in the section of what it changes, with the same rows,
  second lines and GSettings keys as before, which Preferences changes too. Text Size and Reduce Motion are with the
  look, Hold to Confirm with the buttons. Volume is insensitive while Navigation Sounds is off, and Left and Right turn
  it. The SteamGridDB Key row opens the hidden-text keyboard, and its second line says whether a key is stored, read
  again each time Manage shows.
- Over a running game (`game_overlay`), Surprise Me, Add a Store, Add a Game and Diagnostics Report are left out.
- A section with nothing to show is left out of the strip and of LB and RB's steps.
- Notifications marks the notifications read once it shows. When the last background task ends, the Running heading
  and its list go, and focus on a task moves to the first notification.

## The buttons

| Button | On a section | At a level |
|---|---|---|
| LB, RB, <kbd>Ctrl</kbd>+<kbd>Page Up</kbd>, <kbd>Ctrl</kbd>+<kbd>Page Down</kbd> | The section before or after, wrapping from the last to the first, with the move sound | Nothing, except that the diagnostics report turns its pages |
| B | Closes Quick Access | Back to the section that holds the level's entry: the one it was opened from, or else the one it belongs to (the games with a log lead back to the report) |
| Up, Down | Move through the section, from one of its lists to the next and past headings and hidden lists; Up from its first item stops there | Move through the level |
| A | Activates the focused item | Activates the focused item |

The hint bar shows the LB and RB pair with **Section** while a section shows and more than one has anything to show,
in the main window and over a running game. Quick Access's Controls and the keyboard shortcuts dialog say the same.

The pointer and touch can press a section button. The controller's focus never lands on the strip; a screen reader
hears it as tabs, each section page as the tab panel its button names, and the Notifications button carries the
unread count.

## Focus

- A section shown for the first time since Quick Access opened focuses its first item: in Quick the first Playing Now
  card's Return, or else its first entry; in Notifications the first task, or else the first notification; in Design
  the design in force; in Controller the first controller that can rumble, or else the first button prompt; in System
  its first entry, or Navigation Sounds where the system offers none; in Manage Sources.
- Coming back to a section while Quick Access stays open focuses the item it had focused.
- Coming back from a level focuses the entry that led to it.

## Motion

With Reduce Motion on, or the system's reduced motion or animations off, sections change without a crossfade.
