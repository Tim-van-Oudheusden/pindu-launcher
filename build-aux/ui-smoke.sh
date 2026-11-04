#!/bin/sh
# UI smoke test: the installed app starts on the display and is still running a few
# seconds later, without a panic or a GLib critical (G_DEBUG=fatal-criticals turns those
# into crashes). It needs a display and the installed resources, so outside a Flatpak
# build, where tests run after the install, it is skipped (exit 77).
#
# Usage: ui-smoke.sh APP RESOURCES
app=$1
resources=$2

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
kill "$pid"
wait "$session" 2>/dev/null
cat "$tmp/log"
if grep -q "panicked" "$tmp/log"; then
  echo "The app panicked"
  exit 1
fi
echo "The app started and kept running"
