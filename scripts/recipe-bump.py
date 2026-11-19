#!/usr/bin/env python3
"""Bump the AUR and nixpkgs recipes to a final release, from the release's SHA256SUMS.

The Release workflow's `recipes` job (.github/workflows/release.yml) runs this after a final release is published, then
opens the bump pull request; docs/releasing.md ("Distribution recipes") is the runbook. It sets, in
build-aux/aur/PKGBUILD, `pkgver`, `pkgrel=1` and `sha256sums` to the tarball's SHA-256, and in
build-aux/nix/package.nix `version` and the `src` hash, the same SHA-256 in SRI form. Nothing else changes; the AUR's
.SRCINFO is generated on Arch when the AUR repository is updated.

Usage:
    python3 -I scripts/recipe-bump.py [--root DIR] [--body FILE [--issue N] [--repo-url URL] [--run-url URL]]
        VERSION SHA256SUMS

VERSION is a final version, X.Y.Z without `v`: pre-releases never reach these recipes. SHA256SUMS is the release's
checksum file, `sha256sum` output that lists pindu-launcher-VERSION.tar.xz. --root is the repository (default: the one
holding this script). When the recipes change, --body writes the bump pull request's body to FILE, with `Refs #N` for
the "Release X.Y.Z" issue --issue names. Prints `bumped to VERSION` or, when the recipes already build it, `already at
VERSION` (and writes no body), and exits 0; exits 2, writing no file, on a usage error, a missing or malformed
checksum, or a recipe line it cannot find.
"""

import argparse
import base64
import re
import sys
from collections.abc import Sequence
from pathlib import Path

PKGBUILD = Path("build-aux/aur/PKGBUILD")
NIX = Path("build-aux/nix/package.nix")
FINAL = re.compile(r"^[0-9]+\.[0-9]+\.[0-9]+$")
DIGEST = re.compile(r"^[0-9a-f]{64}$")


class BumpError(Exception):
    """Why the recipes could not be bumped."""


def tarball_digest(sums: str, version: str) -> str:
    """The SHA-256 that `sha256sum` output lists for the release tarball of `version`."""
    tarball = f"pindu-launcher-{version}.tar.xz"
    for line in sums.splitlines():
        digest, _, name = line.partition(" ")
        # `sha256sum` marks a file read in binary mode with `*` before its name.
        if name.removeprefix(" ").removeprefix("*") == tarball:
            if not DIGEST.match(digest):
                raise BumpError(f"SHA256SUMS lists {tarball} with a malformed checksum '{digest}'")
            return digest
    raise BumpError(f"SHA256SUMS does not list {tarball}")


def sri(digest: str) -> str:
    """A hex SHA-256 in nixpkgs' SRI form, `sha256-` and the base64 of its bytes."""
    return "sha256-" + base64.b64encode(bytes.fromhex(digest)).decode()


def replace_line(text: str, name: Path, pattern: str, line: str) -> str:
    """`text` with its one line matching `pattern` replaced by `line`."""
    new, count = re.subn(pattern, line.replace("\\", "\\\\"), text, flags=re.MULTILINE)
    if count != 1:
        raise BumpError(f"{name}: expected one line matching {pattern}, found {count}")
    return new


def bump_pkgbuild(text: str, version: str, digest: str) -> str:
    """The PKGBUILD building `version` from a tarball with SHA-256 `digest`."""
    text = replace_line(text, PKGBUILD, r"^pkgver=.*$", f"pkgver={version}")
    text = replace_line(text, PKGBUILD, r"^pkgrel=.*$", "pkgrel=1")
    return replace_line(text, PKGBUILD, r"^sha256sums=\(.*\)$", f"sha256sums=('{digest}')")


def bump_nix(text: str, version: str, digest: str) -> str:
    """The nixpkgs recipe building `version` from a tarball with SHA-256 `digest`."""
    text = replace_line(text, NIX, r'^  version = ".*";$', f'  version = "{version}";')
    return replace_line(text, NIX, r'^    hash = "sha256-.*";$', f'    hash = "{sri(digest)}";')


def pr_body(version: str, digest: str, issue: int | None, repo_url: str, run_url: str) -> str:
    """The bump pull request's body, in the shape of prompts/pull-request.md."""
    reference = f"Refs #{issue}\n\n" if issue is not None else ""
    return f"""{reference}## Summary

- **What:** the AUR and nixpkgs recipes build {version}.
- **Why:** `docs/releasing.md` bumps them once the final release is published, from its `SHA256SUMS`.

- `build-aux/aur/PKGBUILD`: `pkgver={version}`, `pkgrel=1`; `sha256sums` is the tarball's SHA-256
- `build-aux/nix/package.nix`: `version = "{version}"`; `hash` is the same SHA-256 in SRI form

Tarball SHA-256, from {repo_url}/releases/download/v{version}/SHA256SUMS: `{digest}`

## Testing

Opened by the Release workflow ({run_url}) with `scripts/recipe-bump.py`, which changes only these lines. A pull request
opened with `GITHUB_TOKEN` starts no workflow: close and reopen it to run the checks. Try the recipes as
`docs/releasing.md` says; `.SRCINFO` is generated on Arch when the AUR repository is updated.
"""


def main(argv: Sequence[str] | None = None) -> int:
    """Bump the recipes; the exit status of the module docstring."""
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--body", type=Path, help="write the bump pull request's body here when the recipes change")
    parser.add_argument("--issue", type=int, help='the "Release X.Y.Z" issue the body refers to')
    parser.add_argument("--repo-url", default="https://github.com/Tim-van-Oudheusden/pindu-launcher")
    parser.add_argument("--run-url", default="", help="the workflow run that opens the pull request")
    parser.add_argument("version")
    parser.add_argument("sums", type=Path)
    args = parser.parse_args(argv)
    try:
        if not FINAL.match(args.version):
            raise BumpError(f"'{args.version}' is not a final release version X.Y.Z; pre-releases are never bumped")
        digest = tarball_digest(args.sums.read_text(), args.version)
        recipes = {PKGBUILD: bump_pkgbuild, NIX: bump_nix}
        old = {path: (args.root / path).read_text() for path in recipes}
        # Both recipes are bumped in memory first, so a failure writes neither.
        new = {path: bump(old[path], args.version, digest) for path, bump in recipes.items()}
    except (BumpError, OSError) as error:
        print(f"recipe-bump: {error}", file=sys.stderr)
        return 2
    if new == old:
        print(f"The AUR and nixpkgs recipes are already at {args.version}.")
        return 0
    for path, text in new.items():
        (args.root / path).write_text(text)
    if args.body is not None:
        args.body.write_text(pr_body(args.version, digest, args.issue, args.repo_url, args.run_url))
    print(f"The AUR and nixpkgs recipes are bumped to {args.version} ({digest}).")
    return 0


if __name__ == "__main__":
    sys.exit(main())
