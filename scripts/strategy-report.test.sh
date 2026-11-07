#!/bin/sh
# Tests for strategy-report.sh: the strategic dashboard for the milestones of `gh api
# repos/{repo}/milestones`, the issues of `gh issue list`, the pull requests of `gh pr list` and
# the output of scripts/acmm-self-check.py, and the usage errors.
# Usage: scripts/strategy-report.test.sh (exits 1 if any case fails).
# shellcheck disable=SC2016 # backticks in single quotes are Markdown code spans, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
report="$root/scripts/strategy-report.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
failures=0
nl='
'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# milestone <number> <title> <state> <open> <closed> <description>: one entry of the milestones API.
milestone() {
  jq -cn --argjson n "$1" --arg title "$2" --arg state "$3" --argjson open "$4" --argjson closed "$5" \
    --arg description "$6" \
    '{number: $n, title: $title, state: $state, open_issues: $open, closed_issues: $closed,
      description: (if $description == "" then null else $description end)}'
}

# issue <number> <state> <createdAt> <closedAt> <labels, comma-separated> [title] [body]: one entry
# of gh issue list.
issue() {
  jq -cn --argjson n "$1" --arg state "$2" --arg created "$3" --arg closed "$4" --arg labels "$5" \
    --arg title "${6-Issue $1}" --arg body "${7-}" \
    '{number: $n, title: $title, state: $state, createdAt: $created,
      closedAt: (if $closed == "" then null else $closed end),
      labels: ($labels | split(",") | map(select(. != "") | {name: .})), body: $body}'
}

# pr <number> <branch> <author> <createdAt> <mergedAt> [body]: one entry of gh pr list.
pr() {
  jq -cn --argjson n "$1" --arg branch "$2" --arg login "$3" --arg created "$4" --arg merged "$5" \
    --arg body "${6-}" \
    '{number: $n, headRefName: $branch, author: {login: $login}, createdAt: $created,
      mergedAt: (if $merged == "" then null else $merged end), body: $body}'
}

# list <entries...>: a JSON array of the entries.
list() {
  printf '%s\n' "$@" | jq -cs .
}

acmm_ok='ACMM codebase level: L5 Semi-Autonomous (required: L0 Prerequisites not met): ok

- L0 Prerequisites: 7/8 (88%) pass
  - missing `acmm:prereq-e2e` (E2E tests): none of e2e/, tests/e2e/
- L2 Advisory: 8/8 (100%) pass
- L6 Fully Autonomous: 9/12 (75%) pass
  - missing `acmm:merge-queue` (Merge queue): none of .github/workflows/merge-queue.yml
  - missing `aef:audit-trail` (Audit trail | workflow): none of .github/workflows/ai-audit.yml
  - missing `acmm:risk-assessment-config` (Risk assessment config): none of risk-config.json'

# fixtures <milestones> <issues> <prs> <acmm>: write the four inputs to $work.
fixtures() {
  printf '%s' "$1" > "$work/milestones.json"
  printf '%s' "$2" > "$work/issues.json"
  printf '%s' "$3" > "$work/prs.json"
  printf '%s\n' "$4" > "$work/acmm.txt"
}

# run_report: the report over the fixtures in $work.
run_report() {
  sh "$report" --since 2026-10-10 --milestones "$work/milestones.json" --issues "$work/issues.json" \
    --prs "$work/prs.json" --acmm "$work/acmm.txt"
}

# expect_line <description> <expected line>: the report over the fixtures has the line.
expect_line() {
  status=0
  output="$(run_report 2>&1)" || status=$?
  if [ "$status" -ne 0 ]; then
    not_ok "$1 (exit $status): $output"
  elif ! printf '%s\n' "$output" | grep -qxF -- "$2"; then
    not_ok "$1 (output lacks the line '$2'):$nl$output"
  else
    ok "$1"
  fi
}

# expect_error <description> <exit status> <message>: the report over the fixtures fails so.
expect_error() {
  status=0
  output="$(run_report 2>&1)" || status=$?
  if [ "$status" -eq "$2" ] && printf '%s' "$output" | grep -qF -- "$3"; then
    ok "$1"
  else
    not_ok "$1 (exit $status, expected $2 and '$3'): $output"
  fi
}

# The whole report.
fixtures \
  "$(list \
    "$(milestone 7 'Hive readiness' open 9 12 "Gate: Hive's ACMM evaluation would score L6.")" \
    "$(milestone 5 'M5 Release' open 0 54 'Gate: GitHub Releases are the release channel.')" \
    "$(milestone 6 'Nice to have' open 1 3 'No gate: work that can wait.')" \
    "$(milestone 8 'Later' closed 0 0 '')")" \
  "$(list \
    "$(issue 1 OPEN 2026-09-01T08:00:00Z '' area:ui,type:feature)" \
    "$(issue 2 OPEN 2026-10-10T00:00:00Z '' area:core,area:ui,type:chore)" \
    "$(issue 3 CLOSED 2026-10-11T09:00:00Z 2026-10-12T09:00:00Z area:ui,type:feature)" \
    "$(issue 4 CLOSED 2026-08-01T09:00:00Z 2026-10-09T23:59:59Z type:spike)" \
    "$(issue 5 OPEN 2026-10-20T09:00:00Z '' acmm,type:chore '[ACMM L6] Add Merge queue' \
      'Criterion ID: `acmm:merge-queue`')" \
    "$(issue 6 OPEN 2026-10-20T09:00:00Z '' acmm '[ACMM L0] Add tests' \
      'Covers `acmm:prereq-e2e` and `acmm:risk-assessment-config`')" \
    "$(issue 7 CLOSED 2026-10-20T09:00:00Z 2026-10-21T09:00:00Z acmm '[ACMM L6] Add Audit trail' \
      'Criterion ID: `aef:audit-trail`')" \
    "$(issue 8 OPEN 2026-10-21T09:00:00Z '' '' 'Mentions `acmm:merge-queue` without the label')")" \
  "$(list \
    "$(pr 10 claude/12-search tim 2026-10-11T08:00:00Z 2026-10-12T08:00:00Z)" \
    "$(pr 11 feat/13-tiles app/hive-worker-bee 2026-10-11T08:00:00Z '')" \
    "$(pr 12 feat/14-art tim 2026-09-30T08:00:00Z 2026-10-11T08:00:00Z \
      "Fixes #14${nl}${nl}— omp (Claude Code), model=claude-opus-5-5")" \
    "$(pr 13 dependabot/cargo/serde-1 app/dependabot 2026-10-12T08:00:00Z 2026-10-13T08:00:00Z)" \
    "$(pr 14 feat/15-icons tim 2026-10-14T08:00:00Z 2026-10-15T08:00:00Z)" \
    "$(pr 15 feat/16-old tim 2026-09-01T08:00:00Z 2026-09-02T08:00:00Z)")" \
  "$acmm_ok"
expected='# Strategy report

Since 2026-10-10 (UTC).

## Milestones

| Milestone | State | Open | Closed | Done | Gate |
| --- | --- | ---: | ---: | ---: | --- |
| M5 Release | open | 0 | 54 | 100% | GitHub Releases are the release channel. |
| Nice to have | open | 1 | 3 | 75% | — |
| Hive readiness | open | 9 | 12 | 57.1% | Hive'"'"'s ACMM evaluation would score L6. |
| Later | closed | 0 | 0 | n/a | — |

## Issue flow

| Measure | Count |
| --- | ---: |
| Issues opened | 6 |
| Issues closed | 2 |
| Net backlog change | +4 |
| Open issues now | 5 |

## Open backlog by area

| Area | Open issues |
| --- | ---: |
| area:core | 1 |
| area:ui | 2 |
| No area | 3 |

## Open backlog by type

| Type | Open issues |
| --- | ---: |
| type:chore | 2 |
| type:feature | 1 |
| No type | 2 |

## Pull requests by source

Pull requests into main opened or merged since 2026-10-10.

| Source | Opened | Merged |
| --- | ---: | ---: |
| Hive | 1 | 1 |
| Claude Code (CI) | 1 | 1 |
| Dependabot | 1 | 1 |
| Other | 1 | 1 |
| **Agent** | 2 | 2 |
| **All** | 4 | 4 |

Agent: the sources Hive (author `hive-worker-bee`, or a Hive or omp signature line in the body) and Claude Code (CI) (head branch `claude/*`) of `scripts/lib/pr-source.jq`. Local agent runs that open pull requests under the maintainer'"'"'s account without a signature count as Other.

## ACMM gaps

ACMM codebase level: L5 Semi-Autonomous (`scripts/acmm-self-check.py`).

| Level | Met | Status | Still open |
| --- | ---: | --- | --- |
| L0 Prerequisites | 7/8 (88%) | pass | `acmm:prereq-e2e` E2E tests (#6) |
| L2 Advisory | 8/8 (100%) | pass | — |
| L6 Fully Autonomous | 9/12 (75%) | pass | `acmm:merge-queue` Merge queue (#5), `aef:audit-trail` Audit trail \| workflow, `acmm:risk-assessment-config` Risk assessment config (#6) |'
status=0
output="$(run_report 2>&1)" || status=$?
if [ "$status" -eq 0 ] && [ "$output" = "$expected" ]; then
  ok 'renders the whole report'
else
  not_ok "renders the whole report (exit $status; expected:$nl$expected${nl}got:$nl$output)"
fi

# Edges.
fixtures '[]' '[]' '[]' "$acmm_ok"
expect_line 'no milestones is a row of its own' '| No milestones | — | 0 | 0 | n/a | — |'
expect_line 'no issues leaves the backlog flat' '| Net backlog change | 0 |'
expect_line 'no pull requests counts nothing' '| **All** | 0 | 0 |'
expect_line 'no issues has no area' '| No area | 0 |'

fixtures '[]' "$(list "$(issue 1 CLOSED 2026-09-01T08:00:00Z 2026-10-11T08:00:00Z '')")" '[]' "$acmm_ok"
expect_line 'more closed than opened shrinks the backlog' '| Net backlog change | -1 |'

fixtures '[]' '[]' '[]' 'ACMM codebase level: L1 Prerequisites (required: L0 Prerequisites not met): ok

- L0 Prerequisites: 8/8 (100%) pass
- L2 Advisory: 3/8 (38%) FAIL
  - missing `acmm:a` (A): none of a'
expect_line 'a failed level is marked' '| L2 Advisory | 3/8 (38%) | FAIL | `acmm:a` A |'

# Errors.
fixtures '{"title": "M1"}' '[]' '[]' "$acmm_ok"
expect_error 'milestones that are not a list fail' 1 'expected a JSON array of milestones'
fixtures '[]' '{}' '[]' "$acmm_ok"
expect_error 'issues that are not a list fail' 1 'expected a JSON array of issues'
fixtures '[]' '[]' 'null' "$acmm_ok"
expect_error 'pull requests that are not a list fail' 1 'expected a JSON array of pull requests'
fixtures '[]' '[]' '[]' 'Traceback (most recent call last):'
expect_error 'output that is not the self-check report fails' 1 'expected the output of acmm-self-check.py'
fixtures '[]' '[]' '[]' "$acmm_ok"
rm "$work/prs.json"
expect_error 'an unreadable input fails' 1 "strategy-report: cannot read $work/prs.json"

for args in '' '--since 2026-10-10' '--since 2026-10-10 --milestones m --issues i --prs p' \
  '--since 2026-10-10 --milestones m --issues i --prs p --acmm a --days 28' '--milestones m --issues i --prs p --acmm a' \
  '--since 10-10-2026 --milestones m --issues i --prs p --acmm a' '--milestones m --issues i --prs p --acmm a --since'; do
  status=0
  # shellcheck disable=SC2086 # each case is a list of arguments
  output="$(sh "$report" $args 2>&1)" || status=$?
  if [ "$status" -eq 2 ] && printf '%s' "$output" | grep -qF 'usage: strategy-report.sh --since YYYY-MM-DD'; then
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
