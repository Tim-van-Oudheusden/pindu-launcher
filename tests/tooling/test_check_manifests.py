"""Tests for build-aux/check-manifests.sh, on the repository's manifests and on copies of them.

Run with: python3 tests/tooling/test_check_manifests.py (or `meson test -C _build --suite tooling`)
"""

import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "build-aux" / "check-manifests.sh"
MANIFESTS = ROOT / "build-aux" / "flatpak"
DEVEL = "io.github.tim_van_oudheusden.pindu-launcher-devel.yml"
RELEASE = "io.github.tim_van_oudheusden.pindu-launcher.yml"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(["sh", str(SCRIPT), *args], capture_output=True, text=True, check=False)


class CheckManifestsTest(unittest.TestCase):
    def setUp(self) -> None:
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        self.copy = Path(tmp.name) / "flatpak"
        shutil.copytree(MANIFESTS, self.copy)

    def add_after(self, manifest: str, anchor: str, line: str) -> None:
        """Insert `line` after the first line of a copied manifest that is `anchor`."""
        file = self.copy / manifest
        lines = file.read_text().splitlines(keepends=True)
        index = lines.index(anchor + "\n")
        lines.insert(index + 1, line + "\n")
        file.write_text("".join(lines))

    def test_passes_on_the_repository(self) -> None:
        result = run()
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_passes_on_an_unchanged_copy(self) -> None:
        result = run(str(self.copy))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_fails_when_the_release_manifest_gains_a_permission(self) -> None:
        self.add_after(RELEASE, "finish-args:", "  - --filesystem=home")
        result = run(str(self.copy))
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("+  - --filesystem=home", result.stdout)
        self.assertIn("permissions or modules differ", result.stderr)

    def test_fails_when_the_release_manifest_bundles_another_module(self) -> None:
        self.add_after(RELEASE, "modules:", "  - name: extra")
        result = run(str(self.copy))
        self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
        self.assertIn("+  - name: extra", result.stdout)

    def test_ignores_the_development_manifests_debug_environment(self) -> None:
        self.add_after(DEVEL, "finish-args:", "  - --env=G_MESSAGES_DEBUG=all")
        result = run(str(self.copy))
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)


if __name__ == "__main__":
    unittest.main()
