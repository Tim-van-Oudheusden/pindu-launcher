#!/bin/sh
# PR acceptance metric: of the pull requests into main closed in the reporting window, how many
# were merged and how many closed without merging, for agent work and for the rest.
#
# Usage:
#   gh pr list --base main --state closed --search "closed:>=YYYY-MM-DD" \
#     --json number,mergedAt,headRefName,author,body | scripts/pr-metrics.sh --since YYYY-MM-DD --days N
#
# Reads the pull requests on stdin and prints a Markdown table. Agent work is `agent_authored` of
# scripts/lib/pr-source.jq, the rule every report shares.
#
# Exit status: 0 on success, 1 for input that is not a JSON array, 2 for bad arguments.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

usage() {
  echo "usage: pr-metrics.sh --since YYYY-MM-DD --days N" >&2
  exit 2
}

since=''
days=''
while [ "$#" -gt 0 ]; do
  # A flag needs its value: `shift 2` with one argument left shifts nothing and would loop forever.
  case "$1" in
    --since)
      [ "$#" -ge 2 ] || usage
      since="$2"
      shift 2
      ;;
    --days)
      [ "$#" -ge 2 ] || usage
      days="$2"
      shift 2
      ;;
    *) usage ;;
  esac
done
case "$since" in
  [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
  *) usage ;;
esac
case "$days" in
  '' | *[!0-9]*) usage ;;
esac

if ! jq -r -L "$here/lib" --arg since "$since" --arg days "$days" '
  include "pr-source";
  def row($label; $set):
    ($set | length) as $total
    | ($set | map(select(.mergedAt != null)) | length) as $merged
    | "| \($label) | \($total) | \($merged) | \($total - $merged) | "
      + (if $total == 0 then "n/a" else "\(($merged * 1000 / $total | round) / 10)%" end)
      + " |";
  if type != "array" then error("expected a JSON array of pull requests") else . end
  | map(select(agent_authored)) as $agent
  | map(select(agent_authored | not)) as $other
  | "## PR acceptance metric",
    "",
    "PRs into main closed since \($since) (last \($days) days).",
    "",
    "| Source | Closed | Merged | Rejected | Acceptance |",
    "| --- | ---: | ---: | ---: | ---: |",
    row("Agent"; $agent),
    row("Other"; $other),
    row("**All**"; .),
    "",
    "Agent: the sources Hive (author `hive-worker-bee`, or a Hive or omp signature line in the body) and"
      + " Claude Code (CI) (head branch `claude/*`) of `scripts/lib/pr-source.jq`. Local agent runs that open"
      + " pull requests under the maintainer'"'"'s account without a signature count as Other."
'; then
  exit 1
fi
