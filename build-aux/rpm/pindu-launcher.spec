# RPM for Pindu launcher, built from the release tarball with the crates vendored
# (pindu-launcher-X.Y.Z.tar.xz, made by `meson dist`), so rpmbuild needs no network.
# Version must match meson.build; build-aux/check-version.sh checks it.

%global app_id io.github.tim_van_oudheusden.pindu-launcher

Name:           pindu-launcher
Version:        0.2.0
Release:        1%{?dist}
Summary:        Every PC game on your computer in one GNOME library

# The app is GPL-3.0-or-later; the rest are the licences of the vendored crates that
# build for Linux (cargo metadata --filter-platform x86_64-unknown-linux-gnu).
License:        GPL-3.0-or-later AND MIT AND Apache-2.0 AND (MIT OR Apache-2.0) AND (MPL-2.0 OR LGPL-2.1-or-later) AND BSD-3-Clause AND BSD-2-Clause AND Zlib AND ISC AND Unicode-3.0 AND IJG AND (Unlicense OR MIT) AND (0BSD OR MIT OR Apache-2.0) AND (Apache-2.0 WITH LLVM-exception)
URL:            https://github.com/Tim-van-Oudheusden/pindu-launcher
Source0:        %{url}/releases/download/v%{version}/%{name}-%{version}.tar.xz

ExclusiveArch:  x86_64

BuildRequires:  meson >= 1.6.0
BuildRequires:  cargo
BuildRequires:  rust >= 1.93
BuildRequires:  gcc
BuildRequires:  blueprint-compiler
BuildRequires:  gettext
BuildRequires:  desktop-file-utils
BuildRequires:  appstream
BuildRequires:  pkgconfig(glib-2.0)
BuildRequires:  pkgconfig(gtk4) >= 4.24
BuildRequires:  pkgconfig(libadwaita-1) >= 1.10
BuildRequires:  pkgconfig(manette-0.2) >= 0.2.10
BuildRequires:  pkgconfig(libsoup-3.0)
BuildRequires:  pkgconfig(sqlite3)
# The glycin crate's own build needs
BuildRequires:  pkgconfig(fontconfig)
BuildRequires:  pkgconfig(libseccomp)

# Image decoding loads glycin's loaders, sandboxed with bubblewrap, at runtime
Requires:       glycin-loaders
Requires:       bubblewrap
Requires:       hicolor-icon-theme
# GameMode for games that turn it on; umu-launcher, which runs Windows games, has no
# Fedora package: https://github.com/Open-Wine-Components/umu-launcher#installing
Recommends:     gamemode

%description
Pindu launcher gathers every game on your computer into one library, whichever launcher
installed it: Steam, Heroic, Lutris, Bottles, Faugus, installed apps and games you add
yourself. Browse covers, pick up where you left off and start any game with one click, or
switch to the fullscreen mode and do the same from the couch with a controller.

%prep
%autosetup -p1

%build
%meson --buildtype=release
%meson_build

%install
%meson_install

%check
%meson_test
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

# No translations yet (po/LINGUAS is empty). With the first one, add `%find_lang %{name}`
# to %install and `-f %{name}.lang` here: rpmbuild fails on the unpackaged .mo until then.
%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}
%{_datadir}/%{name}/
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/dbus-1/services/%{app_id}.service
%{_datadir}/glib-2.0/schemas/%{app_id}.gschema.xml
%{_datadir}/gnome-shell/search-providers/%{app_id}.search-provider.ini
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{app_id}-symbolic.svg
%{_metainfodir}/%{app_id}.metainfo.xml

%changelog
* Sat Nov 07 2026 Tim van Oudheusden - 0.2.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.2.0
