# Brand artwork

The images that introduce Pindu launcher: the README's banner and the repository's social preview. Both show the
app's name beside the desktop window, with a television in front of it showing fullscreen mode. They follow the
artwork of [Pindu OS](https://github.com/Tim-van-Oudheusden/pindu-os/tree/main/docs/brand), so the two projects look
like a pair.

| File | Size | Use |
| --- | --- | --- |
| [`readme-banner.webp`](readme-banner.webp) | 2560x800, shown at 1280x400, transparent corners | The top of the [README](../../README.md) |
| [`social-preview.png`](social-preview.png) | 1280x640 | The repository's social preview: **Settings → General → Social preview** |

GitHub has no API for the social preview, so a person uploads `social-preview.png` once, and again when it changes.

Nothing else is kept here. The images are made from files that live elsewhere in the repository:

- the app icon, [`data/icons/io.github.tim_van_oudheusden.pindu-launcher.svg`](../../data/icons/io.github.tim_van_oudheusden.pindu-launcher.svg);
- the desktop window, [`docs/screenshots/desktop-1-library.webp`](../screenshots/desktop-1-library.webp);
- the television's screen, [`docs/screenshots/fullscreen-marquee-home.webp`](../screenshots/fullscreen-marquee-home.webp).

## Rendering the images

Both are drawn by [`source/scene.html`](source/scene.html), a page with one layout per image, and rendered by
[`source/build.js`](source/build.js) with headless Chromium. It needs Node.js with
[Playwright](https://playwright.dev) and the [Inter](https://github.com/rsms/inter) typeface (Inter and Inter Display)
installed. Neither is a dependency of the app or of its build.

```sh
cd docs/brand/source
node build.js                       # the files in docs/brand/
node build.js --extras ~/pindu-art  # a link-preview image, an article cover and two lockups, into a folder of your own
```

Render them again when the icon or either screenshot changes.

- **The words.** The tagline and the line under it are the `SUB` and `LINE` constants at the top of `build.js`. The
  README's `alt` text for the banner repeats them.
- **The layouts.** Each image picks a layout by class name (`social`, `og`, `cover`, `banner`, `lockup`); the sizes and
  positions are at the end of the style sheet in `scene.html`. `screens: 1` adds the window and the television.
- **The screenshots.** To show other screens, point the `.window` and `.tv .shot` rules in `scene.html` at other files
  in `docs/screenshots/`: a 1280x900 one for the window and a 16:9 one for the television.
- **The colours.** Navy `#0a1340` and blue `#0a6cf0` for the name, on a background that runs from `#f6fbff` to
  `#c4dfff`, as on Pindu OS's banner.

The extras are not kept in the repository: they are for articles and profiles, and the command above makes them when
they are needed.

| Extra | Size | Use |
| --- | --- | --- |
| `og-image.png` | 1200x630 | Link previews on sites that want that shape |
| `article-cover.png` | 1920x1080 | The cover of an article or a video |
| `lockup-for-light.png`, `lockup-for-dark.png` | 1160x320, transparent | Icon and name, for light and dark pages |

## Licence

The page, the script and the images are [GPL-3.0-or-later](../../LICENSE). The text in the images is set in
[Inter](https://github.com/rsms/inter), SIL Open Font License 1.1. The game art in the two screenshots keeps each
game's own licence, listed in [`docs/screenshots/CREDITS.md`](../screenshots/CREDITS.md).
