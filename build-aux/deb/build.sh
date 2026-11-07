#!/bin/sh
# Builds the .deb from a release tarball (`meson dist`, crates vendored), as root in a
# Debian or Ubuntu container: installs debian/control's build dependencies, then runs
# dpkg-buildpackage, which needs no network. A .deb only fits the release it was built on,
# so the package lands in OUTDIR as pindu-launcher-X.Y.Z-TARGET-amd64.deb.
#
# Usage: build-aux/deb/build.sh TARBALL OUTDIR TARGET (for example debian-testing)
set -eu

tarball=$(realpath "$1")
mkdir -p "$2"
out=$(realpath "$2")
target=$3
debian=$(cd "$(dirname "$0")" && pwd)/debian

export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends devscripts equivs xz-utils

work=$(mktemp -d)
tar -xJf "$tarball" -C "$work"
src=$(find "$work" -mindepth 1 -maxdepth 1 -type d)
cp -r "$debian" "$src/debian"
cd "$src"
mk-build-deps --install --remove --tool 'apt-get -y --no-install-recommends' debian/control
dpkg-buildpackage -b -us -uc

version=$(dpkg-parsechangelog -S Version | sed 's/-[^-]*$//')
cp "$work"/pindu-launcher_*_amd64.deb "$out/pindu-launcher-$version-$target-amd64.deb"
ls -l "$out"
