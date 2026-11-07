#!/bin/sh
# Tests for auto-qa-tuner.sh: the floors it proposes from a quality report's coverage rows (raised
# to observed minus margin, rounded down, never lowered), what --write saves, and its errors.
# Usage: scripts/auto-qa-tuner.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
tuner="$root/scripts/auto-qa-tuner.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
nl='
'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# config <json>: writes the tuning config the cases share, $tmp/tuning.json.
config() {
  printf '%s\n' "$1" >"$tmp/tuning.json"
}

# quality <rows>: writes a quality report whose coverage table holds <rows>, prints its path.
quality() {
  printf '# Quality report\n\n## Unit-test coverage\n\n| Module | Lines | Regions | Functions |\n| ------ | ----- | ------- | --------- |\n%s\n' \
    "$1" >"$tmp/quality.md"
  echo "$tmp/quality.md"
}

# tune <expected status> <description> <expected output lines, one per line, or ''> <tuner arguments...>
tune() {
  want="$1"
  description="$2"
  lines="$3"
  shift 3
  got=0
  output="$(sh "$tuner" --config "$tmp/tuning.json" "$@" 2>&1)" || got=$?
  if [ "$got" != "$want" ]; then
    not_ok "$description (expected exit $want, got $got): $output"
    return
  fi
  missing="$(printf '%s\n' "$lines" | while IFS= read -r line; do
    [ -z "$line" ] || printf '%s\n' "$output" | grep -qxF -- "$line" || printf '%s\n' "$line"
  done)"
  if [ -n "$missing" ]; then
    not_ok "$description (output lacks:$nl$missing${nl}got:$nl$output)"
  else
    ok "$description"
  fi
}

# floors_are <description> <expected compact JSON of the whole config>
floors_are() {
  got="$(jq -c . "$tmp/tuning.json")"
  if [ "$got" = "$2" ]; then
    ok "$1"
  else
    not_ok "$1 (expected $2, got $got)"
  fi
}

base='{"margin":2,"floors":{"core":{"lines":95,"regions":94,"functions":94}}}'

config "$base"
tune 0 'coverage at floor plus margin proposes no change' "Margin: 2 percentage points below observed coverage.
| core | lines | 95% | 97% | 95% |
| core | regions | 94% | 96% | 94% |
| core | functions | 94% | 96% | 94% |" --report "$(quality '| core | 97% | 96% | 96% |')"
floors_are 'without --write the config is left alone' "$base"

tune 0 'raises floors to observed minus margin, rounded down' '| core | lines | 95% | 98.99% | 96% ⬆️ |
| core | regions | 94% | 96.06% | 94% |
| core | functions | 94% | 97.5% | 95% ⬆️ |' --report "$(quality '| core | 98.99% | 96.06% | 97.5% |')"

tune 0 'never lowers a floor' '| core | lines | 95% | 90% | 95% |' --report "$(quality '| core | 90% | 96% | 96% |')"

config '{"margin":0.1,"floors":{"core":{"lines":60,"regions":60,"functions":60}}}'
tune 0 'float error does not drop a proposal below a whole percent' '| core | lines | 60% | 64.1% | 64% ⬆️ |' \
  --report "$(quality '| core | 64.1% | 60% | 60% |')"

config '{"margin":2,"floors":{"core":{"lines":79.99,"regions":60,"functions":60}}}'
tune 0 'a fractional floor is kept, not rounded down' '| core | lines | 79.99% | 80% | 79.99% |' \
  --report "$(quality '| core | 80% | 60% | 60% |')"

config "$base"
tune 0 'metrics without a percentage keep their floors' '| core | lines | 95% | n/a | 95% |
| core | regions | 94% | n/a | 94% |
| core | functions | 94% | n/a | 94% |' --report "$(quality '| core | ❌ n/a | none | not measured |')"

tune 0 'a module missing from the report keeps its floors' '| core | lines | 95% | n/a | 95% |' \
  --report "$(quality '| ui | 99% | 99% | 99% |')"

config '{"margin":2,"floors":{"core":{"lines":95,"regions":94,"functions":94}},"note":"kept"}'
tune 0 '--write saves the proposed floors' 'Wrote the proposed floors to '"$tmp/tuning.json" \
  --report "$(quality '| core | 99.5% | 96% | 98% |')" --write
floors_are '--write keeps every other key' '{"margin":2,"floors":{"core":{"lines":97,"regions":94,"functions":96}},"note":"kept"}'

config "$base"
tune 2 'missing report' "No quality report at $tmp/none.md; run scripts/quality-report.sh > $tmp/none.md first" \
  --report "$tmp/none.md" --write
floors_are 'a missing report leaves the config' "$base"

config '{"margin":-1,"floors":{"core":{"lines":95,"regions":94,"functions":94}}}'
tune 2 'malformed config' "Malformed $tmp/tuning.json: expected {\"margin\": N, \"floors\": {\"<module>\": {\"lines\": N, \"regions\": N, \"functions\": N}}} with percentages between 0 and 100" \
  --report "$(quality '| core | 99% | 99% | 99% |')" --write
floors_are 'a malformed config is left untouched' '{"margin":-1,"floors":{"core":{"lines":95,"regions":94,"functions":94}}}'

config "$base"
for args in '--report' '--config' '--bogus' 'extra'; do
  # shellcheck disable=SC2086 # each case is a list of arguments
  tune 2 "usage error: '$args'" 'usage: auto-qa-tuner.sh [--config <path>] [--report <path>] [--write]' $args
done

# The shipped config is one the tuner accepts.
got=0
output="$(sh "$tuner" --config "$root/.github/auto-qa-tuning.json" --report "$(quality '| core | 1% | 1% | 1% |')" 2>&1)" ||
  got=$?
if [ "$got" -eq 0 ]; then
  ok 'the shipped config is valid'
else
  not_ok "the shipped config is valid (exit $got): $output"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
