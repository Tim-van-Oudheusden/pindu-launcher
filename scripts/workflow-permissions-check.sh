#!/bin/sh
# Checks every workflow's GITHUB_TOKEN grants against the policy as code in
# .github/policies/workflow-permissions.yml. Prints one line per violation and exits 1 if
# there is any:
# - a workflow whose top-level `permissions:` block differs from the policy's `default`
# - a workflow with no rule, or a rule for a workflow that does not exist
# - a rule with a key other than `jobs`, or naming a job the workflow does not have
# - a job whose `permissions:` block differs from its rule in either direction, declares
#   one without a rule, or has a rule but declares none
# Usage: scripts/workflow-permissions-check.sh [policy file] [workflows directory]
# Needs yq 4 (mikefarah), which GitHub's Ubuntu runners ship.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
policy="${1:-$root/.github/policies/workflow-permissions.yml}"
workflows="${2:-$root/.github/workflows}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
violations="$tmp/violations"
: >"$violations"

violation() {
  echo "$1" >>"$violations"
}

# canonical <yq path> <file>: the YAML value at the path as one line of JSON, keys sorted.
canonical() {
  yq "($1) | sort_keys(.) | to_json(0)" "$2"
}

[ -r "$policy" ] || {
  echo "cannot read the workflow permissions policy $policy"
  exit 1
}
default="$(canonical .default "$policy")"
[ "$default" != null ] || violation "the policy has no default permissions"

for file in "$workflows"/*.yml "$workflows"/*.yaml; do
  [ -e "$file" ] || continue
  W="${file##*/}"
  export W
  top="$(canonical .permissions "$file")"
  [ "$top" = "$default" ] || violation "$W: top-level permissions are $top, expected $default"
  if [ "$(yq '.workflows | has(strenv(W))' "$policy")" != true ]; then
    violation "$W: no rule in the workflow permissions policy"
    continue
  fi
  yq '.workflows[strenv(W)] // {} | keys | .[] | select(. != "jobs")' "$policy" |
    while IFS= read -r key; do
      violation "$W: the policy rule has key '$key'; only 'jobs' is allowed"
    done
  # One `<job> <grants as canonical JSON>` line per job: every job the rule names, and every
  # job of the workflow that declares a `permissions:` block.
  yq '.workflows[strenv(W)].jobs // {} | to_entries | .[] | .key + " " + (.value | sort_keys(.) | to_json(0))' \
    "$policy" >"$tmp/want"
  yq '.jobs // {} | to_entries | .[] | select(.value | has("permissions")) |
    .key + " " + (.value.permissions | sort_keys(.) | to_json(0))' "$file" >"$tmp/got"
  while read -r job want; do
    if [ "$(J="$job" yq '.jobs | has(strenv(J))' "$file")" != true ]; then
      violation "$W: the policy names job $job, which the workflow does not have"
      continue
    fi
    got="$(grep "^$job " "$tmp/got" | cut -d' ' -f2-)" || true
    if [ -z "$got" ]; then
      violation "$W: job $job declares no permissions, but the policy grants $want"
    elif [ "$got" != "$want" ]; then
      violation "$W: job $job permissions are $got, the policy grants $want"
    fi
  done <"$tmp/want"
  while read -r job got; do
    grep -q "^$job " "$tmp/want" ||
      violation "$W: job $job declares permissions $got, which the policy does not grant"
  done <"$tmp/got"
done

yq '.workflows // {} | keys | .[]' "$policy" | while IFS= read -r name; do
  [ -e "$workflows/$name" ] || violation "$name: the policy has a rule, but the workflow does not exist"
done

if [ -s "$violations" ]; then
  cat "$violations"
  exit 1
fi
echo "ok: every workflow's permissions match $policy"
