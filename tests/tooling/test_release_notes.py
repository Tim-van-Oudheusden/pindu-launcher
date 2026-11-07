"""Tests for build-aux/release-notes.py, which turns a MetaInfo <release> into GitHub release notes.

Run with: python3 tests/tooling/test_release_notes.py (or `meson test -C _build --suite tooling`)
"""

import subprocess
import sys
import unittest
import xml.etree.ElementTree as ET
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "build-aux" / "release-notes.py"
METAINFO = ROOT / "data" / "io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in"


def run(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, str(SCRIPT), *args], capture_output=True, text=True, check=False
    )


def newest_release() -> ET.Element:
    release = ET.parse(METAINFO).getroot().find("./releases/release")
    assert release is not None
    return release


def flat(element: ET.Element) -> str:
    return " ".join("".join(element.itertext()).split())


class ReleaseNotesTest(unittest.TestCase):
    def test_prints_the_newest_releases_notes(self) -> None:
        release = newest_release()
        version = release.get("version")
        result = run(version)
        self.assertEqual(result.returncode, 0, result.stderr)
        description = release.find("description")
        self.assertIsNotNone(description)
        for paragraph in description.findall("p"):
            self.assertIn(flat(paragraph) + "\n", result.stdout)
        items = description.findall("./ul/li")
        self.assertTrue(items, "the newest release lists no changes")
        for item in items:
            self.assertIn(f"- {flat(item)}\n", result.stdout)
        self.assertIn("## Install", result.stdout)
        bundle = f"pindu-launcher-{version}-x86_64.flatpak"
        self.assertIn(f"flatpak install --user ./{bundle}", result.stdout)
        self.assertIn(f"pindu-launcher-{version}.tar.xz", result.stdout)

    def test_fails_for_an_unknown_version(self) -> None:
        result = run("99.0.0")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn('no <release version="99.0.0">', result.stderr)

    def test_fails_without_a_version(self) -> None:
        result = run()
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(result.stdout, "")
        self.assertIn("Usage: build-aux/release-notes.py VERSION", result.stderr)


if __name__ == "__main__":
    unittest.main()
