#!/bin/sh
# Enforce the unit-test coverage floors.
#
# Usage:
#   scripts/coverage-check.sh <llvm-cov.json> <tuning.json>
#
# Reads the JSON export that `cargo llvm-cov --json` writes and the floors from the tuning config
# (.github/auto-qa-tuning.json), and fails if any module's line, region or function coverage is
# below its floor, in percent. The config's contract and how a module's counts are summed live in
# scripts/lib/coverage.jq. A module with no regions or functions is fully covered there. The margin
# is only read by the floor tuner.
#
# Exit status: 0 when every module is at or above its floors, 1 when one is below a floor, 2 for
# bad arguments, a missing or malformed config or report, or a module with no measured lines.
set -eu

lib="$(cd "$(dirname "$0")" && pwd)/lib"

if [ "$#" -ne 2 ]; then
  echo "usage: coverage-check.sh <llvm-cov.json> <tuning.json>" >&2
  exit 2
fi
report="$1"
config="$2"

if ! command -v jq >/dev/null; then
  echo "coverage-check.sh needs jq" >&2
  exit 2
fi
if [ ! -f "$config" ]; then
  echo "No tuning config at $config" >&2
  exit 2
fi

# metrics, valid_config, valid_report and module_counts live in scripts/lib/coverage.jq.
metrics="$(jq -nr -L "$lib" 'include "coverage"; metrics | join(" ")')"
if ! jq -e -L "$lib" 'include "coverage"; valid_config' "$config" >/dev/null 2>&1; then
  jq -nr -L "$lib" --arg path "$config" 'include "coverage"; config_error($path)' >&2
  exit 2
fi

if [ ! -f "$report" ]; then
  echo "No coverage report at $report; run cargo llvm-cov --json first" >&2
  exit 2
fi
if ! jq -e -L "$lib" 'include "coverage"; valid_report' "$report" >/dev/null 2>&1; then
  echo "$report is not a cargo-llvm-cov JSON export" >&2
  exit 2
fi

# meets <covered> <count> <floor>: compares the exact ratio, so a rounded 100.00% is still below 100%.
meets() {
  awk -v c="$1" -v n="$2" -v f="$3" 'BEGIN { exit !(n == 0 || c * 100 >= f * n) }'
}

# percent <covered> <count>
percent() {
  awk -v c="$1" -v n="$2" 'BEGIN { printf "%.2f", n == 0 ? 100 : c * 100 / n }'
}

failed=0
for module in $(jq -r '.floors | keys[]' "$config"); do
  dir="crates/pindu-$module"
  # covered and count per metric, summed over the module's files.
  counters="$(jq -r -L "$lib" --arg m "$module" \
    'include "coverage"; module_counts($m) as $c | [metrics[] | $c[.].covered, $c[.].count] | join(" ")' "$report")"
  # shellcheck disable=SC2086 # six numbers, split on purpose
  set -- $counters
  if [ "$2" -eq 0 ]; then
    echo "$module ($dir) has no measured lines in $report; was it left out of cargo llvm-cov?" >&2
    exit 2
  fi

  summary=''
  for metric in $metrics; do
    covered="$1"
    count="$2"
    shift 2
    floor="$(jq -r --arg m "$module" --arg metric "$metric" '.floors[$m][$metric]' "$config")"
    value="$(percent "$covered" "$count")"
    summary="$summary, $metric $value% (floor $floor%)"
    if ! meets "$covered" "$count" "$floor"; then
      echo "::error::$module $metric coverage $value% is below its floor of $floor%" >&2
      failed=1
    fi
  done
  echo "Coverage ($module): ${summary#, }"
done
exit "$failed"
