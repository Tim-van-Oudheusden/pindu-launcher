#!/bin/sh
# Decides whether a pull request needs CI's Flatpak build: reads the paths it changes, one per
# line (`git diff --name-only`), and prints `build=false` when every one is documentation,
# Markdown, a translation, or agent prompts and settings, else `build=true`. Nothing read means
# `build=true`. The `Rust and Meson checks` job still compiles and checks the translations.
# Usage: git diff --name-only BASE...HEAD | scripts/needs-build.sh
set -eu

changed=0
while IFS= read -r path; do
  [ -n "$path" ] || continue
  changed=1
  case "$path" in
    docs/* | *.md | po/*.po | prompts/* | .claude/*) ;;
    *)
      echo build=true
      exit 0
      ;;
  esac
done
if [ "$changed" = 1 ]; then
  echo build=false
else
  echo build=true
fi
