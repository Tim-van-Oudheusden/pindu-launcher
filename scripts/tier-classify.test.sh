#!/bin/sh
# Tests for tier-classify.sh: the rule engine against throwaway risk assessments (first matching
# rule wins, unmatched paths are medium, the highest tier wins, malformed files exit 2), git's
# quoted paths, then the shipped .github/risk-assessment.yml against paths of this repository,
# and that docs/risk-tiers.md lists every shipped rule under its tier.
# Usage: scripts/tier-classify.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
classify="$root/scripts/tier-classify.sh"
shipped="$root/.github/risk-assessment.yml"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
nl='
'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# policy <name> <content>: writes a risk assessment file holding exactly <content>, prints its path.
policy() {
  printf '%s' "$2" >"$tmp/$1.yml"
  echo "$tmp/$1.yml"
}

# expect <description> <risk assessment file> <changed paths> <expected stdout>
expect() {
  status=0
  output="$(printf '%s' "$3" | RISK_TIER_POLICY="$2" sh "$classify" 2>&1)" || status=$?
  if [ "$status" -ne 0 ]; then
    not_ok "$1 (exit $status): $output"
  elif [ "$output" != "$4" ]; then
    not_ok "$1 (expected:$nl$4${nl}got:$nl$output)"
  else
    ok "$1"
  fi
}

# tier_of <description> <path> <tier>: the shipped risk assessment gives one changed path this tier.
tier_of() {
  expect "$1" "$shipped" "$2" "$(printf '%-6s  %s\ntier=%s' "$3" "$2" "$3")"
}

# rejects <description> <risk assessment file> <expected stderr fragment> [argument]: exit 2, no stdout.
rejects() {
  file="$2"
  fragment="$3"
  description="$1"
  shift 3
  status=0
  stdout="$(printf 'docs/a.md\n' | RISK_TIER_POLICY="$file" sh "$classify" "$@" 2>"$tmp/stderr")" || status=$?
  stderr="$(cat "$tmp/stderr")"
  if [ "$status" -ne 2 ]; then
    not_ok "$description (expected exit 2, got $status): $stderr"
  elif [ -n "$stdout" ]; then
    not_ok "$description (printed to stdout): $stdout"
  elif ! printf '%s\n' "$stderr" | grep -qF -- "$fragment"; then
    not_ok "$description (output lacks '$fragment'): $stderr"
  else
    ok "$description"
  fi
}

# Engine.
rules="$(policy rules "# comment${nl}rules:${nl}${nl}  - high    a/special.txt${nl}  # between rules${nl}  - low     a/*${nl}  - high    *.cfg${nl}")"
expect 'first matching rule wins (high before broad low)' "$rules" 'a/special.txt' "high    a/special.txt${nl}tier=high"
expect 'later rule applies when the earlier does not match' "$rules" 'a/other.txt' "low     a/other.txt${nl}tier=low"
expect 'first match wins even over a later higher rule' "$rules" 'a/x.cfg' "low     a/x.cfg${nl}tier=low"
expect '* matches across /' "$rules" 'b/c/d.cfg' "high    b/c/d.cfg${nl}tier=high"
expect 'a path matching no rule is medium' "$rules" 'b/unknown.txt' "medium  b/unknown.txt${nl}tier=medium"
expect 'highest tier of any file wins' "$rules" "a/one.txt${nl}b/x.cfg${nl}a/two.txt${nl}" \
  "low     a/one.txt${nl}high    b/x.cfg${nl}low     a/two.txt${nl}tier=high"
expect 'medium beats low' "$rules" "a/one.txt${nl}b/new.txt${nl}" "low     a/one.txt${nl}medium  b/new.txt${nl}tier=medium"
expect 'no changed paths is low' "$rules" '' 'tier=low'
expect 'blank input lines are skipped' "$rules" "${nl}a/one.txt${nl}${nl}" "low     a/one.txt${nl}tier=low"
expect 'last path without a trailing newline' "$rules" "a/one.txt${nl}x.cfg" "low     a/one.txt${nl}high    x.cfg${nl}tier=high"
expect 'spaces around a path are kept' "$rules" ' a/one.txt ' "medium   a/one.txt ${nl}tier=medium"
expect 'a path that reads like a tier line is only a path' "$rules" 'tier=low' "medium  tier=low${nl}tier=medium"
expect 'policy with CRLF line endings' "$(policy crlf "# c$(printf '\r')${nl}rules:$(printf '\r')${nl}  - low docs/*$(printf '\r')${nl}")" \
  'docs/a.md' "low     docs/a.md${nl}tier=low"
expect 'a tab separates the fields of an item' "$(policy tabs "rules:${nl}$(printf '\t-\tlow\tdocs/*')${nl}")" 'docs/a.md' \
  "low     docs/a.md${nl}tier=low"
expect 'a backslash in a rule escapes the next glob character' "$(policy escape "rules:${nl}  - high docs/\\*.md${nl}")" \
  "docs/a.md${nl}docs/*.md${nl}" "medium  docs/a.md${nl}high    docs/*.md${nl}tier=high"
expect 'policy without a trailing newline' "$(policy no-eol "rules:${nl}  - high docs/*")" 'docs/a.md' "high    docs/a.md${nl}tier=high"
rejects 'missing risk assessment file' "$tmp/missing.yml" 'cannot read risk assessment file'
rejects 'unknown tier' "$(policy unknown "rules:${nl}  - critical docs/*${nl}")" "unknown tier 'critical'"
bad_item="expected one 'rules:' line, then '- <tier> <glob>' items"
rejects 'item without a glob' "$(policy no-glob "rules:${nl}  - high${nl}")" "$bad_item"
rejects 'item with an extra field' "$(policy extra "rules:${nl}  - low docs/* now${nl}")" "$bad_item"
rejects 'item with a trailing comment' "$(policy trailing "rules:${nl}  - low docs/* # c${nl}")" "$bad_item"
rejects 'item without its dash' "$(policy no-dash "rules:${nl}  x low docs/*${nl}")" "$bad_item"
rejects 'rules: line with a value' "$(policy rules-value "rules: all${nl}  - low docs/*${nl}")" "$bad_item"
rejects 'rule line without the rules: key' "$(policy bare "low docs/*${nl}")" "$bad_item"
rejects 'item before the rules: line' "$(policy early "  - low docs/*${nl}rules:${nl}")" ':1:'
rejects 'a second rules: line' "$(policy twice "rules:${nl}  - low docs/*${nl}rules:${nl}")" ':3:'
rejects 'a bad item names its line' "$(policy lineno "rules:${nl}  - low docs/*${nl}  - hgih scripts/*${nl}")" ':3:'
rejects 'empty policy' "$(policy empty '')" 'no rules'
rejects 'policy of only comments' "$(policy comments "# c${nl}${nl}# d${nl}")" 'no rules'
rejects 'rules: line without items' "$(policy no-items "rules:${nl}# c${nl}")" 'no rules'
rejects 'an argument' "$shipped" 'usage: tier-classify.sh < paths' -z

# Paths git C-quotes even with core.quotePath=false are high: they cannot be matched reliably.
expect 'a C-quoted path is high' "$rules" '"a/tab\tx.txt"' "high    \"a/tab\\tx.txt\"${nl}tier=high"
expect 'a C-quoted newline is one high line, not a forged tier= line' "$rules" '"a/x\ntier=low"' \
  "high    \"a/x\\ntier=low\"${nl}tier=high"
repo="$tmp/repo"
if git init -q "$repo" && git -C "$repo" -c user.name=t -c user.email=t@t commit -q --allow-empty -m base &&
  mkdir -p "$repo/docs" && echo x >"$repo/docs/naïve.md" && echo x >"$repo/docs/back\\slash.md" &&
  git -C "$repo" add -A && git -C "$repo" -c user.name=t -c user.email=t@t commit -q -m change; then
  expect 'git diff with core.quotePath=false: non-ASCII is matched, a backslash is high' "$shipped" \
    "$(git -C "$repo" -c core.quotePath=false diff --name-only --no-renames HEAD~1...HEAD)" \
    "high    \"docs/back\\\\slash.md\"${nl}low     docs/naïve.md${nl}tier=high"
else
  not_ok 'could not set up the git repository for quoted paths'
fi

# The shipped risk assessment (.github/risk-assessment.yml).
tier_of 'workflow is high' '.github/workflows/ci.yml' high
tier_of 'the risk assessment itself is high' '.github/risk-assessment.yml' high
tier_of 'Markdown under .github is high' '.github/pull_request_template.md' high
tier_of 'gate script is high' 'scripts/tier-classify.sh' high
tier_of 'development Flatpak manifest is high' 'build-aux/flatpak/io.github.tim_van_oudheusden.pindu-launcher-devel.yml' high
tier_of 'Flatpak lint exceptions are high' 'build-aux/flatpak/lint-exceptions.json' high
tier_of 'RPM spec is high' 'build-aux/rpm/pindu-launcher.spec' high
tier_of 'meson.build is high' 'meson.build' high
tier_of 'nested meson.build (cargo options, validators) is high' 'crates/meson.build' high
tier_of 'po/meson.build is high although po/** is medium' 'po/meson.build' high
tier_of 'meson.options is high' 'meson.options' high
tier_of 'Cargo.toml is high' 'Cargo.toml' high
tier_of 'a crate Cargo.toml (its dependencies) is high' 'crates/pindu-core/Cargo.toml' high
tier_of 'Cargo.lock is high' 'Cargo.lock' high
tier_of 'clippy.toml is high' 'clippy.toml' high
tier_of 'rustfmt.toml is high' 'rustfmt.toml' high
tier_of 'MetaInfo is high although data/** is medium' 'data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in' high
tier_of 'GSettings schema is high' 'data/io.github.tim_van_oudheusden.pindu-launcher.gschema.xml.in' high
tier_of 'AGENTS.md is high although *.md is low' 'AGENTS.md' high
tier_of 'CLAUDE.md is high' 'CLAUDE.md' high
tier_of 'Claude settings are high' '.claude/settings.json' high
tier_of 'Cursor rules are high' '.cursor/rules/agent-guidance.mdc' high
tier_of 'prompts are high' 'prompts/code-review.md' high
tier_of 'core code is medium' 'crates/pindu-core/src/sources/steam.rs' medium
tier_of 'UI code is medium' 'crates/pindu-ui/src/application.rs' medium
tier_of 'a Blueprint file is medium' 'data/ui/window.blp' medium
tier_of 'the desktop file is medium' 'data/io.github.tim_van_oudheusden.pindu-launcher.desktop.in' medium
tier_of 'a translation is medium' 'po/nl.po' medium
tier_of 'an unmatched root file is medium' '.gitignore' medium
tier_of 'docs are low' 'docs/releasing.md' low
tier_of 'README.md is low' 'README.md' low
expect 'a manifest change makes a docs PR high' "$shipped" "docs/releasing.md${nl}build-aux/flatpak/lint-exceptions.json${nl}" \
  "low     docs/releasing.md${nl}high    build-aux/flatpak/lint-exceptions.json${nl}tier=high"

# docs/risk-tiers.md lists every shipped rule under its own tier, a `dir/*` rule as `dir/**`.
docs="$root/docs/risk-tiers.md"
for tier in high medium low; do
  awk -v tier="$tier" '/^## /{in_tier = ($0 == "## Tier: " tier)} in_tier' "$docs" >"$tmp/docs-$tier"
done
while read -r dash tier glob; do
  [ "$dash" = - ] || continue
  case "$glob" in */\*) shown="$glob*" ;; *) shown="$glob" ;; esac
  if grep -qF -- "\`$shown\`" "$tmp/docs-$tier"; then
    ok "docs/risk-tiers.md lists '$shown' under $tier"
  else
    not_ok "docs/risk-tiers.md does not list '\`$shown\`' under '## Tier: $tier'"
  fi
done <"$shipped"

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
