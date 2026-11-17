#!/bin/sh
# Builds the RPM from a release tarball (`meson dist`, crates vendored), as root in a Fedora
# container: installs the spec's build dependencies, then runs rpmbuild, which needs no
# network. The package lands in OUTDIR as pindu-launcher-X.Y.Z-1.fcNN.ARCH.rpm, named after
# the tarball's version: a pre-release's file is pindu-launcher-X.Y.Z-rc.N-1.fcNN.ARCH.rpm,
# while the package inside is X.Y.Z~rcN (GitHub renames release files with a `~`).
#
# Usage: build-aux/rpm/build.sh TARBALL OUTDIR
set -eu

tarball=$(realpath "$1")
mkdir -p "$2"
out=$(realpath "$2")
spec=$(cd "$(dirname "$0")" && pwd)/pindu-launcher.spec
version=$(basename "$tarball" .tar.xz)
version=${version#pindu-launcher-}

dnf -y install rpm-build 'dnf5-command(builddep)'
dnf -y builddep "$spec"
rpmbuild -bb \
  --define "_sourcedir $(dirname "$tarball")" \
  --define "_rpmdir $out" \
  --define "_build_name_fmt %%{NAME}-$version-%%{RELEASE}.%%{ARCH}.rpm" \
  "$spec"
# Only the package itself is released, not its debuginfo and debugsource packages
find "$out" -name 'pindu-launcher-debug*.rpm' -delete
ls -l "$out"
