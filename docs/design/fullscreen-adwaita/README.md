# Reference mockups: the Adwaita fullscreen design, redrawn after GNOME

Reference material for the issue "Redraw the Adwaita fullscreen design after GNOME". This folder is on the
`design/fullscreen-home-designs` branch only; it is not meant to be merged.

| Screen | Picture | Named parts | Source |
|---|---|---|---|
| Home, `explore` layout | `adwaita-home.jpg` | `adwaita-home-annotated.jpg` | `adwaita-home.html` |
| Quick Access, main level | `adwaita-quick-access.jpg` | `adwaita-quick-access-annotated.jpg` | `adwaita-quick-access.html` |
| Quick Access, Design level | `adwaita-quick-access-design.jpg` | `adwaita-quick-access-design-annotated.jpg` | `adwaita-quick-access-design.html` |
| Menu drawer | `adwaita-menu-drawer.jpg` | `adwaita-menu-drawer-annotated.jpg` | `adwaita-menu-drawer.html` |

- Every picture is a 1280×720 logical-pixel screen, rendered at 1.5×, in the dark style with a purple accent.
- The mockups are web pages, not screenshots of the app. `adwaita.css` holds the sizes, radii and spacing; the
  layout technique is the web's, not GTK's. `mock.js` only builds the markup.
- `adwaita.css` writes libadwaita's dark palette out as hex values under the variables' own names. In the app,
  use the variables, never the hex values.
- Add `?annot` to a page's address to outline the named parts, as in the `-annotated` pictures.
- The game art in `art/` was drawn for these mockups; the games do not exist.
- Where a mockup and the issue disagree, the issue wins.
