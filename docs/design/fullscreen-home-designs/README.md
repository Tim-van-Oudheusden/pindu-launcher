# Reference mockups: three fullscreen Home designs

Reference material for the issue "Add three fullscreen Home designs: Marquee, Shelves and Lobby". This folder is on
the `design/fullscreen-home-designs` branch only; it is not meant to be merged.

| Design | Picture | Named parts | Source |
|---|---|---|---|
| Marquee | `marquee.jpg` | `marquee-annotated.jpg` | `marquee.html` |
| Shelves | `shelves.jpg` | `shelves-annotated.jpg` | `shelves.html` |
| Lobby | `lobby.jpg` | `lobby-annotated.jpg` | `lobby.html` |

- Every picture is a 1280×720 logical-pixel screen, rendered at 1.5×.
- The mockups are web pages, not screenshots of the app. `mock.css` and each page's `<style>` hold the sizes,
  colours, radii and opacities; the layout technique (absolute positioning, flexbox) is the web's, not GTK's.
  Nothing uses `backdrop-filter`, which GTK CSS does not have.
- Add `?annot` to a page's address to outline the named parts, as in the `-annotated` pictures.
- The game art in `art/` was drawn for these mockups; the games do not exist. `art/red-ribbons.jpg` is a frame
  of `data/home/red-ribbons-1080p60.webm`.
- Where a mockup and the issue disagree, the issue wins.
