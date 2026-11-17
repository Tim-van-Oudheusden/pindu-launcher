#!/bin/sh
# Builds the .deb from a release tarball (`meson dist`, crates vendored), as root in a
# Debian or Ubuntu container: installs debian/control's build dependencies, then runs
# dpkg-buildpackage, which needs no network. A .deb only fits the release it was built on,
# so the package lands in OUTDIR as pindu-launcher-X.Y.Z-TARGET-ARCH.deb (amd64 or arm64),
# named after the tarball's version: a pre-release's file is
# pindu-launcher-X.Y.Z-rc.N-TARGET-ARCH.deb, while the package inside is X.Y.Z~rcN-1
# (GitHub renames release files with a `~`).
#
# Usage: build-aux/deb/build.sh TARBALL OUTDIR TARGET (for example debian-testing)
set -eu

tarball=$(realpath "$1")
mkdir -p "$2"
out=$(realpath "$2")
target=$3
debian=$(cd "$(dirname "$0")" && pwd)/debian
version=$(basename "$tarball" .tar.xz)
version=${version#pindu-launcher-}

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

arch=$(dpkg --print-architecture)
cp "$work"/pindu-launcher_*_"$arch".deb "$out/pindu-launcher-$version-$target-$arch.deb"
ls -l "$out"
