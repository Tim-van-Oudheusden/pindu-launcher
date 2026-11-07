#!/bin/sh
# PR review rubric: the mechanically checkable items of AGENTS.md's pull request conventions.
# Reads PR_TITLE, PR_BODY and PR_AUTHOR from the environment, reports every failed item as a
# GitHub `::error::` line and exits 1 if any failed.
#
# - The title follows Conventional Commits and ends with the issue it works, as AGENTS.md
#   writes it: `feat(import): import Steam libraries (#8)`, or `(#235, #236)` for two.
# - The body links the issue: `Fixes #8`, or `Refs #8` when the issue stays open.
# Bots (`dependabot[bot]`) open pull requests for no issue, so they only need the
# Conventional Commits title.
set -eu

title="${PR_TITLE:-}"
body="${PR_BODY:-}"
author="${PR_AUTHOR:-}"
failures=0
newline='
'

fail() {
  echo "::error::$1"
  failures=$((failures + 1))
}

# matches <extended regex>: the whole title matches it. A title is one line; one holding a
# newline matches nothing, so grep cannot accept it on the strength of one of its lines.
matches() {
  case "$title" in *"$newline"*) return 1 ;; esac
  printf '%s\n' "$title" | grep -Eq "$1"
}

types='build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test'
conventional="^($types)(\\([a-z0-9._/-]+\\))?!?: [^ ]"
issues=' \(#[0-9]+(, #[0-9]+)*\)$'

case "$author" in
  *'[bot]')
    if matches "$conventional"; then
      echo "ok: title follows Conventional Commits (bot author, no issue needed)"
    else
      fail "PR title '$title' must follow Conventional Commits, e.g. 'build(deps): bump gtk4 to 0.11.1'"
    fi
    ;;
  *)
    if matches "$conventional.*$issues"; then
      echo "ok: title follows Conventional Commits and names its issue"
    else
      fail "PR title '$title' must follow Conventional Commits and end with its issue, e.g. 'feat(import): import Steam libraries (#8)' (types: $(echo "$types" | sed 's/|/, /g'))"
    fi
    # The keyword must be a whole word ("hotfix #3" is no link) and may end in a colon
    # ("Closes: #10"); "owner/repo#N" is GitHub's long form.
    if printf '%s\n' "$body" |
      grep -Eiq '(^|[^[:alnum:]_])(fix(es|ed)?|close[sd]?|resolve[sd]?|refs?):?[[:space:]]+([[:alnum:]_.-]+/[[:alnum:]_.-]+)?#[0-9]+'; then
      echo "ok: body links a GitHub issue"
    else
      fail "PR body must link its GitHub issue, e.g. 'Fixes #8', or 'Refs #8' if the issue stays open"
    fi
    ;;
esac

[ "$failures" -eq 0 ]
