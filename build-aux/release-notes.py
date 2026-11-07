#!/usr/bin/env python3
"""Print a version's GitHub release notes, in Markdown, from its MetaInfo <release> entry.

Usage: build-aux/release-notes.py VERSION
"""

import sys
import xml.etree.ElementTree as ET
from pathlib import Path

METAINFO = (
    Path(__file__).resolve().parent.parent
    / "data"
    / "io.github.tim_van_oudheusden.pindu-launcher.metainfo.xml.in"
)
APP_ID = "io.github.tim_van_oudheusden.pindu-launcher"


def text(element: ET.Element) -> str:
    """An element's text with inline markup (<em>, <code>) flattened and spaces collapsed."""
    return " ".join("".join(element.itertext()).split())


def notes(version: str, metainfo: Path = METAINFO) -> str:
    """The Markdown notes for `version`; raises LookupError when the MetaInfo has no entry."""
    root = ET.parse(metainfo).getroot()
    release = root.find(f"./releases/release[@version='{version}']")
    if release is None:
        raise LookupError(f"{metainfo.name} has no <release version=\"{version}\">")
    blocks = []
    description = release.find("description")
    for block in [] if description is None else description:
        if block.tag == "p":
            blocks.append(text(block))
        elif block.tag in ("ul", "ol"):
            blocks.append("\n".join(f"- {text(item)}" for item in block.findall("li")))
    bundle = f"pindu-launcher-{version}-x86_64.flatpak"
    blocks.append(
        "## Install\n\n"
        f"Download `{bundle}` and install it with Flatpak. It needs the GNOME runtime from "
        "Flathub, so add the Flathub remote first:\n\n"
        "```sh\n"
        "flatpak remote-add --if-not-exists --user flathub "
        "https://dl.flathub.org/repo/flathub.flatpakrepo\n"
        f"flatpak install --user ./{bundle}\n"
        f"flatpak run {APP_ID}\n"
        "```\n\n"
        "`SHA256SUMS` lists the checksum of every file in this release."
    )
    return "\n\n".join(blocks) + "\n"


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__.strip())
    try:
        sys.stdout.write(notes(sys.argv[1]))
    except LookupError as error:
        sys.exit(str(error))
