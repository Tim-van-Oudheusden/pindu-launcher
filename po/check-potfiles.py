#!/usr/bin/env python3
"""Fails when a file with translatable strings is missing from POTFILES.in, or when
POTFILES.in names a file that does not exist; either way strings would not reach
translators."""
import pathlib
import re
import sys

root = pathlib.Path(sys.argv[1])
listed = {
    line.strip()
    for line in (root / "po" / "POTFILES.in").read_text().splitlines()
    if line.strip() and not line.startswith("#")
}
marker = re.compile(r'\b(?:n?gettext|pgettext)\(|\b_\("')
candidates = [*root.glob("crates/*/src/**/*.rs"), *root.glob("data/ui/*.blp")]
needed = {
    str(path.relative_to(root))
    for path in candidates
    if marker.search(path.read_text())
}
missing = sorted(needed - listed)
gone = sorted(path for path in listed if not (root / path).exists())
for path in missing:
    print(f"POTFILES.in is missing {path}")
for path in gone:
    print(f"POTFILES.in lists {path}, which does not exist")
sys.exit(1 if missing or gone else 0)
