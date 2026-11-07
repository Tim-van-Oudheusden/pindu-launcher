"""Tests for build-aux/check-version.sh, on the repository and on copies of the files it reads.

Run with: python3 tests/tooling/test_check_version.py (or `meson test -C _build --suite tooling`)
"""

import re
import shutil
import subprocess
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
METAINFO = Path("data/io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in")
SCRIPT = Path("build-aux/check-version.sh")
# Each file that names the version, and the text in it that names it.
VERSIONED = {
    Path("meson.build"): "  version: '{}',",
    Path("Cargo.toml"): 'version = "{}"',
    METAINFO: '<release version="{}"',
    Path("build-aux/rpm/pindu-launcher.spec"): "Version:        {}",
    Path("build-aux/deb/debian/changelog"): "pindu-launcher ({}-",
}


def version() -> str:
    """The version meson.build names."""
    match = re.search(r"^  version: '([^']*)',$", (ROOT / "meson.build").read_text(), re.M)
    assert match is not None
    return match.group(1)


def run(root: Path, *args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        ["sh", str(root / SCRIPT), *args], capture_output=True, text=True, check=False
    )


class CheckVersionTest(unittest.TestCase):
    def copy_tree(self) -> Path:
        """A temporary copy of the script and the files it reads."""
        tmp = tempfile.TemporaryDirectory()
        self.addCleanup(tmp.cleanup)
        copy = Path(tmp.name)
        for path in [SCRIPT, *VERSIONED]:
            (copy / path).parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(ROOT / path, copy / path)
        return copy

    def bump(self, copy: Path, path: Path) -> None:
        """Change the version in one copied file to one no other file names."""
        file = copy / path
        current = VERSIONED[path].format(version())
        text = file.read_text()
        self.assertIn(current, text)
        file.write_text(text.replace(current, VERSIONED[path].format("99.0.0"), 1))

    def test_passes_on_the_repository(self) -> None:
        result = run(ROOT)
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_passes_with_the_matching_tag(self) -> None:
        result = run(self.copy_tree(), f"v{version()}")
        self.assertEqual(result.returncode, 0, result.stdout + result.stderr)

    def test_fails_when_one_file_names_another_version(self) -> None:
        for path in VERSIONED:
            with self.subTest(path=str(path)):
                copy = self.copy_tree()
                self.bump(copy, path)
                result = run(copy)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn("The versions disagree", result.stderr)

    def test_fails_with_a_tag_for_another_version(self) -> None:
        copy = self.copy_tree()
        for tag in ["v99.0.0", version()]:
            with self.subTest(tag=tag):
                result = run(copy, tag)
                self.assertEqual(result.returncode, 1, result.stdout + result.stderr)
                self.assertIn(f"Tag {tag} does not match version {version()}", result.stderr)


if __name__ == "__main__":
    unittest.main()
