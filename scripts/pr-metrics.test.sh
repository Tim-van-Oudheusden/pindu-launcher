#!/bin/sh
# Tests for pr-metrics.sh and lib/pr-source.jq: the acceptance table for pull requests that
# `gh pr list --json number,mergedAt,headRefName,author,body` returns, the source each one is
# given, and the usage errors.
# Usage: scripts/pr-metrics.test.sh (exits 1 if any case fails).
# shellcheck disable=SC2016 # backticks in single quotes are Markdown code spans, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
metrics="$root/scripts/pr-metrics.sh"
failures=0
nl='
'
cr="$(printf '\r')"

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# pr <number> <branch> <author> <merged: true|false> [body]: one entry of gh pr list, as JSON.
pr() {
  jq -cn --argjson n "$1" --arg branch "$2" --arg login "$3" --argjson merged "$4" --arg body "${5-}" \
    --argjson has_body "$(test "$#" -ge 5 && echo true || echo false)" '
    {number: $n, headRefName: (if $branch == "" then null else $branch end),
     author: (if $login == "" then null else {login: $login} end),
     mergedAt: (if $merged then "2026-10-01T12:00:00Z" else null end)}
    + (if $has_body then {body: $body} else {} end)'
}

# list <entries...>: a JSON array of the entries.
list() {
  printf '%s\n' "$@" | jq -cs .
}

# expect_line <description> <stdin> <expected line> [pr-metrics.sh arguments...]
expect_line() {
  description="$1"
  input="$2"
  line="$3"
  shift 3
  status=0
  output="$(printf '%s' "$input" | sh "$metrics" "$@" 2>&1)" || status=$?
  if [ "$status" -ne 0 ]; then
    not_ok "$description (exit $status): $output"
  elif ! printf '%s\n' "$output" | grep -qxF -- "$line"; then
    not_ok "$description (output lacks the line '$line'):$nl$output"
  else
    ok "$description"
  fi
}

# source_is <description> <entry> <expected source>: the source pr-source.jq gives the entry.
source_is() {
  got="$(printf '%s' "$2" | jq -r -L "$root/scripts/lib" 'include "pr-source"; source' 2>&1)" || true
  if [ "$got" = "$3" ]; then
    ok "$1"
  else
    not_ok "$1 (expected '$3', got '$got')"
  fi
}

window='--since 2026-09-17 --days 28'

# The whole report.
mixed="$(list \
  "$(pr 1 claude/12-search tim true)" \
  "$(pr 2 claude/13-library tim false)" \
  "$(pr 3 feat/14-tiles app/hive-worker-bee true)" \
  "$(pr 4 fix/15-crash 'hive-worker-bee[bot]' false)" \
  "$(pr 5 feat/16-icons tim true)" \
  "$(pr 6 feat/claude/17-not-an-agent-branch someone false)" \
  "$(pr 7 '' '' false)")"
expected='## PR acceptance metric

PRs into main closed since 2026-09-17 (last 28 days).

| Source | Closed | Merged | Rejected | Acceptance |
| --- | ---: | ---: | ---: | ---: |
| Agent | 4 | 2 | 2 | 50% |
| Other | 3 | 1 | 2 | 33.3% |
| **All** | 7 | 3 | 4 | 42.9% |

Agent: the sources Hive (author `hive-worker-bee`, or a Hive or omp signature line in the body) and Claude Code (CI) (head branch `claude/*`) of `scripts/lib/pr-source.jq`. Local agent runs that open pull requests under the maintainer'"'"'s account without a signature count as Other.'
# shellcheck disable=SC2086 # the window is two options
output="$(printf '%s' "$mixed" | sh "$metrics" $window 2>&1)" || true
if [ "$output" = "$expected" ]; then
  ok 'splits agent and other pull requests'
else
  not_ok "splits agent and other pull requests (expected:$nl$expected${nl}got:$nl$output)"
fi

# shellcheck disable=SC2086
{
  expect_line 'no closed pull requests is not a rate' '[]' '| **All** | 0 | 0 | 0 | n/a |' $window
  expect_line 'all merged is a full rate' "$(list "$(pr 1 claude/1-x tim true)")" '| Agent | 1 | 1 | 0 | 100% |' $window
  expect_line 'the rate is rounded to a tenth' \
    "$(list "$(pr 1 claude/1-x tim true)" "$(pr 2 claude/2-y tim true)" "$(pr 3 claude/3-z tim false)")" \
    '| Agent | 3 | 2 | 1 | 66.7% |' $window
  expect_line 'a signed body is agent work on any branch' \
    "$(list "$(pr 1 feat/1-x Tim-van-Oudheusden true 'Fixes #1')" "$(pr 2 feat/2-y Tim-van-Oudheusden true "Fixes #2${nl}${nl}— omp (Claude Code), model=claude-opus-5-5")")" \
    '| Agent | 1 | 1 | 0 | 100% |' $window
}

# Sources.
source_is 'the Hive App as gh names it' "$(pr 1 feat/1-x app/hive-worker-bee true)" Hive
source_is 'the Hive App as the REST API names it' "$(pr 1 feat/1-x 'hive-worker-bee[bot]' true)" Hive
source_is 'a Hive signature line anywhere in the body' \
  "$(pr 1 feat/1-x tim true "Fixes #1${nl}— hive: agent=quality backend=claude model=claude-opus-5-5${nl}More text")" Hive
source_is 'an omp signature with Windows line endings' \
  "$(pr 1 feat/1-x tim true "Fixes #1$cr${nl}— omp (Claude Code), model=claude-opus-5-5$cr")" Hive
source_is 'a signature quoted inside a line is not one' \
  "$(pr 1 feat/1-x tim true 'Sign with "— hive: agent=x" at the end')" Other
source_is 'a claude/ branch' "$(pr 1 claude/1-x tim true)" 'Claude Code (CI)'
source_is 'a branch with claude in the middle' "$(pr 1 feat/claude/1-x tim true)" Other
source_is 'a branch named like claude without the slash' "$(pr 1 claude-1-x tim true)" Other
source_is 'Dependabot is not agent work' "$(pr 1 dependabot/cargo/serde-1 'app/dependabot' true)" Dependabot
source_is 'no branch, author or body' "$(pr 1 '' '' false)" Other
agent="$(printf '%s' "$(list "$(pr 1 claude/1-x a true)" "$(pr 2 feat/1 'app/hive-worker-bee' true)" "$(pr 3 dependabot/x a true)" "$(pr 4 feat/2 a true)")" \
  | jq -c -L "$root/scripts/lib" 'include "pr-source"; map(agent_authored)' 2>&1)" || true
if [ "$agent" = '[true,true,false,false]' ]; then
  ok 'agent_authored is Hive and Claude Code (CI)'
else
  not_ok "agent_authored is Hive and Claude Code (CI) (got $agent)"
fi

# Errors.
status=0
output="$(printf '{"number": 1}' | sh "$metrics" --since 2026-09-17 --days 28 2>&1)" || status=$?
if [ "$status" -ne 0 ] && printf '%s' "$output" | grep -qF 'expected a JSON array of pull requests'; then
  ok 'input that is not a list fails'
else
  not_ok "input that is not a list fails (exit $status): $output"
fi
for args in '' '--since 2026-09-17' '--days 28' '--since 2026-09-17 --days 28 --repo x' '--days 28 --since' \
  '--since 2026-09-17 --days' '--since 17-09-2026 --days 28' '--since 2026-09-17 --days four'; do
  status=0
  # shellcheck disable=SC2086 # each case is a list of arguments
  output="$(printf '[]' | sh "$metrics" $args 2>&1)" || status=$?
  if [ "$status" -eq 2 ] && printf '%s' "$output" | grep -qF 'usage: pr-metrics.sh --since YYYY-MM-DD --days N'; then
    ok "usage error: '$args'"
  else
    not_ok "usage error: '$args' (exit $status): $output"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
