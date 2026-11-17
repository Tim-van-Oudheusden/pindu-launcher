"""Tests for build-aux/release-notes.py, which turns a MetaInfo <release> into GitHub release notes.

Run with: python3 tests/tooling/test_release_notes.py (or `meson test -C _build --suite tooling`)
"""

import importlib.util
import subprocess
import sys
import tempfile
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
        for arch in ("x86_64", "aarch64"):
            bundle = f"pindu-launcher-{version}-{arch}.flatpak"
            self.assertIn(f"flatpak install --user ./{bundle}", result.stdout)
            self.assertIn(f"pindu-launcher-{version}-1.fc45.{arch}.rpm", result.stdout)
        for arch in ("amd64", "arm64"):
            for distro in ("debian-testing", "ubuntu-26.10"):
                self.assertIn(f"pindu-launcher-{version}-{distro}-{arch}.deb", result.stdout)
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

    def test_finds_an_rcs_notes_under_its_metainfo_version(self) -> None:
        # A pre-release tag vX.Y.Z-rc.N names its files X.Y.Z-rc.N, while the MetaInfo
        # names the release X.Y.Z~rcN (build-aux/check-version.sh).
        spec = importlib.util.spec_from_file_location("release_notes", SCRIPT)
        assert spec is not None and spec.loader is not None
        module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(module)
        with tempfile.TemporaryDirectory() as tmp:
            metainfo = Path(tmp) / "metainfo.xml"
            metainfo.write_text(
                "<component><releases>"
                '<release version="0.5.0~rc1" type="development" date="2026-11-20">'
                "<description><p>A test release.</p></description></release>"
                "</releases></component>"
            )
            notes = module.notes("0.5.0-rc.1", metainfo)
            self.assertIn("A test release.\n", notes)
            self.assertIn("pindu-launcher-0.5.0-rc.1-x86_64.flatpak", notes)
            self.assertIn("pindu-launcher-0.5.0-rc.1-1.fc45.aarch64.rpm", notes)
            self.assertIn("pindu-launcher-0.5.0-rc.1-debian-testing-amd64.deb", notes)
            self.assertIn("pindu-launcher-0.5.0-rc.1.tar.xz", notes)
            self.assertNotIn("~", notes)
            with self.assertRaisesRegex(LookupError, 'no <release version="0.5.0~rc2">'):
                module.notes("0.5.0-rc.2", metainfo)


if __name__ == "__main__":
    unittest.main()
