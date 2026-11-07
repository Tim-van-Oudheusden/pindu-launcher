#!/bin/sh
# Print a Markdown nightly compliance report for the repository to stdout.
#
# Usage:
#   scripts/nightly-compliance.sh > nightly-compliance.md
#
# Checks that can go stale without a code change, so they run on a schedule
# (.github/workflows/nightly-compliance.yml) rather than only on pull requests:
#   1. Dependabot alerts (high+): no open high or critical Dependabot alert,
#      except those in ALERTS_IGNORE. Alerts come from
#      `gh api repos/$GITHUB_REPOSITORY/dependabot/alerts` (needs a token that
#      can read Dependabot alerts), or from the JSON array in the file named by
#      ALERTS_JSON when set.
#   2. RustSec audit of Cargo.lock: `cargo-audit audit --file Cargo.lock`
#      (cargo-audit 0.22.2 on PATH) finds no vulnerability, except the
#      advisories in AUDIT_IGNORE. Unmaintained crates only warn here; CI's
#      cargo-deny job and deny.toml decide on those for every pull request.
#   3. GNOME runtime supported: `flatpak remote-info --system flathub <ref>`
#      shows no End-of-life line for the runtime the app targets. The ref is
#      RUNTIME_REF, default org.gnome.Platform//51; bump it with the manifests'
#      runtime-version. Needs flatpak and the system-wide flathub remote (the
#      CI image has it); --system keeps a user remote of the same name from
#      making flatpak ask which one to use.
#   4. Workflows: actionlint (1.7.12 on PATH) finds nothing in
#      .github/workflows, with the ignores in .github/actionlint.yaml.
#   5. Branch ruleset: the live ruleset RULESET_ID (default 26762163) has the
#      rule types, rule parameters, required checks, enforcement and
#      conditions of RULESET_FILE (default .github/rulesets/main.json).
#      Parameters the API adds that the file leaves out are not compared. The
#      live ruleset comes from `gh api`, or from the file RULESET_JSON names.
#      Only a token with the Administration (read) permission sees the bypass
#      list, which the GITHUB_TOKEN cannot have: the bypass actors are compared
#      when the API returns them, and otherwise left unchecked with a note.
#   6. ACMM codebase level: scripts/acmm-self-check.py (ACMM_SELF_CHECK
#      replaces it) reaches its default minimum level, or ACMM_MIN_LEVEL when
#      set. Its report always goes in an ACMM self-check section.
# ALERTS_IGNORE and AUDIT_IGNORE are space-separated advisory ids accepted
# after review; record each with its reason and the date to revisit in the
# workflow that sets them. Diagnostics go to a Failures section and what a row
# could not check to a Notes section, so stdout stays a clean report; its last
# line is always `<!-- nightly-compliance: end -->`. Every check runs even
# when an earlier one fails; the script exits 1 if any failed, and 2 on a
# usage error. Needs jq and python3, and gh unless ALERTS_JSON and
# RULESET_JSON are set.
set -eu

if [ "$#" -ne 0 ]; then
  echo "usage: nightly-compliance.sh" >&2
  exit 2
fi

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

commit=${GITHUB_SHA:-$(git rev-parse HEAD 2>/dev/null || echo unknown)}
runtime_ref=${RUNTIME_REF:-org.gnome.Platform//51}
ruleset_id=${RULESET_ID:-26762163}
ruleset_file=${RULESET_FILE:-.github/rulesets/main.json}
acmm_self_check=${ACMM_SELF_CHECK:-scripts/acmm-self-check.py}
failed=0
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
details="$work/details.md"
notes="$work/notes.md"
: >"$details"
: >"$notes"

# row <check> <status>: one table row; a non-zero status fails the run.
row() {
  if [ "$2" -eq 0 ]; then
    echo "| $1 | ✅ pass |"
  else
    echo "| $1 | ❌ fail |"
    failed=1
  fi
}

# detail <title> <text>: a titled block of command output in the Failures section.
detail() {
  # shellcheck disable=SC2016 # Markdown code fences, not command substitution.
  printf '### %s\n\n```text\n%s\n```\n\n' "$1" "$2" >>"$details"
}

repo() {
  if [ -n "${GITHUB_REPOSITORY:-}" ]; then
    echo "$GITHUB_REPOSITORY"
  else
    gh repo view --json nameWithOwner --jq .nameWithOwner
  fi
}

# Prints the open Dependabot alerts as one JSON array.
read_alerts() {
  if [ -n "${ALERTS_JSON:-}" ]; then
    cat "$ALERTS_JSON"
  else
    # --paginate prints one array per page; jq -s merges them.
    gh api --paginate "repos/$(repo)/dependabot/alerts?state=open&per_page=100" | jq -s 'add // []'
  fi
}

read_ruleset() {
  if [ -n "${RULESET_JSON:-}" ]; then
    cat "$RULESET_JSON"
  else
    gh api "repos/$(repo)/rulesets/$ruleset_id"
  fi
}

# Reads the live ruleset on stdin and the committed one from $spec; prints one line per difference.
# shellcheck disable=SC2016 # $spec, $s and $l are jq variables.
ruleset_diff='
def params($rule): ($rule.parameters // {}) | del(.required_status_checks);
def check_list: [(.rules // [])[] | select(.type == "required_status_checks")
  | .parameters.required_status_checks[]? | "\(.context) (app \(.integration_id // "any"))"] | sort;
def actors: (. // []) | map({actor_id, actor_type, bypass_mode}) | sort_by(.actor_type, .actor_id);
$spec[0] as $s | . as $l
| ($s.rules | map(.type) | sort) as $want_types
| (($l.rules // []) | map(.type) | sort) as $live_types
| ($s | check_list) as $want_checks | ($l | check_list) as $live_checks
| (["name", "target", "enforcement", "conditions"][] as $key | select($s[$key] != $l[$key])
    | "\($key) differs: committed \($s[$key] | tojson), live \($l[$key] | tojson)"),
  (select($want_types != $live_types)
    | "rule types differ: committed \($want_types | join(", ")), live \($live_types | join(", "))"),
  ($s.rules[] as $rule
    | (($l.rules // []) | map(select(.type == $rule.type)) | first) as $live_rule
    | select($live_rule != null)
    | params($rule) as $want
    | (params($live_rule) | with_entries(select(.key as $key | $want | has($key)))) as $got
    | select($want != $got)
    | "rule \($rule.type) differs: committed \($want | tojson), live \($got | tojson)"),
  (($want_checks - $live_checks) | select(length > 0)
    | "required checks: committed but not live: \(join(", "))"),
  (($live_checks - $want_checks) | select(length > 0)
    | "required checks: live but not committed: \(join(", "))"),
  (if $l.bypass_actors == null then
     empty
   elif ($s.bypass_actors | actors) != ($l.bypass_actors | actors) then
     "bypass actors differ: committed \($s.bypass_actors | actors | tojson), live \($l.bypass_actors | actors | tojson)"
   else empty end)
'

echo "# Nightly compliance"
echo
echo "Commit: \`${commit}\`"
echo
echo "| Check | Result |"
echo "| ----- | ------ |"

# 1. Dependabot alerts.
alerts_check=1
if alerts=$(read_alerts 2>&1) && blocking=$(printf '%s' "$alerts" | jq -r --arg ignore "${ALERTS_IGNORE:-}" '
  ($ignore | split(" ") | map(select(. != ""))) as $ignored
  | if type != "array" then error("not an array of alerts") else . end
  | .[]
  | select(.state == "open")
  | select(.security_advisory.severity == "high" or .security_advisory.severity == "critical")
  | select(.security_advisory.ghsa_id as $id | $ignored | index($id) | not)
  | "- \(.security_advisory.ghsa_id) (\(.security_advisory.severity)) \(.dependency.package.name): \(.security_advisory.summary) \(.html_url)"
' 2>&1); then
  if [ -z "$blocking" ]; then
    alerts_check=0
  else
    printf 'Open high or critical Dependabot alerts:\n\n%s\n\n' "$blocking" >>"$details"
  fi
else
  detail "Could not read the Dependabot alerts (the token needs Dependabot alerts read access)" "$alerts"
fi
row "Dependabot alerts (high+)" "$alerts_check"

# 2. RustSec advisories against Cargo.lock.
audit_check=1
if command -v cargo-audit >/dev/null 2>&1; then
  set -- audit --file Cargo.lock
  for id in ${AUDIT_IGNORE:-}; do
    set -- "$@" --ignore "$id"
  done
  if audit_out=$(cargo-audit "$@" 2>&1); then
    audit_check=0
  else
    detail "cargo audit --file Cargo.lock" "$audit_out"
  fi
  set --
else
  detail "cargo audit --file Cargo.lock" "cargo-audit is not installed"
fi
row "RustSec audit of Cargo.lock (cargo audit)" "$audit_check"

# 3. GNOME runtime end-of-life. C locale: the field names in the output are translated.
runtime_check=1
if runtime_out=$(LC_ALL=C flatpak remote-info --system flathub "$runtime_ref" </dev/null 2>&1); then
  if eol=$(printf '%s\n' "$runtime_out" | grep -i 'end-of-life'); then
    detail "${runtime_ref} is end-of-life; move the manifests to a supported runtime" "$eol"
  else
    runtime_check=0
  fi
else
  detail "flatpak remote-info --system flathub ${runtime_ref} failed" "$runtime_out"
fi
row "GNOME runtime supported (${runtime_ref})" "$runtime_check"

# 4. actionlint over the workflows.
lint_check=1
if lint_out=$(actionlint -no-color 2>&1); then
  lint_check=0
else
  detail "actionlint" "$lint_out"
fi
row "Workflows (actionlint)" "$lint_check"

# 5. The live branch ruleset against the committed one.
ruleset_check=1
if [ ! -f "$ruleset_file" ]; then
  detail "Branch ruleset" "${ruleset_file} does not exist"
elif ! live=$(read_ruleset 2>&1) || ! printf '%s' "$live" | jq -e '.rules | type == "array"' >/dev/null 2>&1; then
  detail "Could not read ruleset ${ruleset_id}" "$live"
elif ! diff_out=$(printf '%s' "$live" | jq -r --slurpfile spec "$ruleset_file" "$ruleset_diff" 2>&1); then
  detail "Could not compare ruleset ${ruleset_id} with ${ruleset_file}" "$diff_out"
elif [ -n "$diff_out" ]; then
  detail "Ruleset ${ruleset_id} differs from ${ruleset_file}; re-apply the file or update it by pull request" "$diff_out"
else
  ruleset_check=0
  if printf '%s' "$live" | jq -e '.bypass_actors == null' >/dev/null; then
    printf -- '- Branch ruleset %s: bypass actors not checked without an admin token\n' "$ruleset_id" >>"$notes"
  fi
fi
row "Branch ruleset ${ruleset_id} matches .github/rulesets/main.json" "$ruleset_check"

# 6. ACMM codebase level, from Hive's criteria vendored in the self-check.
if [ -n "${ACMM_MIN_LEVEL:-}" ]; then
  set -- --min-level "$ACMM_MIN_LEVEL"
fi
acmm_check=0
acmm_out=$(python3 "$acmm_self_check" "$@" 2>&1) || acmm_check=$?
set --
if [ "$acmm_check" -ne 0 ]; then
  printf -- '- ACMM codebase level: below the required level; the ACMM self-check section lists the missing criteria\n\n' \
    >>"$details"
fi
row "ACMM codebase level" "$acmm_check"

if [ -s "$details" ]; then
  echo
  echo "## Failures"
  echo
  cat "$details"
fi

if [ -s "$notes" ]; then
  echo
  echo "## Notes"
  echo
  cat "$notes"
fi

echo
echo "## ACMM self-check"
echo
printf '%s\n' "$acmm_out"
# The last line, which Markdown does not show. scripts/compliance-issue.py takes a report without it as cut short (the
# job timed out or the script stopped early), so the rows it never printed are not read as passing.
echo
echo "<!-- nightly-compliance: end -->"

exit "$failed"
