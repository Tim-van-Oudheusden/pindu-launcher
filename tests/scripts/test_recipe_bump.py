"""Tests for scripts/recipe-bump.py on copies of the shipped AUR and nixpkgs recipes.

Run with: python3 -m unittest discover -s tests/scripts
"""

import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "recipe-bump.py"
PKGBUILD = Path("build-aux/aur/PKGBUILD")
NIX = Path("build-aux/nix/package.nix")
# sha256 of b"pindu", and the same digest in nixpkgs' SRI form (base64 of its bytes).
DIGEST = "474ce2ba4defdfda4031cb5e843d927674c92dce21485310631cdbeb4bea80ff"
SRI = "sha256-R0ziuk3v39pAMctehD2SdnTJLc4hSFMQYxzb60vqgP8="


class RecipeBumpTest(unittest.TestCase):
    def setUp(self) -> None:
        self.root = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.root)
        for recipe in (PKGBUILD, NIX):
            (self.root / recipe).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(ROOT / recipe, self.root / recipe)
        self.sums = self.root / "SHA256SUMS"
        self.sums.write_text(
            f"{'1' * 64}  pindu-launcher-9.8.7-x86_64.flatpak\n"
            f"{DIGEST}  pindu-launcher-9.8.7.tar.xz\n"
            f"{'2' * 64}  pindu-launcher-9.8.7-1.fc45.x86_64.rpm\n"
        )

    def run_script(self, *args: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            [sys.executable, "-I", str(SCRIPT), "--root", str(self.root), *args],
            capture_output=True,
            text=True,
            check=False,
        )

    def recipe(self, path: Path) -> str:
        return (self.root / path).read_text()

    def test_sets_the_aur_version_checksum_and_resets_the_release(self) -> None:
        path = self.root / PKGBUILD
        path.write_text(path.read_text().replace("pkgrel=1", "pkgrel=3"))

        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 0, result.stderr)
        text = self.recipe(PKGBUILD)
        self.assertIn("\npkgver=9.8.7\n", text)
        self.assertIn("\npkgrel=1\n", text)
        self.assertIn(f"\nsha256sums=('{DIGEST}')\n", text)

    def test_sets_the_nix_version_and_the_sri_source_hash(self) -> None:
        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 0, result.stderr)
        text = self.recipe(NIX)
        self.assertIn('\n  version = "9.8.7";\n', text)
        self.assertIn(f'\n    hash = "{SRI}";\n', text)

    def test_changes_nothing_else_in_the_recipes(self) -> None:
        self.run_script("9.8.7", str(self.sums))

        for recipe in (PKGBUILD, NIX):
            before = (ROOT / recipe).read_text().splitlines()
            after = self.recipe(recipe).splitlines()
            self.assertEqual(len(before), len(after))
            changed = [new for old, new in zip(before, after, strict=True) if old != new]
            self.assertLessEqual(len(changed), 3, changed)

    def test_reports_whether_the_recipes_changed(self) -> None:
        first = self.run_script("9.8.7", str(self.sums))
        second = self.run_script("9.8.7", str(self.sums))

        self.assertIn("bumped to 9.8.7", first.stdout)
        self.assertEqual(second.returncode, 0, second.stderr)
        self.assertIn("already at 9.8.7", second.stdout)

    def test_accepts_the_binary_marker_of_sha256sum(self) -> None:
        self.sums.write_text(f"{DIGEST} *pindu-launcher-9.8.7.tar.xz\n")

        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"sha256sums=('{DIGEST}')", self.recipe(PKGBUILD))

    def test_refuses_a_pre_release(self) -> None:
        result = self.run_script("9.8.7-rc.1", str(self.sums))

        self.assertEqual(result.returncode, 2)
        self.assertIn("final release", result.stderr)
        self.assertEqual(self.recipe(PKGBUILD), (ROOT / PKGBUILD).read_text())

    def test_refuses_sums_without_the_tarball(self) -> None:
        self.sums.write_text(f"{DIGEST}  pindu-launcher-9.8.6.tar.xz\n")

        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 2)
        self.assertIn("pindu-launcher-9.8.7.tar.xz", result.stderr)
        self.assertEqual(self.recipe(NIX), (ROOT / NIX).read_text())

    def test_refuses_a_malformed_checksum(self) -> None:
        self.sums.write_text("abc  pindu-launcher-9.8.7.tar.xz\n")

        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 2)
        self.assertEqual(self.recipe(PKGBUILD), (ROOT / PKGBUILD).read_text())

    def test_refuses_a_recipe_whose_field_it_cannot_find_and_writes_neither(self) -> None:
        path = self.root / NIX
        path.write_text(path.read_text().replace("    hash = ", "    sha256 = "))

        result = self.run_script("9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 2)
        self.assertIn("package.nix", result.stderr)
        self.assertEqual(self.recipe(PKGBUILD), (ROOT / PKGBUILD).read_text())

    def test_writes_the_pull_request_body_that_refs_the_release_issue(self) -> None:
        body = self.root / "body.md"

        result = self.run_script(
            "--body", str(body), "--issue", "42", "--repo-url", "https://github.com/o/r",
            "--run-url", "https://github.com/o/r/actions/runs/7", "9.8.7", str(self.sums),
        )  # fmt: skip

        self.assertEqual(result.returncode, 0, result.stderr)
        text = body.read_text()
        self.assertTrue(text.startswith("Refs #42\n"), text)
        self.assertIn("`pkgver=9.8.7`", text)
        self.assertIn('`version = "9.8.7"`', text)
        self.assertIn(f"https://github.com/o/r/releases/download/v9.8.7/SHA256SUMS: `{DIGEST}`", text)
        self.assertIn("https://github.com/o/r/actions/runs/7", text)
        self.assertIn("close and reopen", text)

    def test_writes_a_body_without_a_reference_when_there_is_no_release_issue(self) -> None:
        body = self.root / "body.md"

        result = self.run_script(
            "--body", str(body), "--repo-url", "https://github.com/o/r",
            "--run-url", "https://github.com/o/r/actions/runs/7", "9.8.7", str(self.sums),
        )  # fmt: skip

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertNotIn("Refs #", body.read_text())

    def test_writes_a_summary_linking_the_compare_page_with_the_body_to_open_the_pull_request_by_hand(self) -> None:
        body = self.root / "body.md"
        summary = self.root / "summary.md"

        result = self.run_script(
            "--body", str(body), "--summary", str(summary), "--issue", "42", "--repo-url", "https://github.com/o/r",
            "9.8.7", str(self.sums),
        )  # fmt: skip

        self.assertEqual(result.returncode, 0, result.stderr)
        text = summary.read_text()
        self.assertIn("(https://github.com/o/r/compare/main...chore/recipes-9.8.7)", text)
        self.assertIn(body.read_text(), text)

    def test_writes_no_summary_when_the_recipes_are_already_at_the_version(self) -> None:
        self.run_script("9.8.7", str(self.sums))
        summary = self.root / "summary.md"

        result = self.run_script("--summary", str(summary), "9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(summary.exists())

    def test_writes_no_body_when_the_recipes_are_already_at_the_version(self) -> None:
        self.run_script("9.8.7", str(self.sums))
        body = self.root / "body.md"

        result = self.run_script("--body", str(body), "9.8.7", str(self.sums))

        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(body.exists())


if __name__ == "__main__":
    unittest.main()
