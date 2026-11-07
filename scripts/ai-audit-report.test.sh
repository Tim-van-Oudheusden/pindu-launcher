#!/bin/sh
# Tests for ai-audit-report.sh: the audit trail of the pull requests that
# `gh pr list --state merged --json number,title,author,headRefName,mergedAt,mergedBy,body` returns,
# with the commits ai-audit.yml adds, and the source (lib/pr-source.jq), agent and model of each one,
# and the usage errors.
# Usage: scripts/ai-audit-report.test.sh (exits 1 if any case fails).
# shellcheck disable=SC2016 # backticks in single quotes are Markdown code spans, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
report="$root/scripts/ai-audit-report.sh"
failures=0
nl='
'
cr="$(printf '\r')"
hive_signature='— hive: agent=quality backend=claude model=claude-opus-5-5 effort=high'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# pr <number> [key=value...]: one entry of gh pr list, as JSON. Keys: branch, merged_at, author,
# merged_by, body, title (merged_at, merged_by and body take `null`), and commit (the message body
# of one more commit; repeatable). Without a commit key the entry has no commits field, as when
# gh pr list is not asked for one.
pr() {
  n="$1"
  shift
  jq -cn --argjson n "$n" '
    def nullable: if . == "null" then null else . end;
    reduce ($ARGS.positional[] | index("=") as $i | {key: .[:$i], value: .[$i + 1:]}) as $o (
      {number: $n, title: "PR \($n)", author: {login: "Tim-van-Oudheusden"}, headRefName: "fix/1-change",
       mergedAt: "2026-10-10T12:00:00Z", mergedBy: {login: "Tim-van-Oudheusden"}, body: "Fixes #1"};
      ($o.value) as $v
      | if $o.key == "branch" then .headRefName = $v
        elif $o.key == "merged_at" then .mergedAt = ($v | nullable)
        elif $o.key == "author" then .author = {login: $v}
        elif $o.key == "merged_by" then .mergedBy = ($v | nullable | if . == null then null else {login: .} end)
        elif $o.key == "body" then .body = ($v | nullable)
        elif $o.key == "title" then .title = $v
        elif $o.key == "commit" then .commits += [{messageHeadline: "change", messageBody: $v}]
        else error("unknown key \($o.key)") end)' --args "$@"
}

# list <entries...>: a JSON array of the entries.
list() {
  printf '%s\n' "$@" | jq -cs .
}

# run <stdin> [ai-audit-report.sh arguments...]: sets $status, $output (stdout) and $errors (stderr).
run() {
  input="$1"
  shift
  [ "$#" -gt 0 ] || set -- --since 2026-10-01
  errfile="$(mktemp)"
  status=0
  output="$(printf '%s' "$input" | sh "$report" "$@" 2>"$errfile")" || status=$?
  errors="$(cat "$errfile")"
  rm -f "$errfile"
}

# rows <heading>: the table rows of the `## <heading>` section of $output, header and divider dropped.
rows() {
  printf '%s\n' "$output" | awk -v heading="## $1" '
    $0 == heading { inside = 1; next }
    inside && /^## / { exit }
    inside && /^\|/ { if (++seen > 2) print }'
}

# rows_are <description> <heading> <expected rows>: run must have succeeded first.
rows_are() {
  got="$(rows "$2")"
  if [ "$status" -ne 0 ]; then
    not_ok "$1 (exit $status): $errors"
  elif [ "$got" = "$3" ]; then
    ok "$1"
  else
    not_ok "$1 (expected:$nl$3${nl}got:$nl$got)"
  fi
}

# column <n>: field n (1 = PR) of every ledger row, one per line.
column() {
  rows Ledger | awk -F ' [|] ' -v n="$1" '{ sub(/^[|] /, ""); sub(/ [|]$/, ""); print $n }'
}

# columns_are <description> <n> <expected fields>
columns_are() {
  got="$(column "$2")"
  if [ "$status" -ne 0 ]; then
    not_ok "$1 (exit $status): $errors"
  elif [ "$got" = "$3" ]; then
    ok "$1"
  else
    not_ok "$1 (expected:$nl$3${nl}got:$nl$got)"
  fi
}

# Attribution, oldest merge first. A local agent run signs without agent=; a body edited in the web
# UI has CRLF line ends that must not stick to the last field.
run "$(list \
  "$(pr 4 branch=quality/test-gap merged_at=2026-10-04T09:30:00Z author=app/hive-worker-bee \
    "body=Adds a test.$nl$nl$hive_signature$nl")" \
  "$(pr 2 branch=claude/issue-1-fix merged_at=2026-10-02T08:00:00Z merged_by=app/github-actions)" \
  "$(pr 3 branch=dependabot/cargo/serde-1 merged_at=2026-10-03T07:15:00Z author=app/dependabot \
    'body=Bumps serde.')" \
  "$(pr 5 merged_at=2026-10-05T10:00:00Z \
    "body=Closes #1$cr$nl$cr$nl— omp (Claude Code), model=anthropic/claude-opus-5-5$cr$nl")" \
  "$(pr 6 branch=chore/6-tidy merged_at=2026-10-06T11:00:00Z)")"
rows_are 'attributes each merged pull request, oldest merge first' Ledger \
  '| #2 | 2026-10-02 08:00 | Claude Code (CI) | — | — | Tim-van-Oudheusden | app/github-actions | `claude/issue-1-fix` | PR 2 |
| #3 | 2026-10-03 07:15 | Dependabot | — | — | app/dependabot | Tim-van-Oudheusden | `dependabot/cargo/serde-1` | PR 3 |
| #4 | 2026-10-04 09:30 | Hive | quality | claude-opus-5-5 | app/hive-worker-bee | Tim-van-Oudheusden | `quality/test-gap` | PR 4 |
| #5 | 2026-10-05 10:00 | Hive | — | anthropic/claude-opus-5-5 | Tim-van-Oudheusden | Tim-van-Oudheusden | `fix/1-change` | PR 5 |
| #6 | 2026-10-06 11:00 | Other | — | — | Tim-van-Oudheusden | Tim-van-Oudheusden | `chore/6-tidy` | PR 6 |'

run "$(list "$(pr 7 branch=claude/issue-7 'body=— hive: agent=scanner model=claude-sonnet-5')")"
columns_are 'a Hive signature outranks the head branch' 3 'Hive'
columns_are 'the signature names the agent' 4 'scanner'
columns_are 'the signature names the model' 5 'claude-sonnet-5'

# The Hive App as gh pr list names it and as the REST API does.
run "$(list "$(pr 8 branch=claude/issue-8 author=app/hive-worker-bee 'body=No signature.')" \
  "$(pr 9 'author=hive-worker-bee[bot]' body=null)")"
columns_are 'the Hive App is Hive without a signature' 3 "Hive${nl}Hive"

# A body that quotes another pull request's signature is not signed by it.
run "$(list "$(pr 15 branch=claude/issue-15 \
  "body=Follows #3, which ended:$nl$nl> — omp (Claude Code), model=anthropic/claude-opus-5-5$nl")")"
columns_are 'a signature counts only at the start of a line' 3 'Claude Code (CI)'
columns_are 'a quoted signature names no model' 5 '—'

run "$(list "$(pr 16 branch=claudette/x)" "$(pr 17 branch=dependabot-config)")"
columns_are 'a source branch prefix includes its slash' 3 "Other${nl}Other"

# The REST API reports an empty body as null.
run "$(list "$(pr 18 branch=claude/issue-18 body=null)")"
columns_are 'a pull request with no body is attributed by its branch' 3 'Claude Code (CI)'

run "$(list "$(pr 19 merged_by=null)")"
columns_are 'a merge with no recorded merger shows a dash' 7 '—'

# Models from the commits' Co-Authored-By trailers, which Claude Code writes with Anthropic's
# no-reply address.
run "$(list "$(pr 20 branch=claude/issue-20 \
  "commit=Fix it.${nl}${nl}Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>" \
  "commit=More.${nl}${nl}co-authored-by: Claude Opus 4.5 <noreply@anthropic.com>$cr" \
  "commit=Last.${nl}${nl}Co-authored-by: Claude Sonnet 4.5 <noreply@anthropic.com>")")"
columns_are 'the models of Claude co-author trailers, each once' 5 'Claude Opus 4.5, Claude Sonnet 4.5'
columns_are 'a co-author trailer names no agent' 4 '—'
columns_are 'a co-author trailer does not change the source' 3 'Claude Code (CI)'

run "$(list "$(pr 21 "body=$hive_signature" \
  "commit=Fix.${nl}${nl}Co-Authored-By: Claude Opus 4.5 <noreply@anthropic.com>")")"
columns_are 'the signature model outranks a co-author trailer' 5 'claude-opus-5-5'

run "$(list "$(pr 22 "commit=Pair.${nl}${nl}Co-Authored-By: Jane Doe <jane@example.com>" \
  "commit=Quoted: Co-Authored-By: Claude <noreply@anthropic.com>")" "$(pr 23 'commit=')")"
columns_are 'people and trailers inside a line name no model' 5 "—${nl}—"

# Order and counts.
run "$(list "$(pr 11 merged_at=2026-10-03T00:00:00Z)" "$(pr 12 merged_at=2026-10-02T00:00:00Z)")"
columns_are 'the ledger follows merge time, not pull request number' 1 "#12${nl}#11"
run "$(list "$(pr 14)" "$(pr 13)")"
columns_are 'pull requests merged at the same time are listed by number' 1 "#13${nl}#14"

run "$(list \
  "$(pr 1 merged_at=2026-09-30T23:59:59Z 'body=— hive: agent=scanner')" \
  "$(pr 2 merged_at=2026-10-01T00:00:00Z 'body=— hive: agent=scanner')" \
  "$(pr 3 'body=— hive: agent=quality' merged_by=app/github-actions)" \
  "$(pr 4 branch=dependabot/cargo/x merged_by=null)" \
  "$(pr 5 merged_by=app/github-actions)" \
  "$(pr 6 merged_by=app/github-actions)" \
  "$(pr 7 merged_at=null)")"
rows_are 'counts per source, leaving out merges before the window and unmerged pull requests' 'By source' \
  '| Hive | 2 |
| Claude Code (CI) | 0 |
| Dependabot | 1 |
| Other | 2 |
| **All** | 5 |'
rows_are 'counts per merger, an unknown merger last' 'Merged by' \
  '| app/github-actions | 3 |
| Tim-van-Oudheusden | 1 |
| Unknown (no merger recorded) | 1 |'
columns_are 'the ledger has only merges in the window' 1 "#2${nl}#3${nl}#4${nl}#5${nl}#6"

run '[]'
if [ "$status" -eq 0 ] && printf '%s\n' "$output" | grep -qxF 'Pull requests merged into `main` since 2026-10-01 (UTC).'; then
  ok 'states the reporting window'
else
  not_ok "states the reporting window (exit $status): $output$errors"
fi

run "$(list "$(pr 10 'title=fix(a|b): keep | out' 'author=a|b')")"
columns_are 'escapes pipes so a title cannot add table columns' 9 'fix(a\|b): keep \| out'
columns_are 'escapes pipes in an author' 6 'a\|b'

# Errors.
run '{"number": 1}'
if [ "$status" -ne 0 ] && [ -z "$output" ] && printf '%s' "$errors" | grep -qF 'expected a JSON array of pull requests'; then
  ok 'input that is not a list fails without output'
else
  not_ok "input that is not a list fails without output (exit $status): $output$errors"
fi
for args in '--since' '--since 1-Oct' '--since 2026-10-01x' '--since x2026-10-01' '--days 7' \
  '--since 2026-10-01 --days 7'; do
  # shellcheck disable=SC2086 # each case is a list of arguments
  run '[]' $args
  if [ "$status" -eq 2 ] && printf '%s' "$errors" | grep -qF 'usage: ai-audit-report.sh --since YYYY-MM-DD'; then
    ok "usage error: '$args'"
  else
    not_ok "usage error: '$args' (exit $status): $errors"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
