# nixpkgs recipe (#382), in nixpkgs style for pkgs/by-name/pi/pindu-launcher/package.nix.
# It builds the release tarball (pindu-launcher-X.Y.Z.tar.xz, made by `meson dist`), whose
# vendor/ holds the crates: crates/meson.build then builds offline from them with --locked,
# so no cargoDeps or cargoHash is needed and the source hash is the only one to bump
# (docs/releasing.md). Try it with:
#
#   nix-build -E 'with import <nixpkgs> { }; callPackage ./build-aux/nix/package.nix { }'
{
  lib,
  stdenv,
  fetchurl,
  meson,
  ninja,
  pkg-config,
  cargo,
  rustc,
  blueprint-compiler,
  desktop-file-utils,
  appstream,
  gettext,
  glib,
  gtk4,
  libadwaita,
  libmanette,
  libsoup_3,
  sqlite,
  fontconfig,
  libseccomp,
  wrapGAppsHook4,
  python3,
  glycin-loaders,
  bubblewrap,
  gst_all_1,
  pipewire,
  wireplumber,
}:

stdenv.mkDerivation (finalAttrs: {
  pname = "pindu-launcher";
  version = "0.4.1";

  src = fetchurl {
    url = "https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/download/v${finalAttrs.version}/pindu-launcher-${finalAttrs.version}.tar.xz";
    hash = "sha256-CVk+5J9yI5JRBjYVDOhvJXrmmFmlYHhd0QTgtl1Bk64=";
  };

  nativeBuildInputs = [
    meson
    ninja
    pkg-config
    cargo
    rustc
    blueprint-compiler
    desktop-file-utils
    appstream
    gettext
    glib
    gtk4
    wrapGAppsHook4
    # meson setup looks for python3, which runs the tooling tests
    python3
  ];

  buildInputs = [
    glib
    gtk4
    libadwaita
    libmanette
    libsoup_3
    sqlite
    fontconfig
    libseccomp
    # Fullscreen Home's background (VP9 in Matroska) and the navigation sounds (WAV) play
    # through GTK's GStreamer backend; the demuxer, decoder and parser are in the good ones
    gst_all_1.gstreamer
    gst_all_1.gst-plugins-base
    gst_all_1.gst-plugins-good
  ];

  mesonBuildType = "release";

  # meson test runs these Python scripts directly, through /usr/bin/env
  postPatch = ''
    patchShebangs --build po/check-potfiles.py build-aux/release-notes.py
  '';

  doCheck = true;

  preFixup = ''
    gappsWrapperArgs+=(
      # Image decoding loads glycin's loaders, sandboxed with bubblewrap
      --prefix XDG_DATA_DIRS : "${glycin-loaders}/share"
      # bwrap for glycin's sandbox; fullscreen mode's Sound Output runs pw-dump and wpctl
      --prefix PATH : "${
        lib.makeBinPath [
          bubblewrap
          pipewire
          wireplumber
        ]
      }"
    )
  '';

  meta = {
    description = "Every PC game on your computer in one GNOME library";
    homepage = "https://github.com/Tim-van-Oudheusden/pindu-launcher";
    changelog = "https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v${finalAttrs.version}";
    # The app is GPL-3.0-or-later; the vendored crates add the licences in the RPM spec
    license = with lib.licenses; [
      gpl3Plus
      mit
      asl20
      mpl20
      bsd3
      bsd2
      zlib
      isc
      unicode-30
      ijg
      unlicense
      bsd0
      llvm-exception
    ];
    mainProgram = "pindu-launcher";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
    maintainers = [ ];
  };
})
