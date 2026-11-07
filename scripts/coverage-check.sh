#!/bin/sh
# Enforce the unit-test coverage floors.
#
# Usage:
#   scripts/coverage-check.sh <llvm-cov.json> <tuning.json>
#
# Reads the JSON export that `cargo llvm-cov --json` writes and the floors from the tuning config
# (.github/auto-qa-tuning.json: {"margin": N, "floors": {"<module>": {"lines": N, "regions": N,
# "functions": N}}}), and fails if any module's line, region or function coverage is below its
# floor, in percent. Module <module> is the crate crates/pindu-<module>; its counts are summed over
# the report's files under that directory. Branch coverage needs a nightly compiler, so it is not
# gated. A module with no regions or functions is fully covered there. The margin is only read by
# the floor tuner.
#
# Exit status: 0 when every module is at or above its floors, 1 when one is below a floor, 2 for
# bad arguments, a missing or malformed config or report, or a module with no measured lines.
set -eu

metrics='lines regions functions'

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

# A floor is a percentage; the margin is a non-negative number of percentage points.
valid='
  def percent: type == "number" and . >= 0 and . <= 100;
  (.margin | type == "number" and . >= 0)
  and (.floors | type == "object" and length > 0
    and all(keys[]; test("^[a-z0-9][a-z0-9-]*$"))
    and all(.[]; type == "object" and keys == ["functions", "lines", "regions"]
      and (.lines | percent) and (.regions | percent) and (.functions | percent)))
'
if ! jq -e "$valid" "$config" >/dev/null 2>&1; then
  echo "Malformed $config: expected {\"margin\": N, \"floors\": {\"<module>\": {\"lines\": N, \"regions\": N," \
    "\"functions\": N}}} with percentages between 0 and 100" >&2
  exit 2
fi

if [ ! -f "$report" ]; then
  echo "No coverage report at $report; run cargo llvm-cov --json first" >&2
  exit 2
fi
shape='
  def counts: type == "object" and (.covered | type == "number") and (.count | type == "number");
  .data | type == "array" and length > 0
    and all(.[]; .files | type == "array"
      and all(.[]; (.filename | type == "string")
        and (.summary | (.lines | counts) and (.regions | counts) and (.functions | counts))))
'
if ! jq -e "$shape" "$report" >/dev/null 2>&1; then
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
  counters="$(jq -r --arg dir "$dir/" --arg metrics "$metrics" '
    [.data[].files[] | select((.filename | startswith($dir)) or (.filename | contains("/" + $dir)))] as $files
    | [$metrics | splits(" ") as $m | ([$files[].summary[$m].covered] | add // 0), ([$files[].summary[$m].count] | add // 0)]
    | map(tostring) | join(" ")
  ' "$report")"
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
