#!/bin/sh
# Keeps one `ci: Packaging failing on main` issue in step with the Packaging runs on main
# (.github/workflows/packaging.yml): a failed run opens it, labelled `automated`,
# `area:packaging` and `type:chore`, or comments on the open one; the first run that passes
# closes it. While Packaging is red on main the merge queue merges nothing
# (scripts/merge-queue.sh), so the issue says so.
# Usage: scripts/packaging-issue.sh success|failure <run-url>
# Needs gh authenticated (GH_TOKEN, issues: write) and pointed at the repository (GH_REPO, or
# run from a checkout). Exits 2 on a wrong command line.
set -eu

TITLE='ci: Packaging failing on main'

if [ "$#" -ne 2 ] || { [ "$1" != success ] && [ "$1" != failure ]; }; then
  echo "usage: $0 success|failure <run-url>" >&2
  exit 2
fi
result="$1"
run_url="$2"

# Into a variable, so a failed listing fails the run instead of reading as no open issue.
open="$(gh issue list --state open --label automated --limit 100 --json number,title \
  --jq "[.[] | select(.title == \"$TITLE\")][0].number // \"\"")"

if [ "$result" = failure ]; then
  if [ -n "$open" ]; then
    gh issue comment "$open" --body "Still failing in [this Packaging run]($run_url)."
  else
    gh issue create --title "$TITLE" --label automated --label area:packaging --label type:chore \
      --body "The [Packaging run]($run_url) on \`main\` failed. The merge queue merges nothing until a Packaging run on \`main\` passes again, which closes this issue (\`.github/workflows/packaging.yml\`)."
  fi
elif [ -n "$open" ]; then
  gh issue close "$open" --reason completed \
    --comment "Packaging passes on \`main\` again in [this run]($run_url). Closing."
fi
