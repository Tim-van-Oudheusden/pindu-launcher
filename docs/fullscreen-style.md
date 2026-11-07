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
- Pick a design in fullscreen mode's **Quick Access** panel (right stick click, the Steam Deck's "…" button,
  or Ctrl+2) under **Design**, or set it with
  `gsettings set io.github.tim_van_oudheusden.pindu-launcher fullscreen-design <id>`. Ribbon (`ribbon`) is
  the default; **Adwaita**, an empty id, is no design: the standard libadwaita look.
- A design can offer **variants** (below): pick one in Quick Access under **Design** → **Variant**, or set it with
  `gsettings set io.github.tim_van_oudheusden.pindu-launcher fullscreen-design-variant <variant-id>`. An empty
  or unknown id picks the design's first variant.
- `fullscreen.css` applies while the `fullscreen-custom-style` setting is on: Quick Access's **My Stylesheet**
  switch under **Design**, which can only be turned on once the file exists.
- Saving a stylesheet, or another file directly in the design's or the variant's folder, reloads it at once.
  Files in other subfolders (such as `assets/`) are picked up the next time the stylesheet loads.
- Problems GTK finds in a stylesheet are logged with their line, and fullscreen mode shows a toast with
  their number. The rest of the stylesheet still applies.

## A design folder

```
themes/my-design/
  theme.json
  theme.css
  assets/…       optional; refer to them as url("assets/bg.svg")
  mono.css       optional; a variant's stylesheet
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
| `layout` | no | The layout options below. Unknown keys and values are ignored and fall back to the default. |
| `variants` | no | The looks the design offers within itself, below. |

### Layout options

CSS restyles widgets but cannot move, add or remove them. These options change what is built:

| Option | Values (default first) | Effect |
|---|---|---|
| `home` | `"hero-bottom"`, `"row-top"` | Where Home's row of tiles sits, on both its screens: under the title at the bottom, or near the top with the title and details below it. |
| `home-tile` | `"landscape"`, `"square"`, `"portrait"` | The shape of Home's tiles, on both its screens, and the art a game's card shows: the banner, else the cover; SteamGridDB square art, else the cover cropped to square; the cover, else the banner. Cards never show icons. |
| `home-tile-size` | a number, 64–400 | The tiles' shorter side in pixels; art is decoded for 1.6× that size, so a focused card can grow that much and stay sharp. Without it the shape's own size applies. |
| `top-bar` | `false`, `true` | A bar at the top of Home and the library with Home and Library tabs and a clock. |
| `home-details` | `false`, `true` | A row under Home's title: on the first screen the focused section's number of games, on the second the focused game's playtime, when it was last played and its source. |

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

Panorama is the worked example: **Accent** (`theme.css` alone) and **Monochrome** (`mono.css`, which
redefines the accent colour variables as white).

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
own **Reduce Motion** switch. Turn off your transitions and animations under the latter, including the
`.home-hero` crossfade:

```css
@media (prefers-reduced-motion: reduce) {
  window.fullscreen-mode .home-carousel > button,
  window.fullscreen-mode .home .home-hero { transition: none; }
}
```

### Larger text

With **Larger Text** on, the window is `large-text` and its type is about a quarter larger: Pindu sets
`font-size` on `window.fullscreen-mode.large-text`. Size your text in `em` so it follows, and give rows and
tiles that hold text room to grow (or no fixed height) so nothing is cut off. A design can set its own scale
under `window.fullscreen-mode.large-text`, or style anything else differently there.

## Names

These are the names designs can rely on in contract 1. A `:checked`, `:focus-within` or `:hover` state is
noted where it carries meaning. In fullscreen mode focus is always shown, so `:focus-within` on a tile is the
controller's position.

**Window**

| Name | What |
|---|---|
| `window.fullscreen-mode` | The window, while in fullscreen mode |
| `window.reduced-motion` | The window, while Pindu moves less: the system asks for less motion (reduced motion, or animations off) or **Reduce Motion** is on in fullscreen mode |
| `window.large-text` | The window, in fullscreen mode while **Larger Text** is on |

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

The background plays muted and looped behind the first screen. It pauses on the second screen, while a
game we started runs and while the window is hidden, and holds its first frame outside fullscreen mode and
while the window is `reduced-motion`.

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
| `.home` | The page; also `.home-sections` on the first screen or `.home-games` on the second, `.home-layout-hero-bottom` or `.home-layout-row-top`, `.no-art` while no art is shown (always on the first screen), and `.blur-low` or `.blur-high` and `.dim-low` or `.dim-high` as Quick Access's Background Blur and Dim say |
| `.home-background` | Home's animated background, filling the page under the art; black where there is no video, hidden (`opacity: 0`) on the second screen. Blurred with `filter` under `.blur-*`; a `filter` of your own replaces Pindu's blur |
| `.home-hero` | The focused game's art filling the page; the one showing the current art also has `.shown`, so `transition: opacity` crossfades. Blurred like `.home-background` |
| `.home-dim` | A layer over the background and the art, under the shade and the text; transparent until `.dim-low` or `.dim-high` darkens it |
| `.hero-gradient` | The shade over the art |
| `.home-title` | The row with the focused section's name, or the game's logo or title |
| `.home-logo` | The logo, contained in 480×160 unless you grow it with `min-width`/`min-height` |
| `.title-1` | The title, when there is no logo |
| `.home-details`, `.detail` | The details row and each detail (with `home-details`) |
| `.home-carousel` | The row of tiles; also `.home-tile-landscape`, `.home-tile-square` or `.home-tile-portrait` |
| `.home-section-tile` | A section's tile (a button) on the first screen, its art filling it; a collection's also has `.home-collection-tile`, an icon over its name |
| `.jump-card` | A game's card (a button) on the second screen; `.jump-title` is its title on the art, `.badge` its pin |
| `.library-tile` | The Library tile at the end of the first screen's row |

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
| `.playing-card` | The Playing Now card while the game runs; the menu drawer and the top of Quick Access show it too, with `.session-time`, how long the game has run |
| `.launch-overlay` | The overlay while a game starts |

**Panels and bars**

| Name | What |
|---|---|
| `.menu-drawer` | The menu drawer on the left |
| `.quick-access` | The Quick Access panel on the right |
| `.quick-access-status` | The header at the top of Quick Access: `.clock` (the time), `.offline` (shown while there is no network) and `.battery` (the machine's battery, absent without one) |
| `.notification-count` | The unread count on Quick Access's Notifications entry |
| `.notification` | A row of Quick Access's notifications; `.unread` while it is new, with an `.unread-dot` |
| `.task` | A row of Quick Access's Activity, which lists what runs in the background: a label, what it is doing, a progress bar and, while it can be cancelled, a Cancel button |
| `.control` | A row of Quick Access's Controls: the button's `.control-glyph` (after a Hold label for a held one) and the keys that do the same in `.control-keys` |
| `.options-sheet` | A game's options at the bottom |
| `.navigation-sidebar > row` | A row in any of the three |
| `.hint-bar` | The controller hints at the bottom; each `.hint` has `.hint-glyph`s and a `.hint-label` |
| `.search-keyboard` | The on-screen keyboard; `.osk-key` a key, `.osk-key.cursor` the one under the cursor |

Other widgets inside these are libadwaita's and GTK's own; their names may change with them.

## Compatibility

The names and options above are a promise for contract 1. Renaming or removing one, or changing what it
means, raises the contract version and is listed in the release notes. Adding names or options does not.

The designs Pindu ships, in `data/themes/`, are worked examples of everything on this page.
