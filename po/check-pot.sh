#!/bin/sh
# Fails when po/pindu-launcher.pot misses strings the code has, or keeps strings it no
# longer has, so translators on Weblate always see the current strings. Source locations
# and the creation date are ignored, so moving code does not need a new template.
# Run from the source root with a configured build folder: po/check-pot.sh _build
set -eu
build=${1:-_build}
work=$(mktemp -d)
trap 'cp "$work/committed.pot" po/pindu-launcher.pot; rm -rf "$work"' EXIT
cp po/pindu-launcher.pot "$work/committed.pot"
meson compile -C "$build" pindu-launcher-pot >/dev/null 2>&1
for pot in committed current; do
  file="$work/committed.pot"
  [ "$pot" = current ] && file=po/pindu-launcher.pot
  msgcat --no-location --sort-output "$file" | grep -v '^"POT-Creation-Date:' > "$work/$pot"
done
if ! diff -u "$work/committed" "$work/current"; then
  echo "po/pindu-launcher.pot is out of date: run 'meson compile -C $build pindu-launcher-pot' and commit it"
  exit 1
fi
