#!/bin/sh
# Tests for quality-report.sh: runs it with a stand-in cargo on PATH and checks the report, that
# every gate runs even after one fails, the coverage rows, and that the tuner reads the report.
# Usage: scripts/quality-report.test.sh (exits 1 if any case fails).
# shellcheck disable=SC2016 # backticks in single quotes are Markdown code spans, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/quality-report.sh"
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

# A stand-in cargo: logs each call, for llvm-cov copies $FAKE_COVERAGE (when set) to --output-path,
# then fails the subcommands in $FAKE_FAIL, so a failed coverage run can still leave a report.
mkdir -p "$tmp/bin"
cat >"$tmp/bin/cargo" <<'EOF'
#!/bin/sh
echo "cargo $* RUSTDOCFLAGS=${RUSTDOCFLAGS-}" >>"$FAKE_LOG"
echo "cargo output for $1"
sub="$1"
if [ "$sub" = llvm-cov ]; then
  out=''
  while [ "$#" -gt 0 ]; do
    [ "$1" = --output-path ] && out="$2"
    shift
  done
  [ -z "${FAKE_COVERAGE-}" ] || cp "$FAKE_COVERAGE" "$out"
fi
case " ${FAKE_FAIL-} " in
  *" $sub "*) exit 1 ;;
esac
EOF
chmod +x "$tmp/bin/cargo"

# file_entry <path> <lines covered> <count> <regions covered> <count> <functions covered> <count>
file_entry() {
  printf '{"filename": "%s", "summary": {"lines": {"covered": %s, "count": %s},' "$1" "$2" "$3"
  printf ' "regions": {"covered": %s, "count": %s}, "functions": {"covered": %s, "count": %s}}}' "$4" "$5" "$6" "$7"
}
# coverage <name> <file entries>: a cargo-llvm-cov export, prints its path.
coverage() {
  printf '{"data": [{"files": [%s]}]}\n' "$2" >"$tmp/$1.json"
  echo "$tmp/$1.json"
}
core="$root/crates/pindu-core/src"
good="$(coverage good "$(file_entry "$core/lib.rs" 975 1000 961 1000 50 52)")"
printf '%s\n' '{"margin": 2, "floors": {"core": {"lines": 95, "regions": 94, "functions": 94}}}' >"$tmp/tuning.json"

# run [VAR=value...]: runs the report with the stand-in cargo; sets $status, $output and $log.
run() {
  : >"$tmp/log"
  status=0
  output="$(env PATH="$tmp/bin:$PATH" FAKE_LOG="$tmp/log" GITHUB_SHA=abc123 QA_TUNING_CONFIG="$tmp/tuning.json" \
    COVERAGE_JSON="$tmp/coverage.json" "$@" sh "$script" 2>"$tmp/stderr")" || status=$?
  log="$(cat "$tmp/log")"
}

# status_is <expected exit status> <description>: checks the last run's exit status.
status_is() {
  if [ "$status" -eq "$1" ]; then
    ok "$2"
  else
    not_ok "$2 (expected exit $1, got $status)"
  fi
}

# has <description> <text> <expected line>
has() {
  if printf '%s\n' "$2" | grep -qxF -- "$3"; then
    ok "$1"
  else
    not_ok "$1 (lacks the line '$3'):$nl$2"
  fi
}

run FAKE_COVERAGE="$good"
expected='# Quality report

Commit: `abc123`

## Quality gates

| Gate | Result |
| ---- | ------ |
| `cargo fmt --all --check` | ✅ pass |
| `cargo clippy --workspace --all-targets -- -D warnings` | ✅ pass |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | ✅ pass |
| `cargo test --workspace` | ✅ pass |
| `cargo llvm-cov --package pindu-core` | ✅ pass |
| `scripts/coverage-check.sh` (floors in `.github/auto-qa-tuning.json`) | ✅ pass |

## Unit-test coverage

| Module | Lines | Regions | Functions |
| ------ | ----- | ------- | --------- |
| core | 97.50% | 96.10% | 96.15% |'
if [ "$status" -eq 0 ] && [ "$output" = "$expected" ]; then
  ok 'reports every gate and the coverage of each module'
else
  not_ok "reports every gate and the coverage of each module (exit $status, expected:$nl$expected${nl}got:$nl$output)"
fi
has 'rustdoc runs with warnings as errors' "$log" 'cargo doc --workspace --no-deps RUSTDOCFLAGS=-D warnings'
has 'coverage is measured as the gate measures it' "$log" \
  "cargo llvm-cov --package pindu-core --json --summary-only --output-path $tmp/coverage.json RUSTDOCFLAGS="
case "$(cat "$tmp/stderr")" in
  *'cargo output for test'*) ok 'tool output goes to stderr' ;;
  *) not_ok 'tool output goes to stderr' ;;
esac

run FAKE_COVERAGE="$good" FAKE_FAIL='clippy'
has 'a failed gate is reported' "$output" '| `cargo clippy --workspace --all-targets -- -D warnings` | ❌ fail |'
if [ "$status" -eq 1 ] && printf '%s' "$log" | grep -q '^cargo test' && printf '%s' "$log" | grep -q '^cargo llvm-cov'; then
  ok 'a failed gate fails the report but everything still runs'
else
  not_ok "a failed gate fails the report but everything still runs (exit $status):$nl$log"
fi
has 'coverage is still reported after a failed gate' "$output" '| core | 97.50% | 96.10% | 96.15% |'

run FAKE_COVERAGE="$good" FAKE_FAIL='llvm-cov'
has 'a failed coverage run reports n/a' "$output" '| core | ❌ n/a | ❌ n/a | ❌ n/a |'
status_is 1 'a failed coverage run fails the report'

cp "$good" "$tmp/coverage.json"
run
has 'a report left by an earlier run is not used' "$output" '| core | ❌ n/a | ❌ n/a | ❌ n/a |'

printf '{"data": [' >"$tmp/truncated.json"
run FAKE_COVERAGE="$tmp/truncated.json"
has 'a malformed coverage report reports n/a' "$output" '| core | ❌ n/a | ❌ n/a | ❌ n/a |'

run FAKE_COVERAGE="$(coverage no-functions "$(file_entry "$core/lib.rs" 3 4 0 0 0 0)")"
has 'a metric with nothing to measure has no percentage' "$output" '| core | 75.00% | none | none |'

run FAKE_COVERAGE="$(coverage ui-only "$(file_entry "$root/crates/pindu-ui/src/main.rs" 3 4 1 1 1 1)")"
has 'a module the run did not measure reports n/a' "$output" '| core | ❌ n/a | ❌ n/a | ❌ n/a |'
status_is 1 'an unmeasured module fails the report'

printf '%s\n' '{"margin": 2, "floors": {}}' >"$tmp/tuning.json"
run FAKE_COVERAGE="$good"
if [ "$status" -eq 2 ] && grep -qF "No modules in $tmp/tuning.json" "$tmp/stderr" && [ -z "$log" ]; then
  ok 'a floors file without modules is an error'
else
  not_ok "a floors file without modules is an error (exit $status): $(cat "$tmp/stderr")"
fi

# The tuner reads the coverage rows the report prints.
printf '%s\n' '{"margin": 2, "floors": {"core": {"lines": 90, "regions": 94, "functions": 94}}}' >"$tmp/tuning.json"
run FAKE_COVERAGE="$good"
printf '%s\n' "$output" >"$tmp/quality.md"
tuned="$(sh "$root/scripts/auto-qa-tuner.sh" --config "$tmp/tuning.json" --report "$tmp/quality.md" 2>&1)" || true
has 'the tuner reads the report' "$tuned" '| core | lines | 90% | 97.50% | 95% ⬆️ |'

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
