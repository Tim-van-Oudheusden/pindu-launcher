#!/bin/sh
# Tests for nightly-compliance.sh: every row against stubbed tools (cargo-audit, flatpak, actionlint and the ACMM
# self-check on PATH or by variable) and canned API responses (ALERTS_JSON, RULESET_JSON), so no network is used.
# Usage: scripts/nightly-compliance.test.sh (exits 1 if any case fails). Needs jq and python3.
set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
script="$ROOT/scripts/nightly-compliance.sh"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
failures=0

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# A stub prints $STUB_<NAME>_OUT, logs its arguments to $tmp/<name>.args and exits with $STUB_<NAME>_STATUS.
mkdir "$tmp/bin"
for tool in cargo-audit flatpak actionlint; do
  var=$(printf '%s' "$tool" | tr 'a-z-' 'A-Z_')
  cat >"$tmp/bin/$tool" <<EOF
#!/bin/sh
printf '%s\n' "\$*" >"$tmp/$tool.args"
printf '%s\n' "\${STUB_${var}_OUT:-}"
exit "\${STUB_${var}_STATUS:-0}"
EOF
  chmod +x "$tmp/bin/$tool"
done
# The script must not reach GitHub while ALERTS_JSON and RULESET_JSON are set.
printf '#!/bin/sh\necho "gh must not be called: $*" >&2\nexit 99\n' >"$tmp/bin/gh"
chmod +x "$tmp/bin/gh"
cat >"$tmp/acmm.py" <<EOF
import os, sys
with open("$tmp/acmm.args", "w") as log:
    log.write(" ".join(sys.argv[1:]) + "\n")
print(os.environ.get("STUB_ACMM_OUT", "ACMM codebase level: L0 Prerequisites not met (required: L0 Prerequisites not met): ok"))
sys.exit(int(os.environ.get("STUB_ACMM_STATUS", "0")))
EOF

alert() { # alert <ghsa id> <severity> [state]
  printf '{"state":"%s","html_url":"https://example.test/%s","dependency":{"package":{"name":"zbus"}},' "${3:-open}" "$1"
  printf '"security_advisory":{"ghsa_id":"%s","severity":"%s","summary":"Summary of %s"}}' "$1" "$2" "$1"
}
printf '[]' >"$tmp/no-alerts.json"
printf '[%s,%s,%s]' "$(alert GHSA-low1 low)" "$(alert GHSA-mod1 moderate)" "$(alert GHSA-fixed critical fixed)" \
  >"$tmp/minor-alerts.json"
printf '[%s,%s]' "$(alert GHSA-high1 high)" "$(alert GHSA-crit1 critical)" >"$tmp/serious-alerts.json"
printf '{"message":"Resource not accessible by integration"}' >"$tmp/alerts-error.json"

checks='[{"context":"Rust and Meson checks","integration_id":15368},{"context":"Source tarball","integration_id":15368}]'
bypass='[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"always"}]'
# ruleset <required checks> [bypass actors, or "none" to leave the list out] [merge methods]: a ruleset as the API
# returns it, with the fields the API adds (id, required_reviewers, ...) that the committed file leaves out.
ruleset() {
  jq -n --argjson checks "$1" --arg bypass "${2:-$bypass}" --argjson methods "${3:-[\"merge\"]}" '{
    id: 26762163, name: "main", target: "branch", enforcement: "active", source_type: "Repository",
    conditions: {ref_name: {exclude: [], include: ["~DEFAULT_BRANCH"]}},
    rules: [
      {type: "deletion"}, {type: "non_fast_forward"},
      {type: "pull_request", parameters: {required_approving_review_count: 0, required_reviewers: [],
        required_review_thread_resolution: true, require_extra_approval_for_unattributed_changes: true,
        allowed_merge_methods: $methods}},
      {type: "required_status_checks", parameters: {strict_required_status_checks_policy: false,
        required_status_checks: $checks}}
    ]
  } + (if $bypass == "none" then {} else {bypass_actors: ($bypass | fromjson)} end)'
}
# The committed file: no API-only fields, and the checks in another order.
jq -n --argjson checks "$checks" --argjson bypass "$bypass" '{
  name: "main", target: "branch", enforcement: "active",
  conditions: {ref_name: {include: ["~DEFAULT_BRANCH"], exclude: []}},
  bypass_actors: $bypass,
  rules: [
    {type: "deletion"}, {type: "non_fast_forward"},
    {type: "pull_request", parameters: {required_approving_review_count: 0, required_review_thread_resolution: true,
      allowed_merge_methods: ["merge"]}},
    {type: "required_status_checks", parameters: {strict_required_status_checks_policy: false,
      required_status_checks: ($checks | reverse)}}
  ]
}' >"$tmp/spec.json"
ruleset "$checks" >"$tmp/live.json"

# run [VAR=value...]: runs the script with every check passing unless a VAR overrides it; sets $out and $status.
run() {
  rm -f "$tmp"/*.args
  status=0
  out=$(env -u ALERTS_IGNORE -u AUDIT_IGNORE -u ACMM_MIN_LEVEL -u RUNTIME_REF PATH="$tmp/bin:$PATH" \
    ALERTS_JSON="$tmp/no-alerts.json" RULESET_JSON="$tmp/live.json" RULESET_FILE="$tmp/spec.json" \
    ACMM_SELF_CHECK="$tmp/acmm.py" GITHUB_SHA=abc123 "$@" sh "$script" 2>"$tmp/stderr") || status=$?
}

# expect <description> <expected status> <fixed string the report must contain>...
expect() {
  description=$1 expected=$2
  shift 2
  if [ "$status" -ne "$expected" ]; then
    not_ok "$description (expected exit $expected, got $status): $out $(cat "$tmp/stderr")"
    return
  fi
  for want in "$@"; do
    if ! printf '%s\n' "$out" | grep -qF -- "$want"; then
      not_ok "$description (report lacks '$want'): $out"
      return
    fi
  done
  ok "$description"
}

# lacks <description> <fixed string the report must not contain>
lacks() {
  if printf '%s\n' "$out" | grep -qF -- "$2"; then
    not_ok "$1 (report has '$2'): $out"
  else
    ok "$1"
  fi
}

run
expect "every check passing exits 0 and lists every row" 0 \
  "# Nightly compliance" "Commit: \`abc123\`" "| Check | Result |" \
  "| Dependabot alerts (high+) | ✅ pass |" \
  "| RustSec audit of Cargo.lock (cargo audit) | ✅ pass |" \
  "| GNOME runtime supported (org.gnome.Platform//51) | ✅ pass |" \
  "| Workflows (actionlint) | ✅ pass |" \
  "| Branch ruleset 26762163 matches .github/rulesets/main.json | ✅ pass |" \
  "| ACMM codebase level | ✅ pass |" \
  "ACMM codebase level: L0 Prerequisites not met"
lacks "a clean run has no Failures section" "## Failures"
if [ "$(cat "$tmp/cargo-audit.args")" = "audit --file Cargo.lock" ] &&
  [ "$(cat "$tmp/flatpak.args")" = "remote-info --system flathub org.gnome.Platform//51" ] &&
  [ "$(cat "$tmp/actionlint.args")" = "-no-color" ] && [ "$(cat "$tmp/acmm.args")" = "" ]; then
  ok "the tools get the expected arguments"
else
  not_ok "the tools get the expected arguments: $(cat "$tmp"/*.args)"
fi

run ALERTS_JSON="$tmp/minor-alerts.json"
expect "low, moderate and fixed alerts pass" 0 "| Dependabot alerts (high+) | ✅ pass |"

run ALERTS_JSON="$tmp/serious-alerts.json"
expect "open high and critical alerts fail and are listed" 1 "| Dependabot alerts (high+) | ❌ fail |" \
  "## Failures" "- GHSA-high1 (high) zbus: Summary of GHSA-high1 https://example.test/GHSA-high1" \
  "- GHSA-crit1 (critical) zbus"

run ALERTS_JSON="$tmp/serious-alerts.json" ALERTS_IGNORE="GHSA-high1 GHSA-crit1"
expect "ignored alerts pass" 0 "| Dependabot alerts (high+) | ✅ pass |"

run ALERTS_JSON="$tmp/alerts-error.json"
expect "an unreadable alerts API fails the row" 1 "| Dependabot alerts (high+) | ❌ fail |" \
  "Could not read the Dependabot alerts"

run STUB_CARGO_AUDIT_STATUS=1 STUB_CARGO_AUDIT_OUT="error: 1 vulnerability found! RUSTSEC-2099-0001"
expect "a RustSec vulnerability fails the row and shows the audit" 1 \
  "| RustSec audit of Cargo.lock (cargo audit) | ❌ fail |" "RUSTSEC-2099-0001" \
  "| Workflows (actionlint) | ✅ pass |"

run AUDIT_IGNORE="RUSTSEC-2099-0001 RUSTSEC-2099-0002"
if [ "$(cat "$tmp/cargo-audit.args")" = "audit --file Cargo.lock --ignore RUSTSEC-2099-0001 --ignore RUSTSEC-2099-0002" ]
then
  ok "accepted advisories are passed to cargo audit"
else
  not_ok "accepted advisories are passed to cargo audit: $(cat "$tmp/cargo-audit.args")"
fi

rm "$tmp/bin/cargo-audit"
run
expect "a missing cargo-audit fails the row" 1 "| RustSec audit of Cargo.lock (cargo audit) | ❌ fail |" \
  "cargo-audit is not installed"
printf '#!/bin/sh\nprintf "%%s\\n" "$*" >"%s/cargo-audit.args"\n' "$tmp" >"$tmp/bin/cargo-audit"
chmod +x "$tmp/bin/cargo-audit"

run STUB_FLATPAK_OUT="$(printf '        ID: org.gnome.Platform\n End-of-life: The GNOME 51 runtime is no longer supported')"
expect "an end-of-life runtime fails the row" 1 "| GNOME runtime supported (org.gnome.Platform//51) | ❌ fail |" \
  "End-of-life: The GNOME 51 runtime is no longer supported"

run STUB_FLATPAK_STATUS=1 STUB_FLATPAK_OUT="error: Nothing matches org.gnome.Platform in remote flathub"
expect "an unreadable runtime fails the row" 1 "| GNOME runtime supported (org.gnome.Platform//51) | ❌ fail |" \
  "Nothing matches org.gnome.Platform"

run RUNTIME_REF=org.gnome.Platform//52
expect "the runtime ref can be changed" 0 "| GNOME runtime supported (org.gnome.Platform//52) | ✅ pass |"

run STUB_ACTIONLINT_STATUS=1 STUB_ACTIONLINT_OUT=".github/workflows/ci.yml:3:1: unexpected key [syntax-check]"
expect "an actionlint finding fails the row" 1 "| Workflows (actionlint) | ❌ fail |" "unexpected key [syntax-check]"

ruleset '[{"context":"Rust and Meson checks","integration_id":15368}]' >"$tmp/live-missing-check.json"
run RULESET_JSON="$tmp/live-missing-check.json"
expect "a required check missing from the live ruleset fails" 1 \
  "| Branch ruleset 26762163 matches .github/rulesets/main.json | ❌ fail |" \
  "required checks: committed but not live: Source tarball"

ruleset "$checks" '[{"actor_id":5,"actor_type":"RepositoryRole","bypass_mode":"pull_request"}]' >"$tmp/live-bypass.json"
run RULESET_JSON="$tmp/live-bypass.json"
expect "a different bypass list fails" 1 "| Branch ruleset 26762163 matches .github/rulesets/main.json | ❌ fail |" \
  "bypass actors differ"

ruleset "$checks" none >"$tmp/live-no-bypass.json"
run RULESET_JSON="$tmp/live-no-bypass.json"
expect "a token that cannot see the bypass list passes with a note" 0 \
  "| Branch ruleset 26762163 matches .github/rulesets/main.json | ✅ pass |" "## Notes" \
  "- Branch ruleset 26762163: bypass actors not checked without an admin token"
lacks "the note is not a failure" "## Failures"

jq '.enforcement = "evaluate"' "$tmp/live-no-bypass.json" >"$tmp/live-no-bypass-evaluate.json"
run RULESET_JSON="$tmp/live-no-bypass-evaluate.json"
expect "the readable fields are still compared when the bypass list is hidden" 1 "enforcement differs"

run
lacks "a visible bypass list leaves no note" "## Notes"

ruleset "$checks" "$bypass" '["merge","squash"]' >"$tmp/live-squash.json"
run RULESET_JSON="$tmp/live-squash.json"
expect "a changed rule parameter fails and names the rule" 1 \
  "| Branch ruleset 26762163 matches .github/rulesets/main.json | ❌ fail |" "rule pull_request differs"

jq '.rules |= map(select(.type != "deletion"))' "$tmp/live.json" >"$tmp/live-no-deletion.json"
run RULESET_JSON="$tmp/live-no-deletion.json"
expect "a rule missing from the live ruleset fails" 1 "rule types differ"

jq '.enforcement = "disabled"' "$tmp/live.json" >"$tmp/live-disabled.json"
run RULESET_JSON="$tmp/live-disabled.json"
expect "a disabled live ruleset fails" 1 "enforcement differs"

run RULESET_FILE="$tmp/absent.json"
expect "a missing committed ruleset fails" 1 \
  "| Branch ruleset 26762163 matches .github/rulesets/main.json | ❌ fail |" "$tmp/absent.json"

# shellcheck disable=SC2016 # Markdown backticks, not command substitution.
run STUB_ACMM_STATUS=1 STUB_ACMM_OUT="$(printf 'ACMM codebase level: L0 (required: L1): BELOW REQUIRED LEVEL\n  - missing `acmm:prereq-cicd`')"
expect "an ACMM level below the minimum fails and names the criterion" 1 "| ACMM codebase level | ❌ fail |" \
  "BELOW REQUIRED LEVEL" "missing \`acmm:prereq-cicd\`"

run ACMM_MIN_LEVEL=3
if [ "$(cat "$tmp/acmm.args")" = "--min-level 3" ]; then
  ok "ACMM_MIN_LEVEL is passed to the self-check"
else
  not_ok "ACMM_MIN_LEVEL is passed to the self-check: $(cat "$tmp/acmm.args")"
fi

run STUB_ACTIONLINT_STATUS=1 ALERTS_JSON="$tmp/serious-alerts.json"
expect "every check still runs after earlier failures" 1 "| ACMM codebase level | ✅ pass |" \
  "| Dependabot alerts (high+) | ❌ fail |" "| Workflows (actionlint) | ❌ fail |"

status=0
sh "$script" --bogus >/dev/null 2>&1 || status=$?
if [ "$status" -eq 2 ]; then ok "an argument is a usage error"; else not_ok "an argument is a usage error (exit $status)"; fi

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases passed"
