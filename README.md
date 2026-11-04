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

## Translating

Translations are gettext `.po` files in `po/`, one per language listed in `po/LINGUAS`, made from the template
`po/pindu-launcher.pot`. After changing translatable strings, refresh the template:

```sh
meson compile -C _build pindu-launcher-pot
```

Every file with translatable strings must be listed in `po/POTFILES.in`; `meson test` checks that.

## Licence

GPL-3.0-or-later; see [LICENSE](LICENSE).
