#!/bin/sh
# Self-tune the unit-test coverage floors from observed coverage.
#
# Usage:
#   scripts/auto-qa-tuner.sh [--config <path>] [--report <path>] [--write]
#
# Reads the floors from the tuning config (default .github/auto-qa-tuning.json) and the observed
# coverage from a scripts/quality-report.sh report (default quality-report.md), then prints a
# Markdown table proposing, per module and metric, the floor raised to the observed coverage minus
# the config's margin, rounded down to a whole percent. Floors are never lowered, and a metric
# without an observed percentage (a failed run's n/a, or nothing to measure) keeps its floor.
# Coverage exactly at floor + margin proposes no change. With --write the proposed floors are saved
# back to the config; the quality report workflow publishes that file for the maintainer to commit.
#
# Exit status: 0 on success, 2 for bad arguments, a missing report, or a malformed config.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
lib="$root/scripts/lib"

usage() {
  echo "usage: auto-qa-tuner.sh [--config <path>] [--report <path>] [--write]" >&2
  exit 2
}

config="$root/.github/auto-qa-tuning.json"
report=quality-report.md
write=0
while [ "$#" -gt 0 ]; do
  case "$1" in
    --config)
      [ "$#" -ge 2 ] || usage
      config="$2"
      shift 2
      ;;
    --report)
      [ "$#" -ge 2 ] || usage
      report="$2"
      shift 2
      ;;
    --write)
      write=1
      shift
      ;;
    *) usage ;;
  esac
done

if [ ! -f "$report" ]; then
  echo "No quality report at $report; run scripts/quality-report.sh > $report first" >&2
  exit 2
fi
if ! jq -e -L "$lib" 'include "coverage"; valid_config' "$config" >/dev/null 2>&1; then
  jq -nr -L "$lib" --arg path "$config" 'include "coverage"; config_error($path)' >&2
  exit 2
fi

# Coverage rows look like `| core | 97.39% | 96.06% | 96.42% |` (module, then the metrics in
# order). A cell without a percentage leaves that metric unobserved. Proposals are computed in
# hundredths and rounded before flooring, so float error (64.1 - 0.1 = 63.999...) cannot drop one
# below a whole percent; only the proposal is rounded down, never the current floor.
if ! tuned="$(jq -L "$lib" --rawfile report "$report" '
  include "coverage";
  def percent: capture("^(?<value>[0-9]+(\\.[0-9]+)?)%$").value? // null;
  ($report | split("\n")
    | map(capture("^\\| (?<module>\\S+) \\| (?<a>[^|]*\\S) \\| (?<b>[^|]*\\S) \\| (?<c>[^|]*\\S) \\|$")
      | {key: .module, value: ([.a, .b, .c] | map(percent) as $p | [metrics, $p] | transpose
        | map({key: .[0], value: .[1]}) | from_entries)})
    | from_entries) as $observed
  | .margin as $margin
  | [.floors | to_entries[] | .key as $module | .value as $floors | metrics[] as $metric
      | $floors[$metric] as $floor
      | $observed[$module][$metric] as $seen
      | {
          module: $module,
          metric: $metric,
          floor: $floor,
          observed: $seen,
          proposed: (if $seen == null then $floor
            else [$floor, (((($seen | tonumber) - $margin) * 100 | round) / 100 | floor)] | max end)
        }] as $rows
  | {rows: $rows, config: (reduce $rows[] as $r (.; .floors[$r.module][$r.metric] = $r.proposed))}
' "$config")"; then
  echo "Could not tune $config from $report" >&2
  exit 2
fi

printf '%s\n' "$tuned" | jq -r '
  "## Coverage floor tuning",
  "",
  "Margin: \(.config.margin) percentage points below observed coverage.",
  "",
  "| Module | Metric | Floor | Observed | Proposed |",
  "| ------ | ------ | ----- | -------- | -------- |",
  (.rows[] | "| \(.module) | \(.metric) | \(.floor)% | \(if .observed == null then "n/a" else "\(.observed)%" end)"
    + " | \(.proposed)%\(if .proposed > .floor then " ⬆️" else "" end) |")
'

if [ "$write" -eq 1 ]; then
  # Keys and their order are kept; only the floors change.
  if ! printf '%s\n' "$tuned" | jq '.config' >"$config.tmp" || ! mv "$config.tmp" "$config"; then
    rm -f "$config.tmp"
    echo "Could not write $config" >&2
    exit 2
  fi
  echo "Wrote the proposed floors to $config" >&2
fi
