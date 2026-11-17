#!/bin/sh
# AI-assisted extent per part of the repository, measured from the git history, for AI_DISCLOSURE.md.
#
# Usage:
#   gh pr list --state merged --limit 1000 --json number,author,headRefName,body \
#     | scripts/ai-extent.sh [--rev REV] [--person-commits FILE]
#
# Reads the merged pull requests on stdin (`[]` measures from the commits alone) and prints two Markdown
# tables for every non-merge commit reachable from REV (default HEAD):
#
# 1. Per part, its commits and changed lines (added plus deleted, from `git log --numstat`; binary
#    files count no lines), split by where each commit came from, first match wins:
#    - Person, no AI: listed in FILE (default scripts/lib/person-commits.txt)
#    - Weblate: authored by Hosted Weblate or a translator's Weblate account
#    - Dependabot: authored by dependabot[bot]
#    - AI, marked: a `Co-authored-by:` trailer naming an AI model, a `Hive-Run:` trailer, authored by
#      the hive-worker-bee App, or merged in a pull request that lib/pr-source.jq calls agent work
#      or whose body says it was written by an AI agent
#    - AI, unmarked: authored under the maintainer's account with none of those marks; the
#      maintainer commits from CLI agent sessions (docs/multi-agent.md)
#    - Other: anyone else
# 2. Per part, the text lines at REV and how many of them `git blame` gives to a commit in FILE.
#
# A commit belongs to the pull request whose merge commit, on REV's first-parent history, first
# brought it in. Exit status: 0 on success, 1 for input that is not a JSON array, 2 for bad arguments.
set -eu

here="$(cd "$(dirname "$0")" && pwd)"

usage() {
  echo "usage: ai-extent.sh [--rev REV] [--person-commits FILE]" >&2
  exit 2
}

rev=HEAD
person_file="$here/lib/person-commits.txt"
while [ "$#" -gt 0 ]; do
  # A flag needs its value: `shift 2` with one argument left shifts nothing and would loop forever.
  case "$1" in
    --rev | --person-commits)
      [ "$#" -ge 2 ] || usage
      case "$1" in
        --rev) rev="$2" ;;
        *) person_file="$2" ;;
      esac
      shift 2
      ;;
    *) usage ;;
  esac
done
[ -r "$person_file" ] || usage
git rev-parse --verify -q "$rev^{commit}" >/dev/null || usage

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Pull requests that carry an AI mark, one number per line.
if ! jq -r -L "$here/lib" '
  include "pr-source";
  if type != "array" then error("not an array") else . end
  | .[] | select(agent_authored or ((.body // "") | test("written by an AI agent"; "i"))) | .number
' >"$work/marked-prs" 2>/dev/null; then
  echo "ai-extent.sh: stdin is not a JSON array of pull requests" >&2
  exit 1
fi

# Full hashes of the person commits that exist in this repository.
sed -e 's/#.*//' "$person_file" | while read -r sha _; do
  [ -n "$sha" ] || continue
  git rev-parse --verify -q "$sha^{commit}" || true
done >"$work/person"

# Commit to pull request, for the commits each first-parent merge of a pull request brought in.
git log --first-parent --merges --format='%H %s' "$rev" | while read -r merge subject; do
  case "$subject" in
    "Merge pull request #"*) ;;
    *) continue ;;
  esac
  pr="${subject#Merge pull request #}"
  pr="${pr%% *}"
  git rev-list --no-merges "$merge^1..$merge^2" | sed "s/\$/ $pr/"
done >"$work/commit-prs"

# The part a path belongs to, as an awk function both programs share.
part_fn='
function part(p) {
  if (p ~ /^crates\/pindu-core\//) return "crates/pindu-core"
  if (p ~ /^crates\/pindu-ui\//) return "crates/pindu-ui"
  if (p ~ /^crates\/manette(-sys)?\//) return "crates/manette, crates/manette-sys"
  if (p ~ /^data\/ui\//) return "data/ui (Blueprint)"
  if (p ~ /^data\/.*metainfo/) return "data: MetaInfo"
  if (p ~ /^data\/icons\//) return "data/icons"
  if (p ~ /^data\/screenshots\//) return "data/screenshots"
  if (p ~ /^data\//) return "data: other (desktop, GSettings, D-Bus, CSS, themes, Home art)"
  if (p ~ /^po\//) return "po (translations)"
  if (p ~ /^tests\//) return "tests"
  if (p ~ /^scripts\//) return "scripts"
  if (p ~ /^\.github\/workflows\//) return ".github/workflows"
  if (p == ".github/copilot-instructions.md" || p ~ /^(AGENTS\.md|CLAUDE\.md|\.claude\/|\.cursor\/|prompts\/)/) \
    return "agent instructions (AGENTS.md, CLAUDE.md, .claude, .cursor, prompts)"
  if (p ~ /^\.github\//) return ".github: other (templates, rulesets, policies)"
  # The development manifest started at the root, as io.github.tim_van_oudheusden.pindu-launcher.devel (#4).
  if (p ~ /^build-aux\/flatpak\// || p ~ /^io\.github\.[^\/]*$/) return "build-aux/flatpak (release and development manifests)"
  if (p ~ /^build-aux\/(rpm|deb)\//) return "build-aux/rpm, build-aux/deb"
  if (p ~ /^build-aux\//) return "build-aux: release and check scripts"
  if (p ~ /^docs\//) return "docs"
  if (p ~ /^assets\//) return "assets"
  if (p == "LICENSE") return "LICENSE (the GPL-3.0 text)"
  if (p ~ /^(README|CONTRIBUTING|SECURITY|CODE_OF_CONDUCT|AI_DISCLOSURE)\.md$/) \
    return "top-level documents"
  return "build and repository configuration"
}
function pct(n, d) { return d ? sprintf("%.0f%%", 100 * n / d) : "—" }
'

echo "## Commits and changed lines per part"
echo
echo "Measured at $(git rev-parse --short "$rev") ($(git log -1 --format=%cs "$rev"))."
echo
git -c core.quotePath=false log --no-merges --no-renames --numstat \
  --format='C%x09%H%x09%ae%x09%an%x09%(trailers:key=Co-authored-by,valueonly,separator=%x2C)%x09%(trailers:key=Hive-Run,valueonly,separator=%x2C)' \
  "$rev" | awk -F '\t' -v marked="$work/marked-prs" -v person="$work/person" -v prs="$work/commit-prs" "$part_fn"'
BEGIN {
  while ((getline line < marked) > 0) marked_pr[line] = 1
  while ((getline line < person) > 0) is_person[line] = 1
  while ((getline line < prs) > 0) { split(line, f, " "); pr_of[f[1]] = f[2] }
  maint["timvanoudheusden@protonmail.com"] = 1
  maint["118523958+tim-van-oudheusden@users.noreply.github.com"] = 1
  maint["tim-van-oudheusden@users.noreply.github.com"] = 1
  nc = split("person weblate dependabot marked unmarked other", classes, " ")
}
function classify(sha, email, name, coauthors, hiverun,    e) {
  e = tolower(email)
  if (sha in is_person) return "person"
  if (e == "hosted@weblate.org" || e ~ /@users\.noreply\.hosted\.weblate\.org$/) return "weblate"
  if (name == "dependabot[bot]") return "dependabot"
  if (tolower(coauthors) ~ /claude|anthropic|copilot|cursor|openai|gpt|gemini/ || hiverun != "" \
      || e ~ /hive-worker-bee/ || ((sha in pr_of) && (pr_of[sha] in marked_pr))) return "marked"
  if (e in maint) return "unmarked"
  return "other"
}
$1 == "C" { cls = classify($2, $3, $4, $5, $6); total[cls]++; split("", seen); next }
NF >= 3 {
  p = part($3)
  lines = ($1 == "-") ? 0 : $1 + $2
  if (!((p, cls) in seen)) { seen[p, cls] = 1; commits[p, cls]++ }
  changed[p, cls] += lines
  all_lines[cls] += lines
  parts[p] = 1
}
function row(name, c, l,    i, k, n, ai, ail, nl) {
  n = 0; nl = 0
  for (i = 1; i <= nc; i++) { k = classes[i]; n += c[k]; nl += l[k] }
  ai = c["marked"] + c["unmarked"]; ail = l["marked"] + l["unmarked"]
  printf "| %s | %d | %d (%s) | %d (%s) | %d | %d | %d | %d (%s) | %d |\n", name, n, ai, pct(ai, n), \
    c["marked"], pct(c["marked"], n), c["person"], c["weblate"] + c["dependabot"] + c["other"], \
    nl, ail, pct(ail, nl), l["person"]
}
END {
  print "| Part | Commits | AI-assisted | of them marked | Person, no AI | Weblate, Dependabot, other | Lines changed | AI-assisted lines | Person lines |"
  print "| --- | --- | --- | --- | --- | --- | --- | --- | --- |"
  n = 0
  for (p in parts) names[++n] = p
  for (i = 2; i <= n; i++) { v = names[i]; for (j = i - 1; j >= 1 && names[j] > v; j--) names[j + 1] = names[j]; names[j + 1] = v }
  for (i = 1; i <= n; i++) {
    for (j = 1; j <= nc; j++) { k = classes[j]; c[k] = commits[names[i], k]; l[k] = changed[names[i], k] }
    row(names[i], c, l)
  }
  row("**Whole repository**", total, all_lines)
}'

echo
echo "## Lines in the tree written by a person without AI"
echo
# Text lines per part at REV (`git grep -I` skips binary files), then the lines `git blame` gives to a
# person commit, in the files those commits touched.
git -c core.quotePath=false grep -I -c '' "$rev" -- . | sed "s|^[^:]*:||" >"$work/line-counts"
while read -r sha; do
  git -c core.quotePath=false diff-tree --no-commit-id --name-only --no-renames -r --root "$sha"
done <"$work/person" | sort -u >"$work/person-paths"
while IFS= read -r path; do
  git cat-file -e "$rev:$path" 2>/dev/null || continue
  git grep -I -q '' "$rev" -- "$path" || continue
  printf '%s\t' "$path"
  git blame --line-porcelain "$rev" -- "$path" | grep -E '^[0-9a-f]{40} ' | cut -d ' ' -f 1 \
    | grep -c -F -x -f "$work/person" || true
done <"$work/person-paths" >"$work/person-lines"
awk -v counts="$work/line-counts" -v pl="$work/person-lines" "$part_fn"'
BEGIN {
  while ((getline l < counts) > 0) { i = match(l, /:[0-9]+$/); p = substr(l, 1, i - 1); total[part(p)] += substr(l, i + 1); all += substr(l, i + 1) }
  while ((getline l < pl) > 0) { split(l, f, "\t"); person[part(f[1])] += f[2]; mine += f[2] }
  print "| Part | Text lines | Last written by a person, no AI |"
  print "| --- | --- | --- |"
  n = 0
  for (p in total) names[++n] = p
  for (i = 2; i <= n; i++) { v = names[i]; for (j = i - 1; j >= 1 && names[j] > v; j--) names[j + 1] = names[j]; names[j + 1] = v }
  for (i = 1; i <= n; i++) printf "| %s | %d | %d (%s) |\n", names[i], total[names[i]], person[names[i]], pct(person[names[i]], total[names[i]])
  printf "| **Whole repository** | %d | %d (%s) |\n", all, mine, pct(mine, all)
}'
