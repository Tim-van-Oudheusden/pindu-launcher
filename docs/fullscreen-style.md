# Styling fullscreen mode

Fullscreen mode can be restyled with CSS: by a **design** (a theme folder Pindu ships, or one you add)
and by your own **`fullscreen.css`** on top of it. Neither ever reaches the desktop window, and both are
switched off while the system asks for high contrast.

This page is the style contract: the files, the `theme.json` fields and the CSS names you can rely on.

<!-- pindu-style-contract: 1 -->
**Style contract version: 1**

## Where the files go

| What | Flatpak | Native |
|---|---|---|
| Your designs | `~/.var/app/io.github.tim_van_oudheusden.pindu-launcher/config/pindu-launcher/themes/<id>/` | `~/.config/pindu-launcher/themes/<id>/` |
| Your `fullscreen.css` | `~/.var/app/io.github.tim_van_oudheusden.pindu-launcher/config/pindu-launcher/fullscreen.css` | `~/.config/pindu-launcher/fullscreen.css` |
| Shipped designs | `/app/share/pindu-launcher/themes/<id>/` | `<prefix>/share/pindu-launcher/themes/<id>/` |

- A design's `<id>` is its folder name: lowercase letters, digits and dashes. Your design with the same id as a
  shipped one replaces it.
- Pick a design in fullscreen mode's **Quick Access** panel (Guide + A, right stick click, the Steam Deck's "…"
  button, or Ctrl+2) in its **Design** section, or set it with
  `gsettings set io.github.tim_van_oudheusden.pindu-launcher fullscreen-design <id>`. Lobby (`lobby`) is
  the default; **Adwaita**, an empty id, is no design: the standard libadwaita look. Four looks ship: Adwaita,
  Lobby, Marquee and Shelves. Ribbon and Panorama no longer ship; a setting that still names `ribbon` or
  `panorama` is switched to `lobby`, unless you have a design of your own with that id.
- A design can offer **variants** (below): pick one in Quick Access's **Design** section under **Variant**, or
  set it with `gsettings set io.github.tim_van_oudheusden.pindu-launcher fullscreen-design-variant
  <variant-id>`. An empty or unknown id picks the design's first variant.
- `fullscreen.css` applies while the `fullscreen-custom-style` setting is on: Quick Access's **My Stylesheet**
  switch under **Design**, or **Use My Stylesheet** in Preferences → Fullscreen, which can only be turned on
  once the file exists. Turning it off removes the style at once.
- Preferences → Fullscreen → **My Stylesheet** also shows where the file goes, opens its folder (creating it
  if needed) and has **Import Style…**, which copies a stylesheet you pick in as `fullscreen.css`, so its
  `url()` and `@import` resolve in that folder. **Reset** turns the setting off. Both keep the file they
  replace as `fullscreen.css.bak`, replacing an older backup.
- Saving a stylesheet, or another file directly in the design's or the variant's folder, reloads it at once.
  Files in other subfolders (such as `assets/`) are picked up the next time the stylesheet loads.
- Problems GTK finds in a stylesheet are logged with their line, and fullscreen mode shows a toast with
  their number. For `fullscreen.css`, Preferences shows the number and the first few, updated each time
  the file changes. The rest of the stylesheet still applies.

## A design folder

```
themes/my-design/
  theme.json
  theme.css
  assets/…       optional; refer to them as url("assets/bg.svg")
  mono.css       optional; a variant's stylesheet
  preview.svg    optional; a picture of the design for Quick Access
  LICENSE        required for designs shipped with Pindu
```

`theme.json`:

```json
{
  "contract": 1,
  "name": "My Design",
  "description": "One line about it",
  "author": "You",
  "license": "CC0-1.0",
  "preview": "preview.svg",
  "layout": {
    "home": "row-top",
    "home-tile": "square",
    "home-tile-size": 112,
    "top-bar": true,
    "home-details": true
  },
  "variants": [
    { "id": "classic", "name": "Classic" },
    { "id": "mono", "name": "Monochrome", "stylesheet": "mono.css" }
  ]
}
```

| Field | Required | Meaning |
|---|---|---|
| `contract` | yes | The style contract version the design was written for. A design for a newer contract than this Pindu launcher's is listed as "Needs a newer Pindu launcher" and not applied. |
| `name` | yes | Shown in the Design list. |
| `description`, `author`, `license` | no | About the design. |
| `preview` | no | A picture of the design, SVG or PNG, inside the design's folder and relative to it, drawn for 16:10; Quick Access shows it at about 108×66 pixels on the design's card. A preview that is missing or lies outside the design's folder (an absolute path, `..`, or a symbolic link out of it) is refused with the reason in the log, and the design shows without a picture. |
| `layout` | no | The layout options below. Unknown keys and values are ignored and fall back to the default. |
| `variants` | no | The looks the design offers within itself, below. |

### Layout options

CSS restyles widgets but cannot move, add or remove them. These options change what is built:

| Option | Values (default first) | Effect |
|---|---|---|
| `home` | `"hero-bottom"`, `"row-top"`, `"shelves"`, `"explore"` | Where Home's row of tiles sits, on both its screens: under the title at the bottom, or near the top with the title and details below it. `"shelves"` makes Home one screen instead: the title and details at the top, and under them every section as a row of its games (below). `"explore"` makes Home one screen of rows under the top bar, with no title, art or background over the window's own: a carousel of the Jump Back In games (`.home-featured`), each page with the game's art, logo or title, details, Play and Details; a row of compact section tiles with the Library and Steam Big Picture tiles, each of which moves focus to its section's row; then a row of covers for each other section (below). |
| `home-start` | `"sections"`, `"games"` | What the first screen's row starts with: a tile for every section, or the Recently Played games as cards, then a divider and a tile for every other section. Ignored with `"home": "shelves"` and `"explore"`. |
| `home-tile` | `"landscape"`, `"square"`, `"portrait"` | The shape of Home's tiles, on both its screens, and the art a game's card shows: the banner, else the cover; SteamGridDB square art, else the cover cropped to square; the cover, else the banner. Cards never show icons. |
| `home-tile-size` | a number, 64–400 | The tiles' shorter side in pixels; art is decoded for 1.6× that size, so a focused card can grow that much and stay sharp. Without it the shape's own size applies. |
| `home-section-tile` | `"art"`, `"label"` | What a section's tile shows: its art filling the tile, or a glyph (a launcher's two letters, or an icon), the section's name and its number of games. With `"label"` the Library and Steam Big Picture tiles take the same shape. |
| `home-section-preview` | `false`, `true` | A strip under the details row, on the first screen, of the focused section's first six covers, then `+N` for the games beyond them. |
| `home-background-choice` | `false`, `true` | Whether the design offers Quick Access → Design → **Background**. Without it Home shows Red Ribbons, whatever was chosen under a design that does. |
| `top-bar` | `false`, `true` | A bar at the top of Home and the library with Home and Library tabs and a clock. |
| `top-bar-tabs` | `"start"`, `"center"` | Where the top bar's Home and Library tabs sit: at its start, or in its middle whatever the clock's width. |
| `home-details` | `false`, `true` | A row under Home's title: the focused section's number of games, or the focused game's playtime, when it was last played and its source. |
| `panel-sections` | `"top"`, `"side"` | Where Quick Access's section strip sits: across the top, or as a rail of icons down the panel's inner edge, with the shown section's name over its content. |
| `quick-tiles` | `"grid"`, `"pills"`, `"list"`, `"shelf"` | Quick's tiles as a grid of three columns with the icon over the name; as two columns of wide pills with the icon beside the name; as full-width rows with the second line at the row's end; or as shelves, each a heading over a row that Left and Right move along: Devices (Sound Output, Bluetooth and Network) and Library (Surprise Me, Refresh Library and Add a Game), a shelf with no tile left out. |
| `menu-entries` | `"list"`, `"shelves"` | The menu's places and collections as rows, or as two shelves of cards, each card with its icon and name: Go To (Home, Library and Search) and Collections (every collection, the row scrolling, so there is no All Collections). The power dock, with the ways out, stays under them either way. |
| `playing-card` | `"compact"`, `"banner"` | The Playing Now card as it has been: "Playing Now" and how long the game has run, its title, then Return and Stop; or as a banner of the game's hero art, else its cover (the card colour without either), across the panel's top with the title over a scrim and Return and Stop on it. |

The last four arrange the menu drawer and Quick Access, in the main window and over a running game; they
never change what the menus hold. In every arrangement a menu and each section hold the same entries in
the same order, and every button does the same thing. What changes is where things sit, and so whether two
neighbours are a Left/Right or an Up/Down apart. In a grid, pills and rows, Left and Right move along a row
and stop at its ends, and Up and Down keep the column, into a shorter row its nearest tile. On shelves,
Left and Right move along a shelf and stop at its ends, and Up and Down change shelf, to the card that shelf
last had focused, as on Home's shelves. With `"menu-entries": "shelves"` the dock keeps its rules: Up from
the top of the menu still lands on Suspend, Down from the last shelf enters the dock, and Up from the dock
returns to the last shelf.

Adwaita, fullscreen mode with no design (and what shows under high contrast), has a layout of its own:
`"home": "explore"`, `"home-tile": "portrait"` at 116 pixels, `"top-bar": true` with `"top-bar-tabs":
"center"`, `"home-details": true`, `"home-section-tile": "label"` and `"quick-tiles": "pills"`, GNOME's
quick settings' two columns. What a `theme.json` leaves out still takes the defaults in the table above,
not Adwaita's.

### Variants

A design can come in variants, as an ES-DE theme has variants and colour schemes: a lighter palette, a
layout without details, white instead of the accent colour. Each entry of `variants` has:

| Key | Required | Meaning |
|---|---|---|
| `id` | yes | Lowercase letters, digits and dashes; unique within the design. The `fullscreen-design-variant` setting holds it. |
| `name` | yes | Shown in Quick Access's Variant list. |
| `stylesheet` | no | A CSS file inside the design's folder, relative to it, loaded after `theme.css` while the variant is chosen. Without one the variant is `theme.css` alone. |

- The first variant is the default: it applies until another is chosen, and when the chosen id is unknown
  (say, after switching designs). Make it the plain look, usually without a `stylesheet`.
- Write `theme.css` as a complete design and each variant's stylesheet as changes on top of it: a Pindu
  launcher older than variants ignores the list and shows `theme.css` alone.
- A `variants` list Pindu cannot follow skips the whole design, with the reason in the log: it is not a list,
  an entry lacks an `id` or `name` or repeats an `id`, or a `stylesheet` is missing or lies outside the
  design's folder (an absolute path, `..`, or a symbolic link out of it).
- Layout options belong to the design, not a variant.

No shipped design has variants. The `theme.json` example above shows the shape: **Classic** is
`theme.css` alone, and **Monochrome** adds `mono.css`, which can be as small as redefining libadwaita's
accent variables, if the design's rules use them:

```css
window.fullscreen-mode {
  --accent-color: #ffffff;
  --accent-bg-color: #f2f2f2;
  --accent-fg-color: #0d0d0d;
}
```

## Writing the CSS

Start every rule with `window.fullscreen-mode`, so it can never match anything in the desktop window, even
if your file is loaded there one day:

```css
window.fullscreen-mode .home-carousel > button:focus-within {
  outline: 3px solid var(--accent-color);
}
```

The order of the stylesheets, last wins: libadwaita, Pindu's own, the design, the design's variant, your
`fullscreen.css`.

GTK's CSS is not the web's: see the [GTK CSS overview](https://docs.gtk.org/gtk4/css-overview.html) and
[properties](https://docs.gtk.org/gtk4/css-properties.html). There is no `@font-face` (use fonts the
runtime has, such as Adwaita Sans at any weight), no `display`, `position` or percentage sizes, and
`min-width`/`min-height` can only grow a widget. Transforms do not move neighbours; margins do.

### Colours

Redefine [libadwaita's colour variables](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/css-variables.html)
on `window.fullscreen-mode` rather than `:root`: `--window-bg-color`, `--window-fg-color`, `--view-bg-color`,
`--card-bg-color`, `--sidebar-bg-color`, `--sheet-bg-color`, `--popover-bg-color`, `--dialog-bg-color` and
their `-fg-` partners. Use `--accent-color`, `--accent-bg-color` and `--accent-fg-color` for the user's accent.

### Media queries

`@media (prefers-color-scheme: dark)` follows the system style, and `@media (prefers-reduced-motion: reduce)`
matches while the window is `reduced-motion`: the system's reduced-motion or animations setting, or Pindu's
own **Reduce Motion** switch (Quick Access → Design). Turn off your transitions and animations under the
latter, including the `.home-hero` crossfade:

```css
@media (prefers-reduced-motion: reduce) {
  window.fullscreen-mode .home-carousel > button,
  window.fullscreen-mode .home .home-hero { transition: none; }
}
```

### Text size

**Text Size** (Quick Access → Design, or Preferences) has four steps, Default, Large, Larger and Largest; each
step past Default adds about a quarter of fullscreen mode's own type size. Above Default the window is
`large-text` and `text-large`, `text-larger` or `text-largest`, and Pindu sets `font-size` on
`window.fullscreen-mode.text-large` and the others. Size your text in `em` so it follows, and give rows and tiles
that hold text room to grow (or no fixed height) so nothing is cut off. A design can set its own scale under the
size classes, or restyle anything at every larger size under `window.fullscreen-mode.large-text`.

## Names

These are the names designs can rely on in contract 1. A `:checked`, `:focus-within` or `:hover` state is
noted where it carries meaning. In fullscreen mode focus is always shown, so `:focus-within` on a tile is the
controller's position.

**Window**

| Name | What |
|---|---|
| `window.fullscreen-mode` | The window, while in fullscreen mode |
| `window.no-design` | Beside `fullscreen-mode` while fullscreen mode shows no design, Adwaita: none is chosen, the chosen one is not installed or needs a newer contract, or the system asks for high contrast. Pindu's own Adwaita look scopes its rules to it, so no design and no `fullscreen.css` is touched by them; `fullscreen.css` still applies on top |
| `window.reduced-motion` | The window, while Pindu moves less: the system asks for less motion (reduced motion, or animations off) or **Reduce Motion** is on in fullscreen mode |
| `window.large-text` | The window, in fullscreen mode while **Text Size** is above Default |
| `window.text-large`, `window.text-larger`, `window.text-largest` | The window, in fullscreen mode at that **Text Size** |

**Top bar** (with `top-bar`)

| Name | What |
|---|---|
| `.fullscreen-top-bar` | The bar |
| `.page-tab` | The Home and Library tabs; the shown page's is `:checked` |
| `.clock` | The clock |

**Home**

Home has two screens. The first, `.home-sections`, is a row of sections over Home's animated background:
Recently played, Favourites, Installed, one per launcher, the user's collections, then the Library tile.
Activating a section opens the second, `.home-games`: that section's games over the focused game's art.
B goes back to the first. Both screens use the same widgets and layout options, so a rule without either
class styles both.

With `"home-start": "games"` the first screen's row starts with the Recently Played games as cards, as on
the second screen, then a divider and the other sections' tiles; Recently Played has no tile. `.home` is
also `.home-start-games`, on both screens. A focused game shows its art, logo or title and details, under
a "Jump Back In" label; a focused section shows its first game's art, its name and its number of games.
Pindu's own style has no shade on `.home-sections`, so give `.home.home-sections.home-start-games
.hero-gradient` one.

With `"home": "shelves"` Home is one screen, `.home-layout-shelves`, which is always `.home-games`: the
logo or title and the details at the top, and under them `.home-shelves`, a list that scrolls. It has a
row, `.home-shelf`, for each section, of its first 20 games and then **See All**, which opens the library
at that section, and a last row with the Library tile. Left and Right move along a row and stop at its
ends; Up and Down change row, to the card that row last had focused, and the row with focus sits at the
top of the list. Recently Played's row is named Jump Back In and holds wide cards 130 pixels tall, with the
art the desktop window's Jump Back In row shows: the banner, else the cover, cropped to fill the card, or
the icon when the game has neither. The other rows take `home-tile` and `home-tile-size`. Each row's cards
sit in a `.home-carousel` of its own, so rules for `.home-carousel > button` style them too; scope a rule
with `.home-shelf` or `.home-carousel`'s `.home-tile-*` class to tell the rows apart.

With `"home": "explore"` (Adwaita's layout) Home is one screen, `.home-layout-explore`, which is neither
`.home-sections` nor `.home-games` and shows no title, art or background: the page is the window's own
`--window-bg-color`, and the rows start under the top bar. `.home-shelves` holds, top to bottom: the Jump
Back In games as a carousel, `.home-featured`, when there are any; a `.home-carousel` of compact section
tiles, one for every section but Recently Played, then the Library tile and, where Steam is present, the
Steam Big Picture tile; then a `.home-shelf` for each of those sections, as in the Shelves layout but
without the Library row. Focus starts on the shown page's Play. In the carousel Right goes from Play to
Details and on to the next page's Play, Left from Details to Play and back to the page before's Play,
stopping at either end; the carousel shows the page with focus, and a swipe or the scroll wheel takes focus
with it. A on a section tile moves focus to that section's row. Up and Down change row as in the Shelves
layout; Up from the carousel reaches the top bar, and Down from the top bar comes back to the row and item
that last had focus.

**Categories on Home.** Home's categories are the genres the library's games have, hidden and removed
games left out; **Categories on Home** (Quick Access → Design, Preferences → Fullscreen, GSettings
`fullscreen-home-categories`, on by default) turns them off in every layout, as does a library with no
genre. In the layouts with two screens the first screen's row has a **Categories** tile right after
Installed, `.home-section-tile.section-categories` (its art is `data/home/categories.svg`; labelled, a glyph,
"Categories" and the number of categories). Focused, it shows what a focused section does, its first
category's first game standing in for the section's. A on it opens the **Categories screen**: `.home` is
`.home-sections` and `.home-categories` while it shows, and its row holds a tile per category, by name,
`.home-section-tile.section-category`, which looks like a collection's tile (`.home-collection-tile` when
not labelled) with the category's name and number of games, over the focused category's first game's art. A
on a category opens its games, `.home-games`, as for a section. B goes back one screen: from a category's
games to the Categories screen, on that category, then to the first screen, on the Categories tile. In the
Shelves and Explore layouts the eight categories with the most games (most first, ties by name) are rows
after the collections' rows, `.home-shelf.home-shelf-category`, with See All opening the library at All Games
filtered to that category; the others share one last row before the Library row, **More Categories**,
`.home-shelf.home-shelf-more-categories`, a `.section-category` tile each, by name, which opens the library
the same way. The Explore layout's row of compact tiles has one Categories tile after the collections' tiles,
which moves focus to the first category's row. These names are added; the contract's version does not change.

A design with `"home-background-choice": true` offers **Quick Access → Design → Background**, which
switches between **Red Ribbons** (the default) and **Blue Station**; Lobby is the shipped one that does.
Use Up and Down to move between Background and the other Design settings. The choice applies immediately
and is saved across restarts. Under any other design, Adwaita included, the Background rows are hidden and
Home shows Red Ribbons; the choice is kept, and applies again under a design that offers it. Both
animations ship at 720p, 1080p and 1440p, with Red Ribbons at 60 fps. Home uses the smallest video that
fills the display, or the largest if none does.

The background plays muted and looped behind the first screen. It pauses on the second screen, under art
that covers it on the first, in the Shelves layout, while Quick Access, the menu drawer or the options
sheet is open, while a game we started runs and while the window is hidden. It holds its first frame
outside fullscreen mode, while the window is `reduced-motion` and when GStreamer has no hardware VP9
decoder. While the system asks for high contrast it is not shown at all: `.home-background`'s plain
black stands in for it.

Quick Access's **Background Blur** and **Background Dim** (each Off, Low or High) put `blur-low` or
`blur-high` and `dim-low` or `dim-high` on `.home`. Pindu blurs `.home-background` and `.home-hero` with
`filter: blur()` and darkens them with a translucent black `.home-dim`; restyle those to make the
strengths your own:

```css
window.fullscreen-mode .home.blur-high .home-background,
window.fullscreen-mode .home.blur-high .home-hero { filter: blur(40px) saturate(1.4); }
window.fullscreen-mode .home.dim-low .home-dim { background-color: rgb(20 0 40 / 30%); }
```

| Name | What |
|---|---|
| `.home` | The page; also `.home-sections` on the first screen or `.home-games` on the second and in the Shelves layout (neither in the Explore layout), `.home-layout-hero-bottom`, `.home-layout-row-top`, `.home-layout-shelves` or `.home-layout-explore`, `.home-start-games` while the first screen's row starts with games, `.no-art` while no art is shown, on either screen, and `.blur-low` or `.blur-high` and `.dim-low` or `.dim-high` as Quick Access's Background Blur and Dim say |
| `.home-background` | Home's animated background, filling the page under the art; black where there is no video, hidden (`opacity: 0`) on the second screen. Blurred with `filter` under `.blur-*`; a `filter` of your own replaces Pindu's blur |
| `.home-hero` | The focused game's art filling the page; the one showing the current art also has `.shown`, so `transition: opacity` crossfades. While a held direction moves focus through the items, the art shown stays until focus stops, so the crossfade runs once. Blurred like `.home-background` |
| `.home-dim` | A layer over the background and the art, under the shade and the text; transparent until `.dim-low` or `.dim-high` darkens it |
| `.hero-gradient` | The shade over the art |
| `.home-eyebrow` | "Jump Back In", over the title while one of the Recently Played games has focus on the first screen (with `"home-start": "games"`) |
| `.home-title` | The row with the focused section's name, or the game's logo or title |
| `.home-logo` | The logo, contained in 480×160, or 360×96 in the Shelves layout, unless you grow it with `min-width`/`min-height` |
| `.title-1` | The title, when there is no logo |
| `.home-details`, `.detail` | The details row and each detail (with `home-details`) |
| `.home-preview` | The strip of the focused section's first covers (with `home-section-preview`): up to six `cover-art`, 80×120, then `.preview-more`, the `+N` for the games beyond them. It takes no focus |
| `.home-carousel` | The row of tiles; also `.home-tile-landscape`, `.home-tile-square` or `.home-tile-portrait` |
| `.home-section-tile` | A section's tile (a button) on the first screen, its art filling it; a collection's also has `.home-collection-tile`, an icon over its name |
| `.home-section-tile.labelled` | A section's tile with `"home-section-tile": "label"`. It also has the section's own class: `.section-recently-played`, `.section-favourites`, `.section-installed`, a launcher's (`.section-steam`, `.section-heroic-epic`, `.section-heroic-gog`, `.section-heroic-amazon`, `.section-heroic-sideload`, `.section-lutris`, `.section-bottles`, `.section-faugus`, `.section-legendary`, `.section-itch`, `.section-desktop`, `.section-manual`) or `.section-collection` for every collection. The Library and Steam Big Picture tiles are `.library-tile.labelled` |
| `.section-label` | What a labelled tile holds: `.section-glyph` at the top left, `.section-name` at the bottom left, of two lines at most, and `.section-count`, the number of games, under it. `.section-glyph` is an `image`: a launcher's own symbolic icon where Pindu ships one (Steam, Lutris, Bottles, Faugus, Legendary, itch; `pindu-source-<id>-symbolic`), else GNOME's icon for the section; only Epic, GOG and Amazon show a `label` with their two letters. Images take the text colour, so `color` restyles them. Pad this, not the tile: a tile's own padding adds to its size |
| `.home-featured` | The Explore layout's carousel (an `AdwCarousel`) of the Jump Back In games, with an `AdwCarouselIndicatorDots` under it while it has more than one page. Each page is `.featured-page` |
| `.featured-page` | A page: the game's hero art, else its cover, filling it (`--card-bg-color` without), `.featured-scrim` over the art, then `.featured-text` at the left: `.home-eyebrow`, the logo (`.home-logo`, 360×96) or the title (`.title-1`), `.home-details`, `.featured-play` and `.featured-details` |
| `.featured-scrim` | The shade over a page's art that keeps the text on it readable; the text is white in the light style too |
| `.featured-play` | Play, or Stop while the game runs (a `pill` `suggested-action` button on `app.play` or `app.stop`) |
| `.featured-details` | Details (a `pill` button), which opens the game page |
| `.home-section-tile.labelled.compact` | A section's compact tile in the Explore layout's row of tiles, 176×64: `.section-label` with `.section-glyph` at the left and `.section-name` over `.section-count` beside it. The Library and Steam Big Picture tiles in that row are `.library-tile.labelled.compact` |
| `.home-divider` | The line between the game cards and the section tiles of a row that starts with games |
| `.jump-card` | A game's card (a button); `.jump-title` is its title on the art, `.badge` its pin. `.no-art` once its art has loaded none: the game has no art for the card's shape, or it failed to load; it goes once art loads. A design that hides `.jump-title` should show it on such a card, where nothing else says which game it is, for example with `.jump-card:not(.no-art) .jump-title { opacity: 0; }`. Not to be confused with `.home.no-art` on the page |
| `.library-tile` | The Library tile at the end of the first screen's row, and the Steam Big Picture tile, which is also `.big-picture-tile` |
| `.home-setup` | The Get Started checklist over Home's background, in place of its content while a new console has nothing set up: a title, a line under it, a `.boxed-list` of rows and the Hide This List pill |
| `.home-shelves` | The list of rows of the Shelves layout |
| `.home-shelf` | A row of it: `.shelf-header`, with `.shelf-title` (the section's name) and `.shelf-count` (its number of games, which Jump Back In does not show), over its `.home-carousel`; `:focus-within` on the row with focus. The last row is also `.shelf-library` |
| `.shelf-more` | **See All** (a button), the last tile of a row whose section holds more games than the row shows |

**Library**

| Name | What |
|---|---|
| `.library-tabs` | The bar of tabs and the sort order |
| `.library-tab` | A tab; the shown one is `:checked` |
| `.filtered-chip` | The Filtered chip beside the tabs, shown while Quick Access's Filters narrow them |
| `gridview.library` | The grid of covers; its `padding-top` is the room above the first row, so give a focus `box-shadow` or a larger `transform` enough of it to show whole |
| `gridview.library > child` | A tile's place in the grid; `:focus-within` on the focused one |
| `.cover-tile` | A tile: `.cover-frame` (the cover with its badges, which takes the focus ring; give it the cover's `border-radius`), `cover-art` (the cover), `.badge` (source and state), `.play-button`, `label` (the title, absent in fullscreen mode) |

**Game page**

| Name | What |
|---|---|
| `.game-page` | The page |
| `.game-hero` | The art at the top, with its `.hero-gradient` |
| `.game-actions` | The row of buttons; `.game-play` is Play |
| `.playing-card` | The Playing Now card while the game runs; the menu drawer and the top of Quick Access show it too, with `.session-time`, how long the game has run. As a banner (a panel with `.playing-card-banner`) it holds the game's art (`.playing-card-art`) under its text in a `.playing-card-scrim`, and is `.no-art` while it has none |
| `.game-about` | The About group: the developer, categories, release year, controller support and a Steam game's Steam Deck rating, in a `.boxed-list` whose rows take focus |
| `.game-description` | The description; it takes focus, and Up and Down scroll through it while it has it |
| `.launch-overlay` | The overlay while a game starts: the game's logo (`.launch-logo`), or its title (`.title-1`) when it has none, over a spinner, the status line and, after a while, Cancel. Without a design it stays dark in the light style too, so a logo made for a dark backdrop shows on it |
| `.launch-logo` | The logo on `.launch-overlay`, contained and centred in 480×160, unless you grow it with `min-width`/`min-height`; it stands in for the title, which it carries as its accessible label |

**Panels and bars**

| Name | What |
|---|---|
| `.menu-drawer` | The menu drawer on the left: the Playing Now cards, Home, Library, Search, the first collections under a Collections heading, then their levels. Each row's number of games is `.numeric` and `.dim-label`. Also `.menu-shelves` with `"menu-entries": "shelves"`, and `.playing-card-banner` with `"playing-card": "banner"` |
| `.menu-drawer row.current` | The row for the page under the menu, Home or Library, while either is shown, and with `.menu-shelves` its card (`.menu-card.current`); a screen reader reads it as the current page |
| `.power-dock` | The menu drawer's power dock, under its main level's list or shelves: Exit Fullscreen Mode, Switch to Desktop and Exit Pindu Launcher at its start, Suspend, Restart and Power Off at its end |
| `.dock-button` | A round button of the dock (also `.circular`), at least 52px across, holding only its icon; one the system does not offer has a `.dimmed` icon and still takes focus |
| `.dock-label` | The line over the dock that names the focused button, and says why a dimmed one is unavailable; empty while focus is elsewhere |
| `.menu-shelf-heading` | With `.menu-shelves`, the heading of a shelf of the menu's main level: Go To or Collections, over its row of cards (`.menu-shelf`), which scrolls sideways. While a card of one shelf has focus, the other shelves' headings and the scrolled windows holding their rows are `.unfocused-shelf` |
| `.menu-card` | A card on one of the menu's shelves, at least 96px square: its icon (`.menu-card-icon`) over its name (`.menu-card-name`) and, under it, a number of games (`.menu-card-detail`) |
| `.quick-access` | The Quick Access panel on the right; also `.panel-sections-side` with `"panel-sections": "side"`, `.quick-tiles-pills`, `.quick-tiles-list` or `.quick-tiles-shelf` as `quick-tiles` says (none for `"grid"`), and `.playing-card-banner` with `"playing-card": "banner"` |
| `.quick-access-status` | The header at the top of Quick Access: `.clock` (the time), `.offline` (shown while there is no network) and `.battery` (the machine's battery, absent without one) |
| `.panel-sections` | The strip under Quick Access's header that switches its sections (Quick, Notifications, Design, Controller, System and Manage); a rail down the panel's inner edge, about 60px wide, with `.panel-sections-side` |
| `.panel-section` | A button of that strip, `:checked` for the section shown; it holds the section's icon, and across the top the checked one its name too |
| `.panel-section-title` | With `.panel-sections-side`, the shown section's name over its page; also `.panel-heading` |
| `.page-card` | A card at the top of Quick Access's Quick section for what the page under the panel offers (Filters, Performance), a button at least 64px tall: its icon in a round chip (`.page-card-icon`), the "For This Page" eyebrow (`.page-card-eyebrow`) over its name, and at its end a second line (`.page-card-state`, the filters that are on) and an arrow |
| `.quick-tiles` | The tiles in Quick Access's Quick section, 10px apart, filled from the start: three equal columns; two with `.quick-tiles-pills`; one with `.quick-tiles-list`; and with `.quick-tiles-shelf` each shelf's heading (`.quick-tiles-heading`, Devices or Library) over a row of its tiles. While a tile of one shelf has focus, the other shelf's heading and tiles are `.unfocused-shelf` |
| `.quick-tile` | A tile, a button about 100px tall that grows taller at larger text sizes: its icon (`.quick-tile-icon`) at the top, a small arrow (`.quick-tile-arrow`) on a tile that leads to a level, and its name (`.quick-tile-name`) at the bottom; insensitive (`:disabled`) when it cannot be used now. As a pill, at least 56px tall, and as a row, at least 48px, its icon sits at its start, its name and second line (`.quick-tile-text`) beside it and the arrow at its end |
| `.quick-tile-state` | A tile's second line under its name, or at a row's end: the sound output, the Bluetooth devices or the network connection in use |
| `.activity-strip` | The button at the bottom of Quick Access's Quick section while something runs in the background: the oldest task's label, its progress bar and its percentage; it shows the Notifications section |
| `.notification-count` | The unread count on Quick Access's Notifications section button |
| `.notification` | A row of Quick Access's notifications; `.unread` while it is new, with an `.unread-dot` |
| `.task` | A row of the Running tasks in Quick Access's Notifications section, which lists what runs in the background: a label, what it is doing, a progress bar and, while it can be cancelled, a Cancel button |
| `.control` | A row of Quick Access → Controller → What Each Button Does: the button's `.control-glyph` (after a Hold label for a held one) and the keys that do the same in `.control-keys`. The buttons are changed in Change Buttons beside it, whose rows are plain `.navigation-sidebar` rows; the menu drawer has no Controls any more |
| `.choice-row` | A row of Quick Access that changes one setting in place with Left and Right (Design's Background, Background Blur, Background Dim, Cover Size and Text Size; Controller's Button Prompts, Swap Confirm and Back, Repeat Delay, Repeat Rate and Stick Dead Zone): the setting's name, under it the chosen option's second line where it has one, and at the end the `.choice-value` (the chosen option) between two `.choice-arrow` buttons, the one at an end insensitive |
| `.design-cards` | The row of design cards at the top of Quick Access's Design section, in a `.design-cards-scroller` that scrolls it sideways and runs to the panel's end edge; the section's page is `.design-section` |
| `.design-card` | A design's card in that row, a toggle button, `:checked` for the design in force and insensitive for a design that needs a newer Pindu launcher: `.design-preview` (the design's `preview` clipped to rounded corners, or empty in `--card-bg-color` without one; Adwaita's is built in) with `.design-check` over it while it is in force, then `.design-name`, and for a design it cannot apply `.design-reason` under it |
| `.options-sheet` | A game's options at the bottom |
| `.panel-heading` | A small heading in a panel, over a group of rows or tiles: the page card's "For This Page", the menu's Collections and Windows, Quick Access's Running, Notifications, Variant, Artwork, Metadata, Copy Order, Category and Proton, the shown section's name on a rail (`.panel-section-title`), and the headings of its levels (Bluetooth's, Network's, Library Health's, Proton's and What Each Button Does's). Each is also libadwaita's `.heading`, but the page card's, which is `.caption`, and the section's name, which is `.title-3` |
| `.navigation-sidebar > row` | A row in any of the three |
| `.hint-bar` | The controller hints at the bottom; each `.hint` has `.hint-glyph`s and a `.hint-label` |
| `.search-keyboard` | The on-screen keyboard; `.osk-key` a key, `.osk-key.cursor` the one under the cursor |

Other widgets inside these are libadwaita's and GTK's own; their names may change with them.

## Styling the menus

The menu drawer, Quick Access and the options sheet are the panels. A design that restyles them should
give each of these parts its look, so none is left with Pindu's base style beside it:

- the panels themselves: `.menu-drawer`, `.quick-access`, `.options-sheet`, their rows
  (`.navigation-sidebar > row`) and their lists (`.boxed-list`), with the focused row's ring;
- the section strip: `.panel-sections`, `.panel-section` and its `:checked` state, and the unread
  count, `.notification-count`; as a rail (`.quick-access.panel-sections-side`), with the section's
  name over its page, `.panel-section-title`;
- the page card, `.page-card`, with `.page-card-icon`, `.page-card-eyebrow` and `.page-card-state`;
- the tiles: `.quick-tiles`, `.quick-tile`, its `.quick-tile-name` and second line
  (`.quick-tile-state`), insensitive (`:disabled`) and focused (`:focus-visible`), in the
  arrangement the design takes: the grid, pills (`.quick-tiles-pills`), rows (`.quick-tiles-list`)
  or shelves (`.quick-tiles-shelf`, with `.quick-tiles-heading` and `.unfocused-shelf`);
- the activity strip, `.activity-strip`;
- the choice rows: `.choice-row`, its `.choice-value` and `.choice-arrow`s, the one at an end
  insensitive;
- the design cards: `.design-card`, `:checked` and focused, with `.design-preview`, `.design-check`,
  `.design-name` and `.design-reason`;
- the Playing Now card of both panels, `.playing-card`, and its two buttons: Return
  (`.suggested-action`) and Stop (`.destructive-action`); as a banner (`.playing-card-banner`), its
  `.playing-card-scrim` over the art, which keeps its text readable over any art, and `.no-art`;
- the menu's rows, with `.current` on the one for the page under it, or with `.menu-shelves` its
  shelves: `.menu-shelf-heading`, `.menu-shelf`, `.menu-card` (`.current` on the one for the page
  under it) and `.unfocused-shelf`;
- the small headings, `.panel-heading`;
- the power dock: `.power-dock`, `.dock-button`, one with a `.dimmed` icon, and `.dock-label`.

Keep text on each surface at 4.5:1, second lines included; tell the focused item apart by more than
colour, such as a ring (`outline` with a negative `outline-offset` draws it inside the item, where a
scrolled list never cuts it); size what holds text in `em`; and turn off under
`@media (prefers-reduced-motion: reduce)` what you animate. The shipped designs style every part on
this list.

## Compatibility

The names and options above are a promise for contract 1. Renaming or removing one, or changing what it
means, raises the contract version and is listed in the release notes. Adding names or options does not.

The designs Pindu ships, in `data/themes/`, are worked examples of everything on this page.
