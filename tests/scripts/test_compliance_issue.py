"""Tests for scripts/compliance-issue.py against a stand-in for the GitHub issues API.

Run with: python3 -m unittest discover -s tests/scripts
"""

import contextlib
import http.client
import importlib.util
import io
import json
import os
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path
from types import ModuleType
from typing import Any
from unittest import mock

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "compliance-issue.py"
REPO = "o/r"
RUN_URL = "https://github.com/o/r/actions/runs/42"
ISSUES = f"/repos/{REPO}/issues"
LABELS = ["automated", "area:packaging", "type:chore"]


def load_script() -> ModuleType:
    # The file name has dashes, so it is loaded from its path rather than imported.
    spec = importlib.util.spec_from_file_location("compliance_issue", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


ci = load_script()


def report(*rows: tuple[str, bool], failures: str = "") -> str:
    """A report as scripts/nightly-compliance.sh prints it: a `| check | result |` row per check, then the end line."""
    lines = ["# Nightly compliance", "", "Commit: `0123abc`", "", "| Check | Result |", "| ----- | ------ |"]
    lines += [f"| {check} | {'✅ pass' if passed else '❌ fail'} |" for check, passed in rows]
    if failures:
        lines += ["", "## Failures", "", failures]
    lines += ["", "## ACMM self-check", "", "ACMM codebase level: L5 (required: L5): ok"]
    lines += ["", "<!-- nightly-compliance: end -->"]
    return "\n".join(lines) + "\n"


PASSING = report(
    ("Dependabot alerts (high+)", True),
    ("RustSec audit of Cargo.lock (cargo audit)", True),
    ("ACMM codebase level", True),
)
FAILING = report(
    ("Dependabot alerts (high+)", False),
    ("RustSec audit of Cargo.lock (cargo audit)", True),
    ("ACMM codebase level", False),
    failures="- GHSA-aaaa-bbbb-cccc (high) zbus: Bad thing",
)


class FakeGitHub:
    """Answers the issue endpoints like api.github.com, from a list of issues; records every request."""

    def __init__(self, issues: list[dict[str, Any]] | None = None, fail: Exception | None = None) -> None:
        self.issues = issues or []
        self.fail = fail
        self.requests: list[tuple[str, str, dict[str, Any] | None]] = []

    def __call__(self, method: str, path: str, payload: dict[str, Any] | None) -> object:
        self.requests.append((method, path, payload))
        if self.fail is not None:
            raise self.fail
        if method == "GET" and path.startswith(f"{ISSUES}?"):
            assert "state=open" in path
            assert "labels=automated" in path
            return [issue for issue in self.issues if issue.get("state", "open") == "open"]
        if method == "POST" and path == ISSUES:
            assert payload is not None
            issue = {"number": 100 + len(self.issues), **payload}
            self.issues.append(issue)
            return issue
        number = int(path.split("/")[-2 if path.endswith("/comments") else -1])
        issue = next(issue for issue in self.issues if issue["number"] == number)
        if method == "POST" and path.endswith("/comments"):
            return {"id": 1}
        if method == "PATCH":
            assert payload is not None
            issue.update(payload)
            return issue
        msg = f"unexpected request {method} {path}"
        raise AssertionError(msg)

    def writes(self) -> list[tuple[str, str, dict[str, Any] | None]]:
        return [request for request in self.requests if request[0] != "GET"]


def compliance_issue(number: int = 7, **fields: Any) -> dict[str, Any]:
    return {
        "number": number,
        "title": "ci: nightly compliance: 1 failing check",
        "body": f"{ci.MARKER}\nold",
        "labels": [{"name": "automated"}],
        **fields,
    }


class SyncTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.report_path = Path(tmp.name) / "nightly-compliance.md"
        env = mock.patch.dict(os.environ, {"GH_TOKEN": "token", "GITHUB_REPOSITORY": REPO})
        env.start()
        self.addCleanup(env.stop)

    def run_main(self, text: str | None, api: FakeGitHub, *extra: str) -> tuple[int, str, str]:
        """Run main() on a report with this text (None: no report); return the exit status, stdout and stderr."""
        if text is not None:
            self.report_path.write_text(text, encoding="utf-8")
        else:
            self.report_path.unlink(missing_ok=True)
        out, err = io.StringIO(), io.StringIO()
        with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
            try:
                status = ci.main(["--report", str(self.report_path), "--run-url", RUN_URL, *extra], api=api)
            except SystemExit as exit_info:
                status = exit_info.code
        return status, out.getvalue(), err.getvalue()

    def test_a_failing_run_opens_one_labelled_issue_naming_the_failing_checks(self) -> None:
        api = FakeGitHub()

        status, _, err = self.run_main(FAILING, api)

        self.assertEqual(status, 0)
        [(method, path, payload)] = api.writes()
        self.assertEqual((method, path), ("POST", ISSUES))
        assert payload is not None
        self.assertEqual(payload["title"], "ci: nightly compliance: 2 failing checks")
        self.assertEqual(payload["labels"], LABELS)
        body = payload["body"]
        self.assertTrue(body.startswith(ci.MARKER))
        self.assertIn("- Dependabot alerts (high+)\n- ACMM codebase level\n", body)
        self.assertNotIn("RustSec audit of Cargo.lock (cargo audit)\n", body.split("<details>")[0])
        self.assertIn(RUN_URL, body)
        # The full report follows, Failures section included.
        self.assertIn("- GHSA-aaaa-bbbb-cccc (high) zbus: Bad thing", body)
        self.assertIn("opened #100", err)

    def test_one_failing_check_is_singular(self) -> None:
        api = FakeGitHub()
        self.run_main(report(("ACMM codebase level", False)), api)
        self.assertEqual(api.issues[0]["title"], "ci: nightly compliance: 1 failing check")

    def test_a_failing_run_updates_the_open_issue_instead_of_opening_another(self) -> None:
        api = FakeGitHub([compliance_issue()])

        status, _, err = self.run_main(FAILING, api)

        self.assertEqual(status, 0)
        [(method, path, payload)] = api.writes()
        self.assertEqual((method, path), ("PATCH", f"{ISSUES}/7"))
        assert payload is not None
        self.assertEqual(payload["title"], "ci: nightly compliance: 2 failing checks")
        self.assertIn("- Dependabot alerts (high+)", payload["body"])
        self.assertIn("updated #7", err)

    def test_the_open_issue_is_found_by_its_title_after_its_body_was_edited(self) -> None:
        api = FakeGitHub([compliance_issue(body="Looking into it.")])
        self.run_main(FAILING, api)
        self.assertEqual([(method, path) for method, path, _ in api.writes()], [("PATCH", f"{ISSUES}/7")])

    def test_other_automated_issues_and_pull_requests_are_left_alone(self) -> None:
        other = {"number": 3, "title": "Sentinel alert", "body": "", "labels": [{"name": "automated"}]}
        pull_request = compliance_issue(number=5, pull_request={"url": "https://api.github.com/repos/o/r/pulls/5"})
        api = FakeGitHub([other, pull_request])

        self.run_main(FAILING, api)

        self.assertEqual([(method, path) for method, path, _ in api.writes()], [("POST", ISSUES)])

    def test_a_passing_run_comments_and_closes_the_open_issue(self) -> None:
        api = FakeGitHub([compliance_issue()])

        status, _, err = self.run_main(PASSING, api)

        self.assertEqual(status, 0)
        comment, close = api.writes()
        self.assertEqual(comment[:2], ("POST", f"{ISSUES}/7/comments"))
        assert comment[2] is not None
        self.assertIn(RUN_URL, comment[2]["body"])
        self.assertEqual(close, ("PATCH", f"{ISSUES}/7", {"state": "closed", "state_reason": "completed"}))
        self.assertIn("closed #7", err)

    def test_a_passing_run_without_an_open_issue_writes_nothing(self) -> None:
        api = FakeGitHub([compliance_issue(state="closed")])
        status, _, err = self.run_main(PASSING, api)
        self.assertEqual(status, 0)
        self.assertEqual(api.writes(), [])
        self.assertIn("nothing to do", err)

    def test_a_run_without_a_report_opens_the_issue_saying_so(self) -> None:
        for name, text in [("missing", None), ("empty", ""), ("not a report", "Error: the job ran out of disk\n")]:
            with self.subTest(name):
                api = FakeGitHub()

                status, _, _ = self.run_main(text, api)

                self.assertEqual(status, 0)
                [(_, _, payload)] = api.writes()
                assert payload is not None
                self.assertEqual(payload["title"], "ci: nightly compliance: no report")
                self.assertIn(RUN_URL, payload["body"])
                self.assertIn("nightly-compliance", payload["body"])

    def test_a_report_cut_short_updates_the_open_issue_instead_of_closing_it(self) -> None:
        # A run that timed out or stopped early still uploads what it printed; the rows it never reached are not passes.
        some_rows = PASSING.split("\n| RustSec")[0] + "\n"
        no_rows = PASSING.split("| Dependabot")[0]
        for name, cut_short in [("some rows", some_rows), ("no rows", no_rows)]:
            with self.subTest(name):
                api = FakeGitHub([compliance_issue()])

                status, _, _ = self.run_main(cut_short, api)

                self.assertEqual(status, 0)
                [(method, path, payload)] = api.writes()
                self.assertEqual((method, path), ("PATCH", f"{ISSUES}/7"))
                assert payload is not None
                self.assertEqual(payload["title"], "ci: nightly compliance: no report")

    def test_a_long_report_is_cut_to_fit_an_issue_body(self) -> None:
        api = FakeGitHub()
        huge = report(("Workflows (actionlint)", False), failures="x" * 200_000)

        self.run_main(huge, api)

        body = api.issues[0]["body"]
        self.assertLessEqual(len(body), ci.MAX_BODY)
        self.assertIn("- Workflows (actionlint)", body)
        self.assertIn("cut", body)
        self.assertTrue(body.rstrip().endswith("</details>"))

    def test_a_dry_run_reads_the_issues_and_prints_the_writes_instead_of_sending_them(self) -> None:
        for name, issues, text, want in [
            ("open", [], FAILING, "POST /repos/o/r/issues"),
            ("update", [compliance_issue()], FAILING, "PATCH /repos/o/r/issues/7"),
            ("close", [compliance_issue()], PASSING, "POST /repos/o/r/issues/7/comments"),
        ]:
            with self.subTest(name):
                api = FakeGitHub(issues)

                status, out, err = self.run_main(text, api, "--dry-run")

                self.assertEqual(status, 0)
                self.assertEqual([method for method, _, _ in api.requests], ["GET"])
                self.assertIn(want, out)
                self.assertIn("dry run", err)
        _, out, _ = self.run_main(FAILING, FakeGitHub(), "--dry-run")
        self.assertIn('"title": "ci: nightly compliance: 2 failing checks"', out)

    def test_an_api_error_exits_2(self) -> None:
        api = FakeGitHub(fail=OSError("HTTP 403: Resource not accessible by integration"))
        status, _, err = self.run_main(FAILING, api)
        self.assertEqual(status, 2)
        self.assertIn("Resource not accessible by integration", err)

    def test_a_missing_token_or_repository_exits_2_without_a_request(self) -> None:
        for name, token, repo in [("no token", None, REPO), ("no repo", "token", None), ("bad repo", "token", "a b")]:
            with self.subTest(name):
                env = {"GITHUB_TOKEN": token, "GITHUB_REPOSITORY": repo}
                with mock.patch.dict(os.environ, {k: v for k, v in env.items() if v}, clear=True):
                    api = FakeGitHub()
                    status, _, _ = self.run_main(FAILING, api)
                self.assertEqual(status, 2)
                self.assertEqual(api.requests, [])

    def test_the_repo_flag_overrides_the_environment(self) -> None:
        api = FakeGitHub()
        with mock.patch.dict(os.environ, {"GITHUB_REPOSITORY": "someone/else"}):
            self.run_main(FAILING, api, "--repo", REPO)
        self.assertEqual(api.writes()[0][1], ISSUES)

    def test_missing_arguments_are_a_usage_error(self) -> None:
        with contextlib.redirect_stderr(io.StringIO()), self.assertRaises(SystemExit) as exit_info:
            ci.main(["--run-url", RUN_URL], api=FakeGitHub())
        self.assertEqual(exit_info.exception.code, 2)


@unittest.skipUnless(shutil.which("jq"), "nightly-compliance.sh needs jq")
class NightlyReportTest(unittest.TestCase):
    """Keeps the two scripts in step: the real nightly-compliance.sh report, with stubbed tools, read by this one."""

    def test_the_failing_rows_of_a_real_report_are_read(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp) / "bin"
            bin_dir.mkdir()
            for tool, status in [("cargo-audit", 0), ("flatpak", 0), ("actionlint", 1)]:
                (bin_dir / tool).write_text(f"#!/bin/sh\necho stub\nexit {status}\n", encoding="utf-8")
                (bin_dir / tool).chmod(0o755)
            alerts = Path(tmp) / "alerts.json"
            alerts.write_text("[]", encoding="utf-8")
            env = {
                **os.environ,
                "PATH": f"{bin_dir}{os.pathsep}{os.environ['PATH']}",
                "ALERTS_JSON": str(alerts),
                # The committed ruleset as the live one: that row passes.
                "RULESET_JSON": str(ROOT / ".github" / "rulesets" / "main.json"),
                "ACMM_MIN_LEVEL": "0",
                "GITHUB_SHA": "abc123",
            }
            result = subprocess.run(
                ["sh", str(ROOT / "scripts" / "nightly-compliance.sh")],
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )

        self.assertEqual(result.returncode, 1, result.stderr)
        self.assertEqual(ci.failing_checks(result.stdout), ["Workflows (actionlint)"])


class FakeConnection:
    """Replaces HTTPSConnection: answers with (status, body) or raises `fail`; records the host and each request."""

    def __init__(self, status: int = 200, body: bytes = b"", fail: Exception | None = None) -> None:
        self.status, self.body, self.fail = status, body, fail
        self.hosts: list[str] = []
        self.requests: list[tuple[str, str, str | None, dict[str, str]]] = []
        self.closed = 0

    def __call__(self, host: str, timeout: float) -> "FakeConnection":
        assert timeout > 0
        self.hosts.append(host)
        return self

    def request(self, method: str, path: str, body: str | None, headers: dict[str, str]) -> None:
        self.requests.append((method, path, body, headers))
        if self.fail is not None:
            raise self.fail

    def getresponse(self) -> "FakeConnection":
        return self

    def read(self) -> bytes:
        return self.body

    def close(self) -> None:
        self.closed += 1


class GitHubApiTest(unittest.TestCase):
    """github_api, the default Api, against a stand-in for http.client.HTTPSConnection."""

    def connect(self, connection: FakeConnection) -> None:
        patch = mock.patch.object(ci.http.client, "HTTPSConnection", connection)
        patch.start()
        self.addCleanup(patch.stop)

    def test_it_sends_json_with_the_token_and_decodes_the_answer(self) -> None:
        connection = FakeConnection(201, b'{"number": 9}')
        self.connect(connection)

        answer = ci.github_api("secret")("POST", ISSUES, {"title": "t"})

        self.assertEqual(answer, {"number": 9})
        self.assertEqual(connection.hosts, ["api.github.com"])
        [(method, path, body, headers)] = connection.requests
        self.assertEqual((method, path), ("POST", ISSUES))
        assert body is not None
        self.assertEqual(json.loads(body), {"title": "t"})
        self.assertEqual(headers["Authorization"], "Bearer secret")
        self.assertEqual(headers["Accept"], "application/vnd.github+json")
        self.assertEqual(connection.closed, 1)

    def test_it_sends_no_body_for_a_get_and_reads_an_empty_answer_as_none(self) -> None:
        connection = FakeConnection(204, b"")
        self.connect(connection)

        self.assertIsNone(ci.github_api("secret")("GET", ISSUES, None))
        self.assertIsNone(connection.requests[0][2])

    def test_it_raises_oserror_with_the_status_on_a_refusal(self) -> None:
        self.connect(FakeConnection(403, b'{"message": "Resource not accessible by integration"}'))

        with self.assertRaisesRegex(OSError, r"HTTP 403 .*Resource not accessible by integration"):
            ci.github_api("secret")("PATCH", f"{ISSUES}/7", {"state": "closed"})

    def test_it_raises_oserror_when_the_connection_drops(self) -> None:
        connection = FakeConnection(fail=http.client.RemoteDisconnected("Remote end closed connection"))
        self.connect(connection)

        with self.assertRaisesRegex(OSError, "RemoteDisconnected"):
            ci.github_api("secret")("GET", ISSUES, None)
        self.assertEqual(connection.closed, 1)


if __name__ == "__main__":
    unittest.main()
