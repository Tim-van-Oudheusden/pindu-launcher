#!/bin/sh
# Builds the RPM from a release tarball (`meson dist`, crates vendored), as root in a Fedora
# container: installs the spec's build dependencies, then runs rpmbuild, which needs no
# network. The package lands in OUTDIR as pindu-launcher-X.Y.Z-1.fcNN.x86_64.rpm.
#
# Usage: build-aux/rpm/build.sh TARBALL OUTDIR
set -eu

tarball=$(realpath "$1")
mkdir -p "$2"
out=$(realpath "$2")
spec=$(cd "$(dirname "$0")" && pwd)/pindu-launcher.spec

dnf -y install rpm-build 'dnf5-command(builddep)'
dnf -y builddep "$spec"
rpmbuild -bb \
  --define "_sourcedir $(dirname "$tarball")" \
  --define "_rpmdir $out" \
  --define "_build_name_fmt %%{NAME}-%%{VERSION}-%%{RELEASE}.%%{ARCH}.rpm" \
  "$spec"
# Only the package itself is released, not its debuginfo and debugsource packages
find "$out" -name 'pindu-launcher-debug*.rpm' -delete
ls -l "$out"
