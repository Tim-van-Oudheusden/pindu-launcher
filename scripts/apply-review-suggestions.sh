#!/bin/sh
# Automated review application: apply the ```suggestion blocks of PR review
# comments to the checkout in the current directory.
# Reads COMMENTS_FILE, a JSON array of review comments as returned by
# `GET /repos/{owner}/{repo}/pulls/{number}/comments`. Prints one "applied:" or
# "skipped:" line per suggestion; edits files in place and leaves committing to
# the caller. Needs a POSIX shell, coreutils, awk, base64, realpath and jq; runs
# nothing from the checkout.
#
# Applied: the one suggestion block of a comment by a reviewer with write access
# (OWNER, MEMBER or COLLABORATOR), to a regular file inside the checkout.
# Skipped, with the reason printed: other authors, outdated comments, comments
# on the base (LEFT) side of the diff, comments with more than one suggestion
# block, line ranges not in the file, ranges overlapping a suggestion already
# applied to the same file, paths outside the checkout (also through symlinks),
# and workflow files under .github/workflows.
set -eu

comments_file="${COMMENTS_FILE:?COMMENTS_FILE must point to the review comments JSON}"

# One record per suggestion, fields separated by US (0x1f): id, base64 path,
# start line, end line, then "apply" plus the base64 suggestion text or "skip"
# plus the base64 reason. Base64 keeps separators and newlines out of the
# fields; a non-whitespace separator keeps empty fields in place.
# Only reviewers with write access count, and only while the comment still
# applies to the head's side of the diff (GitHub nulls `line` once a comment is
# outdated). A comment with several suggestion blocks is ambiguous and skipped.
# Fences count only at the start of a line, so ``` inside a suggested line
# stays part of it. As in CommonMark, a block ends at a line holding only a
# fence at least as long as the one that opened it, so ````suggestion can carry
# a ``` code block and a suggested ```bash line stays content. An opening fence
# that never closes is ignored. Blocks are found line by line rather than with
# a backreference, which not every jq implementation supports. Records are
# sorted bottom-up per file so earlier edits never shift later ones.
rows="$(jq -r '
  def fence: [match("^`+").length][0];
  def suggestion_blocks:
    . as $lines
    | reduce range(0; ($lines | length) - 1) as $i ({next: 0, blocks: []};
        if $i < .next or ($lines[$i] | test("^`{3,}suggestion[ \t]*$") | not) then .
        else ($lines[$i] | fence) as $n
          | ([range($i + 1; $lines | length) | select($lines[.] | test("^`+[ \t]*$") and fence >= $n)][0]) as $j
          | if $j == null then .
            else .next = $j + 1 | .blocks += [$lines[$i + 1:$j] | map(. + "\n") | add // ""] end
        end)
    | .blocks;
  ["OWNER", "MEMBER", "COLLABORATOR"] as $trusted
  | [ .[]
      | . as $c
      | ((.body // "") | gsub("\r"; "") | split("\n") | suggestion_blocks) as $blocks
      | select($blocks | length > 0)
      | (if ($trusted | index([$c.author_association])) == null
           then "author \($c.author_association // null) has no write access"
         elif $c.line == null then "outdated"
         elif ($c.side // "RIGHT") == "LEFT" or $c.start_side == "LEFT" then "on the base side of the diff"
         elif ($blocks | length) > 1 then "more than one suggestion block"
         else null end) as $skip
      | { id: $c.id,
          path: ($c.path // ""),
          start: ($c.start_line // $c.line // 0),
          end: ($c.line // 0),
          action: (if $skip then ["skip", $skip] else ["apply", $blocks[0]] end) } ]
  | sort_by([.path, -.start])
  | .[]
  | [(.id | tostring), (.path | @base64), (.start | tostring), (.end | tostring), .action[0], (.action[1] | @base64)]
  | join("\u001f")
' "$comments_file")"

[ -n "$rows" ] || {
  echo "no suggestions to apply"
  exit 0
}

# Line count of the current file before any edit (review line numbers refer to
# it), and the lowest line already rewritten; a range reaching that overlaps.
checkout="$(realpath .)"
current_path=""
lines=0
floor=0
us="$(printf '\037')"

while IFS="$us" read -r id path_b64 start end action payload_b64; do
  path="$(printf '%s' "$path_b64" | base64 -d)"
  if [ "$action" = skip ]; then
    echo "skipped: comment $id on $path: $(printf '%s' "$payload_b64" | base64 -d)"
    continue
  fi
  # Resolve symlinks and `..` so nothing outside the checkout is ever written.
  resolved="$(realpath -e -- "$path" 2>/dev/null || true)"
  case "$resolved" in
    "$checkout"/*) ;;
    *) resolved="" ;;
  esac
  if [ -z "$resolved" ] || [ ! -f "$resolved" ]; then
    echo "skipped: comment $id on $path: not a regular file in the checkout"
    continue
  fi
  # A GITHUB_TOKEN push touching workflow files is rejected as a whole, which
  # would lose every other suggestion with it.
  case "$resolved" in
    "$checkout"/.github/workflows/*)
      echo "skipped: comment $id on $path: workflow files cannot be pushed with the workflow token"
      continue
      ;;
  esac
  if [ "$path" != "$current_path" ]; then
    current_path="$path"
    # awk counts a last line without a trailing newline; wc -l does not.
    lines="$(awk 'END { print NR }' "$resolved")"
    floor=$((lines + 1))
  fi
  if [ "$start" -lt 1 ] || [ "$end" -lt "$start" ] || [ "$end" -gt "$lines" ]; then
    echo "skipped: comment $id on $path: lines $start-$end are not in the file"
    continue
  fi
  if [ "$end" -ge "$floor" ]; then
    echo "skipped: comment $id on $path: overlaps another applied suggestion"
    continue
  fi
  floor="$start"
  tmp="$(mktemp)"
  {
    head -n "$((start - 1))" "$resolved"
    printf '%s' "$payload_b64" | base64 -d
    tail -n "+$((end + 1))" "$resolved"
  } >"$tmp"
  # Rewriting in place keeps the file's mode and owner.
  cat "$tmp" >"$resolved"
  rm -f "$tmp"
  echo "applied: $path:$start-$end (comment $id)"
done <<EOF
$rows
EOF
