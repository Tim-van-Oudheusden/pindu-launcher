# RPM for Pindu launcher, built from the release tarball with the crates vendored
# (pindu-launcher-X.Y.Z.tar.xz, made by `meson dist`), so rpmbuild needs no network.
# Version must match meson.build, with a pre-release X.Y.Z-rc.N written X.Y.Z~rcN;
# build-aux/check-version.sh checks it.

%global app_id io.github.tim_van_oudheusden.pindu-launcher

Name:           pindu-launcher
Version:        0.7.0~rc3
Release:        1%{?dist}
# A pre-release's Version is X.Y.Z~rcN, which sorts below X.Y.Z; its tag and tarball name
# it X.Y.Z-rc.N
%global upstream_version %{lua: print((rpm.expand("%{version}"):gsub("~rc", "-rc.")))}
Summary:        Every PC game on your computer in one GNOME library

# The app is GPL-3.0-or-later; the rest are the licences of the vendored crates that
# build for Linux (cargo metadata --filter-platform x86_64-unknown-linux-gnu; aarch64 builds
# a subset of those crates).
License:        GPL-3.0-or-later AND MIT AND Apache-2.0 AND (MIT OR Apache-2.0) AND (MPL-2.0 OR LGPL-2.1-or-later) AND BSD-3-Clause AND BSD-2-Clause AND Zlib AND ISC AND Unicode-3.0 AND IJG AND (Unlicense OR MIT) AND (0BSD OR MIT OR Apache-2.0) AND (Apache-2.0 WITH LLVM-exception)
URL:            https://github.com/Tim-van-Oudheusden/pindu-launcher
Source0:        %{url}/releases/download/v%{upstream_version}/%{name}-%{upstream_version}.tar.xz

ExclusiveArch:  x86_64 aarch64

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
# Fullscreen mode's Sound Output lists the outputs with pw-dump and sets the default
# with wpctl
Requires:       pipewire-utils
Requires:       wireplumber
# Fullscreen Home's background (VP9 in Matroska) and the navigation sounds (WAV) play
# through GTK's GStreamer backend; gtk4 pulls in only the base plug-ins, and the Matroska
# demuxer, VP9 decoder and WAV parser are in the good ones
Requires:       gstreamer1-plugins-good
# GameMode for games that turn it on; umu-launcher, which runs Windows games, has no
# Fedora package: https://github.com/Open-Wine-Components/umu-launcher#installing
Recommends:     gamemode

%description
Pindu launcher gathers every game on your computer into one library, whichever launcher
installed it: Steam, Heroic, Lutris, Bottles, Faugus, installed apps and games you add
yourself. Browse covers, pick up where you left off and start any game with one click, or
switch to the fullscreen mode and do the same from the couch with a controller.

%prep
%autosetup -p1 -n %{name}-%{upstream_version}

%build
# -Dgamescope-session: the gamescope-session-plus client and its login entry
%meson --buildtype=release -Dgamescope-session=true
%meson_build

%install
%meson_install
%find_lang %{name}

%check
%meson_test
desktop-file-validate %{buildroot}%{_datadir}/applications/%{app_id}.desktop
appstreamcli validate --no-net %{buildroot}%{_metainfodir}/%{app_id}.metainfo.xml

%files -f %{name}.lang
%license LICENSE
%doc README.md
%{_bindir}/%{name}
# The bundled external importers: Bottles' importer, pindu-import-bottles, and its manifest
%{_libexecdir}/%{name}/
%{_datadir}/%{name}/
%{_datadir}/applications/%{app_id}.desktop
%{_datadir}/dbus-1/services/%{app_id}.service
%{_datadir}/glib-2.0/schemas/%{app_id}.gschema.xml
%{_datadir}/gnome-shell/search-providers/%{app_id}.search-provider.ini
%{_datadir}/icons/hicolor/scalable/apps/%{app_id}.svg
%{_datadir}/icons/hicolor/symbolic/apps/%{app_id}-symbolic.svg
%{_metainfodir}/%{app_id}.metainfo.xml
%dir %{_datadir}/gamescope-session-plus
%dir %{_datadir}/gamescope-session-plus/sessions.d
%{_datadir}/gamescope-session-plus/sessions.d/pindu
%dir %{_datadir}/wayland-sessions
%{_datadir}/wayland-sessions/gamescope-session-pindu.desktop

%changelog
* Mon Nov 23 2026 Tim van Oudheusden - 0.7.0~rc3-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.7.0-rc.3

* Mon Nov 23 2026 Tim van Oudheusden - 0.7.0~rc2-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.7.0-rc.2

* Mon Nov 23 2026 Tim van Oudheusden - 0.7.0~rc1-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.7.0-rc.1

* Sat Nov 21 2026 Tim van Oudheusden - 0.6.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.6.0

* Fri Nov 20 2026 Tim van Oudheusden - 0.6.0~rc1-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.6.0-rc.1

* Thu Nov 19 2026 Tim van Oudheusden - 0.5.1-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.1

* Wed Nov 18 2026 Tim van Oudheusden - 0.5.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.0

* Wed Nov 18 2026 Tim van Oudheusden - 0.5.0~rc4-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.0-rc.4

* Wed Nov 18 2026 Tim van Oudheusden - 0.5.0~rc3-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.0-rc.3

* Tue Nov 17 2026 Tim van Oudheusden - 0.5.0~rc2-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.0-rc.2

* Tue Nov 17 2026 Tim van Oudheusden - 0.5.0~rc1-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.5.0-rc.1

* Tue Nov 17 2026 Tim van Oudheusden - 0.4.1-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.4.1

* Mon Nov 16 2026 Tim van Oudheusden - 0.4.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.4.0

* Sat Nov 07 2026 Tim van Oudheusden - 0.3.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.3.0

* Sat Nov 07 2026 Tim van Oudheusden - 0.2.0-1
- See https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/tag/v0.2.0
