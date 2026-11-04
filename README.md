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

## Licence

GPL-3.0-or-later; see [LICENSE](LICENSE).
