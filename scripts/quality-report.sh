#!/bin/sh
# Print a Markdown quality report for the repository to stdout.
#
# Usage (in the GNOME 51 SDK, with cargo-llvm-cov on PATH):
#   scripts/quality-report.sh > quality-report.md
#
# Runs each Rust gate of AGENTS.md separately (rustfmt, clippy, rustdoc, the unit tests), then one
# coverage run of the modules in .github/auto-qa-tuning.json, `cargo llvm-cov --package
# pindu-<module> --json --summary-only`, as the coverage gate measures them, and the floor check
# (scripts/coverage-check.sh). It reports pass or fail per gate and the line, region and function
# coverage per module. scripts/auto-qa-tuner.sh reads those coverage rows. Tool output goes to
# stderr, so stdout stays a clean report. Every gate runs even when an earlier one fails. A failed
# coverage run, one that leaves no report, or a module it did not measure reports n/a; a metric with
# nothing to measure reports "none", which is not a failure.
#
# Environment: $QA_TUNING_CONFIG replaces the floors file, $COVERAGE_JSON the path the coverage
# report is written to (default coverage.json at the repository root); $GITHUB_SHA names the commit.
#
# Exit status: 0 when everything passed, 1 when a gate or the coverage run failed, 2 when the
# floors file lists no module.
# shellcheck disable=SC2016 # backticks in single quotes are Markdown code spans, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
lib="$root/scripts/lib"
cd "$root"

config="${QA_TUNING_CONFIG:-$root/.github/auto-qa-tuning.json}"
report="${COVERAGE_JSON:-$root/coverage.json}"

if ! modules="$(jq -er '.floors | keys[]' "$config" 2>/dev/null)"; then
  echo "No modules in $config: expected {\"floors\": {\"<module>\": {...}}}" >&2
  exit 2
fi

commit="${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}"
failed=0

# gate <label> <command...>: runs one gate and prints its row.
gate() {
  label="$1"
  shift
  if "$@" >&2; then
    result='✅ pass'
  else
    result='❌ fail'
    failed=1
  fi
  echo "| $label | $result |"
}

packages=''
for module in $modules; do
  packages="$packages --package pindu-$module"
done

echo '# Quality report'
echo
echo "Commit: \`$commit\`"
echo
echo '## Quality gates'
echo
echo '| Gate | Result |'
echo '| ---- | ------ |'
gate '`cargo fmt --all --check`' cargo fmt --all --check
gate '`cargo clippy --workspace --all-targets -- -D warnings`' cargo clippy --workspace --all-targets -- -D warnings
gate '`RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps`' \
  env RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps
gate '`cargo test --workspace`' cargo test --workspace
# A report left by an earlier run must not stand in for this run's.
rm -f "$report"
# shellcheck disable=SC2086 # one --package option per module
gate "\`cargo llvm-cov$packages\`" cargo llvm-cov $packages --json --summary-only --output-path "$report"
measured=0
# The coverage run must pass: a report a failing run left behind is partial.
if [ "$result" = '✅ pass' ] && [ -f "$report" ] &&
  jq -e -L "$lib" 'include "coverage"; valid_report' "$report" >/dev/null 2>&1; then
  measured=1
fi
gate '`scripts/coverage-check.sh` (floors in `.github/auto-qa-tuning.json`)' \
  sh "$root/scripts/coverage-check.sh" "$report" "$config"

echo
echo '## Unit-test coverage'
echo
echo '| Module | Lines | Regions | Functions |'
echo '| ------ | ----- | ------- | --------- |'
for module in $modules; do
  row=''
  if [ "$measured" -eq 1 ]; then
    # Cells in metric order: a percentage, or "none" when there is nothing to measure; empty when the
    # module has no measured lines at all.
    row="$(jq -r -L "$lib" --arg m "$module" '
      include "coverage";
      module_counts($m) as $c
      | if $c.lines.count == 0 then ""
        else [metrics[] | $c[.] | if .count == 0 then "none" else "\(.covered)/\(.count)" end]
          | join(" ") end
    ' "$report")"
  fi
  # A failed or partial run makes its numbers meaningless: n/a rather than a partial measurement.
  if [ -z "$row" ]; then
    echo "| $module | ❌ n/a | ❌ n/a | ❌ n/a |"
    failed=1
    continue
  fi
  cells=''
  for value in $row; do
    case "$value" in
      none) cells="$cells none |" ;;
      *) cells="$cells $(awk -v c="${value%/*}" -v n="${value#*/}" 'BEGIN { printf "%.2f", c * 100 / n }')% |" ;;
    esac
  done
  echo "| $module |$cells"
done

exit "$failed"
