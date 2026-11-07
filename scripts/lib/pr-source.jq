# Where a pull request came from: the one rule for telling agent work from human work. Every
# script that reports on agent pull requests includes this file (`jq -L scripts/lib 'include
# "pr-source"; ...'`) rather than writing its own copy, so no two reports can disagree (two copies
# of the rule diverged in Tim-van-Oudheusden/Dailies#348). Reads one entry of
# `gh pr list --json author,headRefName,body`; without a body only the author and the branch decide.
#
# Sources, first match wins:
#   Hive             the body has a signature line of its own, Hive's "— hive: agent=<a> … model=<m> …"
#                    or a local agent run's "— omp (Claude Code), model=<m>", or the author is the Hive
#                    GitHub App, hive-worker-bee (gh pr list names it app/<slug>, the REST API <slug>[bot])
#   Claude Code (CI) head branch claude/* (claude.yml)
#   Dependabot       head branch dependabot/*
#   Other            anything else, including local agent runs whose pull requests carry no signature
#                    (they open them under the maintainer's account)

def sources: ["Hive", "Claude Code (CI)", "Dependabot", "Other"];

# The first signature line of the body, or null. It is a line of its own but not always the last.
def signature:
  (.body // "") | split("\n")
  | map(rtrimstr("\r") | select(startswith("— hive: ") or test("^— omp \\(Claude Code\\), model="))) | .[0];

def source:
  (.headRefName // "") as $branch
  | if signature != null or ((.author.login // "") | IN("app/hive-worker-bee", "hive-worker-bee[bot]"))
    then "Hive"
    elif ($branch | startswith("claude/")) then "Claude Code (CI)"
    elif ($branch | startswith("dependabot/")) then "Dependabot"
    else "Other" end;

# Agent work: the sources that follow this repository's agent instructions.
def agent_authored: source | IN("Hive", "Claude Code (CI)");
