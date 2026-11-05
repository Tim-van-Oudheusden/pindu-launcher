# Pindu launcher

A native GNOME game library: every PC game on the machine in one library, with a fullscreen controller mode over the
same data. Rust on GTK4 and libadwaita, built with Meson, shipped as a Flatpak on GNOME 51.

## Building

The build needs GTK 4.24, libadwaita 1.10, blueprint-compiler, Meson and Rust 1.93 or newer, all of which the
`org.gnome.Sdk//51` runtime with the `rust-stable` extension provides.

```sh
meson setup _build
meson compile -C _build
meson test -C _build
```

### Development Flatpak

The development manifest builds the app with the `development` profile, which installs
it as `io.github.tim_van_oudheusden.pindu-launcher-devel` beside the stable app, with the
nightly icon:

```sh
flatpak-builder --user --install --force-clean _flatpak \
  build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml
flatpak run io.github.tim_van_oudheusden.pindu-launcher-devel
```

CI builds the same manifest, runs the tests in it (including a UI smoke test under
xvfb), lints the manifest and the build with `flatpak-builder-lint`, and uploads the
bundle. The lint exceptions in `build-aux/flatpak/lint-exceptions.json` each say why.

## Running in gamescope

For a couch setup, run the app's fullscreen mode inside [gamescope](https://github.com/ValveSoftware/gamescope):

```sh
gamescope -f -- flatpak run --nosocket=wayland io.github.tim_van_oudheusden.pindu-launcher --fullscreen
```

- The app runs as an X11 client on gamescope's Xwayland, and its window fills gamescope's screen. A game window that
  opens takes gamescope's focus, and the app gets it back when the game quits.
- `--nosocket=wayland` matters when gamescope starts from a desktop session: without it, Flatpak gives the app the
  desktop's Wayland display, and the window opens on the desktop instead of in gamescope.
- `-f` makes gamescope's window fullscreen when it runs nested in a desktop session.
- Under X11 the app draws at scale 1. On a 4K screen, let gamescope upscale a smaller picture
  (`gamescope -W 3840 -H 2160 -w 1920 -h 1080 -f -- …`) or scale the app (`flatpak run --env=GDK_SCALE=2 …`).
- `--expose-wayland` is not recommended. The app then runs as a Wayland client, but gamescope doesn't make its window
  fullscreen: the window keeps its desktop size, and gamescope scales it up to the screen.

## Translating

Translations are gettext `.po` files in `po/`, one per language listed in `po/LINGUAS`, made from the template
`po/pindu-launcher.pot`. After changing translatable strings, refresh the template:

```sh
meson compile -C _build pindu-launcher-pot
```

Every file with translatable strings must be listed in `po/POTFILES.in`; `meson test` checks that. CI also runs
`po/check-pot.sh _build`, which fails when the template's strings differ from the code's (locations and dates are
ignored).

## Licence

GPL-3.0-or-later; see [LICENSE](LICENSE).
