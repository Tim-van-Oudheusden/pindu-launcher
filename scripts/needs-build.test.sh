#!/bin/sh
# Tests for needs-build.sh: which changed paths let a pull request skip the Flatpak build.
# Usage: scripts/needs-build.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/needs-build.sh"
failures=0
nl='
'

# expect <description> <changed paths, one per line> <expected stdout>
expect() {
  status=0
  output="$(printf '%s' "$2" | sh "$script" 2>&1)" || status=$?
  if [ "$status" -ne 0 ]; then
    echo "FAIL: $1 (exit $status): $output"
    failures=$((failures + 1))
  elif [ "$output" != "$3" ]; then
    echo "FAIL: $1 (expected: $3, got: $output)"
    failures=$((failures + 1))
  else
    echo "ok: $1"
  fi
}

expect 'documentation alone needs no build' "docs/releasing.md${nl}docs/screenshots/home.webp" build=false
expect 'Markdown anywhere needs no build' "README.md${nl}data/screenshots/CREDITS.md" build=false
expect 'translations alone need no build' "po/nl.po${nl}po/de.po" build=false
expect 'agent prompts and settings need no build' "prompts/code-review.md${nl}.claude/settings.json" build=false
expect 'a source file needs a build' 'crates/pindu-core/src/lib.rs' build=true
expect 'one source file among docs needs a build' "docs/a.md${nl}data/style.css${nl}po/nl.po" build=true
# LINGUAS picks the languages the build installs; the template is checked against the sources.
expect 'the language list needs a build' 'po/LINGUAS' build=true
expect 'the translation template needs a build' 'po/pindu-launcher.pot' build=true
expect 'a Markdown-looking directory needs a build' 'docs.md/meson.build' build=true
# Nothing to go on: build rather than skip a check.
expect 'no changed paths needs a build' '' build=true

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
