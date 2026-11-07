#!/bin/sh
# Tests for pr-review-check.sh: runs the rubric against PR titles, bodies and authors and
# checks that each one passes or fails as expected.
# Usage: scripts/pr-review-check.test.sh (exits 1 if any case fails).
set -eu

check="$(dirname "$0")/pr-review-check.sh"
failures=0

# expect <pass|fail> <description> <title> <body> [author]
expect() {
  if PR_TITLE="$3" PR_BODY="$4" PR_AUTHOR="${5:-someone}" sh "$check" >/dev/null 2>&1; then
    got=pass
  else
    got=fail
  fi
  if [ "$got" = "$1" ]; then
    echo "ok: $2"
  else
    echo "FAIL: $2 (expected $1, got $got)"
    failures=$((failures + 1))
  fi
}

# expect_errors <description> <title> <body> <expected ::error:: lines, one per line>
# The check's output holds exactly these errors, in this order.
expect_errors() {
  output="$(PR_TITLE="$2" PR_BODY="$3" PR_AUTHOR=someone sh "$check" 2>&1)" || true
  got="$(printf '%s\n' "$output" | grep '^::error::')" || true
  if [ "$got" = "$4" ]; then
    echo "ok: $1"
  else
    printf 'FAIL: %s (expected errors:\n%s\ngot output:\n%s)\n' "$1" "$4" "$output"
    failures=$((failures + 1))
  fi
}

title='feat(import): import Steam libraries (#8)'
body='Fixes #8'
nl='
'
cr="$(printf '\r')"

# Title: Conventional Commits, ending with the issue.
expect pass 'title with type, scope and issue' "$title" "$body"
expect pass 'title without scope' 'docs: explain the build (#12)' "$body"
expect pass 'title naming two issues' 'feat(packaging): build an RPM and DEBs (#235, #236)' "$body"
expect pass 'title with breaking-change marker' 'feat(core)!: drop the v1 store (#9)' "$body"
expect pass 'title with dotted, slashed, dashed and underscored scope' 'fix(pindu-ui/app_menu.rs): keep focus (#3)' "$body"
expect pass "type 'chore'" 'chore(release): release 0.2.0 (#231)' "$body"
for type in build ci perf refactor revert style test; do
  expect pass "type '$type'" "$type: tidy up (#1)" "$body"
done
expect fail 'title without the issue' 'feat(import): import Steam libraries' "$body"
expect fail 'title with the issue not at the end' 'feat(import): (#8) import Steam libraries' "$body"
expect fail 'title with an issue but no summary' 'feat(import): (#8)' "$body"
expect fail 'title with the issue unparenthesised' 'feat(import): import Steam libraries #8' "$body"
expect fail 'title with the issue but no space before it' 'feat(import): import Steam libraries(#8)' "$body"
expect fail 'title with an empty issue list' 'feat(import): import Steam libraries ()' "$body"
expect fail 'title with two issues without a comma' 'feat: x (#1 #2)' "$body"
expect fail 'title with unknown type' 'feature(ui): add search (#8)' "$body"
expect fail 'title with capitalised type' 'Feat(ui): add search (#8)' "$body"
expect fail 'title with capitalised scope' 'feat(UI): add search (#8)' "$body"
expect fail 'title without space after colon' 'feat(ui):add search (#8)' "$body"
expect fail 'title without type' 'Add game search (#8)' "$body"
expect fail 'title with a type after other text' 'wip feat: add search (#8)' "$body"
expect fail 'title whose second line is valid' "wip${nl}feat: add search (#8)" "$body"
expect fail 'empty title' '' "$body"

# Body: every keyword form links the issue.
for keyword in fix fixes fixed close closes closed resolve resolves resolved ref refs; do
  expect pass "body '$keyword #8'" "$title" "$keyword #8"
done
expect pass "body 'Refs #8' (the issue stays open)" "$title" 'Refs #8'
expect pass 'body keyword in upper case' "$title" 'FIXES #8'
expect pass "body 'Closes: #10' (keyword ending in a colon)" "$title" 'Closes: #10'
expect pass "body 'Fixes owner/repo#N' (GitHub's long form)" "$title" 'Fixes Tim-van-Oudheusden/pindu-launcher#8'
expect pass 'body with several spaces after the keyword' "$title" 'Fixes  #8'
expect pass 'body link after other text on the line' "$title" 'Part of the import work. Closes #4'
expect pass 'body link on a later line' "$title" "## Summary${nl}${nl}What: x${nl}${nl}Fixes #7"
expect pass 'body link inside parentheses' "$title" '(Fixes #8)'
expect pass 'body link with CRLF line endings' "$title" "## Summary${cr}${nl}${cr}${nl}Fixes #8${cr}${nl}"
expect pass 'body closing two issues' "$title" "Fixes #243${nl}Fixes #254"

# Body: no issue linked.
expect fail 'empty body' "$title" ''
expect fail 'body without a link' "$title" 'Small cleanup.'
expect fail "body 'hotfix #3' (keyword is only a word suffix)" "$title" 'Follow-up to the hotfix #3.'
expect fail "body 'prefs #2'" "$title" 'See prefs #2'
expect fail "body 'x_fixes #8' (underscore is a word character)" "$title" 'x_fixes #8'
expect fail "body 'Fixes#8' (no space after the keyword)" "$title" 'Fixes#8'
expect fail 'body with a bare #N and no keyword' "$title" 'See #8'
expect fail 'body with keyword and number on separate lines' "$title" "Fixes${nl}#8"
expect fail 'body with keyword but no number' "$title" 'Fixes #'
expect fail "unfilled pull request template ('Fixes #<!-- ... -->')" "$title" \
  "## Summary${nl}${nl}Fixes #<!-- issue number; use \"Refs #N\" if the issue cannot be closed yet -->"

# Author: bots need no issue, but still need a Conventional Commits title.
expect pass 'Dependabot without an issue' 'build(deps): bump gtk4 from 0.11.0 to 0.11.1' 'Bumps gtk4.' 'dependabot[bot]'
expect fail 'Dependabot with an invalid title' 'Bump gtk4 from 0.11.0 to 0.11.1' 'Bumps gtk4.' 'dependabot[bot]'
expect fail "author merely containing 'bot'" 'feat: add search' 'Small cleanup.' 'robotics-fan'

# Both checks failing: the run fails and reports every problem, not just the first.
expect_errors "'wip' without 'Fixes #N' names both problems" 'wip' 'Some changes.' \
  "::error::PR title 'wip' must follow Conventional Commits and end with its issue, e.g. 'feat(import): import Steam libraries (#8)' (types: build, chore, ci, docs, feat, fix, perf, refactor, revert, style, test)
::error::PR body must link its GitHub issue, e.g. 'Fixes #8', or 'Refs #8' if the issue stays open"
expect_errors 'valid title and body report no problem' "$title" "$body" ''

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
