#!/bin/sh
# Meson, Cargo, the MetaInfo's newest <release>, the RPM spec and the newest Debian changelog
# entry must name the same version. With a tag argument (as the release workflow passes), the
# tag must be that version with a `v` prefix.
#
# A version is X.Y.Z, or X.Y.Z-rc.N for a pre-release (docs/releasing.md). Meson and Cargo
# name a pre-release in SemVer, X.Y.Z-rc.N; the MetaInfo, the RPM and the DEB name it
# X.Y.Z~rcN, which their version comparisons sort below X.Y.Z, and the MetaInfo marks it
# type="development".
#
# Usage: build-aux/check-version.sh [vX.Y.Z | vX.Y.Z-rc.N]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
meson=$(sed -n "s/^  version: '\([^']*\)',$/\1/p" "$root/meson.build" | head -n 1)
cargo=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
release=$(grep -m 1 '^ *<release ' \
  "$root/data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in" || true)
metainfo=$(printf '%s\n' "$release" | sed -n 's/^ *<release version="\([^"]*\)".*/\1/p')
type=$(printf '%s\n' "$release" | sed -n 's/.* type="\([^"]*\)".*/\1/p')
rpm=$(sed -n 's/^Version: *\([^ ]*\)$/\1/p' "$root/build-aux/rpm/pindu-launcher.spec" | head -n 1)
deb=$(sed -n '1s/^pindu-launcher (\([^-)]*\)-[^)]*).*/\1/p' "$root/build-aux/deb/debian/changelog")

echo "meson.build: $meson, Cargo.toml: $cargo, MetaInfo: $metainfo${type:+ ($type)}, RPM: $rpm," \
  "DEB: $deb"
if ! printf '%s\n' "$meson" | grep -Eqx '[0-9]+\.[0-9]+\.[0-9]+(-rc\.[0-9]+)?'; then
  echo "meson.build's version must be X.Y.Z or X.Y.Z-rc.N (docs/releasing.md)." >&2
  exit 1
fi
packaged=$(printf '%s\n' "$meson" | sed 's/-rc\.\([0-9]*\)$/~rc\1/')
if [ "$cargo" != "$meson" ] || [ "$metainfo" != "$packaged" ] || [ "$rpm" != "$packaged" ] ||
  [ "$deb" != "$packaged" ]; then
  echo "The versions disagree: bump meson.build, Cargo.toml, the RPM spec, add a MetaInfo" \
    "<release> and a Debian changelog entry (docs/releasing.md). Meson and Cargo want" \
    "$meson; the MetaInfo, RPM and DEB want $packaged." >&2
  exit 1
fi
if [ "$packaged" != "$meson" ] && [ "$type" != development ]; then
  echo "The MetaInfo's <release version=\"$packaged\"> is a pre-release, so it needs" \
    'type="development".' >&2
  exit 1
fi
if [ "$packaged" = "$meson" ] && [ "$type" = development ]; then
  echo "The MetaInfo's <release version=\"$packaged\"> is a final release, so it must not be" \
    'type="development".' >&2
  exit 1
fi
if [ $# -gt 0 ] && [ "$1" != "v$meson" ]; then
  echo "Tag $1 does not match version $meson; the tag must be v$meson." >&2
  exit 1
fi
