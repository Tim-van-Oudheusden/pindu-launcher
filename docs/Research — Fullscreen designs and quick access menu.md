# Research — Fullscreen designs and quick access menu

Questions:

- Can Pindu launcher ship two bundled **fullscreen designs**, one modelled on the ES-DE theme
  [ps5-menu-es-de](https://github.com/InitialDin/ps5-menu-es-de/) and one on the current console dashboard?
- Can users switch between them as **theme folders**?
- Can a Steam-like **Quick Access** panel hold the switcher?
- Where GTK CSS falls short, what is the smallest code change that lets themes do more?

Researched 2026-11-05 against these sources:

- Pindu `main` at [`7ab7fea`](https://github.com/Tim-van-Oudheusden/pindu-launcher/tree/7ab7fea8a488d0463f7048450f2c81aa87e40e71)
- ps5-menu-es-de `main` at
  [`a70159b`](https://github.com/InitialDin/ps5-menu-es-de/tree/a70159ba50b700edd95a33457a8ac8409515a5d8) (2026-06-01)
- ES-DE `THEMES.md` at
  [`50e4b60`](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md)
- GTK tag 4.24.1 at
  [`2ea935e`](https://gitlab.gnome.org/GNOME/gtk/-/tree/2ea935e3ec680fb122f245b28e18c3495df8c344)
- Pango tag 1.58.2 at [`d360f14`](https://gitlab.gnome.org/GNOME/pango/-/tree/d360f14004e4a4824f2352f6e2722016f9a51dd6)
- Bazaar at [`3d347d5`](https://github.com/kolunmi/bazaar/tree/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33)

Claims not backed by a source or by a command run here are marked **[INFERENCE]**.

Link shorthands used below:

| Shorthand | Base URL |
|---|---|
| `R/` | `https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/` |
| `T/` | `https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/` |
| `G/` | `https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/` |

Every link in this document is written out in full, so the shorthands only explain how the links are built.

Verified locally by running commands:

- **Runtime versions.** `pkg-config` inside `org.gnome.Sdk//51` reports Pango **1.58.2**, fontconfig **2.18.3**, GTK
  **4.24.1** and libadwaita **1.10.0**. The Platform has `libpango-1.0.so.0.5800.2`.
- **Runtime fonts.** `fc-list` inside `org.gnome.Platform//51` lists Adwaita Sans/Mono, Cantarell VF, DejaVu, GNU
  FreeFont, Liberation, Carlito, Caladea, TeX Gyre and Noto Color Emoji (§4.3).
- **ps5 theme assets.** Cloned the ps5 theme and read its XML. `fc-scan` and `strings` were run on its fonts.
  `ffprobe` and `ffmpeg` were run on its background video. Pillow sampled the alpha of its gradient PNG. The four
  README screenshots were downloaded and viewed.

---

## TL;DR

- **Nothing from ps5-menu-es-de can be shipped.**
  - The theme is CC BY-NC-SA 2.0. NonCommercial is non-free and adds restrictions GPL-3.0 forbids.
  - Its fonts are Monotype's proprietary **SST** ("All rights reserved").
  - Its system art is console and publisher trademarks.
  - Its background is a recreation of Sony's PS5 background. Its sounds have no stated source.
  - We can recreate the **layout** from its numbers (§1) with our own CSS and assets, under a neutral name. Copying
    layout ideas is not copying assets **[INFERENCE, not legal advice]**.
- **Both designs are mostly CSS plus a few structural hooks.** Today's Home already has a full-bleed hero of the
  focused game under a gradient, with a carousel over it
  ([home_page.blp L8-L80](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/home_page.blp#L8-L80)).
  - **Already possible with CSS on GTK 4.24:** colour, gradients, blur (`filter` and `backdrop-filter`),
    focus scale, outline, glow, fonts from the runtime, and tab styling.
  - **Not possible with CSS:**
    - Moving the carousel to the top, or putting the title beside the focused tile.
    - Square tiles smaller than today's 320×150 cards.
    - A crossfading background.
    - A top bar with a clock.
    - A details row with playtime.
    - Bundled fonts.

  Each of these needs a small code hook, selected by **theme metadata** (§4).
- **Bundled fonts are feasible without a new crate.** `pango_font_map_add_font_file` has been in Pango since 1.56,
  and GNOME 51 ships 1.58.2. In Rust it sits behind the `pango` crate's `v1_56` feature. pindu-ui would need a direct
  `pango = { version = "0.22", features = ["v1_56"] }` line for the crate already in `Cargo.lock`, which needs
  **maintainer approval**, or two hand-written FFI declarations (§4.3).
- **Theme folders** are `themes/<id>/theme.json` + `theme.css` + assets + optional `fonts/`.
  - Bundled themes go in `pkgdatadir/themes/` **on disk, not in the gresource**, because Pango loads fonts by file
    name.
  - User themes go in `$XDG_CONFIG_HOME/pindu-launcher/themes/`.
  - `load_from_path` already makes `url()` resolve against the theme folder.
  - The theme loads at `PRIORITY_USER - 1`, and the existing `fullscreen.css` still goes on top at `PRIORITY_USER`.
- **Quick Access:** add a right-side panel, mirroring Steam's left Main Menu and right Quick Access Menu. Its first
  entry is **Design**.
  - Open it with the **right-stick click (R3)** and the Deck's **"…"** button when libmanette reads it.
  - The keyboard shortcut is **Ctrl+2**, the key Big Picture uses.
  - The hint-bar entry is "R3 Quick Access". Guide is unreliable because Steam takes it.

---

## 1. ps5-menu-es-de breakdown

### 1.1 Licence and asset provenance (explicit)

| Asset | Where | Owner / licence | Usable in Pindu? |
|---|---|---|---|
| Theme as a whole (XML, layout) | repo | README: "Creative Commons CC-BY-NC-SA", linked to BY-NC-SA **2.0** ([README L63-L64](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L63-L64)). The repo has **no LICENSE file** (`ls` of the clone shows only README.md and XML at the root). | **No.** The FSF says NonCommercial licences do "not qualify as free, because there are restrictions on charging money for copies" ([GNU licence list #CC-BY-NC](https://www.gnu.org/licenses/license-list.html#CC-BY-NC)). GPL-3.0 §10 forbids "further restrictions" ([GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html#section10)). |
| Fonts `SSTLight.ttf`, `SSTMedium.ttf` | [`_inc/fonts/`](https://github.com/InitialDin/ps5-menu-es-de/tree/a70159ba50b700edd95a33457a8ac8409515a5d8/_inc/fonts) | The font's own name table (seen with `strings`) says "© 2012-2017 Monotype GmbH. All rights reserved… SST is a trademark of Monotype GmbH… This font software is the property of Monotype". `fc-scan` reports foundry `LINO`. The fonts are **Sony's corporate typeface, licensed from Monotype**; no licence for redistribution is given. | **No.** This is the SST font the brief bans. |
| Static and animated backgrounds (`background-static.png` 1920×1080; `background-video.mp4`, 56 s of drifting gold bokeh) | [`_inc/images/`](https://github.com/InitialDin/ps5-menu-es-de/tree/a70159ba50b700edd95a33457a8ac8409515a5d8/_inc/images) | Credited as "PS5 Background recreated by u/LegendaryTunes" ([README L54-L55](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L54-L55)). A recreation of Sony's console art; no licence stated. | **No** (console brand art, unknown licence) |
| `background-gradient.png` (1280×720 black alpha mask) | same | The theme author's; CC BY-NC-SA | **No** (NC). We can redo it as a CSS gradient (§1.4). |
| System icons, logos and marquees (≈210 each, e.g. `psx.png`, `ps2.png`, `switch.png`, `xbox360.png`, `n64.png`) | [`_inc/system/`](https://github.com/InitialDin/ps5-menu-es-de/tree/a70159ba50b700edd95a33457a8ac8409515a5d8/_inc/system) | Credited as "System icons from Siddy212's Canvas Theme with some edits" ([README L57-L58](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L57-L58)). Canvas is contradictory: its README says CC-BY-NC-SA 2.0 ([README L129-L131](https://github.com/Siddy212/canvas-es-de/blob/c615aa94313646199f1b0e6c5478b7fe6839e8c4/README.md#L129-L131)) while its [LICENSE](https://github.com/Siddy212/canvas-es-de/blob/c615aa94313646199f1b0e6c5478b7fe6839e8c4/LICENSE) file is CC0. Either way the images are **logos and trademarks of Sony, Nintendo, Microsoft, Sega and others**. | **No.** Pindu has no "systems" anyway. |
| Metadata and badge icons (`icon-players.svg`, `badge-favorite.svg` …) | `_inc/images/` | "Metadata icons from J. Armend's Oasis Theme" ([README L60-L61](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L60-L61)). Oasis is CC-BY-NC-SA 2.0 ([readme L151-L154](https://gitlab.com/jarmend/oasis-es-de/-/blob/ef36ec45b62350e9c69b8a95a244ad2f4b4f395f/readme.md#L151-L154)). | **No** (NC). Use Adwaita symbolic icons. |
| Sounds (`back.wav`, `launch.wav`, `scroll.wav` … 7 files) | [`_inc/sounds/`](https://github.com/InitialDin/ps5-menu-es-de/tree/a70159ba50b700edd95a33457a8ac8409515a5d8/_inc/sounds) | No credit or source anywhere in the README. The commit that added them is titled only "Add files via upload" (`2029c19`). **[INFERENCE]** They are probably recordings of the console's UI sounds. | **No** (unknown provenance) |
| `default-avatar.png` (smiley) | `_inc/images/` | No credit | **No** |
| Per-system metadata XML (descriptions, brand colour palettes) | `_inc/system/metadata/` | For example `psx.xml` carries Wikipedia-style text and the four PlayStation shape colours `DC0029 F1C002 00A79B 326BB1` ([psx.xml L3-L18](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/_inc/system/metadata/psx.xml#L3-L18)). | **No**, and unused by this theme's views |

The README itself says the design is "Inspired by the design of the PlayStation 5's menu made by Sony"
([README L52](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L52)).
The bundled Pindu design must therefore have a neutral name, its own CSS, assets from the runtime and Adwaita icons,
and nothing copied from this repo.

### 1.2 Folder structure and ES-DE mechanics

```
capabilities.xml          theme name, 5 aspect ratios, 12 variants, 10 colour schemes, transitions
theme.xml                 shared elements for system+gamelist; variants; sounds (view "all")
colors.xml                palette + per-scheme booleans (which media to show)
aspect-ratio-{16-9,16-10,4-3,3-2,20-9}.xml   all positions and sizes, per aspect ratio
_inc/fonts|images|sounds|system/{icons,logos,marquees,metadata}
auto-allgames/ auto-favorites/ completed/ now-playing/ …   per-collection theme.xml overrides
customizations/           user's avatar.png, left untouched by theme updates
```

- **Theme files:** ES-DE themes are XML files of `<view>` and elements with properties
  ([THEMES.md L300-L330](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L300-L330)).
- **`capabilities.xml`:** it declares variants, colour schemes, aspect ratios and transitions, and "is mandatory"
  ([L1130-L1140](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1130-L1140)).
- **Coordinates:** `pos` and `size` are "normalized in terms of their parent's size", where `0 0` is the top left
  and `1 1` the bottom right
  ([L1693-L1697](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1693-L1697)).
- **Paths and theme locations:** "./" paths resolve relative to the theme file
  ([L1681-L1690](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1681-L1690)).
  Themes load from `[HOME]/ES-DE/themes/` and `[INSTALLATION PATH]/themes/`
  ([L96-L104](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L96-L104)).
- **Variants:** a variant "could be purely cosmetic … or … provide distinctive functionality by for instance using
  different primary elements"
  ([L444-L452](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L444-L452)).
  That is exactly what Pindu's layout metadata would be (§4.1).
- **This theme's variants:** 12 of them, "PS5 (Small/Large)", optionally "+ XMB (Cover/Minimal/Marquee/Title
  Screen/Physical Media)". It also has 10 "colour schemes" that only toggle which media show
  ([capabilities.xml L13-L84](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/capabilities.xml#L13-L84);
  [colors.xml L3-L12, L14-L28](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/colors.xml#L3-L28)).

### 1.3 Palette and type

| Token | Hex | Use |
|---|---|---|
| `white` | `#FFFFFF` | text, icons, selected tab |
| `black` | `#000000` | gradient |
| `grey` | `#7C7C7C` | metadata pill fill, at opacity 0.5 |
| `greyUnfocused` | `#A3A3A3` | unselected tab, secondary line |
| `divider` | `#83838388` | 1-px pill dividers |
| `online` | `#3BE795` | avatar "online" dot |
| help bar | `#181818` bg, `#CCCCCC` text | menu-scope help |
| backgrounds | tinted `#B3B3B3` | multiplies the static or video background |

Sources: [colors.xml L3-L12](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/colors.xml#L3-L12)
and [theme.xml L22-L72](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L22-L72).

**Type.** Two weights, Light and Medium
([theme.xml L11-L19](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L11-L19)).
ES-DE `fontSize` is a fraction of screen height
([THEMES.md L1979-L1981](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1979-L1981)),
so at 1080p:

| Size | Fraction | Pixels at 1080p |
|---|---|---|
| clock, tabs | 0.0345 | ≈37 px |
| title | 0.036 | ≈39 px |
| title (Large) | 0.052 | ≈56 px |
| metadata | 0.028 | ≈30 px |
| metadata (Large) | 0.041 | ≈44 px |
| help | 0.030 | ≈32 px |

Medium is used for the selected tab and the pill text; Light for the rest.

**Neutral substitute:** **Adwaita Sans**, the runtime's variable font (Thin–Black), at `font-weight: 300` and `500`.
No bundling is needed (§4.3).

### 1.4 Views, exact layout (16:9, from `aspect-ratio-16-9.xml`)

Positions below are ES-DE normalised values `x y`. Pixels at 1920×1080 are in brackets. ES-DE `cornerRadius` is a
fraction of **screen width**
([THEMES.md L1828-L1831](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1828-L1831)).

**Shared by both views**
([16-9 L7-L65](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml#L7-L65);
[theme.xml L21-L164](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L21-L164)):

- **Top-left tabs.**
  - "System" sits at `0.080 0.050` [154, 54] and "Games" at `0.180 0.050` [346, 54], both centred.
  - In System view, "System" is white Medium and "Games" is `#A3A3A3` Light. Gamelist view swaps them
    ([theme.xml L255-L262, L329-L336](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L255-L336)).
- **Top-right row.**
  - The system status (Bluetooth, Wi-Fi, battery) is right-aligned at `0.800 0.050` with spacing 0.010.
  - The avatar sits at `0.830 0.050`, max 0.05 [≈54 px], radius 0.0188.
  - A green online dot at `0.840 0.070`, size `0.007 0.012`, sits under the avatar.
  - The clock is at `0.910 0.050`.
- **Background.**
  - A static image or an always-on looping bokeh video fills the screen (`cropSize 1 1`), tinted `#B3B3B3`, at
    zIndex 1.
  - In Gamelist view the focused game's **fanart or screenshot fills the screen**. `cropSize 1 1` means cover,
    **not blurred**, with `scrollFadeIn`
    ([theme.xml L312-L320](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L312-L320)).
  - Over it lies `background-gradient.png` at opacity 0.78
    ([L321-L328](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L321-L328)).
  - Sampling that PNG's alpha: **255 at the left edge on every row**, ≈135–190 at mid-width, ≈120–160 at
    three-quarters width, rising to 192–255 along the bottom.
  - The result is a black vignette heaviest on the left and bottom, where the text sits, and lightest at
    centre-right.
  - A neutral CSS equivalent is two layered `linear-gradient`s, to-right and to-top, at alpha ≈0.78×.
- **Help bars.**
  - `help-left` sits at `0.017 0.977` (origin bottom-left) and `help-right` at `0.978 0.977` (origin bottom-right).
  - Both use Light at 0.030, white, with entry spacing 0.013
    ([theme.xml L34-L52](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L34-L52)).
  - System view shows `start` on the left and `a` on the right. Gamelist view shows `b, back` on the left and
    `a, x, thumbstickclick` on the right
    ([L166-L172, L265-L271](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L166-L271)).
  - The glyphs are ES-DE's own, not shipped by the theme.
  - **[INFERENCE]** The help bars are hidden in all four README screenshots, probably by an ES-DE setting.

**Carousel**
([16-9 L158-L204](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml#L158-L204)).
The fields mean the following
([THEMES.md L1751-L1925](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L1751-L1925)):

- `itemSize` is the size of every unselected item.
- `itemScale` scales the selected item.
- `selectedItemMargins` adds space beside the selected item.
- `selectedItemOffset` moves the selected item.
- `horizontalOffset` shifts the row, here so that the selected item sits left of centre.

| | Small | Large |
|---|---|---|
| area | `pos 0 0`, `size 1 0.55` (top 55 %) | `pos 0 0.020`, same size |
| item | `0.065 0.120` [≈125×130, near-square] | `0.095 0.175` [≈182×189] |
| focused | ×1.6 [≈200×207] | ×1.6 [≈292×302] |
| visible items | 14 | 9.5 |
| margins beside focus | `0.020 0.020` [≈38 px each side] | `0.030 0.030` |
| focus offset | `0 0.020` [≈22 px down] | `0 0.029` |
| row offset | `horizontalOffset -0.351`, `verticalOffset -0.203` (the row sits near the top, focus at about x≈0.15) | same |
| corner radius | 0.010 [≈19 px] | 0.015 [≈29 px] |
| unfocused | opacity 0.9, dimming 0.9 (system view) | same |
| image | `imageType cover`, `imageFit cover`, transparent background | same |

**Focused-item treatment.** The focused item is larger, pushed down a little, and given extra space on each side.
Every other item is dimmed to 90 %. The XML has **no outline, border or glow**, and the screenshots show none.

**System view** ([16-9 L67-L112](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml#L67-L112)):

- **Name line.** A small system icon at `0.208 0.265` (max 0.05) is followed by the system's full name (Light 0.036,
  white at opacity 0.9) at `0.240 0.231`, size `0.745 0.0695`. Both sit **right of and level with the bottom of the
  focused tile**.
- **Logo.** The system marquee (logo) is centred at `0.210 0.600`, max `0.235 0.155`.
- **Count pill** at `0.076 0.815`, size `0.285 0.0621` [547×67], radius 0.0188, `#7C7C7C` at 0.5.
  - Controller icon and "N Games" at `0.092` / `0.120`.
  - A 1-px divider at `0.253`.
  - Heart icon and the favourites count at `0.270` / `0.300`.
- **Last-played pill** at `0.680 0.815`. It holds a clock icon, a divider and the name of the last game played,
  in Medium.
- **Media.** A screenshot or video of the last-played game is centred at `0.822 0.570`, max `0.280 0.355`
  [538×383], radius 0.010.

**Gamelist view** ([16-9 L114-L156, L206-L254](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml#L114-L254)):

- **Carousel.** The same carousel shows game covers.
- **Name lines.** Beside the focused tile: the game name (Light 0.036), then the source system's full name in
  `#A3A3A3` at `0.241 0.282`.
- **Logo and genre.** The game logo (marquee) is centred at `0.217 0.575`, max `0.230 0.225`. The genre is centred
  at `0.220 0.710`.
- **Stats pill** at `0.076 0.815`:
  - players at `0.087` / `0.110`
  - a divider at `0.153`
  - playtime ("88 minutes") at `0.161` / `0.187`
  - a divider at `0.286`
  - play count at `0.296` / `0.3175`
- **Badges and media.** Badges (favourite, completed …) start at `0.370 0.783`. Media is centred at `0.830 0.700`,
  max `0.300 0.440`.

**XMB variants** ([16-9 L304-L380](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml#L304-L380);
[theme.xml L440-L478](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L440-L478)):

- **Carousel.** The gamelist becomes a **vertical** carousel centred at x=0.225, full height.
  - `itemSize 0.140 0.170`, `itemScale 1.9`, 5 items, unfocused dimming 0.7.
  - A back arrow sits at `0.05 0.5`.
- **Focused item.** The video plays over the focused item at `0.225 0.5`, crop `0.28 0.33`.
- **Text.** The title is at `0.440 0.430` and the stats pill at `0.390 0.545`.
- **Scope.** This is a different primary widget, not a restyle. It is out of scope for Pindu's first designs.

**Animations**
([capabilities.xml L86-L101](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/capabilities.xml#L86-L101);
[theme.xml L124-L136, L297-L320](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L124-L320)):

- **View changes.** All view-to-view transitions are `instant`. Only startup fades.
- **Background art.** The art fades in on scroll (`scrollFadeIn`, "a short fade-in animation"
  ([THEMES.md L2552-L2554](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L2552-L2554))).
- **Media video.** It starts after a 1 s delay, fading in from black over 0.5 s.
- **Long text.** Text marquee-scrolls after 2 s (`containerScrollSpeed 0.7`).
- **Carousel.** Scrolling is ES-DE's built-in animation, with `fastScrolling` on.

**Sounds.** There are 7 navigation sounds in view `all`
([theme.xml L542-L564](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml#L542-L564)).

### 1.5 README screenshots (described)

The [README preview table](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md#L46-L48)
links four 1920×1080 PNGs.

1. **System view, Media - Screenshot, Carousel (Small)**
   ([image](https://github.com/user-attachments/assets/7640e11d-33ab-4ead-a936-25113b8bcabd)).
   - The background is dark blue-grey with gold bokeh.
   - A row of rounded square system tiles runs across the top, with the focused PSP tile larger at the left.
   - Its logo and "Sony PlayStation Portable" sit to the right of the focused tile.
   - A large PSP wordmark is at mid-left, and the pills "5 Games | ♥ 0" and "🕘 | Street Fighter Alpha 3 MAX" are at
     the bottom.
   - A rounded screenshot is at the right.
   - The top bar reads "System  Games … BT Wi-Fi 50% [avatar] 21:59".
2. **Gamelist view, same options**
   ([image](https://github.com/user-attachments/assets/6fbe991a-89b5-4404-9541-88360942c12d)).
   - The full-bleed fanart of OutRun 2006 is darkened at the left and bottom.
   - A cover row is at the top; beside the focused cover are the console icon, "OutRun 2006: Coast 2 Coast" and
     "Sony PlayStation 2".
   - The game logo is at mid-left, with "Racing, Driving" under it.
   - The pill reads "👥 1-6 | ⏱ 88 minutes | ◔ 15", and the screenshot is bottom-right.
3. **Gamelist view, Media - Video, XMB (Marquee)**
   ([image](https://github.com/user-attachments/assets/40e0fe62-d60f-4ecd-84dd-87c567708284)).
   - A vertical column of game logos is at the left, the focused one large.
   - The title and stats pill are mid-right, over undimmed full-bleed key art.
4. **Gamelist view, Media - Video, XMB (Minimal)**
   ([image](https://github.com/user-attachments/assets/85d33e49-850e-4549-bbc4-3047b86940ac)).
   - A vertical column of covers is at the left, and the focused item plays its video in place, widened.
   - The logo and stats pill are at the right.

### 1.6 Brand elements and neutral substitutes

| Brand element | Where | Neutral substitute in Pindu |
|---|---|---|
| SST typeface | all text | Adwaita Sans 300/500 (runtime). A bundled OFL font is optional (§4.3). |
| PS5 background (static or bokeh video) | background | Default to the focused game's own hero art (Pindu's `ArtSet.hero`/`cover`, [library.rs L15-L20](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/library.rs#L15-L20)). When there is no art, use a CSS `radial-gradient` in neutral greys. Optionally add a slow `@keyframes` drift of a self-made, GPL/CC0 particle SVG. |
| PlayStation, Nintendo and other system logos and icons | carousel, title icon, marquee | Pindu has no systems. Use the game's **logo art** (`ArtSet.logo`, already shown on Home) and the **source icon** Pindu already has (`source_badge`, [cover_tile.blp L35-L44](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/cover_tile.blp#L35-L44)) |
| ✕○△□ shapes, and the four-colour palette in `psx.xml` | help glyphs, system colours | Pindu's own glyph set ([resources.gresource.xml L14-L49](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/resources.gresource.xml#L14-L49)). It picks a glyph style per connected pad, so the theme itself never draws shapes. Colour comes only from `--accent-*`. |
| Console UI sounds | navigation | Pindu's generated tones ([fullscreen.rs L435-L456](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/fullscreen.rs#L435-L456)) |
| "System / Games" tabs | top-left | "Home / Library", Pindu's two fullscreen pages |
| Avatar with the green "online" dot; Bluetooth, Wi-Fi and battery | top-right | **Clock only.** Optionally the user's name. The sandbox has "No access to host services like … system D-Bus" ([Flatpak sandbox](https://docs.flatpak.org/en/latest/sandbox-permissions.html)), so UPower battery and AccountsService avatars would need new permissions, which are off-limits. **[INFERENCE]** Network state via `GNetworkMonitor` works through the network portal. |
| The name "PS5 Menu" | | A neutral id and name, for example `ribbon` / "Ribbon" |

---

## 2. Current console dashboard breakdown (neutral)

Sources are support.xbox.com, Xbox Wire and the repo's own research PDF. Pixel figures are estimates from screenshots
Microsoft published and are marked **[INFERENCE]**.

### 2.1 Home

- **The current layout** shipped to everyone on 2023-07-26. The post lists seven parts:
  - a "quick access menu" across the top
  - recent tiles moved "to the bottom of the screen" so that more of the background shows
  - a background that matches the highlighted game
  - curated lists
  - pins for games and groups
  - Friends & Community
  - Watch & Listen

  Source: [Xbox Wire, 2023-07-26](https://news.xbox.com/en-us/2023/07/26/welcome-to-your-new-xbox-home/). The
  [2023-05-01 Insider post](https://news.xbox.com/en-us/2023/05/01/xbox-insiders-your-feedback-shapes-the-new-home-experience/)
  describes the "responsive game art feature to update the default background … when you hover over the tiles".
- **Background.** With "Show selected game art" on, your background stays "until you highlight a game that you
  recently played" and then shows that game's art "until you highlight another game or section"
  ([support: change background](https://support.xbox.com/en-US/help/account-profile/customize-xbox-home/change-xbox-one-background)).
  - The background types are solid colour, custom image and dynamic backgrounds (same page).
  - New dynamic backgrounds arrived in March 2026
    ([Xbox Wire, 2026-03-25](https://news.xbox.com/en-us/2026/03/25/march-2026-xbox-update/)).
  - **[INFERENCE]** The change is a crossfade; no timing is documented.
- **Top bar.** It has no surface; the icons sit straight on the wallpaper **[INFERENCE: from the
  [2023 screenshot](https://xboxwire.thesourcemediaassets.com/sites/2/2023/04/0501-New-Dashboard-experiment_Top-of-Home-FINAL-72653034f196251c18d6.png)]**.
  - **Left:** round avatar, name and a stat line.
  - **Centre:** five round icon-only buttons (Library, Store, a subscription entry, Search, Settings).
  - **Right:** status icons and a 12-hour clock.
  - Since 2026 the top bar also has a "What's new" entry and a service-alert indicator
    ([Insiders, 2026-05-29](https://news.xbox.com/en-us/2026/05/29/xbox-insiders-july-2026-console-features/)).
  - Notifications live in the Guide, not the top bar
    ([support: using the Guide](https://support.xbox.com/en-US/help/hardware-network/getting-started-set-up/using-the-guide)).
- **Recent row.** About ten **square** tiles in the lower half of the screen.
  - **The focused tile is much larger**, about 1.7× the edge.
  - It has a rounded outline about 3 px wide in the user colour with a soft glow, and its title in two lines on a
    dark band inside the tile. Unfocused tiles show art only.
  - This is **[INFERENCE: from the same screenshot]**.
  - Recent-row settings since 2025: tile count, hide system apps, up to 3 pins
    ([Insiders, 2025-05-16](https://news.xbox.com/en-us/2025/05/16/xbox-insiders-get-new-ways-to-personalize-home/);
    [support: customize Home](https://support.xbox.com/en-US/help/account-profile/customize-xbox-home/customize-your-home)).
  - A "play history" tile sits "at the end of the 'Jump back in' list"
    ([Xbox Wire, 2025-07-21](https://news.xbox.com/en-us/2025/07/21/now-available-for-xbox-insiders-your-cross-device-play-history-and-easier-access-to-your-cloud-playable-console-games-on-pc/)).
- **Rows below.**
  - Wide (≈16:9) cards with bottom-left labels **[INFERENCE: screenshot]**.
  - Pinned **groups above pinned games**, up to 10 of each since April 2026
    ([support: customize Home](https://support.xbox.com/en-US/help/account-profile/customize-xbox-home/customize-your-home);
    [Insiders, 2026-03-18](https://news.xbox.com/en-us/2026/03/18/xbox-insiders-april-2026-console-features/)).
- **Immersive mode.** Since October 2026, pressing the **right stick** on Home fades the UI out and leaves only the
  background ([Insiders, 2026-08-19](https://news.xbox.com/en-us/2026/08/19/xbox-insiders-october-2026-console-features-2/)).
  This bears on the R3 choice in §3.3.

### 2.2 My games & apps

- **Left rail and grid.** A rail of about 23 % of the width holds search, Games, Apps, Groups, Play history, Full
  library and Manage, each with a count, plus a storage gauge. A grid with a sort dropdown, an item count and a
  Filter button sits beside it.
  - This is **[INFERENCE: from the
    [2026 screenshot](https://xboxwire.thesourcemediaassets.com/sites/2/2026/06/2.-Library-Tiles-Updates-643b19811396e3270b30-1900x1080.png)]**.
  - The selected rail row is filled with the user colour.
- **Tile style.** Tiles are square by default. A "rich poster-style artwork" 2:3 style and badge toggles shipped in
  July 2026 ([Insiders, 2026-06-10](https://news.xbox.com/en-us/2026/06/10/xbox-insiders-july-2026-console-features-2/);
  [support: what's new](https://support.xbox.com/en-US/help/hardware-network/settings-updates/whats-new-xbox-one-system-updates)).
- **Filters** open as a sheet from the right
  ([Xbox Wire, 2024-02-14](https://news.xbox.com/en-us/2024/02/14/xbox-february-update-rolls-out-2024/)).
- **Groups** are user lists ([support: groups](https://support.xbox.com/en-US/help/games-apps/my-games-apps/group-games-apps)).
  They are the equivalent of Pindu's Collections.
- **The repo's own PDF** adds three points:
  - Console tiles "appear square", while store art is 2:3 poster, 1:1 box and 16:9 hero (pp. 3–4).
  - Pindu should adopt 2:3 posters with square as a fallback (pp. 7–8).
  - Pindu should avoid "any Xbox name, logo or glyph and Xbox green" (p. 8).

  Source: [Research — Xbox Game Library UI.pdf](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Research%20%E2%80%94%20Xbox%20Game%20Library%20UI.pdf),
  read with `pdftotext -layout`.

### 2.3 The Guide

- **Opening and tabs.** The Guide opens with the console button, and LB/RB cycle its tabs.
  - Top tabs: Guide, People, Parties & chats, Game activity, Capture & share, Profile & system.
  - Bottom tabs: Notifications, the subscription tab, Store, Search, Audio & music.

  Source: [support: using the Guide](https://support.xbox.com/en-US/help/hardware-network/getting-started-set-up/using-the-guide).
  The tabs can be reordered
  ([support: customize the guide](https://support.xbox.com/en-US/help/hardware-network/getting-started-set-up/customize-guide-tabs)).
- **Visual.** A **left-anchored, full-height panel about 37 % wide** over the dimmed page. Sub-pages stack as offset
  cards, and the focused row has the accent outline.
  - This is **[INFERENCE: from the
    [2026 screenshot](https://xboxwire.thesourcemediaassets.com/sites/2/2026/03/5-Profile-Badge-1e4123f9191104db6f8c-1900x1080.png)]**.
  - Pindu's menu drawer is already this shape: a left `AdwOverlaySplitView` sidebar, `sidebar-width-fraction 0.4`,
    max 440 ([window.blp L28-L46](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/window.blp#L28-L46)).

### 2.4 Neutral design to imitate ("Panorama")

1. **Background.** The focused game's 16:9 hero art fills the screen and **crossfades** on focus change. A bottom
   gradient covers about 40 % of the height.
2. **Top bar.** It has no surface: page tabs or icon buttons at the centre, the clock on the right, white text with a
   soft `text-shadow`.
3. **Recent row.**
   - Placed at about 55–65 % of the height, with **square** tiles and about 12 px gaps.
   - The focused tile grows to about 1.6×, gets a 3 px outline and a glow in `var(--accent-color)`, and shows its
     title on an in-tile scrim.
   - Unfocused tiles show no title.
   - The Library tile is a 16:9 card at the end.
4. **Rows below** (optional, later): one row per collection, the equivalent of "groups".
5. **Library.** Today's tabs are restyled as text tabs underlined in the accent colour. The tiles stay 2:3 posters,
   and only the focused tile shows its title. A left rail would be a later layout variant.
6. **Menu drawer** as the Guide-like left panel: near-black surface, full-width rows, accent outline on focus.
7. **Colour.**
   - The accent is the **system accent** (`--accent-*`). The theme never sets a brand colour, so **no green**.
   - Surfaces are neutral greys, with radii of about 4 px on tiles and 8 px on panels.
   - There are no logos and no button art.

---

## 3. Steam Quick Access Menu and Pindu mapping

### 3.1 What Valve says

- **Two system menus.** Valve names two system menus: the **Main Menu** on the Steam button and the **Quick Access
  Menu** on the "…" button. A 2025 client update "Added in-game keyboard shortcuts to open the Main Menu and Quick
  Access Menu"
  ([Steam Deck Client Update, 2025-06-30](https://steamcommunity.com/games/1675200/announcements/detail/529850584204837333)).
- **Buttons.** The Deck's tech page labels the left "Steam" button and the right "Quick access" button
  ([steamdeck.com/en/tech](https://www.steamdeck.com/en/tech)). Other SteamOS handhelds have "Guide and Quick
  Access Menu buttons" ([SteamOS 3.8.28](https://steamcommunity.com/games/1675200/announcements/detail/711161056325534553)).
- **Big Picture.** New Big Picture became the default on 2023-02-01 with the Deck UI, including "Quick Access >
  Notifications" ([Steam Client Update, 2023-02-01](https://store.steampowered.com/news/189167)).
- **Sections confirmed in Valve changelogs:**
  - **Notifications**. The QAM "will now open to the notifications tab when you have new friend chat messages"
    ([Oct 2022](https://store.steampowered.com/news/app/1675200/view/4714731755406618593)).
  - **Friends** ([Jun 2023](https://store.steampowered.com/news/app/1675200/view/5126832680918898811)).
  - **Voice Chat**, shown only while a chat is active
    ([Apr 2026](https://store.steampowered.com/news/app/1675200/view/1830163047258963)).
  - **Performance**: frame limit, refresh rate, per-game profiles
    ([Apr 2022](https://store.steampowered.com/news/app/1675200/view/4348797798091499756)).
  - **Help** ([Mar 2022](https://store.steampowered.com/news/app/1675200/view/4247462367431424132)).
  - The quick settings include brightness
    ([Jan 2025](https://steamcommunity.com/games/1675200/announcements/detail/523080879756542660)) and volume
    ([Jun 2022](https://store.steampowered.com/news/app/1675200/view/4480532686349289437)).
- **Not confirmed by Valve:** **[INFERENCE]** The tab name "Quick Settings", the Wi-Fi, airplane and night-mode
  toggles, a Music/Soundtracks tab and a Keyboard tab are common descriptions that no Valve source found here
  confirms.
- **Layout: [INFERENCE].** The QAM is a right-side panel with a vertical strip of tab icons, and the Main Menu is a
  left column. Valve publishes no layout prose. The Deck puts the QAM button on the right
  ([tech page](https://www.steamdeck.com/en/tech)).
- **Opening it:**
  - **Deck and handhelds:** the "…" button.
  - **Other controllers:** **Guide + A**. Valve moved a screen-reader binding "so that external gamepads can still
    open the Quick Access Menu with Guide button + their equivalent of A button"
    ([Steam Deck Client Update, 2025-12-19](https://steamcommunity.com/games/1675200/announcements/detail/500593045099513244)).
    Valve calls the Guide button "typically reserved for system use"
    ([Steamworks: controller basics for players](https://partner.steamgames.com/doc/features/steam_controller/getting_started_for_players)).
  - **Keyboard in a game:** "Default Quick Access Menu hotkey: Ctrl+Shift+Tab" and Shift+Tab for the Main Menu
    ([2025-06-30](https://steamcommunity.com/games/1675200/announcements/detail/529850584204837333)).
  - **Keyboard in Steam's own UI:** "crtl+1 and ctrl+2 work for bringing up the system menu and QAM respectively".
    The only source is a user report on Valve's tracker
    ([steam-for-linux#10247](https://github.com/ValveSoftware/steam-for-linux/issues/10247)); no Valve doc states it.
- **Closing it: [INFERENCE]** with B. Valve only notes that it "Removed display of 'B' back button in Overlay Quick
  Access Menu" ([Mar 2022](https://store.steampowered.com/news/app/1675200/view/4347669995458478178)).
- **Steam owns Guide.** Valve warns when "the Guide button is disabled (which prevents bringing up the Steam main
  menu and other functionality like chords)" ([2023-02-01](https://store.steampowered.com/news/189167)). Pindu's spec
  already concedes "The Guide button cannot be caught while a game has focus"
  ([Main Spec p. 10](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Adwaita%20Game%20Library%20%E2%80%94%20Main%20Spec.pdf#page=10)).

### 3.2 Pindu today

| Button | Intent | Fullscreen effect |
|---|---|---|
| South (A) | Activate | activate the focused item |
| East (B) | Back; held 1 s → BackHeld | close or back; cancel a launch |
| North (Y) | Search | search entry + on-screen keyboard |
| West (X) **and** Select (View) | Options | options sheet (bottom) |
| Start | Menu | toggle the left drawer |
| Guide | Home | desktop: enter fullscreen. Fullscreen: toggle the drawer, the same as Start |
| LB / RB | Previous/Next tab | library tabs |
| LT / RT | Previous/Next sort | sort order |
| D-pad, left stick | Navigate | move focus |

Sources:

- Mapping: [gamepad.rs L406-L423](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/gamepad.rs#L406-L423).
- Intents: [L13-L37](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/gamepad.rs#L13-L37).
- Guide toggling the drawer: [panels.rs L60-L90](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/panels.rs#L60-L90).
- Spec table: "Menu drawer (left): Start or Guide toggles; B closes"; "Options sheet: View or X opens; B closes"
  ([Main Spec p. 9](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Adwaita%20Game%20Library%20%E2%80%94%20Main%20Spec.pdf#page=9)).

**Free inputs.** The libmanette backend maps only the face buttons, bumpers, triggers, Select, Start, Mode and the
D-pad, and reads only the left stick
([manette_backend.rs L196-L215](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/manette_backend.rs#L196-L215)).
That leaves these free:

- **L3/R3** (stick clicks)
- the right stick
- the paddles
- **misc1**. libmanette 0.2.13's Steam Deck driver maps the Deck's QAM button to `BTN_TRIGGER_HAPPY5`
  ([manette-steam-deck-driver.c L661](https://gitlab.gnome.org/GNOME/libmanette/-/blob/990f8797da563b32f15f56d5f297d9963aa59d9c/src/drivers/manette-steam-deck-driver.c#L661)),
  which is the mapping name "misc1"
  ([manette-mapping.c L305](https://gitlab.gnome.org/GNOME/libmanette/-/blob/990f8797da563b32f15f56d5f297d9963aa59d9c/src/manette-mapping.c#L305)).

**[INFERENCE]** The Deck button only reaches Pindu when libmanette drives the Deck itself, in Desktop Mode.

**Keyboard.** The fullscreen stand-ins are:

- F10 → Menu
- Menu key / Shift+F10 → Options
- Ctrl+PgUp/PgDn → tabs
- Ctrl+Shift+PgUp/PgDn → sort

Sources: [keys.rs L19-L37](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/keys.rs#L19-L37),
wired at [fullscreen.rs L54-L83](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/fullscreen.rs#L54-L83).
Escape leaves fullscreen
([L45-L53](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/fullscreen.rs#L45-L53)).

Ctrl+1…9, F12 and Ctrl+Tab are bound nowhere: not in `keys.rs`, not among the app accelerators
([application.rs L70-L77](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/application.rs#L70-L77)),
and not in [shortcuts-dialog.blp](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/shortcuts-dialog.blp#L73-L95).

**Hint bar.** It shows:

- A [action]
- B Back/Close
- Y Search
- View Options
- Start Menu
- LB/RB Tabs
- LT/RT Sort

Page hints hide while a panel is open, which shows "B Close" instead
([hints.rs L112-L199](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/hints.rs#L112-L199)).
The bar is right-aligned (`halign: end`,
[window.blp L748-L755](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/window.blp#L748-L755)).

### 3.3 Proposal

- **Panel.** Add a new `Panel::QuickAccess` in `pindu_core::panels` and `Intent::QuickAccess`. The panel is a
  **right-side** sheet, an `AdwOverlaySplitView` with `sidebar-position: end`. That property exists since
  libadwaita 1.4: "if GTK_PACK_END, it's displayed after the content"
  ([docs](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/property.OverlaySplitView.sidebar-position.html)).
  - Nest it inside the existing `drawer` split view.
  - Opening one panel closes the others, as Steam's two menus are siblings.
  - It closes with B, with its own button again, or with Esc. Esc already closes panels before it leaves fullscreen.
- **Entries:**
  1. **Design**: a list of installed themes. The first row is "Adwaita (no design)". A check marks the active theme,
     and A applies it live.
  2. **[INFERENCE, maintainer's call]** The drawer's quick settings could move here: cover size, navigation sounds,
     exit after launch (`DrawerLevel::Settings`,
     [panels.rs L19-L25](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/panels.rs#L19-L25)).
     The drawer then stays navigation (Home, Library, Collections, Exit, Quit) and the panel holds settings,
     mirroring Steam's Main Menu and QAM split.
- **Button: right-stick click (R3).**
  - It is free in Pindu, present on every target pad including Steam Input's virtual pad, and never taken by the
    system.
  - It needs no chord timer and changes no existing binding.
  - Also map the **Deck "…" button** (misc1) to the same intent.
  - Rejected:
    - **Guide**: Steam takes it, and on a Deck it is the *left* menu.
    - **Guide + A**: Steam swallows it, and it would push Guide's own action to button release.
    - **Holding Start**: it would delay the drawer.
  - Conflict to note: the console dashboard uses the right stick on Home for its 2026 immersive mode (§2.1). Pindu has
    no such mode, so nothing clashes.
- **Keyboard: Ctrl+2**, which matches Big Picture's QAM key (#10247 above) and is free in Pindu.
  - Add `("2", control) → QuickAccess` in `keys.rs`, and an `Adw.ShortcutsItem` under "Fullscreen Mode".
  - **[INFERENCE]** `keys.rs` matches on key names, so on AZERTY Ctrl+2 arrives as `eacute`. Match on the hardware
    keycode or accept that gap.
  - Avoid Ctrl+Shift+Tab, which Steam's in-game overlay takes.
- **Hint bar.** Add "R3 Quick Access" after "Start Menu", shown under the same rule as Menu. It needs new neutral
  glyphs `pindu-glyph-rs-symbolic` (labelled RS) and `pindu-glyph-r3-symbolic` (labelled R3); which label each pad
  gets is **[INFERENCE]**. The spec's p. 9 table gains a row "Quick Access (right): R3 or … toggles; B closes".

---

## 4. Gap analysis against GTK 4.24 CSS

### 4.1 What GTK 4.24 CSS can and cannot do (verified in source)

- **Filters.** `filter` takes `blur, brightness, contrast, grayscale, hue-rotate, invert, opacity, saturate, sepia,
  drop-shadow` ([gtkcssfiltervalue.c L921-L1000](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssfiltervalue.c#L921-L1000)),
  plus SVG `url(#id)` filters.
- **`backdrop-filter`** is registered and rendered in 4.24
  ([gtkcssstylepropertyimpl.c L1435-L1440](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssstylepropertyimpl.c#L1435-L1440);
  [gtkwidget.c L12172-L12214](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkwidget.c#L12172-L12214);
  [NEWS 4.21.2](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/NEWS#L1527-L1540)).
  It is not yet in the [property table](https://docs.gtk.org/gtk4/css-properties.html).
- **Transforms.** `transform` supports `scale`, `translate`, `rotate`, 3D and `perspective`
  ([gtkcsstransformvalue.c L943-L1187](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcsstransformvalue.c#L943-L1187)).
  **[INFERENCE]** A transform does not change allocation, so a scaled tile overlaps its neighbours and is clipped by
  the enclosing viewport.
- **Images.** `background-image` takes `url()` (quoted), linear, radial and conic gradients, `cross-fade` and
  `image()` ([gtkcssimage.c L529-L539](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssimage.c#L529-L539)).
  `background-size: cover|contain` is supported.
- **Other properties.** Transitions, `@keyframes` and animations, `box-shadow`, `text-shadow`, `outline-*`, margin,
  padding, `min-width`/`min-height` (no percentages), radii, `opacity`, `letter-spacing`, `text-transform`,
  `line-height`, `font-feature-settings` and `font-variation-settings` are all supported
  ([css-properties](https://docs.gtk.org/gtk4/css-properties.html)).
- **Selectors.** `:focus-visible`, `:focus-within`, `:nth-child()`, `:not()` and `:checked` exist. **No `:has()`**
  ([gtkcssselector.c L1318-L1495](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssselector.c#L1318-L1495)).
  In GTK, `:focus-within` is "Set on all ancestors of the focus widget"
  ([css-overview.md L68-L69](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/docs/reference/gtk/css-overview.md#L68-L69)).
- **No layout properties.** There is no `mask`, `mix-blend-mode`, `visibility`, `display`, `order`, `halign`,
  `hexpand`, `width`, `max-*` or `position`. Layout and visibility stay in code.
- **[INFERENCE] Minimum sizes only grow.** CSS `min-*` can only *grow* a widget past its `set_size_request`, never
  shrink it, so the 320×150 Home cards cannot become 130 px squares from CSS.
- **No `@font-face`.** The provider's only at-rules are `@import`, `@media`, `@define-color` and `@keyframes`
  ([gtkcssprovider.c L1022-L1194](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssprovider.c#L1022-L1194)).

### 4.2 Today's fullscreen tree (what CSS can target)

- **Home.**
  - `overlay.home` is an `Overlay` with the class `home`.
  - Under it: a `picture` (hero, `content-fit: cover`), `box.hero-gradient`, and a vertical box (`valign: end`,
    `margin-bottom: 24`).
  - The vertical box holds a title row (logo `picture` 480×160 or `label.title-1`) **above** a `scrolledwindow`.
  - Inside that: `box.home-carousel` (spacing 24, margins 48/12) of `button.jump-card.card` and the
    `button.card.library-tile` (320×150).
  - Sources: [home_page.blp L8-L80](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/home_page.blp#L8-L80).
- **Jump cards.** Each card is `button.jump-card.card > overlay > cover-art` (320×150 by `set_size_request`) with a
  `label.jump-title` overlay at the bottom and an optional pin `image.badge`
  ([window.rs L1227-L1285](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/window.rs#L1227-L1285);
  size [home_page.rs L19-L20](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/home_page.rs#L19-L20)).
- **Hero swaps.** The hero swaps its paintable at once, with no fade
  ([home_page.rs L224-L282](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/home_page.rs#L224-L282)).
- **Current Pindu CSS.** The fullscreen rules
  ([style.css L80-L207](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/style.css#L80-L207))
  set:
  - `font-size: 1.35em` on the window
  - the focused grid child and carousel button at `scale(1.05)` with a 3 px accent outline
  - the hero gradient
  - pill tabs
  - the hint-bar padding
- **Library.** `box.library-tabs` holds a tab-button row and the sort label
  ([window.blp L452-L490](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/window.blp#L452-L490)).
  Below it is `gridview.library` of `.cover-tile` (2:3 cover 150×225, 200×300 or 250×375 from the `tile-size`
  setting, [cover_tile.rs L16-L40](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/widgets/cover_tile.rs#L16-L40)).
  The tile title is a `GtkInscription`, whose CSS node is `label`
  ([GtkInscription](https://docs.gtk.org/gtk4/class.Inscription.html)).
- **Game page.** `overlay.game-hero` (320 px hero) contains `.game-actions` and `.game-play`
  ([game_page.blp L30-L107](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/game_page.blp#L30-L107)).
- **Panels and bars.**
  - The drawer is `box.menu-drawer`.
  - The options sheet is `box.options-sheet`.
  - The search keyboard is `grid.search-keyboard` of `.osk-key` / `.osk-key.cursor`.
  - The hint bar is `box.hint-bar` of anonymous `box > image + label` items
    ([hint_bar.rs L59-L77](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/hint_bar.rs#L59-L77)).
- **Not yet present:** a top bar, a clock and a details row.

### 4.3 Fonts

- **Loading a font file.** `pango_font_map_add_font_file(fontmap, filename, error)` is available **since Pango 1.56**
  ([docs](https://docs.gtk.org/Pango/method.FontMap.add_font_file.html)). The docs say it "Loads a font file with one
  or more fonts into the PangoFontMap. The added fonts will take precedence over preexisting fonts with the same
  name."
  - It loads one file per call
    ([pango-fontmap.c L600-L629](https://gitlab.gnome.org/GNOME/pango/-/blob/d360f14004e4a4824f2352f6e2722016f9a51dd6/pango/pango-fontmap.c#L600-L629)).
  - The fontconfig backend accepts TTF, OTF and TTC
    ([pangofc-fontmap.c L3906-L3934](https://gitlab.gnome.org/GNOME/pango/-/blob/d360f14004e4a4824f2352f6e2722016f9a51dd6/pango/pangofc-fontmap.c#L3906-L3934)).
  - Runtime 51 has Pango 1.58.2 (verified).
  - GTK 4.24 itself uses the function on private `pango_cairo_font_map_new()` maps
    ([gtksvg.c L519-L543](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/svg/gtksvg.c#L519-L543)).
- **Scope it to a private font map.** `gtk_widget_set_font_map` is "useful … when you need to add
  application-specific fonts", and children inherit it
  ([gtkwidget.c L6912-L6928](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkwidget.c#L6912-L6928)).
- **Caveat.** GTK clears the default font map when host fonts change
  ([gtksettings.c L1657-L1690](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtksettings.c#L1657-L1690)),
  and that rebuilds the font list only from fontconfig
  ([pangofc-fontmap.c L2543-L2682](https://gitlab.gnome.org/GNOME/pango/-/blob/d360f14004e4a4824f2352f6e2722016f9a51dd6/pango/pangofc-fontmap.c#L2543-L2682)).
  **[INFERENCE]** Fonts added to the *default* map would then vanish. A **private map** (a new map, plus
  `add_font_file`, plus `window.set_font_map()` while the theme is active) avoids that. The fontconfig route
  (`FcConfigAppFontAddFile`) has the same problem and no Rust binding in Pindu's dependencies.
- **Rust.** `FontMapExt::add_font_file(&self, filename) -> Result<(), glib::Error>` is in pango 0.22.9
  (`src/auto/font_map.rs:22-40`) behind `#[cfg(feature = "v1_56")]`. gtk4 0.11.5's `v4_24` feature does not enable
  it, and pindu-ui has no direct `pango` dependency (`crates/pindu-ui/Cargo.toml`). Creating a *private* map needs
  `pango_cairo_font_map_new`, which has no binding in Pindu's dependencies (no `pangocairo` crate in `Cargo.lock`).
  Two options:
  - **(a) Default map, re-added.** Add `pango = { version = "0.22", features = ["v1_56"] }`. This is the same crate
    and version already in `Cargo.lock`, but it is still a dependency change and **needs maintainer approval**
    (AGENTS.md: "Never add, upgrade, or remove a dependency without explicit user approval",
    [L139](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/AGENTS.md#L139)).
    Add the theme's files to the default map (`widget.font_map()` on an unconfigured widget falls back to it,
    [gtkwidget.c L6746-L6758](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkwidget.c#L6746-L6758)),
    and add them again when the map's `items-changed` fires after a host font change **[INFERENCE]**. The fonts are
    then visible app-wide, which is harmless because only the theme's CSS names their families. A font stays loaded
    until the app quits.
  - **(b) Private map.** Hand-write `extern "C"` declarations for `pango_cairo_font_map_new` and
    `pango_font_map_add_font_file` over `gtk::pango::ffi::PangoFontMap`, and set the map on the window only while
    the theme is active. This needs no dependency change but adds `unsafe` FFI.
  - **Recommend (a)**, unless the maintainer prefers that unloading a theme also unloads its fonts.
- **Runtime fonts** (verified with `fc-list`):
  - Adwaita Sans: variable, Thin–Black. This is the default and the substitute for SST.
  - Adwaita Mono.
  - Cantarell VF (Thin–Extra Bold).
  - DejaVu Sans/Serif/Mono.
  - FreeSans/Serif/Mono.
  - Liberation Sans/Serif/Mono.
  - Carlito, Caladea.
  - TeX Gyre Adventor (geometric, Regular/Bold), Heros, Termes, Pagella, Bonum, Schola, Chorus, Cursor.
  - Noto Color Emoji.

  There is **no rounded face**. A rounded look needs a bundled font; any OFL-1.1 font is usable, since the FSF calls
  OFL "a free copyleft license for fonts" ([licence list #SILOFL](https://www.gnu.org/licenses/license-list.html#SILOFL)).
  **[INFERENCE]** Shipping an OFL font file beside GPL code is aggregation; check each font's licence when picking.
  Fonts under `/run/host/fonts` come from the host and cannot be relied on.

### 4.4 Element-by-element table

"CSS-only?" means achievable with a theme stylesheet on today's tree.

| Element | Design | CSS-only? | Expansion needed |
|---|---|---|---|
| Full-bleed focused-game art | both | **Yes.** The hero `picture` already fills `.home` with `content-fit: cover`. | — |
| Blurred or dimmed art | either | **Yes.** `.home > picture { filter: blur(24px) brightness(0.7); }` | — |
| Left and bottom black vignette (ps5 PNG) | Ribbon | **Yes.** Layered `linear-gradient`s on `.home .hero-gradient` | — |
| Background when there is no art (Library tile focused) | both | **Partly.** `background-image: url("bg.svg")` on `.home` resolves in the theme folder, but CSS cannot tell "no art" from "art". | **E3**: a `.no-art` class on `.home` while the hero is empty |
| Crossfade on focus change | both | **No.** The paintable swaps at once, and CSS cannot transition a paintable. | **E3**: two hero pictures, the app toggles `.shown`; the theme sets `transition: opacity` |
| Animated bokeh | Ribbon | **Approximate.** `@keyframes` on `background-position` of an SVG | Own CC0/GPL asset; no video |
| Carousel row at the **top**, title **below** it | Ribbon | **No.** `valign: end` and child order are code. | **E1**: `home-layout` = `hero-bottom` (today) \| `row-top`, chosen by theme metadata and built from Blueprint |
| Title and source icon **beside** the focused tile | Ribbon | **No.** The title is a separate label above the row. | **E1** (`row-top` puts the title row under the carousel at the focused tile's x offset) **[INFERENCE: tracking x needs code]** |
| Square tiles ≈130 px, focused ×1.6 | Ribbon, Panorama | **No** for the size (`set_size_request` 320×150). **Yes** for the scale (`transform: scale(1.6)`), but it overlaps and clips. | **E2**: `home-tile` = `landscape` (320×150) \| `square` \| `portrait` in metadata, with sizes from code; plus padding on `.home-carousel` from CSS for headroom |
| Extra space beside the focused tile | Ribbon | **Yes.** `margin` on `.home-carousel > button:focus-visible` changes layout, with a transition. | — |
| Unfocused dimmed to 90 % | Ribbon | **Yes.** `.home-carousel > button:not(:focus-visible) { filter: brightness(0.9); opacity: 0.9 }` | — |
| Accent outline and glow on focus | Panorama | **Yes.** `outline: 3px solid var(--accent-color); box-shadow: 0 0 24px alpha(var(--accent-color), .6)` | — |
| Title only on the focused tile, in-tile scrim | Panorama | **Yes.** `.home-carousel > button .jump-title { opacity: 0 }` / `:focus-visible .jump-title { opacity: 1 }`; the same for `gridview.library > child:focus-within .cover-tile label` | — |
| Rounded corners ≈19 px or ≈4 px | both | **Yes.** `border-radius` on `cover-art` | — |
| Text tabs (white/grey, Medium/Light) or underline | both | **Yes.** `.library-tabs button`, `:checked`, `box-shadow: inset 0 -3px …` | Tab-label hook already planned in #147 |
| "Home / Library" tabs at top-left | Ribbon | **No.** The widget does not exist. | **E4**: fullscreen top bar (`.fullscreen-top-bar`, `.page-tab`, `.clock`), hidden unless metadata `top-bar: true` |
| Clock top-right | both | **No** | **E4** (GLib DateTime; no permission) |
| Avatar, battery, Wi-Fi | both | **No**, and **not planned**: the system bus is outside the sandbox. | Omit |
| Stats pill (playtime, last played, source) | Ribbon | **No.** Home shows only logo or title. | **E5**: Home details row (`.home-details > .detail`), metadata `home-details: true`; data from the core model |
| Grey translucent pills with dividers | Ribbon | **Yes** once E5 exists: `background: alpha(#7c7c7c, .5); border-radius: 999px` | — |
| Game logo art big at mid-left | Ribbon | **Partly.** The logo picture exists (480×160 request); CSS can enlarge it with `min-*`, not move it. | **E1** places it |
| Media (screenshot or video) at right | Ribbon | **No.** Pindu has no screenshot art (`ArtSet` = cover, hero, logo, icon). | Not planned; could show the cover in a card **[INFERENCE]** |
| Vertical (XMB) carousel | Ribbon variant | **No** | Out of scope |
| Floating top bar with centre icon buttons | Panorama | **No** | **E4** (same widget; icon buttons for Library, Search, Quick Access) |
| Recent row low on screen, rows of collections below | Panorama | Row position **yes** (today's layout). Extra rows **no**. | Later: Home collection rows |
| Left rail library | Panorama | **No** (tabs are code) | Later layout variant `library-layout` |
| Square or 2:3 library tiles | Panorama | **No** for square (sizes are code). 2:3 is today's default. | **E2** extends `tile` aspect to the grid (optional) |
| Guide-like left panel styling | Panorama | **Yes.** `.menu-drawer { background: …; }`, rows, focus outline | — |
| Panel width ≈37 % | Panorama | **Mostly.** `sidebar-width-fraction 0.4` / max 440 is set in Blueprint. CSS `min-width` can raise it **[INFERENCE]**. | — |
| Hint glyph and label styling; left and right hint groups | both | **Partly.** Item boxes are anonymous; position is `halign: end`. | **E6**: `.hint`, `.hint-glyph`, `.hint-label` classes (#147 scope); keep right-aligned |
| Fonts from the runtime (Adwaita Sans weights, Cantarell, Adventor) | both | **Yes.** `font-family`, `font-weight: 300/500`, `font-variation-settings` | — |
| Bundled font in the theme folder | optional | **No** (no `@font-face`) | **E7**: `fonts` list in metadata; the service adds the files to the font map (option (a) or (b), §4.3) |
| Theme sounds | optional | **No** | **E8** (optional): `sounds/{move,select,back}.wav` replace the generated tones |
| Reduced motion and high contrast | both | **Yes.** `@media (prefers-reduced-motion)` is synced, and high contrast suspends the style ([style_service.rs L118-L161](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/style_service.rs#L118-L161)) | — |

---

## 5. Theme folder format

### 5.1 Prior art

- **ES-DE.**
  - A theme is a directory with a mandatory `capabilities.xml` that declares its variants, colour schemes and aspect
    ratios, plus `theme.xml` and assets with "./"-relative paths.
  - Themes come from a user dir and an install dir, and are picked in the UI Settings menu (§1.2 links).
  - Directory names carry `-es-de`; the display name is `themeName`
    ([THEMES.md L3](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md#L3)).
- **Bazaar.**
  - Progress-bar "themes" are data: `pride-flags.yaml` gives an `id` (which "should match the gsettings file"), a
    `name` and stripes
    ([pride-flags.yaml L1-L13](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/progress-bar-designs/pride/pride-flags.yaml#L1-L13)).
  - Code generates one CSS class per id, `.%s-theme { --flag-gradient: linear-gradient(…) }`
    ([pride.c L153-L220](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/src/progress-bar-designs/pride/pride.c#L153-L220)).
  - A GSettings enum picks the theme
    ([gschema L24-L52](https://github.com/kolunmi/bazaar/blob/3d347d5d0fb828a0ff6ff0c1dcd356b26ddfdd33/data/io.github.kolunmi.Bazaar.gschema.xml#L24-L52)).
  - Lesson: **metadata + id + one setting**, with the app turning metadata into classes.

### 5.2 Proposed layout

```
themes/<id>/                 id: [a-z0-9-]+, the folder name
  theme.json                 metadata (below)
  theme.css                  the stylesheet, loaded with load_from_path
  assets/…                   images referenced as url("assets/bg.svg")
  fonts/…                    optional .ttf/.otf/.ttc listed in theme.json
  LICENSE                    required for bundled themes
```

```json
{
  "contract": 1,
  "name": "Ribbon",
  "description": "A row of square tiles across the top over the game's art.",
  "author": "Pindu launcher contributors",
  "license": "GPL-3.0-or-later",
  "layout": {
    "home": "row-top",
    "home-tile": "square",
    "top-bar": true,
    "home-details": true
  },
  "fonts": []
}
```

- **Parsing in core.** `pindu-core` parses `theme.json` with `serde_json`, which it already depends on
  ([crates/pindu-core/Cargo.toml](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/Cargo.toml)),
  so no new crate is needed. The core decides validity, defaults and precedence and gets **bytes** from the service.
  This follows "Shells never read files"
  ([AGENTS.md L109-L110](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/AGENTS.md#L109-L110)).
- **Validation rules:**
  - Unknown keys are ignored.
  - An unknown layout value falls back to the default.
  - `contract` greater than the supported version lists the theme as "Needs a newer Pindu launcher" and does not
    apply it.
  - The contract version is the `pindu-style-contract` number from #147.
- **Bundled themes** live in `pkgdatadir/themes/<id>/`, which is `/app/share/pindu-launcher/themes` in the Flatpak.
  - `pkgdatadir` is already a build constant
    ([config.rs L18](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/config.rs#L18);
    [meson.build L22](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/meson.build#L22)).
    Installing there is a `data/meson.build` `install_subdir`, not a manifest change.
  - **They stay on disk, not in the gresource.** `load_from_resource` would resolve `url()` inside the resource
    ([gtkcssprovider.c L1812-L1830](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssprovider.c#L1812-L1830)),
    but `add_font_file` needs a real file name (§4.3). One rule for bundled and user themes keeps the code single.
- **User themes** live in `$XDG_CONFIG_HOME/pindu-launcher/themes/<id>/`, next to today's `fullscreen.css`
  ([paths.rs L60-L75](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/paths.rs#L60-L75)).
  The sandbox can always read that folder, as verified in the
  [custom-CSS research](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Research%20%E2%80%94%20Custom%20CSS%20for%20fullscreen%20mode.md).
  A user theme with the same id as a bundled one shadows it **[design choice]**.
- **`url()` resolution.**
  - `load_from_path` and `load_from_file` give the parser the file, and `gtk_css_parser_resolve_url` resolves
    relative URLs against `g_file_get_parent(file)`. So `url("assets/bg.svg")` and `@import "parts/home.css"` resolve
    inside the theme folder
    ([gtkcssparser.c L288-L320](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/css/gtkcssparser.c#L288-L320);
    [gtkcssprovider.c L1739-L1767](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssprovider.c#L1739-L1767)).
  - `load_from_string` has no base, so relative URLs fail
    ([gtkcssimageurl.c L44-L48](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/gtk/gtkcssimageurl.c#L44-L48)).
    Themes must therefore always load by path, as `StyleService::load` already does
    ([style_service.rs L237-L245](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/style_service.rs#L237-L245)).
  - Quote URLs; GTK does not support unquoted ones
    ([css-properties.md L283](https://gitlab.gnome.org/GNOME/gtk/-/blob/2ea935e3ec680fb122f245b28e18c3495df8c344/docs/reference/gtk/css-properties.md#L283)).

### 5.3 Fit with `fullscreen-custom-style` and `fullscreen.css`

- **Today.** `StyleService` owns one provider for `fullscreen.css`. It installs it at `PRIORITY_USER` only while the
  setting is on, the window is fullscreen and high contrast is off, watches the folder and reports parse errors
  ([style_service.rs L1-L5, L163-L188](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/style_service.rs#L163-L188);
  key in [gschema L85-L89](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/io.github.tim_van_oudheusden.pindu-launcher.gschema.xml.in#L85-L89)).
- **Proposed setting.** Add a new string key `fullscreen-design`, default `""`, meaning stock Adwaita.
- **Provider order.** Add a second provider in the same service, for the theme, at **`PRIORITY_USER - 1`** (799).
  The resulting order is:
  1. Adwaita (200)
  2. the app's `style.css` (600)
  3. the design (799)
  4. `fullscreen.css` (800, the "user's last word")

  The priority constants are documented on
  [docs.gtk.org](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_USER.html). Both providers follow the same
  `StyleState` gate, so a design never reaches the desktop window either.
- **Layout metadata** from `theme.json` reaches the shell as a core value (`ThemeLayout`). The window applies
  `home-layout-*` and `home-tile-*` classes and builds the chosen Home variant.
- **Fonts** are added to the font map when the design is first applied: the default map under option (a), or a
  private map set on the window while the design is active under option (b) (§4.3).
- **Live reload.** Watch the active theme folder, using the same debounce, so theme authors get the reload that
  `fullscreen.css` users have.
- **Issue #146** (Preferences → Fullscreen: switch, path, Open Style Folder, Import, Reset) stays as it is for
  `fullscreen.css`. Add a **Design** combo row above it, the same list as the Quick Access entry, and an
  "Open Themes Folder" button.
- **Issue #147** (contract doc, class list, missing hooks for hint items, tab labels and the game-page root; neutral
  sample stylesheet) is the prerequisite. The contract names *are* the theme API. Its "Out of scope: a theme picker
  or bundled themes" now becomes the issues below.

---

## 6. Proposed issue split

Expansion issues come first; each one is a hook, not a design.

1. **E1 Home layout variants.** Add `home-layout`: `hero-bottom` (today) or `row-top`, with the carousel at the top,
   the title, logo and details under it, and the logo at mid-left. The variants are built from Blueprint and chosen
   by `ThemeLayout`. Add `.home-layout-*` classes.
2. **E2 Tile aspect.** Add `home-tile`: `landscape` (320×150) \| `square` \| `portrait`, with sizes from core. The
   grid may follow later.
3. **E3 Hero crossfade and states.** Use two hero pictures with a `.shown` class, so the CSS `transition: opacity`
   sets the fade. Add `.no-art` on `.home` when nothing is shown, and respect reduced motion.
4. **E4 Fullscreen top bar.** Add `.fullscreen-top-bar` with `.page-tab` items (Home, Library), a `.clock`, and
   optional icon buttons (Library, Search, Quick Access). It is hidden unless `top-bar: true`. No avatar, battery or
   Wi-Fi.
5. **E5 Home details row.** Add `.home-details` with `.detail` items: playtime, last played, source icon. It is
   hidden unless `home-details: true`.
6. **E6 Style hooks.** Add the `.hint`, `.hint-glyph` and `.hint-label` classes and a `.home-hero` class on the hero
   picture. Fold these into #147 if it is still open.
7. **E7 Theme fonts.** Load `fonts[]` through `pango_font_map_add_font_file`. Under option (a) this needs approval for
   the direct `pango` 0.22 `v1_56` line; option (b) uses hand-written FFI and a private map (§4.3).
8. **E8 (optional) Theme sounds.** Let a theme replace the three generated tones.
9. **T1 Theme folders and switching.**
   - Core: `theme.json` parsing, discovery across `pkgdatadir/themes` and the config themes folder, precedence and
     contract check.
   - Service: the second provider at `PRIORITY_USER - 1`, plus folder watch.
   - The `fullscreen-design` key.
   - A Design row in Preferences (extending #146).
   - A docs section in the #147 contract doc.
10. **T2 Quick Access panel.**
    - `Panel::QuickAccess`, a right-side `OverlaySplitView` (`sidebar-position: end`).
    - `Intent::QuickAccess` on **R3** and the Deck's misc1 "…" button.
    - **Ctrl+2**, the "R3 Quick Access" hint, a shortcuts-dialog entry, and the spec p. 9 row.
    - First entry: **Design**. Possibly move the drawer's Settings level here.
11. **D1 Bundled design "Ribbon"** (from ps5-menu-es-de).
    - Layout from §1.4: `row-top`, `square`, `top-bar`, `home-details`.
    - Adwaita Sans 300/500.
    - A CSS vignette, white and grey pills, no outline, 90 % dimming.
    - Own or CC0 assets only, no brand art (§1.6), with a LICENSE file.
12. **D2 Bundled design "Panorama"** (from the console dashboard).
    - Layout from §2.4: `hero-bottom`, `square`, `top-bar`.
    - Crossfade, accent outline and glow, titles on focus only.
    - Guide-like drawer styling and underlined text tabs.
    - System accent only.

Dependencies:

- #147 comes before E1–E6.
- T1 depends on #147 and E1–E2.
- T2 depends on T1.
- D1 depends on E1–E5 and T1.
- D2 depends on E2–E4 and T1.

---

## Sources

- **Pindu (commit `7ab7fea`):**
  - [home_page.blp](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/home_page.blp)
  - [home_page.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/home_page.rs)
  - [window.blp](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/window.blp)
  - [window.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/window.rs)
  - [style.css](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/style.css)
  - [cover_tile.blp](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/cover_tile.blp)
  - [cover_tile.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/widgets/cover_tile.rs)
  - [game_page.blp](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/data/ui/game_page.blp)
  - [fullscreen.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/fullscreen.rs)
  - [hint_bar.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/hint_bar.rs)
  - [style_service.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/style_service.rs)
  - [custom_style.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/custom_style.rs)
  - [paths.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/paths.rs)
  - [gamepad.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/gamepad.rs)
  - [keys.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/keys.rs)
  - [panels.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/panels.rs)
  - [hints.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-core/src/hints.rs)
  - [manette_backend.rs](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/crates/pindu-ui/src/manette_backend.rs)
  - [AGENTS.md](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/AGENTS.md)
  - [Main Spec](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Adwaita%20Game%20Library%20%E2%80%94%20Main%20Spec.pdf)
  - [Research — Xbox Game Library UI.pdf](https://github.com/Tim-van-Oudheusden/pindu-launcher/blob/7ab7fea8a488d0463f7048450f2c81aa87e40e71/docs/Research%20%E2%80%94%20Xbox%20Game%20Library%20UI.pdf)
  - Issues [#146](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/146) and
    [#147](https://github.com/Tim-van-Oudheusden/pindu-launcher/issues/147)
- **ps5-menu-es-de (commit `a70159b`):**
  [README](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/README.md),
  [capabilities.xml](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/capabilities.xml),
  [colors.xml](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/colors.xml),
  [theme.xml](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/theme.xml),
  [aspect-ratio-16-9.xml](https://github.com/InitialDin/ps5-menu-es-de/blob/a70159ba50b700edd95a33457a8ac8409515a5d8/aspect-ratio-16-9.xml).
  Upstream assets: [Canvas](https://github.com/Siddy212/canvas-es-de/tree/c615aa94313646199f1b0e6c5478b7fe6839e8c4)
  and [Oasis](https://gitlab.com/jarmend/oasis-es-de/-/tree/ef36ec45b62350e9c69b8a95a244ad2f4b4f395f).
- **ES-DE:** [THEMES.md @ 50e4b60](https://gitlab.com/es-de/emulationstation-de/-/blob/50e4b600ae533d772bae3ff880d11a09b05dbe84/THEMES.md)
- **GTK, Pango, libadwaita:**
  - GTK 4.24.1 source (links inline)
  - [css-properties](https://docs.gtk.org/gtk4/css-properties.html), [css-overview](https://docs.gtk.org/gtk4/css-overview.html)
  - [Pango.FontMap.add_font_file](https://docs.gtk.org/Pango/method.FontMap.add_font_file.html), Pango 1.58.2 source
  - [GtkInscription](https://docs.gtk.org/gtk4/class.Inscription.html)
  - [AdwOverlaySplitView:sidebar-position](https://gnome.pages.gitlab.gnome.org/libadwaita/doc/1.10/property.OverlaySplitView.sidebar-position.html)
  - [STYLE_PROVIDER_PRIORITY_USER](https://docs.gtk.org/gtk4/const.STYLE_PROVIDER_PRIORITY_USER.html)
  - [fontconfig FcConfigAppFontAddFile](https://www.freedesktop.org/software/fontconfig/fontconfig-devel/fcconfigappfontaddfile.html)
  - libmanette [0.2.13 Steam Deck driver](https://gitlab.gnome.org/GNOME/libmanette/-/blob/990f8797da563b32f15f56d5f297d9963aa59d9c/src/drivers/manette-steam-deck-driver.c)
- **Valve:**
  - [steamdeck.com/en/tech](https://www.steamdeck.com/en/tech)
  - Steam Deck client updates (links in §3.1)
  - [Steam Client Update 2023-02-01](https://store.steampowered.com/news/189167)
  - [Steamworks: getting started for players](https://partner.steamgames.com/doc/features/steam_controller/getting_started_for_players)
  - [Steam Deck basic use](https://help.steampowered.com/en/faqs/view/69E3-14AF-9764-4C28)
  - [steam-for-linux#10247](https://github.com/ValveSoftware/steam-for-linux/issues/10247)
- **Microsoft:**
  - Xbox Wire posts of [2023-07-26](https://news.xbox.com/en-us/2023/07/26/welcome-to-your-new-xbox-home/),
    [2025-05-16](https://news.xbox.com/en-us/2025/05/16/xbox-insiders-get-new-ways-to-personalize-home/),
    [2026-03-18](https://news.xbox.com/en-us/2026/03/18/xbox-insiders-april-2026-console-features/),
    [2026-06-10](https://news.xbox.com/en-us/2026/06/10/xbox-insiders-july-2026-console-features-2/) and
    [2026-08-19](https://news.xbox.com/en-us/2026/08/19/xbox-insiders-october-2026-console-features-2/)
  - support.xbox.com pages on
    [backgrounds](https://support.xbox.com/en-US/help/account-profile/customize-xbox-home/change-xbox-one-background),
    [Home](https://support.xbox.com/en-US/help/account-profile/customize-xbox-home/customize-your-home),
    [the Guide](https://support.xbox.com/en-US/help/hardware-network/getting-started-set-up/using-the-guide) and
    [groups](https://support.xbox.com/en-US/help/games-apps/my-games-apps/group-games-apps)
- **Licences:** [GNU licence list](https://www.gnu.org/licenses/license-list.html), [GPL-3.0](https://www.gnu.org/licenses/gpl-3.0.html)
- **Flatpak:** [sandbox permissions](https://docs.flatpak.org/en/latest/sandbox-permissions.html)
