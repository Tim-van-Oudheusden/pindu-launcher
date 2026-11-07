#!/bin/sh
# Tests for merge-queue.sh, run offline: a stub `gh` answers from a JSON listing and records
# every other call, and the pull requests are real commits in a throwaway origin repository
# whose `refs/pull/<n>/head` carry each head, as GitHub's do. Main's risk classifier is a stub
# that rates .github/, scripts/ and build-aux/ high and everything else medium. Then checks
# .github/workflows/merge-queue.yml and the other workflows against the script.
# Needs git, jq and yq 4 (all three ship on GitHub's Ubuntu runners).
# Usage: scripts/merge-queue.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/merge-queue.sh"
queue_yml="$root/.github/workflows/merge-queue.yml"
for tool in git jq yq; do
  command -v "$tool" >/dev/null || {
    echo "merge-queue.test.sh: needs $tool" >&2
    exit 1
  }
done
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
nl='
'
run_url=https://github.com/owner/pindu/actions/runs/42

GIT_CONFIG_GLOBAL=/dev/null
GIT_CONFIG_NOSYSTEM=1
GIT_AUTHOR_NAME=Test
GIT_AUTHOR_EMAIL=test@example.com
GIT_COMMITTER_NAME=Test
GIT_COMMITTER_EMAIL=test@example.com
export GIT_CONFIG_GLOBAL GIT_CONFIG_NOSYSTEM GIT_AUTHOR_NAME GIT_AUTHOR_EMAIL GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

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

# check_has <description> <text> <fragment>
check_has() {
  case "$2" in
    *"$3"*) ok "$1" ;;
    *) not_ok "$1 (lacks '$3' in:$nl$2)" ;;
  esac
}

# check_lacks <description> <text> <fragment>
check_lacks() {
  case "$2" in
    *"$3"*) not_ok "$1 (has '$3' in:$nl$2)" ;;
    *) ok "$1" ;;
  esac
}

# The space-separated value of `<NAME>='...'` in the script.
script_list() {
  sed -n "s/^$1='\\(.*\\)'\$/\\1/p" "$script"
}
keep_out_labels="$(script_list KEEP_OUT_LABELS)"
main_push_workflows="$(script_list MAIN_PUSH_WORKFLOWS)"

# Stands in for the gh CLI. `gh pr list` and `gh pr view <n>` answer from $FAKE_DIR/prs.json,
# filtered through `--jq` as gh does. Every other call is appended to $FAKE_DIR/calls.log, one
# line per call. A call fails when it starts with the text in $FAKE_DIR/fail, or only the first
# such call does if $FAKE_DIR/fail-once exists. While $FAKE_DIR/unknown-<n> holds a count above
# 0, asking for the merge state of pull request <n> answers UNKNOWN, as GitHub does before it
# has computed it, and takes one off the count.
cat >"$tmp/gh" <<'EOF'
#!/bin/sh
set -eu
fails() {
  [ -f "$FAKE_DIR/fail" ] || return 1
  prefix="$(cat "$FAKE_DIR/fail")"
  case "$*" in "$prefix"*) ;; *) return 1 ;; esac
  [ ! -f "$FAKE_DIR/fail-once" ] || rm "$FAKE_DIR/fail"
}
if [ "$1" = pr ] && { [ "$2" = list ] || [ "$2" = view ]; }; then
  if fails "$@"; then
    echo "gh: HTTP 502" >&2
    exit 1
  fi
  sub="$2"
  shift 2
  number=0
  filter=.
  while [ $# -gt 0 ]; do
    case "$1" in
      --jq) filter="$2"; shift 2 ;;
      --*) shift 2 ;;
      *) number="$1"; shift ;;
    esac
  done
  countdown="$FAKE_DIR/unknown-$number"
  left=0
  [ ! -f "$countdown" ] || left="$(cat "$countdown")"
  if [ "$sub" = view ] && [ "$filter" = .mergeStateStatus ] && [ "$left" -gt 0 ]; then
    echo $((left - 1)) >"$countdown"
    echo UNKNOWN
  elif [ "$sub" = view ]; then
    jq -r --argjson n "$number" ".[] | select(.number == \$n) | $filter" "$FAKE_DIR/prs.json"
  else
    jq -r "$filter" "$FAKE_DIR/prs.json"
  fi
  exit 0
fi
echo "$*" >>"$FAKE_DIR/calls.log"
if fails "$@"; then
  exit 1
fi
EOF
# The script waits between attempts; the tests only record how long.
cat >"$tmp/sleep" <<'EOF'
#!/bin/sh
echo "$1" >>"$FAKE_DIR/sleeps.log"
EOF
# Main's risk classifier, as tier-classify.sh's interface has it: paths on stdin, `tier=` last.
cat >"$tmp/tier-classify.sh" <<'EOF'
#!/bin/sh
max=medium
while IFS= read -r path; do
  case "$path" in .github/* | scripts/* | build-aux/*) max=high ;; esac
done
echo "tier=$max"
EOF
chmod +x "$tmp/gh" "$tmp/sleep"

# commit <repo> <message> [<path>=<content>]...: writes the files, commits them.
commit() {
  repo="$1"
  message="$2"
  shift 2
  for file in "$@"; do
    mkdir -p "$repo/$(dirname "${file%%=*}")"
    printf '%s\n' "${file#*=}" >"$repo/${file%%=*}"
  done
  git -C "$repo" add --all
  git -C "$repo" commit --quiet -m "$message"
}

# new_queue <name>: a bare origin with a main, and a seed repository to make pull requests in.
# The queue's main has the classifier and the risk assessment, and the workflows in
# $main_workflows (default: every one in MAIN_PUSH_WORKFLOWS).
new_queue() {
  work="$tmp/$1"
  mkdir -p "$work"
  heads=''
  git init --quiet --bare --initial-branch=main "$work/origin.git"
  git init --quiet --initial-branch=main "$work/seed"
  commit "$work/seed" main README.md=Pindu build-aux/release.sh=release
  git -C "$work/seed" push --quiet "$work/origin.git" main
  with_classifier=1
  main_workflows="$main_push_workflows"
}

# add_pr <number> [<path>=<content>]...: a pull request off main changing those files (an app
# file by default), pushed to refs/pull/<number>/head.
add_pr() {
  number="$1"
  shift
  git -C "$work/seed" switch --quiet --create "pr-$number" main
  if [ "$#" -eq 0 ]; then
    set -- "crates/pr_$number.rs=fn main() {}"
  fi
  commit "$work/seed" "pr $number" "$@"
  git -C "$work/seed" push --quiet "$work/origin.git" "HEAD:refs/pull/$number/head"
  heads="$heads${heads:+,}\"$number\":\"$(head_of "$number")\""
}

head_of() {
  git -C "$work/seed" rev-parse "pr-$1"
}

# move_main: a new commit on main in origin.
move_main() {
  git -C "$work/seed" switch --quiet main
  commit "$work/seed" "$1" "$2"
  git -C "$work/seed" push --quiet "$work/origin.git" main
}

# listing <json>: what gh lists, from entries with a number and optionally isDraft,
# isCrossRepository, labels, mergeStateStatus and closingIssues (issue numbers).
listing() {
  jq -n --argjson prs "$1" --argjson heads "{$heads}" '[$prs[] | {
    number,
    headRefOid: $heads[.number | tostring],
    isDraft: (.isDraft // false),
    isCrossRepository: (.isCrossRepository // false),
    labels: [(.labels // [])[] | {name: .}],
    mergeStateStatus: (.mergeStateStatus // "CLEAN"),
    closingIssuesReferences: [(.closingIssues // [])[] | {number: ., url: "https://github.com/owner/pindu/issues/\(.)"}]
  }]' >"$work/prs.json"
}

fail_calls_starting_with() {
  printf '%s' "$1" >"$work/fail"
}

fail_first_call_starting_with() {
  fail_calls_starting_with "$1"
  : >"$work/fail-once"
}

# unknown_merge_state <number> <times>: GitHub answers UNKNOWN for the next <times> asks.
unknown_merge_state() {
  echo "$2" >"$work/unknown-$1"
}

# run_queue <args>...: clones origin's main as the workflow checks it out, with this
# merge-queue.sh in it, and runs it there. Sets status, out, err, outputs (GITHUB_OUTPUT),
# calls (one line per recorded gh call) and sleeps.
run_queue() {
  rm -rf "$work/clone" "$work/calls.log"
  git clone --quiet "$work/origin.git" "$work/clone"
  mkdir -p "$work/clone/scripts" "$work/clone/.github/workflows"
  cp "$script" "$work/clone/scripts/merge-queue.sh"
  if [ "$with_classifier" = 1 ]; then
    cp "$tmp/tier-classify.sh" "$work/clone/scripts/tier-classify.sh"
    printf 'rules:\n' >"$work/clone/.github/risk-assessment.yml"
  fi
  for main_workflow in $main_workflows; do
    printf 'on:\n  push:\n' >"$work/clone/.github/workflows/$main_workflow"
  done
  : >"$work/github-output"
  : >"$work/sleeps.log"
  status=0
  (cd "$work/clone" && PATH="$tmp:$PATH" FAKE_DIR="$work" GITHUB_OUTPUT="$work/github-output" \
    sh scripts/merge-queue.sh "$@" >"$work/stdout" 2>"$work/stderr") || status=$?
  out="$(cat "$work/stdout")"
  err="$(cat "$work/stderr")"
  outputs="$(cat "$work/github-output")"
  calls="$(cat "$work/calls.log" 2>/dev/null || true)"
  sleeps="$(cat "$work/sleeps.log")"
}

main_sha() {
  git -C "$work/origin.git" rev-parse main
}

merge_call() {
  echo "pr merge $1 --merge --match-head-commit $(head_of "$1")"
}

# The calls land makes to start main's push workflows present on main, then the next run.
starts() {
  for main_workflow in $main_workflows; do
    echo "workflow run $main_workflow --ref main"
  done
  echo "workflow run merge-queue.yml --ref main"
}

calls_starting() {
  printf '%s\n' "$calls" | grep -c "^$1" || true
}

# --- next ------------------------------------------------------------------------------------

new_queue oldest
for n in 9 3 4 5 6 7; do add_pr "$n"; done
listing '[{"number":9},{"number":3,"isDraft":true},{"number":4,"isCrossRepository":true},
  {"number":5,"labels":["hold"]},{"number":6,"mergeStateStatus":"BLOCKED"},{"number":7,"labels":["area:ui"]}]'
run_queue next
check 'next picks the oldest same-repo ready pull request' \
  "number=7${nl}head=$(head_of 7)${nl}main=$(main_sha)" "$outputs"
check 'next exits 0 after a pick' 0 "$status"

for label in $keep_out_labels; do
  new_queue "keep-out-$label"
  add_pr 2
  add_pr 8
  listing "[{\"number\":2,\"labels\":[\"$label\"]},{\"number\":8}]"
  run_queue next
  check "next leaves a pull request labelled $label and picks the next one" \
    "number=8${nl}head=$(head_of 8)${nl}main=$(main_sha)" "$outputs"
done

# `hold` first: reject adds it. `sentinel-alert` is Hive's flag on a sensitive change, `blocked`
# marks a pull request waiting on something outside the repository, `automated` one a workflow
# opened, whose merge policy is its own.
check 'the keep-out labels are the ones the maintainer uses to stop a merge' \
  'hold blocked sentinel-alert automated' "$keep_out_labels"

new_queue high-risk
add_pr 2 crates/a.rs=x .github/workflows/ci.yml=ci
add_pr 8 crates/b.rs=y
listing '[{"number":2},{"number":8}]'
run_queue next
check 'next leaves a high-risk pull request and picks the next one' \
  "number=8${nl}head=$(head_of 8)${nl}main=$(main_sha)" "$outputs"
check_has 'next says why it left the high-risk one' "$out" '::notice title=Merge queue::#2 is high risk'

# Moving a file out of a high-risk path still changes it; rename detection would list only
# the new, medium path.
new_queue renamed
git -C "$work/seed" switch --quiet --create pr-5 main
git -C "$work/seed" mv build-aux/release.sh release.sh
git -C "$work/seed" commit --quiet -m move
git -C "$work/seed" push --quiet "$work/origin.git" HEAD:refs/pull/5/head
heads="\"5\":\"$(head_of 5)\""
listing '[{"number":5}]'
run_queue next
check 'next rates a file moved out of a high-risk path by its old path' '' "$outputs"

# A workflow change merged to main after the pull request branched is not part of it:
# tier-classifier.yml rates BASE...HEAD, from the merge base, and so does the queue.
new_queue main-moved-high
add_pr 5
move_main workflow .github/workflows/other.yml=other
listing '[{"number":5}]'
run_queue next
check "next rates only the pull request's own changes when main moved since it branched" \
  "number=5${nl}head=$(head_of 5)${nl}main=$(main_sha)" "$outputs"

new_queue no-classifier
add_pr 5
listing '[{"number":5}]'
with_classifier=0
run_queue next
check 'next picks nothing while main has no risk classifier' '' "$outputs"
check 'next passes while main has no risk classifier' 0 "$status"
check_has 'next says every pull request counts as high risk without a classifier' "$out" 'counts as high risk'

# UNSTABLE: every required check passed and another did not, so nothing else says why the pull
# request waits. The queue still requires CLEAN: picking it would merge past a red check.
new_queue unstable
add_pr 4
listing '[{"number":4,"mergeStateStatus":"UNSTABLE"}]'
run_queue next
check_has 'next reports a pull request held back by a check that is not required' \
  "$out" '::notice title=Merge queue::#4 is UNSTABLE'
check 'next does not pick an UNSTABLE pull request' '' "$outputs"
check 'next makes no call for an UNSTABLE pull request' '' "$calls"

# GitHub answers UNKNOWN until it has computed the merge state; taking the first answer would
# skip a ready pull request until something starts the queue again.
new_queue unknown
add_pr 7
listing '[{"number":7}]'
unknown_merge_state 7 2
run_queue next
check 'next asks again while GitHub is still computing the merge state' \
  "number=7${nl}head=$(head_of 7)${nl}main=$(main_sha)" "$outputs"
check 'next waits 5 s between asks' "5${nl}5" "$sleeps"

new_queue unknown-thrice
add_pr 7
listing '[{"number":7}]'
unknown_merge_state 7 3
run_queue next
check 'next gives up on a merge state still unknown after three asks' '' "$outputs"
check 'next asks three times' "5${nl}5" "$sleeps"
check 'next passes after giving up on an unknown merge state' 0 "$status"

new_queue none-ready
add_pr 4
listing '[{"number":4,"mergeStateStatus":"BLOCKED"}]'
run_queue next
check 'next picks nothing when no pull request is ready' '' "$outputs"
check 'next makes no call when no pull request is ready' '' "$calls"

new_queue none-open
listing '[]'
run_queue next
check 'next picks nothing when no pull request is open' '' "$outputs"
check 'next passes when no pull request is open' 0 "$status"

new_queue pushed
add_pr 4
add_pr 5
git -C "$work/seed" switch --quiet pr-4
commit "$work/seed" push crates/more.rs=more
git -C "$work/seed" push --quiet --force "$work/origin.git" HEAD:refs/pull/4/head
listing '[{"number":4},{"number":5}]'
run_queue next
check 'next skips a pull request pushed to since it was listed' \
  "number=5${nl}head=$(head_of 5)${nl}main=$(main_sha)" "$outputs"

# An empty listing is an empty queue; a failed one must not pass for it.
new_queue list-fails
add_pr 7
listing '[{"number":7}]'
fail_calls_starting_with 'pr list'
run_queue next
check 'next fails when listing fails' 1 "$status"
check 'next picks nothing when listing fails' '' "$outputs"

# A failed read is not a pull request that is merely not ready (Dailies#410).
new_queue state-fails
add_pr 7
listing '[{"number":7}]'
fail_calls_starting_with 'pr view 7 --json mergeStateStatus'
run_queue next
check 'next fails when a merge state cannot be read' 1 "$status"
check 'next picks nothing when a merge state cannot be read' '' "$outputs"

# --- land ------------------------------------------------------------------------------------

# A merge made with GITHUB_TOKEN starts no push workflow, so land starts every workflow that
# runs on a push to main, or that gate never runs for queue merges.
new_queue land
add_pr 7
listing '[{"number":7}]'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land passes' 0 "$status"
check 'land merges the verified head, then starts main push workflows and the next run' \
  "$(merge_call 7)${nl}$(starts)" "$calls"

new_queue land-missing-workflow
add_pr 7
listing '[{"number":7}]'
main_workflows=ci.yml
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land starts only the main push workflows main has' \
  "$(merge_call 7)${nl}workflow run ci.yml --ref main${nl}workflow run merge-queue.yml --ref main" "$calls"
check 'land passes when a listed workflow is not on main yet' 0 "$status"

# A refused merge (the head moved, a token without write access) must not close the linked
# issues or start main's push workflows and the next run as if it had landed.
new_queue land-refused
add_pr 7
listing '[{"number":7,"closingIssues":[12]}]'
fail_calls_starting_with 'pr merge'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land fails when the merge is refused' 1 "$status"
check 'land closes nothing and starts nothing when the merge is refused' "$(merge_call 7)" "$calls"

# GitHub does not close an issue linked with `Fixes #N` when GITHUB_TOKEN merges.
new_queue land-closes
add_pr 7
listing '[{"number":7,"closingIssues":[12,13]}]'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land closes every issue the merged pull request links, after merging it' \
  "$(merge_call 7)
issue close https://github.com/owner/pindu/issues/12 --reason completed --comment Closed by #7, which the merge queue merged.
issue close https://github.com/owner/pindu/issues/13 --reason completed --comment Closed by #7, which the merge queue merged.
$(starts)" "$calls"

new_queue land-close-fails
add_pr 7
listing '[{"number":7,"closingIssues":[12,13]}]'
fail_calls_starting_with 'issue close https://github.com/owner/pindu/issues/12'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land passes when closing an issue fails' 0 "$status"
check 'land tries every linked issue when closing one fails' 2 "$(calls_starting 'issue close ')"
check_has 'land warns about the issue it could not close' "$out" \
  '::warning title=Merge queue::could not close https://github.com/owner/pindu/issues/12 after merging #7'
check 'land still starts main push workflows and the next run when closing an issue fails' \
  "$(starts)" "$(printf '%s\n' "$calls" | grep '^workflow run ')"

new_queue land-links-fail
add_pr 7
listing '[{"number":7,"closingIssues":[12]}]'
fail_calls_starting_with 'pr view 7 --json closingIssuesReferences'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land passes when reading the linked issues fails' 0 "$status"
check_has 'land warns when reading the linked issues fails' "$out" \
  "::warning title=Merge queue::could not read the issues #7 closes"
check 'land still starts main push workflows and the next run when reading links fails' \
  "$(merge_call 7)${nl}$(starts)" "$calls"

# The merge has landed by now, and with GITHUB_TOKEN these runs are the only way main's gates
# run on it (Dailies#392).
new_queue land-retry
add_pr 7
listing '[{"number":7}]'
fail_first_call_starting_with 'workflow run ci.yml'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land passes when a workflow starts on the second try' 0 "$status"
check_lacks 'land reports no error after a successful retry' "$out" '::error'
check 'land tries a workflow again when it does not start the first time' 2 \
  "$(calls_starting 'workflow run ci.yml --ref main')"
check 'land waits 10 s before trying again' 10 "$sleeps"

for workflow in $main_push_workflows merge-queue.yml; do
  new_queue "land-start-fails-$workflow"
  add_pr 7
  listing '[{"number":7}]'
  fail_calls_starting_with "workflow run $workflow"
  run_queue land 7 "$(head_of 7)" "$(main_sha)"
  check "land fails when $workflow will not start" 1 "$status"
  check_has "land names $workflow and how to start it by hand" "$out" \
    "::error title=Merge queue::could not start $workflow; start it by hand: gh workflow run $workflow --ref main"
  check "land tries $workflow three times" 3 "$(calls_starting "workflow run $workflow --ref main")"
  check "land waits 10 s between tries of $workflow" "10${nl}10" "$sleeps"
  check "land starts the rest when $workflow will not start" "$(starts)" \
    "$(printf '%s\n' "$calls" | grep '^workflow run ' | uniq)"
done

new_queue land-main-moved
add_pr 7
listing '[{"number":7,"closingIssues":[12]}]'
verified_main="$(main_sha)"
move_main other docs/other.md=other
run_queue land 7 "$(head_of 7)" "$verified_main"
check 'land passes when main moved after verification' 0 "$status"
check 'land does not merge when main moved after verification, and queues a rerun' \
  'workflow run merge-queue.yml --ref main' "$calls"

for change in '"labels":["hold"]' '"labels":["blocked"]' '"labels":["sentinel-alert"]' \
  '"labels":["automated"]' '"isDraft":true' '"mergeStateStatus":"BLOCKED"'; do
  new_queue land-not-ready
  add_pr 7
  listing "[{\"number\":7,\"closingIssues\":[12],$change}]"
  run_queue land 7 "$(head_of 7)" "$(main_sha)"
  check "land passes when the pull request stopped being ready ($change)" 0 "$status"
  check "land does not merge a pull request that stopped being ready ($change)" \
    'workflow run merge-queue.yml --ref main' "$calls"
  rm -rf "$work"
done

new_queue land-state-fails
add_pr 7
listing '[{"number":7,"closingIssues":[12]}]'
fail_calls_starting_with 'pr view 7 --json mergeStateStatus'
run_queue land 7 "$(head_of 7)" "$(main_sha)"
check 'land fails when the merge state cannot be read' 1 "$status"
check_lacks 'land does not call an unreadable pull request not ready' "$out" 'stopped being ready'
check 'land merges nothing when the merge state cannot be read' '' "$calls"

# --- reject ----------------------------------------------------------------------------------

new_queue reject
add_pr 7
listing '[{"number":7}]'
run_queue reject 7 "$run_url"
check 'reject passes' 0 "$status"
check 'reject holds the pull request first' 'pr edit 7 --add-label hold' "$(printf '%s\n' "$calls" | sed -n 1p)"
check_has 'reject links the failed run in a comment' "$(printf '%s\n' "$calls" | sed -n 2p)" \
  "pr comment 7 --body The merge queue held this pull request"
check_has 'reject comment carries the run URL' "$calls" "$run_url"
check 'reject moves on to the next run' 'workflow run merge-queue.yml --ref main' \
  "$(printf '%s\n' "$calls" | sed -n '3,$p')"

# --- usage -----------------------------------------------------------------------------------

new_queue usage
for args in '' bogus 'next extra' 'land 7' 'reject 7'; do
  # Word splitting is the point: each case is a command line.
  # shellcheck disable=SC2086
  run_queue $args
  check "'$args' exits 2" 2 "$status"
  check_has "'$args' prints usage" "$err" 'usage:'
done

# --- workflows -------------------------------------------------------------------------------

# GitHub expressions cannot read a shell variable, so merge-queue.yml restates KEEP_OUT_LABELS
# in its concurrency group and in the pick job's `if`. A partial edit would stall the queue, or
# let a run that picks nothing replace a pending one that would merge.
expected_list="$(jq -cn --arg labels "$keep_out_labels" '$labels | split(" ")')"
for where in .concurrency.group .jobs.pick.if; do
  lists="$(yq "$where" "$queue_yml" | grep -o "fromJSON('[^']*')" | sed "s/^fromJSON('//; s/')\$//" | jq -c .)"
  check "the label list in $where is KEEP_OUT_LABELS" "$expected_list" "$lists"
  expression="$(yq "$where" "$queue_yml" | tr -s ' \n' '  ')"
  check_has "only a successful workflow run starts the queue ($where)" "$expression" \
    "(github.event_name == 'workflow_run' && github.event.workflow_run.conclusion == 'success')"
  check_has "a fork's pull request event neither starts nor joins the queue ($where)" "$expression" \
    "(github.event_name == 'pull_request' && github.event.pull_request.head.repo.full_name == github.repository && ("
  check_has "a label only starts the queue when a person removed it ($where)" "$expression" \
    "github.event.sender.type == 'User'"
done
check 'the queue never cancels a run in progress' false "$(yq '.concurrency."cancel-in-progress"' "$queue_yml")"
check 'the queue starts on workflow runs, ready drafts, removed labels and by hand' \
  'workflow_run pull_request workflow_dispatch' "$(yq '.on | keys | join(" ")' "$queue_yml")"
check 'pull request events are a draft marked ready or a label removed' \
  'ready_for_review unlabeled' "$(yq '.on.pull_request.types | join(" ")' "$queue_yml")"

# workflow_run matches workflows by name: a required check in a workflow the queue does not
# watch would leave a pull request whose last check came from there waiting for an unrelated
# event. A job name holding `${{ matrix.x }}` matches each required check it expands to. Only
# workflows that run on pull requests count: release.yml reuses CI's job names on tags.
watched="$(yq '.on.workflow_run.workflows[]' "$queue_yml")"
required="$(jq -r '.rules[] | select(.type == "required_status_checks") | .parameters.required_status_checks[].context' \
  "$root/.github/rulesets/main.json")"
for file in "$root"/.github/workflows/*.yml; do
  name="$(yq '.name' "$file")"
  holds=''
  [ "$(yq '.on | has("pull_request") or has("pull_request_target")' "$file")" = true ] || continue
  for job in $(yq '.jobs | keys | .[]' "$file"); do
    pattern="$(yq ".jobs.\"$job\".name // \"$job\"" "$file" | sed 's/\${{[^}]*}}/*/g')"
    while IFS= read -r context; do
      # Unquoted: the pattern is a glob.
      # shellcheck disable=SC2254
      case "$context" in $pattern) holds=1 ;; esac
    done <<EOF
$required
EOF
  done
  [ -z "$holds" ] && continue
  if printf '%s\n' "$watched" | grep -qxF "$name"; then
    ok "the queue watches $name, which holds a required check"
  else
    not_ok "the queue does not watch $name ($(basename "$file")), which holds a required check"
  fi
done

# A merge made with GITHUB_TOKEN starts no push workflow, so land starts each one, and `gh
# workflow run` fails on a workflow without a workflow_dispatch trigger.
for file in "$root"/.github/workflows/*.yml; do
  workflow="$(basename "$file")"
  on_main="$(yq '.on | [has("push"), ((.push.branches // ["main"]) | contains(["main"])), ((.push // {}) | has("tags") | not)] | all' "$file")"
  listed=''
  case " $main_push_workflows " in *" $workflow "*) listed=1 ;; esac
  if [ "$on_main" = true ] && [ -z "$listed" ]; then
    not_ok "$workflow runs on a push to main but is not in MAIN_PUSH_WORKFLOWS"
  elif [ -n "$listed" ]; then
    check "$workflow, started by land, can be started by hand" true "$(yq '.on | has("workflow_dispatch")' "$file")"
  fi
done

# Verification runs the pull request's code merged onto main, so it keeps no token. pick, land
# and reject check out main, so the jobs with write access run main's scripts.
check 'verify keeps no token in its checkout' false \
  "$(yq '.jobs.verify.steps[] | select(.uses == "actions/checkout@*") | .with."persist-credentials"' "$queue_yml")"
check 'verify gets no permissions beyond the read-only default' null "$(yq '.jobs.verify.permissions' "$queue_yml")"
check 'verify restores the cache but never saves it' 'actions/checkout@v4 actions/cache/restore@v4' \
  "$(yq '[.jobs.verify.steps[] | select(has("uses")) | .uses] | join(" ")' "$queue_yml")"
for job in pick land reject; do
  check "$job checks out main only" main \
    "$(yq ".jobs.$job.steps[] | select(.uses == \"actions/checkout@*\") | .with.ref" "$queue_yml")"
done
check 'land needs a passed verification' 'pick verify' "$(yq '.jobs.land.needs | join(" ")' "$queue_yml")"
check 'reject runs only when verification failed' "always() && needs.verify.result == 'failure'" \
  "$(yq '.jobs.reject.if' "$queue_yml")"

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
