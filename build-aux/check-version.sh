#!/bin/sh
# Meson, Cargo, the MetaInfo's newest <release>, the RPM spec and the newest Debian changelog
# entry must name the same version. With a tag argument (as the release workflow passes), the
# tag must be that version with a `v` prefix.
#
# Usage: build-aux/check-version.sh [vX.Y.Z]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
meson=$(sed -n "s/^  version: '\([^']*\)',$/\1/p" "$root/meson.build" | head -n 1)
cargo=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
metainfo=$(sed -n 's/^ *<release version="\([^"]*\)".*/\1/p' \
  "$root/data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in" | head -n 1)
rpm=$(sed -n 's/^Version: *\([^ ]*\)$/\1/p' "$root/build-aux/rpm/pindu-launcher.spec" | head -n 1)
deb=$(sed -n '1s/^pindu-launcher (\([^-)]*\)-[^)]*).*/\1/p' "$root/build-aux/deb/debian/changelog")

echo "meson.build: $meson, Cargo.toml: $cargo, MetaInfo: $metainfo, RPM: $rpm, DEB: $deb"
for version in "$cargo" "$metainfo" "$rpm" "$deb"; do
  if [ -z "$meson" ] || [ "$version" != "$meson" ]; then
    echo "The versions disagree: bump meson.build, Cargo.toml, the RPM spec, add a MetaInfo" \
      "<release> and a Debian changelog entry (docs/releasing.md)." >&2
    exit 1
  fi
done
if [ $# -gt 0 ] && [ "$1" != "v$meson" ]; then
  echo "Tag $1 does not match version $meson; the tag must be v$meson." >&2
  exit 1
fi
