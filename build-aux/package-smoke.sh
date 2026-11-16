#!/bin/sh
# Installs a native package in a clean Fedora, Debian or Ubuntu container, with the
# dependencies it declares, and runs the UI smoke test on it under Xvfb twice. First the
# app must start from the installed files, keep running and decode a cover through glycin,
# which checks that the distribution's glycin loaders work with the glycin crate the app is
# built with. Then, in fullscreen mode, GStreamer must play Home's background and a
# navigation sound, which checks that the package pulls in the GStreamer plug-ins they need.
#
# Usage: build-aux/package-smoke.sh PACKAGE (an .rpm or a .deb)
set -eu

package=$(realpath "$1")
here=$(cd "$(dirname "$0")" && pwd)

case $package in
  *.rpm)
    # gdbus comes with glib2
    dnf -y install "$package" xorg-x11-server-Xvfb xorg-x11-xauth dbus-daemon
    ;;
  *.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y "$package" xvfb xauth dbus-daemon libglib2.0-bin
    ;;
  *)
    echo "Not an .rpm or a .deb: $package" >&2
    exit 1
    ;;
esac

xvfb-run -a sh "$here/../tests/e2e/ui-smoke.sh" /usr/bin/pindu-launcher \
  /usr/share/pindu-launcher/resources.gresource "$here/../assets/pindu_icon.png"
xvfb-run -a sh "$here/../tests/e2e/ui-smoke.sh" \
  --fullscreen io.github.tim_van_oudheusden.pindu-launcher /usr/bin/pindu-launcher \
  /usr/share/pindu-launcher/resources.gresource
