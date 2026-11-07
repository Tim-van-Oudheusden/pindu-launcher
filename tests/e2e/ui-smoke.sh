#!/bin/sh
# UI smoke test: the installed app starts on the display and is still running a few
# seconds later, without a panic or a GLib critical (G_DEBUG=fatal-criticals turns those
# into crashes). It needs a display and the installed resources, so outside a Flatpak
# build, where tests run after the install, it is skipped (exit 77).
#
# Given a COVER image, the library also holds one game with that cover (a Faugus game,
# the simplest library to write), and the app must decode it through glycin: the grid
# stores a resized copy in the art cache only after the decode succeeds. The image must
# be bigger than a tile.
#
# Usage: tests/e2e/ui-smoke.sh APP RESOURCES [COVER]
app=$1
resources=$2
cover=${3-}

if [ -z "${DISPLAY}${WAYLAND_DISPLAY}" ]; then
  echo "No display; skipping"
  exit 77
fi
if [ ! -f "$resources" ]; then
  echo "Resources not installed at $resources; skipping"
  exit 77
fi

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
# A fresh, empty library and settings, nothing of the user's.
export HOME="$tmp" XDG_DATA_HOME="$tmp/data" XDG_CONFIG_HOME="$tmp/config" \
  XDG_CACHE_HOME="$tmp/cache" GSETTINGS_BACKEND=memory G_DEBUG=fatal-criticals GTK_A11Y=none

if [ -n "$cover" ]; then
  mkdir -p "$XDG_DATA_HOME/faugus-launcher/covers"
  cp "$cover" "$XDG_DATA_HOME/faugus-launcher/covers/smoke.png"
  echo '[{"gameid": "smoke", "title": "Smoke Test", "runner": "Linux-Native"}]' \
    > "$XDG_DATA_HOME/faugus-launcher/games.json"
  # Logs why a cover could not be decoded.
  export G_MESSAGES_DEBUG=pindu
fi

# The app is a GApplication and needs a session bus of its own, so a running copy of
# the app does not take over.
dbus-run-session -- sh -c '"$1" & echo $! > "$2/pid"; wait $!' sh "$app" "$tmp" \
  > "$tmp/log" 2>&1 &
session=$!

sleep 5
pid=$(cat "$tmp/pid" 2>/dev/null)
if [ -z "$pid" ] || ! kill -0 "$pid" 2>/dev/null; then
  wait "$session"
  echo "The app did not stay up (exit status $?):"
  cat "$tmp/log"
  exit 1
fi
decoded=
if [ -n "$cover" ]; then
  # The first decode starts glycin's loader, which can take a while on a cold start.
  for _ in 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15; do
    set -- "$XDG_CACHE_HOME"/pindu-launcher/art/covers/*/*.png
    if [ -f "$1" ]; then
      decoded=yes
      break
    fi
    kill -0 "$pid" 2>/dev/null || break
    sleep 1
  done
fi
kill "$pid"
wait "$session" 2>/dev/null
cat "$tmp/log"
if grep -q "panicked" "$tmp/log"; then
  echo "The app panicked"
  exit 1
fi
if [ -n "$cover" ] && [ -z "$decoded" ]; then
  echo "The app did not decode the cover through glycin"
  exit 1
fi
echo "The app started and kept running"
