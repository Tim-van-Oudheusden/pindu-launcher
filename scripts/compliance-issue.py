#!/usr/bin/env python3
"""Open, update or close the one nightly compliance issue from a scripts/nightly-compliance.sh report.

Ported from Tim-van-Oudheusden/Dailies (scripts/compliance_issue.py, with the cut-short report fix of Dailies#290).
.github/workflows/auto-issues.yml runs this after each Nightly compliance run on main, with that run's
`nightly-compliance` artifact. A failing check opens one issue "ci: nightly compliance: N failing checks", labelled
`automated`, `area:packaging` and `type:chore`, or updates the open one; the first run where every check passes again
comments on it and closes it. A run that left no report (it failed before the upload), or one cut short before every
check ran, opens or updates the issue as "ci: nightly compliance: no report" and never closes it. The open issue is
found by MARKER in its body or by its title, so it is still found after its body was edited.

Usage:
    python3 -I scripts/compliance-issue.py --report nightly-compliance.md --run-url URL [--repo OWNER/REPO] [--dry-run]

The token comes from GH_TOKEN or GITHUB_TOKEN (it needs issues: write, or only read access with --dry-run) and the
repository from --repo or GITHUB_REPOSITORY. --dry-run reads the open issues but prints the writes it would send to
stdout instead of sending them. The standard library is enough. Exits 0 once the issue matches the report (failing
checks included: the compliance run itself already failed), 2 on a usage error or when GitHub refused a request.
"""

import argparse
import http
import http.client
import json
import os
import re
import sys
import urllib.parse
from collections.abc import Callable, Sequence
from pathlib import Path
from typing import Any

MARKER = "<!-- pindu-nightly-compliance -->"
# The first label is the one the open issue is looked up by.
LABELS = ["automated", "area:packaging", "type:chore"]
TITLE_PREFIX = "ci: nightly compliance: "
# How an earlier run titled its issue, so it is found even if its body was edited.
ISSUE_TITLE = re.compile(rf"^{re.escape(TITLE_PREFIX)}(\d+ failing checks?|no report)$")
REPORT_HEADING = "# Nightly compliance\n"
# scripts/nightly-compliance.sh prints this last. Without it the run stopped before every check had its row, so the
# missing rows cannot be read as passing.
REPORT_END = "<!-- nightly-compliance: end -->"
FAILING_ROW = re.compile(r"^\| (?P<check>.+) \| ❌ fail \|$", re.MULTILINE)
REPOSITORY = re.compile(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$")
# GitHub refuses an issue body longer than 65536 characters.
MAX_BODY = 65536
API_HOST = "api.github.com"
TIMEOUT = 30

# (method, path, JSON payload or None) -> the decoded JSON answer. Raises OSError when the request fails or is refused.
Api = Callable[[str, str, dict[str, Any] | None], Any]


def github_api(token: str) -> Api:
    """The default Api: HTTPS requests to api.github.com with `token`."""
    headers = {
        "Accept": "application/vnd.github+json",
        "Authorization": f"Bearer {token}",
        "User-Agent": "pindu-compliance-issue",
        "X-GitHub-Api-Version": "2022-11-28",
    }

    def request(method: str, path: str, payload: dict[str, Any] | None) -> object:
        connection = http.client.HTTPSConnection(API_HOST, timeout=TIMEOUT)
        body = None if payload is None else json.dumps(payload)
        try:
            connection.request(method, path, body=body, headers=headers)
            response = connection.getresponse()
            data = response.read()
        except http.client.HTTPException as error:
            msg = f"{method} {path}: {error!r}"
            raise OSError(msg) from error
        finally:
            connection.close()
        if not http.HTTPStatus.OK <= response.status < http.HTTPStatus.MULTIPLE_CHOICES:
            msg = f"{method} {path}: HTTP {response.status} {data.decode(errors='replace')[:300]}"
            raise OSError(msg)
        return json.loads(data) if data else None

    return request


def dry_run(api: Api) -> Api:
    """An Api that sends reads through `api` and prints each write to stdout instead of sending it."""

    def request(method: str, path: str, payload: dict[str, Any] | None) -> object:
        if method == "GET":
            return api(method, path, payload)
        sys.stdout.write(f"{method} {path}\n{json.dumps(payload, indent=2, ensure_ascii=False)}\n")
        return {"number": "<new>"}

    return request


def failing_checks(report: str) -> list[str] | None:
    """The failing checks of a nightly compliance report; None if it is no report.

    A report that does not end with REPORT_END was cut short and counts as no report.
    """
    if not report.startswith(REPORT_HEADING) or report.rstrip().splitlines()[-1] != REPORT_END:
        return None
    return [match["check"] for match in FAILING_ROW.finditer(report)]


def issue_title(failing: list[str] | None) -> str:
    """The issue title for a run with these failing checks (None: no report)."""
    if failing is None:
        return f"{TITLE_PREFIX}no report"
    return f"{TITLE_PREFIX}{len(failing)} failing check{'s' if len(failing) != 1 else ''}"


def issue_body(failing: list[str] | None, report: str, run_url: str) -> str:
    """The issue body: the failing checks and the run, then the report, cut to fit GitHub's limit."""
    footer = (
        "Each Nightly compliance run on `main` updates this issue, and the first run where every check passes closes"
        " it (`.github/workflows/auto-issues.yml`)."
    )
    if failing is None:
        return (
            f"{MARKER}\n[This Nightly compliance run]({run_url}) left no complete `nightly-compliance` report: it"
            f" failed before the upload or stopped before every check ran. Its log says why.\n\n{footer}\n"
        )
    checks = "".join(f"- {check}\n" for check in failing)
    head = f"{MARKER}\n[Nightly compliance]({run_url}) has failing checks:\n\n{checks}\n{footer}\n\n"
    open_report, close_report = "<details>\n<summary>Report</summary>\n\n", "\n</details>\n"
    cut_note = "\n\n… (cut to fit an issue; the run's `nightly-compliance` artifact has the full report)"
    room = MAX_BODY - len(head) - len(open_report) - len(close_report)
    if len(report) > room:
        report = report[: room - len(cut_note)] + cut_note
    return head + open_report + report + close_report


def _is_compliance_issue(issue: dict[str, Any]) -> bool:
    if "pull_request" in issue:
        return False
    return MARKER in (issue.get("body") or "") or bool(ISSUE_TITLE.match(issue.get("title") or ""))


def sync(api: Api, repo: str, report: str | None, run_url: str) -> str:
    """Make the open compliance issue match the report; return what was done ("opened #N", ..., "nothing to do")."""
    failing = failing_checks(report or "")
    issues = f"/repos/{repo}/issues"
    query = urllib.parse.urlencode({"state": "open", "labels": LABELS[0], "per_page": 100})
    existing = next((issue for issue in api("GET", f"{issues}?{query}", None) if _is_compliance_issue(issue)), None)
    if failing == []:
        if existing is None:
            return "nothing to do"
        number = int(existing["number"])
        comment = f"Every nightly compliance check passes again in [this run]({run_url}). Closing."
        api("POST", f"{issues}/{number}/comments", {"body": comment})
        api("PATCH", f"{issues}/{number}", {"state": "closed", "state_reason": "completed"})
        return f"closed #{number}"
    title, body = issue_title(failing), issue_body(failing, report or "", run_url)
    if existing is None:
        created = api("POST", issues, {"title": title, "body": body, "labels": LABELS})
        return f"opened #{created['number']}"
    number = int(existing["number"])
    api("PATCH", f"{issues}/{number}", {"title": title, "body": body})
    return f"updated #{number}"


def _read_report(path: Path) -> str | None:
    try:
        return path.read_text(encoding="utf-8")
    except OSError:
        return None


def main(argv: Sequence[str] | None = None, *, api: Api | None = None) -> int:
    """Sync the compliance issue with the report named on the command line; return the exit code."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--report", type=Path, required=True, help="the run's nightly-compliance.md (may be missing)")
    parser.add_argument("--run-url", required=True, help="the Nightly compliance run, linked from the issue")
    parser.add_argument("--repo", help="OWNER/REPO; GITHUB_REPOSITORY by default")
    parser.add_argument("--dry-run", action="store_true", help="print the writes to stdout instead of sending them")
    args = parser.parse_args(argv)
    token = os.environ.get("GH_TOKEN") or os.environ.get("GITHUB_TOKEN")
    repo = args.repo or os.environ.get("GITHUB_REPOSITORY")
    if not token or not repo or not REPOSITORY.match(repo):
        sys.stderr.write("compliance-issue: needs GH_TOKEN or GITHUB_TOKEN and --repo or GITHUB_REPOSITORY\n")
        return 2
    client = api or github_api(token)
    if args.dry_run:
        client = dry_run(client)
    try:
        outcome = sync(client, repo, _read_report(args.report), args.run_url)
    except (OSError, KeyError, TypeError, ValueError) as error:
        sys.stderr.write(f"compliance-issue: could not write the nightly compliance issue ({error})\n")
        return 2
    sys.stderr.write(f"compliance-issue: {'dry run, nothing sent: ' if args.dry_run else ''}{outcome}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
