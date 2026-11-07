#!/bin/sh
# Merge queue for main: merges ready pull requests one at a time, each after re-running the
# fast Rust and Meson checks on top of the latest main. Driven by
# .github/workflows/merge-queue.yml; docs/ai-ops-runbook.md says how to watch and stop it.
#
# Usage:
#   scripts/merge-queue.sh next
#       From a full-history checkout of main: pick the oldest pull request that is ready to
#       merge and write number=, head= and main= to $GITHUB_OUTPUT. Writes nothing when none
#       is ready.
#   scripts/merge-queue.sh land <number> <head-sha> <verified-main-sha>
#       Merge the pull request at <head-sha> if main is still the commit it was verified on and
#       the pull request is still ready, close the issues it links, then start main's push
#       workflows (a merge made with GITHUB_TOKEN starts none) and the next run.
#   scripts/merge-queue.sh reject <number> <run-url>
#       Hold a pull request that failed verification, link the run, move on.
#
# Ready means: from this repository (not a fork), not a draft, none of the KEEP_OUT_LABELS,
# merge state CLEAN (every required check passing and no other check red or pending, so
# UNSTABLE stays out; review threads resolved), and not high risk by main's
# scripts/tier-classify.sh and .github/risk-assessment.yml (docs/risk-tiers.md): high-risk
# changes wait for the maintainer. While main lacks either file, every pull request counts as
# high risk and the queue merges nothing.
# Needs gh authenticated (GH_TOKEN) and pointed at the repository (GH_REPO, or run from a
# checkout). Exits 2 on a wrong command line.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
# Labels that keep a pull request out of the queue; reject adds the first.
#   hold           the maintainer's stop sign, and what reject sets
#   blocked        waiting on something outside this repository
#   sentinel-alert Hive's flag on a sensitive change; a maintainer removes it
#   automated      opened by a workflow, which merges it or leaves it to the maintainer
# merge-queue.yml restates them in its concurrency group and pick job, and
# scripts/merge-queue.test.sh fails when either list differs from this one.
KEEP_OUT_LABELS='hold blocked sentinel-alert automated'
HOLD_LABEL="${KEEP_OUT_LABELS%% *}"
# Every workflow that runs on a push to main (release.yml runs on tags only); each needs a
# workflow_dispatch trigger. scripts/merge-queue.test.sh checks both against the workflows'
# own triggers. One missing from main (coverage-gate.yml before #241 lands) is not started.
MAIN_PUSH_WORKFLOWS='ci.yml coverage-gate.yml'

# jq test that a pull request carries no label that keeps it out.
unheld='all(.labels[]; '
for label in $KEEP_OUT_LABELS; do
  unheld="$unheld.name != \"$label\" and "
done
unheld="${unheld}true)"

# GitHub's workflow dispatch answers HTTP 500 now and then, so one failure is not final. The
# last failure names the workflow and how to start it by hand.
start_workflow() {
  tries=1
  until gh workflow run "$1" --ref main; do
    if [ "$tries" -ge 3 ]; then
      echo "::error title=Merge queue::could not start $1; start it by hand: gh workflow run $1 --ref main"
      return 1
    fi
    tries=$((tries + 1))
    sleep 10
  done
}

# Picks up runs that land, reject or skip a pull request, so each merge is verified against
# the main it lands on.
queue_next_run() {
  start_workflow merge-queue.yml
}

# GitHub computes the merge state lazily and answers UNKNOWN until it has. A failed gh call
# fails the function rather than reading as a pull request that is not ready; callers assign
# the result to a variable, so the failure stops the run.
merge_state() {
  asks=1
  while :; do
    state="$(gh pr view "$1" --json mergeStateStatus --jq .mergeStateStatus)" || return 1
    if [ "$state" != UNKNOWN ] || [ "$asks" -ge 3 ]; then
      break
    fi
    asks=$((asks + 1))
    sleep 5
  done
  echo "$state"
}

# Prints the risk tier of the change from main to <sha> as tier-classifier.yml rates it: from
# the merge base, with renamed files listed under their old path too.
tier_of() {
  paths="$(git -c core.quotePath=false diff --name-only --no-renames "HEAD...$1")" || return 1
  rated="$(printf '%s\n' "$paths" | sh "$root/scripts/tier-classify.sh")" || return 1
  printf '%s\n' "$rated" | sed -n 's/^tier=//p'
}

# GitHub does not act on `Fixes #N` when GITHUB_TOKEN merges a pull request, so close the
# issues it links ourselves. By URL, so a link to another repository closes that issue, not
# this repository's issue with the same number. The merge has landed by now: failing to read
# the links or to close an issue is a warning, not a reason to skip main's workflows or the
# next run.
close_linked_issues() {
  if ! issues="$(gh pr view "$1" --json closingIssuesReferences --jq '.closingIssuesReferences[].url')"; then
    echo "::warning title=Merge queue::could not read the issues #$1 closes; close them by hand"
    return 0
  fi
  while read -r issue; do
    [ -n "$issue" ] || continue
    gh issue close "$issue" --reason completed \
      --comment "Closed by #$1, which the merge queue merged." ||
      echo "::warning title=Merge queue::could not close $issue after merging #$1"
  done <<EOF
$issues
EOF
}

next() {
  if [ ! -f "$root/scripts/tier-classify.sh" ] || [ ! -f "$root/.github/risk-assessment.yml" ]; then
    echo "::notice title=Merge queue::main has no scripts/tier-classify.sh or .github/risk-assessment.yml, so every pull request counts as high risk and waits for the maintainer"
    return 0
  fi
  # gh lists open pull requests by default; --jq runs gh's built-in jq, so the runner needs
  # no jq. Listed into a variable so a failed listing fails the job instead of passing for an
  # empty queue. gh lists newest first, so the limit sits far above the number of pull
  # requests ever open at once.
  candidates="$(gh pr list --base main --limit 1000 \
    --json number,headRefOid,isDraft,isCrossRepository,labels \
    --jq "sort_by(.number) | .[]
      | select((.isDraft or .isCrossRepository) | not)
      | select($unheld)
      | \"\\(.number) \\(.headRefOid)\"")"
  while read -r number head; do
    # An empty listing still reads one empty line.
    [ -n "$number" ] || continue
    state="$(merge_state "$number")"
    # Required checks pass here, so nothing else on the pull request says why it waits.
    if [ "$state" = UNSTABLE ]; then
      echo "::notice title=Merge queue::#$number is UNSTABLE: a check that is not required is red or pending; it stays out of the queue until that check passes"
    fi
    [ "$state" = CLEAN ] || continue
    git fetch --no-tags --quiet origin "refs/pull/$number/head"
    # A push since the listing: the run its checks start sees it.
    [ "$(git rev-parse FETCH_HEAD)" = "$head" ] || continue
    tier="$(tier_of "$head")"
    if [ "$tier" = high ]; then
      echo "::notice title=Merge queue::#$number is high risk (docs/risk-tiers.md); the maintainer merges it"
      continue
    fi
    {
      echo "number=$number"
      echo "head=$head"
      echo "main=$(git rev-parse HEAD)"
    } >>"$GITHUB_OUTPUT"
    return 0
  done <<EOF
$candidates
EOF
}

land() {
  main="$(git ls-remote origin refs/heads/main)"
  main="${main%%[[:space:]]*}"
  if [ "$main" != "$3" ]; then
    echo "::notice title=Merge queue::main moved since #$1 was verified; verifying again"
    queue_next_run
    return 0
  fi

  ready="$(gh pr view "$1" --json isDraft,labels --jq "(.isDraft | not) and $unheld")"
  state="$(merge_state "$1")"
  if [ "$ready" != true ] || [ "$state" != CLEAN ]; then
    echo "::notice title=Merge queue::#$1 stopped being ready during verification; skipped"
    queue_next_run
    return 0
  fi

  # --match-head-commit refuses the merge if the branch moved past what was verified.
  gh pr merge "$1" --merge --match-head-commit "$2"
  close_linked_issues "$1"
  # The merge has landed, so a workflow that will not start must not stop the others or the
  # next run. The run still fails: a gate that never ran on main leaves no other trace,
  # unlike an issue left open.
  failed=0
  for workflow in $MAIN_PUSH_WORKFLOWS; do
    [ -f "$root/.github/workflows/$workflow" ] || continue
    start_workflow "$workflow" || failed=1
  done
  queue_next_run || failed=1
  return "$failed"
}

reject() {
  gh pr edit "$1" --add-label "$HOLD_LABEL"
  gh pr comment "$1" --body "The merge queue held this pull request: the Rust and Meson checks failed on it merged with the latest main ($2). Fix it and remove the \`$HOLD_LABEL\` label to queue it again."
  queue_next_run
}

usage() {
  echo "usage: $0 next | land <number> <head-sha> <verified-main-sha> | reject <number> <run-url>" >&2
  exit 2
}

case "${1:-}" in
  next)
    [ "$#" -eq 1 ] || usage
    next
    ;;
  land)
    [ "$#" -eq 4 ] || usage
    land "$2" "$3" "$4"
    ;;
  reject)
    [ "$#" -eq 3 ] || usage
    reject "$2" "$3"
    ;;
  *) usage ;;
esac
