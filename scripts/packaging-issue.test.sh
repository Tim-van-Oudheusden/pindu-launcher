#!/bin/sh
# Tests for packaging-issue.sh, run offline against a stub `gh`: `gh issue list` answers from a
# JSON listing, filtered through `--jq` as gh does, and every other call is recorded.
# Needs jq (it ships on GitHub's Ubuntu runners).
# Usage: scripts/packaging-issue.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/packaging-issue.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
nl='
'
run_url=https://github.com/owner/pindu/actions/runs/42
title='ci: Packaging failing on main'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# check <description> <expected> <actual>
check() {
  if [ "$2" = "$3" ]; then
    ok "$1"
  else
    not_ok "$1 (expected:$nl$2${nl}got:$nl$3)"
  fi
}

# A call fails when it starts with the text in $tmp/fail.
cat >"$tmp/gh" <<'EOF'
#!/bin/sh
set -eu
if [ -f "$FAKE_DIR/fail" ]; then
  case "$*" in "$(cat "$FAKE_DIR/fail")"*) exit 1 ;; esac
fi
if [ "$1" = issue ] && [ "$2" = list ]; then
  filter=.
  shift 2
  while [ $# -gt 0 ]; do
    [ "$1" != --jq ] || filter="$2"
    shift 2
  done
  jq -r "$filter" "$FAKE_DIR/issues.json"
  exit 0
fi
echo "$*" >>"$FAKE_DIR/calls.log"
EOF
chmod +x "$tmp/gh"

# run <open issues as JSON> <args>...: sets status and calls (one line per recorded gh call).
run() {
  printf '%s' "$1" >"$tmp/issues.json"
  shift
  rm -f "$tmp/calls.log"
  status=0
  PATH="$tmp:$PATH" FAKE_DIR="$tmp" sh "$script" "$@" >/dev/null 2>&1 || status=$?
  calls="$(cat "$tmp/calls.log" 2>/dev/null || true)"
}

other='{"number":5,"title":"ci: nightly compliance: 1 failing check"}'
open="{\"number\":9,\"title\":\"$title\"}"

run "[$other]" failure "$run_url"
check 'a failure opens the issue' 0 "$status"
check 'the new issue is labelled and links the run' \
  "issue create --title $title --label automated --label area:packaging --label type:chore --body The [Packaging run]($run_url) on \`main\` failed. The merge queue merges nothing until a Packaging run on \`main\` passes again, which closes this issue (\`.github/workflows/packaging.yml\`)." \
  "$calls"

run "[$other,$open]" failure "$run_url"
check 'another failure comments on the open issue' \
  "issue comment 9 --body Still failing in [this Packaging run]($run_url)." "$calls"

run "[$other,$open]" success "$run_url"
check 'a pass closes the open issue' \
  "issue close 9 --reason completed --comment Packaging passes on \`main\` again in [this run]($run_url). Closing." "$calls"

run "[$other]" success "$run_url"
check 'a pass with no open issue does nothing' '' "$calls"
check 'a pass with no open issue exits 0' 0 "$status"

printf 'issue list' >"$tmp/fail"
run '[]' failure "$run_url"
check 'an unreadable issue list fails' 1 "$status"
check 'an unreadable issue list opens nothing' '' "$calls"
rm "$tmp/fail"

for args in '' 'failure' "cancelled $run_url" "failure $run_url extra"; do
  # Word splitting is the point: each case is a command line.
  # shellcheck disable=SC2086
  run '[]' $args
  check "'$args' exits 2" 2 "$status"
done

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
