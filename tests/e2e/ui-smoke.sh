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
# With --fullscreen APP_ID the app starts in fullscreen mode (`--fullscreen`) with the
# navigation sounds on, so Home shows its animated background, and plays one navigation
# sound (win.preview-navigation-sound, sent over the app's D-Bus session with gdbus).
# GStreamer plays both through GTK, so a missing GStreamer plug-in shows here: the test
# fails when the log says the background or a sound could not play.
#
# With --narrow APP_ID the desktop window starts at its narrowest, 360 px wide, and the
# library page must fit it: libadwaita logs "… exceeds PinduWindow width" when the page
# asks for more width than the window has.
#
# Usage: tests/e2e/ui-smoke.sh [--fullscreen APP_ID | --narrow APP_ID] APP RESOURCES [COVER]
app_id=
narrow=
if [ "${1-}" = --fullscreen ]; then
  app_id=$2
  shift 2
elif [ "${1-}" = --narrow ]; then
  narrow=$2
  shift 2
fi
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

app_args=
if [ -n "$app_id" ]; then
  app_args=--fullscreen
  # Settings in a key file instead of memory, so the sounds are on from the start.
  export GSETTINGS_BACKEND=keyfile
  mkdir -p "$XDG_CONFIG_HOME/glib-2.0/settings"
  printf '[%s]\nnavigation-sounds=true\n' "$(echo "$app_id" | tr . /)" \
    > "$XDG_CONFIG_HOME/glib-2.0/settings/keyfile"
elif [ -n "$narrow" ]; then
  # The window's minimum width (`width-request` in window.blp), remembered as its size.
  export GSETTINGS_BACKEND=keyfile
  mkdir -p "$XDG_CONFIG_HOME/glib-2.0/settings"
  printf '[%s]\nwindow-width=360\nwindow-height=640\n' "$(echo "$narrow" | tr . /)" \
    > "$XDG_CONFIG_HOME/glib-2.0/settings/keyfile"
  # GNOME's window buttons, a close button only. Without a desktop GTK adds minimize and
  # maximize, which take more of the header bar than the 360 px a GNOME app plans for.
  mkdir -p "$XDG_CONFIG_HOME/gtk-4.0"
  printf '[Settings]\ngtk-decoration-layout=appmenu:close\n' \
    > "$XDG_CONFIG_HOME/gtk-4.0/settings.ini"
fi

# The app is a GApplication and needs a session bus of its own, so a running copy of
# the app does not take over.
dbus-run-session -- sh -c \
  'echo "$DBUS_SESSION_BUS_ADDRESS" > "$2/bus"; "$1" $3 & echo $! > "$2/pid"; wait $!' \
  sh "$app" "$tmp" "$app_args" > "$tmp/log" 2>&1 &
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
fullscreen_state=
sound_asked=
if [ -n "$app_id" ]; then
  # The window's actions, on the app's own session bus: whether the window is in
  # fullscreen mode, where Home plays its background, and a navigation sound.
  bus=$(cat "$tmp/bus")
  window=/$(echo "$app_id" | tr .- /_)/window/1
  fullscreen_state=$(DBUS_SESSION_BUS_ADDRESS=$bus gdbus call --session --dest "$app_id" \
    --object-path "$window" --method org.gtk.Actions.Describe fullscreen-mode)
  if DBUS_SESSION_BUS_ADDRESS=$bus gdbus call --session --dest "$app_id" \
    --object-path "$window" --method org.gtk.Actions.Activate preview-navigation-sound \
    '[]' '{}' > /dev/null; then
    sound_asked=yes
  fi
  # The sound starts 200 ms after the request (VOLUME_PREVIEW_DELAY) and lasts 60 ms.
  sleep 2
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
if [ -n "$app_id" ]; then
  case $fullscreen_state in
    *'[<true>]'*) ;;
    *)
      echo "The window was not in fullscreen mode (win.fullscreen-mode: ${fullscreen_state:-unknown})"
      exit 1
      ;;
  esac
  if [ -z "$sound_asked" ]; then
    echo "The app did not take win.preview-navigation-sound"
    exit 1
  fi
fi
if [ -n "$narrow" ] && grep -E -q "exceeds [A-Za-z]+ (width|size)" "$tmp/log"; then
  echo "The window does not fit its 360 px minimum width"
  exit 1
fi
# GStreamer plays Home's background and the navigation sounds; a missing plug-in shows up
# as one of these.
if grep -q -e "Could not play Home's background" -e "Could not play a navigation sound" \
  "$tmp/log"; then
  echo "GStreamer could not play Home's background or a navigation sound: is a plug-in missing?"
  exit 1
fi
echo "The app started and kept running"
