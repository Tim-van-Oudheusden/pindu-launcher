#!/bin/sh
# Tests for coverage-check.sh: runs the gate against small cargo-llvm-cov JSON reports and tuning
# configs and checks each exit status (0 pass, 1 below a floor, 2 usage, config or report error)
# and the message that explains it, then that the shipped .github/auto-qa-tuning.json is valid.
# Usage: scripts/coverage-check.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
check="$root/scripts/coverage-check.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# file_entry <path> <lines covered> <lines count> <regions covered> <regions count>
#   <functions covered> <functions count>: one file of a cargo-llvm-cov JSON export.
file_entry() {
  printf '{"filename": "%s", "summary": {"lines": {"covered": %s, "count": %s},' "$1" "$2" "$3"
  printf ' "regions": {"covered": %s, "count": %s}, "functions": {"covered": %s, "count": %s}}}' "$4" "$5" "$6" "$7"
}

# report <name> <file entries, comma-separated>: writes a cargo-llvm-cov JSON export, prints its path.
report() {
  printf '{"type": "llvm.coverage.json.export", "version": "3.0.1", "data": [{"files": [%s]}]}\n' "$2" \
    >"$tmp/$1.json"
  echo "$tmp/$1.json"
}

# floors <name> <json>: writes a tuning config, prints its path.
floors() {
  printf '%s\n' "$2" >"$tmp/$1-floors.json"
  echo "$tmp/$1-floors.json"
}

# expect <status> <description> <expected output line, or ''> <coverage-check.sh arguments...>
expect() {
  want="$1"
  description="$2"
  pattern="$3"
  shift 3
  got=0
  output="$(sh "$check" "$@" 2>&1)" || got=$?
  if [ "$got" != "$want" ]; then
    not_ok "$description (expected exit $want, got $got): $output"
  elif [ -n "$pattern" ] && ! printf '%s\n' "$output" | grep -qxF -- "$pattern"; then
    not_ok "$description (output lacks the line '$pattern'): $output"
  else
    ok "$description"
  fi
}

core=/build/crates/pindu-core/src
# core: lines 80/100, regions 150/200, functions 9/10 over two files; one pindu-ui file the gate ignores.
pass="$(report pass "$(file_entry "$core/lib.rs" 30 40 50 80 4 5),$(file_entry "$core/steam.rs" 50 60 100 120 5 5),$(file_entry /build/crates/pindu-ui/src/main.rs 0 50 0 70 0 9)")"
config="$(floors default '{"margin": 2, "floors": {"core": {"lines": 70, "regions": 70, "functions": 80}}}')"

# Pass.
expect 0 'coverage above every floor' \
  'Coverage (core): lines 80.00% (floor 70%), regions 75.00% (floor 70%), functions 90.00% (floor 80%)' \
  "$pass" "$config"
expect 0 'coverage exactly at a floor' '' "$pass" \
  "$(floors exact '{"margin": 2, "floors": {"core": {"lines": 80, "regions": 75, "functions": 90}}}')"
expect 0 'fractional floor met' '' "$pass" \
  "$(floors fractional '{"margin": 2, "floors": {"core": {"lines": 79.99, "regions": 75, "functions": 90}}}')"
expect 0 'a module with no regions or functions is fully covered there' \
  'Coverage (core): lines 100.00% (floor 90%), regions 100.00% (floor 90%), functions 100.00% (floor 90%)' \
  "$(report empty-fns "$(file_entry "$core/lib.rs" 3 3 0 0 0 0)")" \
  "$(floors high '{"margin": 2, "floors": {"core": {"lines": 90, "regions": 90, "functions": 90}}}')"

# Below a floor: every metric below its floor is named, not only the first.
expect 1 'lines below the floor' '::error::core lines coverage 80.00% is below its floor of 81%' "$pass" \
  "$(floors lines '{"margin": 2, "floors": {"core": {"lines": 81, "regions": 70, "functions": 80}}}')"
expect 1 'regions below the floor' '::error::core regions coverage 75.00% is below its floor of 76%' "$pass" \
  "$(floors regions '{"margin": 2, "floors": {"core": {"lines": 70, "regions": 76, "functions": 80}}}')"
expect 1 'functions below the floor' '::error::core functions coverage 90.00% is below its floor of 95%' "$pass" \
  "$(floors functions '{"margin": 2, "floors": {"core": {"lines": 81, "regions": 70, "functions": 95}}}')"
expect 1 'a rounded 100.00% is still below a 100% floor' \
  '::error::core lines coverage 100.00% is below its floor of 100%' \
  "$(report almost "$(file_entry "$core/lib.rs" 99999 100000 1 1 1 1)")" \
  "$(floors full '{"margin": 0, "floors": {"core": {"lines": 100, "regions": 0, "functions": 0}}}')"
expect 1 'just above the coverage is below' '::error::core lines coverage 80.00% is below its floor of 80.01%' \
  "$pass" "$(floors strict '{"margin": 2, "floors": {"core": {"lines": 80.01, "regions": 0, "functions": 0}}}')"

# Report errors.
expect 2 'missing report' "No coverage report at $tmp/missing.json; run cargo llvm-cov --json first" \
  "$tmp/missing.json" "$config"
printf '{"data": [' >"$tmp/truncated.json"
expect 2 'truncated report' "$tmp/truncated.json is not a cargo-llvm-cov JSON export" "$tmp/truncated.json" "$config"
expect 2 'summary without files' "$tmp/summary.json is not a cargo-llvm-cov JSON export" \
  "$(printf '{"data": [{"totals": {}}]}' >"$tmp/summary.json" && echo "$tmp/summary.json")" "$config"
expect 2 'module not measured' \
  "core (crates/pindu-core) has no measured lines in $tmp/ui-only.json; was it left out of cargo llvm-cov?" \
  "$(report ui-only "$(file_entry /build/crates/pindu-ui/src/main.rs 1 2 1 2 1 2)")" "$config"
expect 2 'a file without counts' "$tmp/no-counts.json is not a cargo-llvm-cov JSON export" \
  "$(report no-counts "{\"filename\": \"$core/lib.rs\", \"summary\": {}}")" "$config"

# Usage and config errors.
expect 2 'no arguments' 'usage: coverage-check.sh <llvm-cov.json> <tuning.json>'
expect 2 'one argument' 'usage: coverage-check.sh <llvm-cov.json> <tuning.json>' "$pass"
expect 2 'missing config' "No tuning config at $tmp/none.json" "$pass" "$tmp/none.json"
malformed='Malformed %s: expected {"margin": N, "floors": {"<module>": {"lines": N, "regions": N, "functions": N}}} with percentages between 0 and 100'
for case in 'not-json|margin: 2' \
  'over|{"margin": 2, "floors": {"core": {"lines": 120, "regions": 70, "functions": 80}}}' \
  'under|{"margin": 2, "floors": {"core": {"lines": 70, "regions": -1, "functions": 80}}}' \
  'no-functions|{"margin": 2, "floors": {"core": {"lines": 70, "regions": 70}}}' \
  'branches|{"margin": 2, "floors": {"core": {"lines": 70, "branches": 70, "functions": 80}}}' \
  'margin|{"margin": -1, "floors": {"core": {"lines": 70, "regions": 70, "functions": 80}}}' \
  'margin-text|{"margin": "2", "floors": {"core": {"lines": 70, "regions": 70, "functions": 80}}}' \
  'list|{"margin": 2, "floors": [{"lines": 70, "regions": 70, "functions": 80}]}' \
  'none|{"margin": 2, "floors": {}}' \
  'bad-module|{"margin": 2, "floors": {"Core UI": {"lines": 70, "regions": 70, "functions": 80}}}'; do
  name="${case%%|*}"
  # shellcheck disable=SC2059 # the message is the format
  expect 2 "malformed config: $name" "$(printf "$malformed" "$tmp/$name-floors.json")" "$pass" \
    "$(floors "$name" "${case#*|}")"
done

# The shipped config is valid, names only crates that exist, and its floors hold for a fully covered core.
shipped="$root/.github/auto-qa-tuning.json"
expect 0 'shipped config is valid' '' \
  "$(report full "$(file_entry "$root/crates/pindu-core/src/lib.rs" 1 1 1 1 1 1)")" "$shipped"
for module in $(jq -r '.floors | keys[]' "$shipped"); do
  if [ -d "$root/crates/pindu-$module" ]; then
    ok "shipped module $module is a crate"
  else
    not_ok "shipped module $module has no crates/pindu-$module"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
