#!/bin/sh
# Installs a native package in a clean Fedora, Debian or Ubuntu container, with the
# dependencies it declares, and runs the UI smoke test on it under Xvfb: the app must start
# from the installed files and keep running.
#
# Usage: build-aux/package-smoke.sh PACKAGE (an .rpm or a .deb)
set -eu

package=$(realpath "$1")
smoke=$(cd "$(dirname "$0")" && pwd)/ui-smoke.sh

case $package in
  *.rpm)
    dnf -y install "$package" xorg-x11-server-Xvfb xorg-x11-xauth dbus-daemon
    ;;
  *.deb)
    export DEBIAN_FRONTEND=noninteractive
    apt-get update
    apt-get install -y "$package" xvfb xauth dbus-daemon
    ;;
  *)
    echo "Not an .rpm or a .deb: $package" >&2
    exit 1
    ;;
esac

xvfb-run -a sh "$smoke" /usr/bin/pindu-launcher /usr/share/pindu-launcher/resources.gresource
