#!/bin/sh
# The release manifest must grant what the development manifest grants and bundle the same
# modules (AGENTS.md: release manifests copy them). Everything from `finish-args:` on is
# compared, minus the lines that differ on purpose: the development build's debug
# environment, and the profile and build type of the app module. Both must build the app
# optimised.
set -eu

dir=${1:-$(dirname "$0")/flatpak}
devel=$dir/io.github.tim_van_oudheusden.pindu-launcher-devel.yml
release=$dir/io.github.tim_van_oudheusden.pindu-launcher.yml
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

shared() {
  sed -n '/^finish-args:/,$p' "$1" | grep -v \
    -e '^  # Debug output$' \
    -e '^  - --env=' \
    -e '^      - -Dprofile=' \
    -e '^      - --buildtype=' \
    -e '^      # flatpak-builder passes no build type'
}

shared "$devel" >"$tmp/devel"
shared "$release" >"$tmp/release"
if ! diff -u "$tmp/devel" "$tmp/release"; then
  echo "The release manifest's permissions or modules differ from the development manifest's." >&2
  exit 1
fi

# Both build the app optimised: Meson's default build type, debug, builds cargo's unoptimised
# dev profile (crates/meson.build), which ships a slow app (#774).
for manifest in "$devel" "$release"; do
  if ! grep -Eq '^      - --buildtype=(release|debugoptimized)$' "$manifest"; then
    echo "$(basename "$manifest") builds the app unoptimised: give its app module --buildtype=release or debugoptimized." >&2
    exit 1
  fi
done
