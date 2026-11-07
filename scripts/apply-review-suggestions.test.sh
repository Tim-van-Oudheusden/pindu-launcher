#!/bin/sh
# Tests for apply-review-suggestions.sh: each case builds a checkout and a review comments JSON
# in its own directory, runs the script there and checks its exact output and the files it
# leaves behind.
# Usage: scripts/apply-review-suggestions.test.sh (exits 1 if any case fails).
# shellcheck disable=SC2016 # backticks in single quotes are Markdown fences, not command substitutions
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
script="$root/scripts/apply-review-suggestions.sh"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
failures=0
cases=0

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

# Texts below are printf %b strings, so \n, \t and \r stand for the characters.
file='one\ntwo\nthree\nfour\nfive\n'

# begin <description>: starts a case in a fresh directory $dir holding the checkout $dir/pr
# with a.txt, an empty $dir/scratch as TMPDIR and nothing else.
begin() {
  name="$1"
  problems=''
  cases=$((cases + 1))
  dir="$tmp/$cases"
  mkdir -p "$dir/pr" "$dir/scratch"
  printf '%b' "$file" >"$dir/pr/a.txt"
}

problem() { problems="$problems; $1"; }

# finish: reports the case begun last.
finish() {
  if [ -z "$problems" ]; then
    ok "$name"
  else
    not_ok "$name:${problems#;}"
  fi
}

# comment <id> <path> <line or null> <body> [<JSON object overriding fields>]: one entry of
# `GET /repos/{owner}/{repo}/pulls/{number}/comments`, by a member, on the head's side.
comment() {
  body="$(printf '%b.' "$4")"
  jq -nc --argjson id "$1" --arg path "$2" --argjson line "$3" --arg body "${body%.}" \
    --argjson fields "${5:-null}" \
    '{id: $id, path: $path, line: $line, start_line: null, side: "RIGHT", start_side: null,
      author_association: "MEMBER", body: $body} + ($fields // {})'
}

# suggestion <line>...: a comment body with one suggestion block holding the lines.
suggestion() {
  text='Try this:\n```suggestion\n'
  for line in "$@"; do
    text="$text$line\n"
  done
  printf '%s' "$text"'```\n'
}

# apply_in <directory> <comment>...: runs the script in the directory over the comments,
# leaving its exit status in $status and its output in $dir/stdout and $dir/stderr.
apply_in() {
  cwd="$1"
  shift
  printf '%s\n' "$@" | jq -s . >"$dir/comments.json"
  status=0
  (cd "$cwd" && COMMENTS_FILE="$dir/comments.json" TMPDIR="$dir/scratch" sh "$script") \
    >"$dir/stdout" 2>"$dir/stderr" || status=$?
}

# apply <comment>...: apply_in the checkout.
apply() { apply_in "$dir/pr" "$@"; }

# stdout_is <text>: the script succeeded and printed exactly the text.
stdout_is() {
  printf '%b' "$1" >"$dir/want"
  if [ "$status" -ne 0 ]; then
    problem "exit $status: $(cat "$dir/stderr")"
  elif ! cmp -s "$dir/want" "$dir/stdout"; then
    problem "stdout was '$(cat "$dir/stdout")'"
  fi
}

# file_is <path> <text>: the file holds exactly the text.
file_is() {
  printf '%b' "$2" >"$dir/want"
  if ! cmp -s "$dir/want" "$1"; then
    problem "${1#"$dir"/} holds '$(cat "$1")'"
  fi
}

begin 'applies a single-line suggestion'
apply "$(comment 1 a.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nTWO\nthree\nfour\nfive\n'
finish

begin 'replaces a line range'
apply "$(comment 1 a.txt 4 "$(suggestion middle)" '{"start_line": 2}')"
stdout_is 'applied: a.txt:2-4 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nmiddle\nfive\n'
finish

begin 'an empty suggestion deletes the lines'
apply "$(comment 1 a.txt 3 '```suggestion\n```')"
stdout_is 'applied: a.txt:3-3 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\ntwo\nfour\nfive\n'
finish

begin 'applies several suggestions in one file bottom-up'
apply "$(comment 1 a.txt 1 "$(suggestion ONE ONE-AND-A-HALF)")" "$(comment 2 a.txt 4 "$(suggestion FOUR)")"
stdout_is 'applied: a.txt:4-4 (comment 2)\napplied: a.txt:1-1 (comment 1)\n'
file_is "$dir/pr/a.txt" 'ONE\nONE-AND-A-HALF\ntwo\nthree\nFOUR\nfive\n'
finish

begin 'skips an overlapping suggestion'
apply "$(comment 1 a.txt 3 "$(suggestion A)" '{"start_line": 2}')" \
  "$(comment 2 a.txt 4 "$(suggestion B)" '{"start_line": 3}')"
stdout_is 'applied: a.txt:3-4 (comment 2)\nskipped: comment 1 on a.txt: overlaps another applied suggestion\n'
file_is "$dir/pr/a.txt" 'one\ntwo\nB\nfive\n'
finish

begin 'a fence inside a line is part of the suggestion'
apply "$(comment 1 a.txt 1 "$(suggestion 'use ``` here')")"
stdout_is 'applied: a.txt:1-1 (comment 1)\n'
file_is "$dir/pr/a.txt" 'use ``` here\ntwo\nthree\nfour\nfive\n'
finish

# CommonMark: a closing fence holds only backticks and blanks, so ```bash is the suggestion's content (Dailies #162).
begin 'a suggested fence line is content until a bare closing fence'
printf 'Install:\n```sh\nmake\n```\n' >"$dir/pr/a.txt"
apply "$(comment 1 a.txt 2 "$(suggestion '```bash')")" \
  "$(comment 2 a.txt 3 "$(suggestion 'make all' '``` not a fence')")"
stdout_is 'applied: a.txt:3-3 (comment 2)\napplied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'Install:\n```bash\nmake all\n``` not a fence\n```\n'
finish

begin 'a longer fence lets a suggestion carry a code block'
apply "$(comment 1 a.txt 2 'Nit:\n````suggestion\nRun:\n```sh\nmake\n```\n`````  \nThanks.\n')"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nRun:\n```sh\nmake\n```\nthree\nfour\nfive\n'
finish

for case in 'fence after other text|Try ```suggestion\nX\n```\n' 'two backticks|``suggestion\nX\n``\n'; do
  begin "only a fence of three backticks at the start of a line opens a suggestion: ${case%%|*}"
  apply "$(comment 1 a.txt 2 "${case#*|}")"
  stdout_is 'no suggestions to apply\n'
  file_is "$dir/pr/a.txt" "$file"
  finish
done

begin 'blanks after the opening fence are ignored'
apply "$(comment 1 a.txt 1 '```suggestion \t\nONE\n```\n')"
stdout_is 'applied: a.txt:1-1 (comment 1)\n'
file_is "$dir/pr/a.txt" 'ONE\ntwo\nthree\nfour\nfive\n'
finish

begin 'Windows line endings in the comment are ignored'
apply "$(comment 1 a.txt 1 '```suggestion\r\nONE\r\n```\r\n')"
stdout_is 'applied: a.txt:1-1 (comment 1)\n'
file_is "$dir/pr/a.txt" 'ONE\ntwo\nthree\nfour\nfive\n'
finish

for association in OWNER MEMBER COLLABORATOR; do
  begin "applies a suggestion from anyone with write access: $association"
  apply "$(comment 1 a.txt 2 "$(suggestion TWO)" "{\"author_association\": \"$association\"}")"
  stdout_is 'applied: a.txt:2-2 (comment 1)\n'
  file_is "$dir/pr/a.txt" 'one\nTWO\nthree\nfour\nfive\n'
  finish
done

# b.txt is longer than a.txt, and its range sorts between the two overlapping ranges of a.txt.
begin 'each file has its own line count and overlaps'
printf '1\n2\n3\n4\n5\n6\n7\n8\n' >"$dir/pr/b.txt"
apply "$(comment 1 a.txt 4 "$(suggestion A)" '{"start_line": 3}')" \
  "$(comment 2 b.txt 8 "$(suggestion B)" '{"start_line": 3}')" \
  "$(comment 3 a.txt 3 "$(suggestion C)" '{"start_line": 2}')"
stdout_is 'applied: a.txt:3-4 (comment 1)\nskipped: comment 3 on a.txt: overlaps another applied suggestion\napplied: b.txt:3-8 (comment 2)\n'
file_is "$dir/pr/a.txt" 'one\ntwo\nA\nfive\n'
file_is "$dir/pr/b.txt" '1\n2\nB\n'
finish

begin 'applies to a last line without a trailing newline'
printf 'one\ntwo' >"$dir/pr/a.txt"
apply "$(comment 1 a.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nTWO\n'
finish

begin 'keeps the mode of the file'
chmod 755 "$dir/pr/a.txt"
apply "$(comment 1 a.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nTWO\nthree\nfour\nfive\n'
mode="$(stat -c %a "$dir/pr/a.txt")"
[ "$mode" = 755 ] || problem "mode is $mode"
finish

begin 'leaves no temporary file behind'
apply "$(comment 1 a.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nTWO\nthree\nfour\nfive\n'
left="$(ls -A "$dir/scratch")"
[ -z "$left" ] || problem "left $left in TMPDIR"
finish

begin 'a checkout entered through a symlink is still the checkout'
ln -s "$dir/pr" "$dir/link"
apply_in "$dir/link" "$(comment 1 a.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: a.txt:2-2 (comment 1)\n'
file_is "$dir/pr/a.txt" 'one\nTWO\nthree\nfour\nfive\n'
finish

# <description>|<comment fields>|<reason>; a.txt and $dir/outside.txt stay as they were.
for case in \
  'untrusted author|{"author_association": "CONTRIBUTOR"}|author CONTRIBUTOR has no write access' \
  'outdated|{"line": null}|outdated' \
  'base side|{"side": "LEFT"}|on the base side of the diff' \
  'range starting on the base side|{"start_line": 1, "start_side": "LEFT"}|on the base side of the diff' \
  'two blocks|{"body": "Try this:\n```suggestion\nX\n```\nTry this:\n```suggestion\nY\n```\n"}|more than one suggestion block' \
  'past the end|{"line": 9}|lines 9-9 are not in the file' \
  'before the start|{"start_line": 0}|lines 0-2 are not in the file' \
  'reversed range|{"start_line": 3}|lines 3-2 are not in the file' \
  'missing|{"path": "missing.txt"}|not a regular file in the checkout' \
  'outside|{"path": "../outside.txt"}|not a regular file in the checkout'; do
  description="${case%%|*}"
  rest="${case#*|}"
  fields="${rest%%|*}"
  begin "skips a suggestion it must not apply: $description"
  printf '%b' "$file" >"$dir/outside.txt"
  review="$(comment 1 a.txt 2 "$(suggestion X)" "$fields")"
  apply "$review"
  stdout_is "skipped: comment 1 on $(printf '%s' "$review" | jq -r .path): ${rest#*|}\n"
  file_is "$dir/pr/a.txt" "$file"
  file_is "$dir/outside.txt" "$file"
  finish
done

begin 'never writes through a symlink out of the checkout'
printf '%b' "$file" >"$dir/outside.txt"
ln -s "$dir/outside.txt" "$dir/pr/link.txt"
apply "$(comment 1 link.txt 1 "$(suggestion X)")"
stdout_is 'skipped: comment 1 on link.txt: not a regular file in the checkout\n'
file_is "$dir/outside.txt" "$file"
finish

begin 'a directory named like the checkout is outside it'
mkdir "$dir/pr-other"
printf '%b' "$file" >"$dir/pr-other/b.txt"
apply "$(comment 1 ../pr-other/b.txt 1 "$(suggestion X)")"
stdout_is 'skipped: comment 1 on ../pr-other/b.txt: not a regular file in the checkout\n'
file_is "$dir/pr-other/b.txt" "$file"
finish

begin 'skips a directory'
mkdir "$dir/pr/sub"
apply "$(comment 1 sub 1 "$(suggestion X)")"
stdout_is 'skipped: comment 1 on sub: not a regular file in the checkout\n'
file_is "$dir/pr/a.txt" "$file"
finish

begin 'skips a symlink to a workflow file'
mkdir -p "$dir/pr/.github/workflows"
printf 'name: CI\n' >"$dir/pr/.github/workflows/ci.yml"
ln -s "$dir/pr/.github/workflows/ci.yml" "$dir/pr/link.yml"
apply "$(comment 1 link.yml 1 "$(suggestion 'name: Evil')")"
stdout_is 'skipped: comment 1 on link.yml: workflow files cannot be pushed with the workflow token\n'
file_is "$dir/pr/.github/workflows/ci.yml" 'name: CI\n'
finish

begin 'applies to other files under .github'
mkdir "$dir/pr/.github"
printf '* @old\n' >"$dir/pr/.github/CODEOWNERS"
apply "$(comment 1 .github/CODEOWNERS 1 "$(suggestion '* @new')")"
stdout_is 'applied: .github/CODEOWNERS:1-1 (comment 1)\n'
file_is "$dir/pr/.github/CODEOWNERS" '* @new\n'
finish

begin 'a file name starting with a dash is not an option'
printf '%b' "$file" >"$dir/pr/-x.txt"
apply "$(comment 1 -x.txt 2 "$(suggestion TWO)")"
stdout_is 'applied: -x.txt:2-2 (comment 1)\n'
file_is "$dir/pr/-x.txt" 'one\nTWO\nthree\nfour\nfive\n'
finish

begin 'skips workflow files'
mkdir -p "$dir/pr/.github/workflows"
printf 'name: CI\n' >"$dir/pr/.github/workflows/ci.yml"
apply "$(comment 1 .github/workflows/ci.yml 1 "$(suggestion 'name: Evil')")"
stdout_is 'skipped: comment 1 on .github/workflows/ci.yml: workflow files cannot be pushed with the workflow token\n'
file_is "$dir/pr/.github/workflows/ci.yml" 'name: CI\n'
finish

begin 'comments without suggestions change nothing'
apply "$(comment 1 a.txt 2 'Looks good, but ```code``` here.')"
stdout_is 'no suggestions to apply\n'
file_is "$dir/pr/a.txt" "$file"
finish

begin 'needs the comments file'
status=0
(unset COMMENTS_FILE && cd "$dir/pr" && sh "$script") >"$dir/stdout" 2>"$dir/stderr" || status=$?
[ "$status" -ne 0 ] || problem 'exited 0'
[ ! -s "$dir/stdout" ] || problem "stdout was '$(cat "$dir/stdout")'"
grep -qF 'COMMENTS_FILE must point to the review comments JSON' "$dir/stderr" ||
  problem "stderr was '$(cat "$dir/stderr")'"
file_is "$dir/pr/a.txt" "$file"
finish

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo 'all cases passed'
