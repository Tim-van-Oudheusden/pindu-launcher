#!/bin/sh
# Strategy report: where the project is heading rather than how one change fared. Prints Markdown
# for the job summary of .github/workflows/strategy-report.yml: milestone progress with each
# milestone's gate, issues opened and closed since a date, the open backlog by area and type label,
# pull requests by source (scripts/lib/pr-source.jq, the rule every report shares) and the ACMM
# criteria scripts/acmm-self-check.py still finds missing, per level, with the open `acmm` issues
# that name them.
#
# Usage:
#   scripts/strategy-report.sh --since YYYY-MM-DD --milestones FILE --issues FILE --prs FILE --acmm FILE
#
#   --milestones  JSON of `gh api 'repos/{owner}/{repo}/milestones?state=all'`
#   --issues      JSON of `gh issue list --state all --json number,title,state,labels,createdAt,closedAt,body`
#   --prs         JSON of `gh pr list --base main --state all --json number,createdAt,mergedAt,headRefName,author,body`
#   --acmm        output of `python3 scripts/acmm-self-check.py`
#
# The window runs from <since> (UTC midnight) to now; inputs may hold entries from before it.
#
# Exit status: 0 on success, 1 for an input that cannot be read or has the wrong shape, 2 for bad
# arguments.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

usage() {
  echo "usage: strategy-report.sh --since YYYY-MM-DD --milestones FILE --issues FILE --prs FILE --acmm FILE" >&2
  exit 2
}

since=''
milestones=''
issues=''
prs=''
acmm=''
while [ "$#" -gt 0 ]; do
  # A flag needs its value: `shift 2` with one argument left shifts nothing and would loop forever.
  [ "$#" -ge 2 ] || usage
  case "$1" in
    --since) since="$2" ;;
    --milestones) milestones="$2" ;;
    --issues) issues="$2" ;;
    --prs) prs="$2" ;;
    --acmm) acmm="$2" ;;
    *) usage ;;
  esac
  shift 2
done
case "$since" in
  [0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]) ;;
  *) usage ;;
esac
if [ -z "$milestones" ] || [ -z "$issues" ] || [ -z "$prs" ] || [ -z "$acmm" ]; then
  usage
fi
for input in "$milestones" "$issues" "$prs" "$acmm"; do
  if [ ! -r "$input" ]; then
    echo "strategy-report: cannot read $input" >&2
    exit 1
  fi
done

# gh prints UTC timestamps (2026-10-01T08:00:00Z), which order as strings, so a timestamp is in
# the window when it sorts at or after the bare date.
if ! jq -nr -L "$here/lib" --arg since "$since" --slurpfile milestones "$milestones" \
  --slurpfile issues "$issues" --slurpfile prs "$prs" --rawfile acmm "$acmm" '
  include "pr-source";
  def list($what): if type != "array" then error("expected a JSON array of \($what)") else . end;
  def inwindow: . != null and . >= $since;
  def signed: if . > 0 then "+\(.)" else "\(.)" end;
  def cell: gsub("\r?\n"; " ") | gsub("\\|"; "\\|");
  def percent($part; $total):
    if $total == 0 then "n/a" else "\(($part * 1000 / $total | round) / 10)%" end;
  # Open issues per label with the given prefix, in name order, then those without one.
  def backlog($open; $prefix; $none):
    ($open | map(.labels[].name | select(startswith($prefix))) | group_by(.)[] | "| \(.[0]) | \(length) |"),
    "| \($none) | \($open | map(select(all(.labels[]; .name | startswith($prefix) | not))) | length) |";
  # The self-check report: its level line, then per level its score and missing criteria.
  def acmm_report:
    reduce (split("\n")[] | rtrimstr("\r")) as $line ({level: null, levels: []};
      if ($line | test("^ACMM codebase level: ")) then
        .level = ($line | capture("^ACMM codebase level: (?<level>L[0-9]+ .+?) \\(required: ").level)
      elif ($line | test("^- L[0-9]+ ")) then
        .levels += [$line
          | capture("^- (?<name>L[0-9]+ .+): (?<met>[0-9]+/[0-9]+ \\([0-9]+%\\)) (?<status>pass|FAIL)$")
          | . + {missing: []}]
      elif ($line | test("^  - missing `")) and (.levels | length) > 0 then
        .levels[-1].missing += [$line | capture("^  - missing `(?<key>[^`]+)` \\((?<name>.*)\\): none of ")]
      else . end)
    | if .level == null or .levels == [] then error("expected the output of acmm-self-check.py") else . end;

  ($milestones[0] | list("milestones") | sort_by(.number)) as $milestones
  | ($issues[0] | list("issues")) as $all
  | ($prs[0] | list("pull requests")) as $prs
  | ($acmm | acmm_report) as $acmm
  | ($all | map(select(.state == "OPEN"))) as $open
  | ($open | map(select(any(.labels[]; .name == "acmm")))) as $gap_issues
  | ($all | map(select(.createdAt | inwindow)) | length) as $opened
  | ($all | map(select(.closedAt | inwindow)) | length) as $closed
  | ($prs | map(select((.createdAt | inwindow) or (.mergedAt | inwindow)))) as $prs
  | def pr_row($label; $set):
      "| \($label) | \($set | map(select(.createdAt | inwindow)) | length)"
      + " | \($set | map(select(.mergedAt | inwindow)) | length) |";
  "# Strategy report",
  "",
  "Since \($since) (UTC).",
  "",
  "## Milestones",
  "",
  "| Milestone | State | Open | Closed | Done | Gate |",
  "| --- | --- | ---: | ---: | ---: | --- |",
  (if $milestones == [] then "| No milestones | — | 0 | 0 | n/a | — |" else
    $milestones[]
    | "| \(.title | cell) | \(.state) | \(.open_issues) | \(.closed_issues)"
      + " | \(percent(.closed_issues; .open_issues + .closed_issues))"
      + " | \(.description // "" | if startswith("Gate: ") then ltrimstr("Gate: ") | cell else "—" end) |"
  end),
  "",
  "## Issue flow",
  "",
  "| Measure | Count |",
  "| --- | ---: |",
  "| Issues opened | \($opened) |",
  "| Issues closed | \($closed) |",
  "| Net backlog change | \($opened - $closed | signed) |",
  "| Open issues now | \($open | length) |",
  "",
  "## Open backlog by area",
  "",
  "| Area | Open issues |",
  "| --- | ---: |",
  backlog($open; "area:"; "No area"),
  "",
  "## Open backlog by type",
  "",
  "| Type | Open issues |",
  "| --- | ---: |",
  backlog($open; "type:"; "No type"),
  "",
  "## Pull requests by source",
  "",
  "Pull requests into main opened or merged since \($since).",
  "",
  "| Source | Opened | Merged |",
  "| --- | ---: | ---: |",
  (sources[] as $source | pr_row($source; $prs | map(select(source == $source)))),
  pr_row("**Agent**"; $prs | map(select(agent_authored))),
  pr_row("**All**"; $prs),
  "",
  "Agent: the sources Hive (author `hive-worker-bee`, or a Hive or omp signature line in the body) and"
    + " Claude Code (CI) (head branch `claude/*`) of `scripts/lib/pr-source.jq`. Local agent runs that open"
    + " pull requests under the maintainer'"'"'s account without a signature count as Other.",
  "",
  "## ACMM gaps",
  "",
  "ACMM codebase level: \($acmm.level) (`scripts/acmm-self-check.py`).",
  "",
  "| Level | Met | Status | Still open |",
  "| --- | ---: | --- | --- |",
  ($acmm.levels[]
    | "| \(.name | cell) | \(.met) | \(.status) | "
      + (if .missing == [] then "—" else
          .missing
          | map(.key as $key
            | ($gap_issues | map(select(.body // "" | contains("`\($key)`"))) | sort_by(.number) | map("#\(.number)"))
                as $refs
            | "`\($key)` \(.name | cell)" + (if $refs == [] then "" else " (\($refs | join(", ")))" end))
          | join(", ")
        end)
      + " |")
'; then
  exit 1
fi
