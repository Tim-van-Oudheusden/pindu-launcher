#!/bin/sh
# Tests for ai-extent.sh: a throwaway repository with one commit of each source, the tables the
# script prints for it, and the usage errors.
# Usage: scripts/ai-extent.test.sh (exits 1 if any case fails).
set -eu

root="$(cd "$(dirname "$0")/.." && pwd)"
extent="$root/scripts/ai-extent.sh"
failures=0
nl='
'

ok() { echo "ok: $1"; }
not_ok() {
  echo "FAIL: $1"
  failures=$((failures + 1))
}

repo="$(mktemp -d)"
trap 'rm -rf "$repo"' EXIT
cd "$repo"
git init -q -b main
git config commit.gpgsign false
GIT_COMMITTER_NAME=ci GIT_COMMITTER_EMAIL=ci@example.org
export GIT_COMMITTER_NAME GIT_COMMITTER_EMAIL

# commit <author name> <author email> <file> <lines> <message>: appends <lines> lines to <file>.
commit() {
  mkdir -p "$(dirname "$3")"
  i=0
  while [ "$i" -lt "$4" ]; do
    echo "$1 $i" >>"$3"
    i=$((i + 1))
  done
  git add "$3"
  GIT_AUTHOR_NAME="$1" GIT_AUTHOR_EMAIL="$2" git commit -q -m "$5"
}

maintainer=timvanoudheusden@protonmail.com
commit Tim "$maintainer" build-aux/flatpak/app.yml 10 "manifest by hand"
person="$(git rev-parse HEAD)"
commit Tim "$maintainer" crates/pindu-core/src/a.rs 4 "feat: trailer${nl}${nl}Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>"
commit scanner 1+hive-worker-bee[bot]@users.noreply.github.com crates/pindu-core/src/b.rs 2 "feat: hive"
commit Tim "$maintainer" crates/pindu-ui/src/c.rs 3 "feat: unmarked"
commit "Hosted Weblate" hosted@weblate.org po/nl.po 5 "chore(l10n): update"
commit "dependabot[bot]" 2+dependabot[bot]@users.noreply.github.com Cargo.lock 1 "build(deps): bump"
commit Stranger someone@example.org docs/x.md 2 "docs: outside"
git checkout -q -b feat/7
commit Tim "$maintainer" crates/pindu-ui/src/d.rs 6 "feat: in a marked pull request"
git checkout -q main
git merge -q --no-ff -m "Merge pull request #7 from owner/feat/7" feat/7
commit Tim "$maintainer" build-aux/flatpak/app.yml 2 "feat: agent edit of the manifest"
echo "$person manifest" >"$repo/person.txt"

prs='[{"number":7,"headRefName":"feat/7","author":{"login":"owner"},"body":"- AI disclosure: written by an AI agent (Claude)."}]'

status=0
output="$(printf '%s' "$prs" | sh "$extent" --person-commits "$repo/person.txt" 2>&1)" || status=$?
[ "$status" -eq 0 ] || not_ok "measures the repository (exit $status): $output"

# has <description> <expected line>
has() {
  if printf '%s\n' "$output" | grep -qxF -- "$2"; then
    ok "$1"
  else
    not_ok "$1 (output lacks the line '$2'):$nl$output"
  fi
}

has "a listed commit is a person's, an agent edit after it is AI-assisted" \
  "| build-aux/flatpak (release and development manifests) | 2 | 1 (50%) | 0 (0%) | 1 | 0 | 12 | 2 (17%) | 10 |"
has "an AI co-author trailer and the hive-worker-bee App are marked" \
  "| crates/pindu-core | 2 | 2 (100%) | 2 (100%) | 0 | 0 | 6 | 6 (100%) | 0 |"
has "a marked pull request marks its commits; the maintainer's other commits are unmarked" \
  "| crates/pindu-ui | 2 | 2 (100%) | 1 (50%) | 0 | 0 | 9 | 9 (100%) | 0 |"
has "Weblate is not AI-assisted" "| po (translations) | 1 | 0 (0%) | 0 (0%) | 0 | 1 | 5 | 0 (0%) | 0 |"
has "Dependabot is not AI-assisted" \
  "| build and repository configuration | 1 | 0 (0%) | 0 (0%) | 0 | 1 | 1 | 0 (0%) | 0 |"
has "another author is not counted as AI-assisted" "| docs | 1 | 0 (0%) | 0 (0%) | 0 | 1 | 2 | 0 (0%) | 0 |"
has "the whole repository counts each commit once" \
  "| **Whole repository** | 9 | 5 (56%) | 3 (33%) | 1 | 3 | 35 | 17 (49%) | 10 |"
has "blame gives the person's surviving lines" \
  "| build-aux/flatpak (release and development manifests) | 12 | 10 (83%) |"
has "blame totals the tree" "| **Whole repository** | 35 | 10 (29%) |"

# fails_with <description> <exit status> <stdin> [arguments...]
fails_with() {
  description="$1"
  want="$2"
  input="$3"
  shift 3
  got=0
  printf '%s' "$input" | sh "$extent" "$@" >/dev/null 2>&1 || got=$?
  if [ "$got" -eq "$want" ]; then
    ok "$description"
  else
    not_ok "$description (expected exit $want, got $got)"
  fi
}

fails_with "input that is not an array" 1 '{}' --person-commits "$repo/person.txt"
fails_with "an unknown flag" 2 '[]' --since 2026-01-01
fails_with "a flag without its value" 2 '[]' --rev
fails_with "a revision that does not exist" 2 '[]' --rev no-such-rev --person-commits "$repo/person.txt"
fails_with "a missing person-commits file" 2 '[]' --person-commits "$repo/none.txt"

if [ "$failures" -ne 0 ]; then
  echo "$failures case(s) failed"
  exit 1
fi
echo "all cases passed"
