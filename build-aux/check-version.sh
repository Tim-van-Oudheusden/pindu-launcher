#!/bin/sh
# Meson, Cargo and the MetaInfo's newest <release> must name the same version. With a tag
# argument (as the release workflow passes), the tag must be that version with a `v` prefix.
#
# Usage: build-aux/check-version.sh [vX.Y.Z]
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
meson=$(sed -n "s/^  version: '\([^']*\)',$/\1/p" "$root/meson.build" | head -n 1)
cargo=$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$root/Cargo.toml" | head -n 1)
metainfo=$(sed -n 's/^ *<release version="\([^"]*\)".*/\1/p' \
  "$root/data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in" | head -n 1)

echo "meson.build: $meson, Cargo.toml: $cargo, MetaInfo: $metainfo"
if [ -z "$meson" ] || [ "$meson" != "$cargo" ] || [ "$meson" != "$metainfo" ]; then
  echo "The versions disagree: bump meson.build, Cargo.toml and add a MetaInfo <release>." >&2
  exit 1
fi
if [ $# -gt 0 ] && [ "$1" != "v$meson" ]; then
  echo "Tag $1 does not match version $meson; the tag must be v$meson." >&2
  exit 1
fi
