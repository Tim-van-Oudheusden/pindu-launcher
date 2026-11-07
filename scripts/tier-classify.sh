#!/bin/sh
# Change classification: reads changed file paths (one per line) on stdin, prints each path
# with its risk tier, then the pull request's overall tier as `tier=<low|medium|high>`. The
# highest tier of any changed file wins. Tiers are explained in docs/risk-tiers.md.
#
# Per-path tiers come from the risk assessment .github/risk-assessment.yml (RISK_TIER_POLICY
# overrides the path): the first rule whose glob matches a path sets its tier, and a path
# matching no rule is medium. The file is YAML, read here as a strict line subset: `rules:`,
# then one `- <tier> <glob>` item per line, plus blank and whole-line `#` comment lines. A
# missing or malformed file, or one without a single rule, exits 2 without printing a tier.
#
# Feed it `git -c core.quotePath=false diff --name-only --no-renames`. git still C-quotes a
# path holding `"`, `\` or a control character (`"a\tb.rs"`); such a line cannot be matched
# reliably, so it is high, and a newline in a file name cannot forge a `tier=` line.
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
policy="${RISK_TIER_POLICY:-$root/.github/risk-assessment.yml}"

fail() {
  echo "tier-classify: $*" >&2
  exit 2
}

[ "$#" -eq 0 ] || fail "usage: tier-classify.sh < paths"
[ -r "$policy" ] || fail "cannot read risk assessment file $policy"

rank() {
  case "$1" in
    low) echo 1 ;;
    medium) echo 2 ;;
    high) echo 3 ;;
    *) return 1 ;;
  esac
}

name() {
  case "$1" in
    1) echo low ;;
    2) echo medium ;;
    3) echo high ;;
  esac
}

# Parse the policy into "<rank> <glob>" lines. CR counts as whitespace, so a file saved with
# CRLF line endings still parses.
cr="$(printf '\r')"
rules=''
in_rules=''
lineno=0
while IFS=" 	$cr" read -r item tier glob extra || [ -n "$item" ]; do
  lineno=$((lineno + 1))
  case "$item" in '' | '#'*) continue ;; esac
  if [ "$item" = rules: ] && [ -z "$tier" ] && [ -z "$in_rules" ]; then
    in_rules=1
    continue
  fi
  if [ -z "$in_rules" ] || [ "$item" != - ] || [ -z "$glob" ] || [ -n "$extra" ]; then
    fail "$policy:$lineno: expected one 'rules:' line, then '- <tier> <glob>' items"
  fi
  r="$(rank "$tier")" || fail "$policy:$lineno: unknown tier '$tier' (expected low, medium or high)"
  rules="$rules$r $glob
"
done <"$policy"

# Zero rules would rate every path medium, so a gate file would not rate high.
[ -n "$rules" ] || fail "$policy: no rules; expected one 'rules:' line, then '- <tier> <glob>' items"

max=1
while IFS= read -r path || [ -n "$path" ]; do
  [ -n "$path" ] || continue
  r=2
  case "$path" in
    '"'*'"') r=3 ;;
    *)
      while read -r rule_rank rule_glob; do
        # Unquoted: the rule is a glob pattern, not a literal. `*` matches across `/`.
        # shellcheck disable=SC2254
        case "$path" in
          $rule_glob)
            r="$rule_rank"
            break
            ;;
        esac
      done <<EOF
$rules
EOF
      ;;
  esac
  printf '%-6s  %s\n' "$(name "$r")" "$path"
  [ "$r" -le "$max" ] || max="$r"
done

echo "tier=$(name "$max")"
