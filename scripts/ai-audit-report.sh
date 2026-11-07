#!/bin/sh
# AI audit trail: every pull request merged into main since a date, with its source (Hive, Claude
# Code in CI, Dependabot or Other, from scripts/lib/pr-source.jq, the rule every report shares), the
# agent and model behind it, who opened it and who merged it.
#
# Usage:
#   gh pr list --base main --state merged --search "merged:>=YYYY-MM-DD" \
#     --json number,title,author,headRefName,mergedAt,mergedBy,body | scripts/ai-audit-report.sh --since YYYY-MM-DD
#
# Reads the pull requests on stdin and prints a Markdown report. The agent and model come from the
# body's signature line. Without a model there, the model is the name in the
# `Co-Authored-By: <name> <noreply@anthropic.com>` trailers, which Claude Code writes, of each entry's
# `commits` (`[{messageHeadline, messageBody}]`, as .github/workflows/ai-audit.yml adds them);
# entries without `commits` are attributed from the signature alone.
#
# Exit status: 0 on success, 1 for input that is not a JSON array, 2 for bad arguments.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

usage() {
  echo "usage: ai-audit-report.sh --since YYYY-MM-DD" >&2
  exit 2
}

since=''
while [ "$#" -gt 0 ]; do
  # A flag needs its value: `shift 2` with one argument left shifts nothing and would loop forever.
  case "$1" in
    --since)
      [ "$#" -ge 2 ] || usage
      since="$2"
      shift 2
      ;;
    *) usage ;;
  esac
done
# mergedAt is compared as text, which orders correctly only against a YYYY-MM-DD date.
case "$since" in
  [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
  *) usage ;;
esac

if ! jq -r -L "$here/lib" --arg since "$since" '
  include "pr-source";
  def none: "—";
  # The key=value fields of a signature line.
  def fields($line):
    if $line == null then {}
    else [$line | scan("([A-Za-z_]+)=([^ ]+)")] | map({(.[0]): .[1]}) | add // {} end;
  # The names in the Co-Authored-By trailers (a line of their own, any case) that Claude Code adds.
  def trailer_models:
    [(.commits // [])[] | (.messageBody // "") | split("\n")[] | rtrimstr("\r")
      | capture("^co-authored-by:[ \t]*(?<name>[^<]*[^< \t])[ \t]*<noreply@anthropic\\.com>[ \t]*$"; "i")
      | .name]
    | unique | if length == 0 then null else join(", ") end;
  def attribution:
    fields(signature) as $sig
    | {source: source, agent: ($sig.agent // none), model: ($sig.model // trailer_models // none)};
  # Escapes | so text from a pull request cannot add or shift table columns.
  def cell: tostring | split("|") | join("\\|");

  if type != "array" then error("expected a JSON array of pull requests") else . end
  | map(select(.mergedAt != null and .mergedAt >= $since) | . + {attribution: attribution})
  | sort_by(.mergedAt, .number) as $merged
  | "# AI audit trail",
    "",
    "Pull requests merged into `main` since \($since) (UTC).",
    "",
    "## By source",
    "",
    "| Source | Merged PRs |",
    "| --- | ---: |",
    (sources[] as $source | "| \($source) | \($merged | map(select(.attribution.source == $source)) | length) |"),
    "| **All** | \($merged | length) |",
    "",
    "## Merged by",
    "",
    "| Account | Merged PRs |",
    "| --- | ---: |",
    # Most merges first; a merge with no recorded merger is listed last so it stands out.
    ($merged
      | group_by(.mergedBy.login)
      | map({account: .[0].mergedBy.login, count: length})
      | sort_by((.account == null), -.count, .account)[]
      | "| \(if .account == null then "Unknown (no merger recorded)" else (.account | cell) end) | \(.count) |"),
    "",
    "## Ledger",
    "",
    "| PR | Merged (UTC) | Source | Agent | Model | Author | Merged by | Branch | Title |",
    "| --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ($merged[]
      | "| #\(.number) | \(.mergedAt[0:10]) \(.mergedAt[11:16]) | \(.attribution.source)"
        + " | \(.attribution.agent | cell) | \(.attribution.model | cell) | \(.author.login // none | cell)"
        + " | \(.mergedBy.login // none | cell) | `\(.headRefName // "" | cell)` | \(.title | cell) |")
'; then
  exit 1
fi
