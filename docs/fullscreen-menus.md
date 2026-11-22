# Fullscreen mode's menus

How fullscreen mode's Quick Access panel lays out its entries and what the controller's buttons do in them. The
classes a design can style are in [fullscreen-style.md](fullscreen-style.md); what a design folder holds is there too.

## Quick Access → Design

Design opens on a row of cards, one per design, in the order `pindu_core::panels::design_choices` gives: Adwaita
first, then each installed design by name. Under the row come Variant (a list, hidden for a design without variants),
Background (while the design in force offers the choice), Background Blur, Background Dim and My Stylesheet.

- **A card** shows a picture of the design's Home, its preview, clipped to rounded corners, over the design's name.
  The shipped designs and Adwaita have one; a design of your own shows its `"preview"` from `theme.json`, or an empty
  card in the card colour without one.
- **The design in force** is marked with a check and a ring, and a screen reader hears "In use" with its name.
- **A design for a newer Pindu launcher** shows dimmed with "Needs a newer Pindu launcher" under its name; it cannot
  take focus or be chosen.
- **The row runs to the panel's edge**, so a card cut off there shows there are more.

| Button | What it does |
|---|---|
| A | Applies the focused card's design at once; Quick Access stays open |
| Left, Right | Move along the row and stop at its ends; the row scrolls so the focused card shows whole |
| Down | Leaves the row for Variant, or for the first setting under it while Variant is hidden |
| Up (from a list under the row) | Back to the row, on the design in force |
| B | Back to Quick Access's entries |

When Design shows, focus starts on the design in force.
