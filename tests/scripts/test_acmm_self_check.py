"""Tests for scripts/acmm-self-check.py against throwaway file trees.

Run with: python3 -m unittest discover -s tests/scripts
"""

import contextlib
import importlib.util
import io
import tempfile
import unittest
from collections import Counter
from pathlib import Path
from types import ModuleType

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "acmm-self-check.py"


def load_script() -> ModuleType:
    # The file name has dashes, so it is loaded from its path rather than imported.
    spec = importlib.util.spec_from_file_location("acmm_self_check", SCRIPT)
    assert spec is not None
    assert spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


acmm = load_script()


def create(root: Path, *patterns: str) -> None:
    for pattern in patterns:
        path = root / pattern.rstrip("/")
        if pattern.endswith("/"):
            path.mkdir(parents=True, exist_ok=True)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.touch()


def satisfy(root: Path, level: int, count: int | None = None) -> None:
    """Create the first path of the first `count` criteria of a level (all of them by default)."""
    at_level = [criterion for criterion in acmm.CRITERIA if criterion.level == level]
    for criterion in at_level[:count]:
        create(root, criterion.patterns[0])


def level_of(root: Path) -> int:
    return acmm.codebase_level(acmm.evaluate(root))


def run_main(argv: list[str]) -> tuple[int, str, str]:
    """Run main() and return its exit status, stdout and stderr; argparse errors exit with status 2."""
    out, err = io.StringIO(), io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(err):
        try:
            status = acmm.main(argv)
        except SystemExit as exit_info:
            status = exit_info.code
    return status, out.getvalue(), err.getvalue()


class TreeTestCase(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.root = Path(tmp.name)


class VendoredCriteriaTest(unittest.TestCase):
    def test_the_vendored_list_has_hives_50_criteria_per_level(self) -> None:
        self.assertEqual(len(acmm.CRITERIA), 50)
        self.assertEqual(
            Counter(criterion.level for criterion in acmm.CRITERIA), {0: 8, 2: 8, 3: 8, 4: 9, 5: 5, 6: 12}
        )
        self.assertEqual(len({criterion.key for criterion in acmm.CRITERIA}), 50)
        self.assertTrue(all(criterion.key.startswith(f"{criterion.source}:") for criterion in acmm.CRITERIA))
        self.assertTrue(all(criterion.patterns for criterion in acmm.CRITERIA))

    def test_the_header_records_the_hive_commit_it_was_vendored_from(self) -> None:
        text = SCRIPT.read_text(encoding="utf-8")
        self.assertIn("hivecommons/hive", text)
        self.assertIn("3fcbd6d1b5d40fc7658202ac66cb28149f9a81c7", text)


class ScoringTest(TreeTestCase):
    def test_an_empty_tree_has_unmet_prerequisites(self) -> None:
        results = acmm.evaluate(self.root)
        self.assertEqual([result.level for result in results], [0, 2, 3, 4, 5, 6])
        self.assertFalse(any(result.met for result in results))
        self.assertEqual(acmm.codebase_level(results), 0)

    def test_a_directory_pattern_needs_a_directory(self) -> None:
        (self.root / "tests").touch()
        self.assertFalse(acmm.pattern_exists(self.root, "tests/"))
        self.assertTrue(acmm.pattern_exists(self.root, "tests"))

    def test_a_file_pattern_matches_a_directory_too(self) -> None:
        # .cursor/rules is a directory of .mdc files in the owner's repositories and passes in Hive.
        create(self.root, ".cursor/rules/agent-guidance.mdc")
        self.assertTrue(acmm.pattern_exists(self.root, ".cursor/rules"))

    def test_any_one_pattern_meets_a_criterion(self) -> None:
        test_suite = next(criterion for criterion in acmm.CRITERIA if criterion.key == "acmm:prereq-test-suite")
        self.assertFalse(acmm.criterion_met(self.root, test_suite))
        create(self.root, "spec/")
        self.assertTrue(acmm.criterion_met(self.root, test_suite))

    def test_passing_l0_alone_gives_l1(self) -> None:
        satisfy(self.root, 0)
        self.assertEqual(level_of(self.root), 1)

    def test_a_level_passes_at_70_percent(self) -> None:
        for level, met, passed in [(0, 6, True), (0, 5, False), (4, 7, True), (4, 6, False)]:
            with self.subTest(level=level, met=met), tempfile.TemporaryDirectory() as tmp:
                root = Path(tmp)
                satisfy(root, level, met)
                result = next(result for result in acmm.evaluate(root) if result.level == level)
                self.assertEqual(len(result.met), met)
                self.assertIs(result.passed, passed)

    def test_levels_must_pass_consecutively(self) -> None:
        for level in (0, 3, 4, 5):
            satisfy(self.root, level)
        self.assertEqual(level_of(self.root), 1)
        satisfy(self.root, 2)
        self.assertEqual(level_of(self.root), 5)

    def test_higher_levels_do_not_count_without_prerequisites(self) -> None:
        for level in (2, 3, 4, 5, 6):
            satisfy(self.root, level)
        self.assertEqual(level_of(self.root), 0)

    def test_every_level_passing_gives_l6(self) -> None:
        for level in (0, 2, 3, 4, 5, 6):
            satisfy(self.root, level)
        self.assertEqual(level_of(self.root), 6)


class MainTest(TreeTestCase):
    def test_without_min_level_the_default_minimum_applies(self) -> None:
        # DEFAULT_MIN_LEVEL rises as Hive readiness (#237) completes levels; see the comment above it.
        required = acmm.level_label(acmm.DEFAULT_MIN_LEVEL)
        status, report, _ = run_main(["--root", str(self.root)])
        self.assertEqual(status, 0 if acmm.DEFAULT_MIN_LEVEL == 0 else 1)
        self.assertTrue(report.startswith(f"ACMM codebase level: L0 Prerequisites not met (required: {required})"))

    def test_this_checkout_reaches_the_default_minimum(self) -> None:
        status, report, _ = run_main(["--root", str(ROOT)])
        self.assertEqual(status, 0, report)

    def test_main_passes_at_the_minimum_level_and_still_lists_the_gaps(self) -> None:
        for level in (0, 2, 3):
            satisfy(self.root, level)
        status, report, _ = run_main(["--root", str(self.root), "--min-level", "3"])
        self.assertEqual(status, 0)
        self.assertTrue(report.startswith("ACMM codebase level: L3 Quality-Gated (required: L3 Quality-Gated): ok"))
        self.assertIn("- L3 Quality-Gated: 8/8 (100%) pass", report)
        self.assertIn("- L4 Security-Aware: 1/9 (11%) FAIL", report)
        self.assertIn("missing `acmm:nightly-compliance` (Nightly compliance)", report)

    def test_main_fails_below_the_minimum_and_names_the_missing_criterion(self) -> None:
        satisfy(self.root, 0, 7)
        status, report, _ = run_main(["--root", str(self.root), "--min-level", "2"])
        self.assertEqual(status, 1)
        self.assertTrue(
            report.startswith("ACMM codebase level: L1 Inception (required: L2 Advisory): BELOW REQUIRED LEVEL")
        )
        self.assertIn("- L0 Prerequisites: 7/8 (88%) pass", report)
        self.assertIn(
            "missing `acmm:prereq-coverage-gate` (Coverage gate): none of codecov.yml, .codecov.yml", report
        )
        self.assertIn("- L2 Advisory: 0/8 (0%) FAIL", report)

    def test_deleting_a_path_drops_the_level_and_names_the_criterion(self) -> None:
        satisfy(self.root, 0, 6)
        self.assertEqual(run_main(["--root", str(self.root), "--min-level", "1"])[0], 0)
        (self.root / ".github" / "workflows").rmdir()
        status, report, _ = run_main(["--root", str(self.root), "--min-level", "1"])
        self.assertEqual(status, 1)
        self.assertIn("- L0 Prerequisites: 5/8 (62%) FAIL", report)
        self.assertIn("missing `acmm:prereq-cicd` (CI/CD pipeline)", report)

    def test_min_level_is_configurable(self) -> None:
        satisfy(self.root, 0)
        self.assertEqual(run_main(["--root", str(self.root), "--min-level", "1"])[0], 0)
        self.assertEqual(run_main(["--root", str(self.root), "--min-level", "2"])[0], 1)
        self.assertEqual(run_main(["--root", str(self.root), "--min-level", "0"])[0], 0)

    def test_bad_arguments_are_rejected(self) -> None:
        for argv in (["--min-level", "7"], ["--min-level", "x"], ["--root", "/nonexistent/acmm-root"]):
            with self.subTest(argv=argv):
                status, _, err = run_main(argv)
                self.assertEqual(status, 2)
                self.assertIn("error", err)

    def test_root_defaults_to_this_checkout(self) -> None:
        self.assertEqual(acmm.parse_args([]).root, ROOT)


if __name__ == "__main__":
    unittest.main()
