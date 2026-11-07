#!/bin/sh
# Tests for workflow-permissions-check.sh: fixture workflows and policies that break each
# rule, then this repository's workflows against .github/policies/workflow-permissions.yml.
# Usage: scripts/workflow-permissions-check.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
check="$root/scripts/workflow-permissions-check.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
case_number=0

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# expect <description> <pass|fail> <expected output> <policy> [<workflow name> <workflow>]...
# Runs the check on a fresh directory holding the given workflows.
expect() {
  description="$1"
  want="$2"
  expected="$3"
  case_number=$((case_number + 1))
  dir="$tmp/$case_number"
  mkdir -p "$dir/workflows"
  printf '%s\n' "$4" >"$dir/policy.yml"
  shift 4
  while [ "$#" -ge 2 ]; do
    printf '%s\n' "$2" >"$dir/workflows/$1"
    shift 2
  done
  got=pass
  output="$(sh "$check" "$dir/policy.yml" "$dir/workflows" 2>&1)" || got=fail
  if [ "$got" != "$want" ]; then
    not_ok "$description (expected $want, got $got): $output"
  elif [ "$want" = fail ] && [ "$output" != "$expected" ]; then
    not_ok "$description (expected:
$expected
got:
$output)"
  else
    ok "$description"
  fi
}

default='default:
  contents: read'
read_only='on: push
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-latest
    steps: []'
labelling='on: push
permissions:
  contents: read
jobs:
  build:
    runs-on: ubuntu-latest
    steps: []
  label:
    runs-on: ubuntu-latest
    permissions:
      pull-requests: write
      contents: read
    steps: []'

expect 'a read-only workflow with an empty rule passes' pass '' \
  "$default
workflows:
  a.yml: {}" a.yml "$read_only"
expect 'a job grant matching its rule passes, in any key order' pass '' \
  "$default
workflows:
  a.yml:
    jobs:
      label: { contents: read, pull-requests: write }" a.yml "$labelling"
expect 'a .yaml workflow is checked too' fail \
  'b.yaml: no rule in the workflow permissions policy' \
  "$default
workflows:
  a.yml: {}" a.yml "$read_only" b.yaml "$read_only"
expect 'a workflow without a rule fails' fail \
  'b.yml: no rule in the workflow permissions policy' \
  "$default
workflows:
  a.yml: {}" a.yml "$read_only" b.yml "$read_only"
expect 'a rule for a missing workflow fails' fail \
  'gone.yml: the policy has a rule, but the workflow does not exist' \
  "$default
workflows:
  a.yml: {}
  gone.yml: {}" a.yml "$read_only"
expect 'a top-level write grant fails' fail \
  'a.yml: top-level permissions are {"contents":"write"}, expected {"contents":"read"}' \
  "$default
workflows:
  a.yml: {}" a.yml 'on: push
permissions:
  contents: write
jobs: {}'
expect 'a workflow without top-level permissions fails' fail \
  'a.yml: top-level permissions are null, expected {"contents":"read"}' \
  "$default
workflows:
  a.yml: {}" a.yml 'on: push
jobs: {}'
expect 'a top-level read-all fails' fail \
  'a.yml: top-level permissions are "read-all", expected {"contents":"read"}' \
  "$default
workflows:
  a.yml: {}" a.yml 'on: push
permissions: read-all
jobs: {}'
expect 'a job grant without a rule fails' fail \
  'a.yml: job label declares permissions {"contents":"read","pull-requests":"write"}, which the policy does not grant' \
  "$default
workflows:
  a.yml: {}" a.yml "$labelling"
expect 'a job grant wider than its rule fails' fail \
  'a.yml: job label permissions are {"contents":"read","pull-requests":"write"}, the policy grants {"contents":"read"}' \
  "$default
workflows:
  a.yml:
    jobs:
      label: { contents: read }" a.yml "$labelling"
expect 'a job grant narrower than its rule fails' fail \
  'a.yml: job label permissions are {"contents":"read","pull-requests":"write"}, the policy grants {"contents":"read","issues":"write","pull-requests":"write"}' \
  "$default
workflows:
  a.yml:
    jobs:
      label: { contents: read, issues: write, pull-requests: write }" a.yml "$labelling"
expect 'a rule for a job that declares no permissions fails' fail \
  'a.yml: job build declares no permissions, but the policy grants {"contents":"read"}' \
  "$default
workflows:
  a.yml:
    jobs:
      build: { contents: read }" a.yml "$read_only"
expect 'a rule naming a missing job fails' fail \
  'a.yml: the policy names job deploy, which the workflow does not have' \
  "$default
workflows:
  a.yml:
    jobs:
      deploy: { contents: write }" a.yml "$read_only"
expect 'a rule key other than jobs fails' fail \
  "a.yml: the policy rule has key 'steps'; only 'jobs' is allowed" \
  "$default
workflows:
  a.yml:
    steps: {}" a.yml "$read_only"
expect 'a policy without a default fails' fail \
  'the policy has no default permissions
a.yml: top-level permissions are {"contents":"read"}, expected null' \
  'workflows:
  a.yml: {}' a.yml "$read_only"
expect 'every violation is reported, not only the first' fail \
  'a.yml: top-level permissions are {"contents":"write"}, expected {"contents":"read"}
a.yml: job label declares permissions {"contents":"read","pull-requests":"write"}, which the policy does not grant
gone.yml: the policy has a rule, but the workflow does not exist' \
  "$default
workflows:
  a.yml: {}
  gone.yml: {}" a.yml "$(printf '%s\n' "$labelling" | sed 's/^  contents: read$/  contents: write/')"

# This repository's workflows against its policy.
if output="$(sh "$check" 2>&1)"; then
  ok "this repository's workflows match .github/policies/workflow-permissions.yml"
else
  not_ok "this repository's workflows do not match .github/policies/workflow-permissions.yml:
$output"
fi

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
