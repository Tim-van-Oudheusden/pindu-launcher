# Security Policy

Pindu launcher reads other launchers' game libraries, keeps a library database and playtime on your computer, starts
games through portals, on the host or with Proton, downloads artwork and Proton builds, and keeps your SteamGridDB API
key in the system keyring. A flaw in it could start the wrong program, leak the key, let a launcher's files or a
download reach past the Flatpak sandbox, or damage another launcher's data, so reports are welcome.

## Supported versions

Fixes go into the next release; older releases are not patched.

| Version | Supported |
| --- | --- |
| Latest [release](https://github.com/Tim-van-Oudheusden/pindu-launcher/releases/latest) (Flatpak bundle and native packages) | Yes |
| `main` | Yes |
| Older releases, CI development bundles | No; update to the latest release |

## Reporting a vulnerability

**Do not open a public issue, pull request or discussion for a vulnerability.**

Report it privately through GitHub's private vulnerability reporting: the repository's **Security** tab → **Report a
vulnerability** ([direct link](https://github.com/Tim-van-Oudheusden/pindu-launcher/security/advisories/new)). Only
the maintainer ([@Tim-van-Oudheusden](https://github.com/Tim-van-Oudheusden)) sees the report. If the form is not
available, contact the maintainer through [GitHub](https://github.com/Tim-van-Oudheusden) without any details and ask
for a private channel.

Include what you can of:

- the version (Main Menu → About Pindu launcher), how you installed it (Flatpak bundle, native package, built from
  source), your distribution and, for Flatpak, `flatpak --version`
- what an attacker can do, and what they need first (a crafted launcher library file, a malicious download, a local
  account, …)
- steps or a proof of concept that reproduce it
- for a leaked secret: where you found it

## What to expect

This is a one-person project, so these are aims, not guarantees:

- an acknowledgement within 7 days
- a first assessment (accepted or not, and the severity) within 14 days
- a fix in a release as soon as practical, coordinated with you, then a published GitHub security advisory that credits
  you unless you prefer otherwise

Please keep the details private until the fix is released or 90 days have passed, whichever comes first.

## Scope

In scope:

- the app (`crates/`, `data/`): the importers and how they parse other launchers' files, launch plans and how games
  are started, the SteamGridDB and Proton downloads, the art cache, backups, custom fullscreen styles, and the
  handling of the SteamGridDB key
- the Flatpak manifests in `build-aux/flatpak/`, including the sandbox permissions they request, and the released
  Flatpak bundles, source tarballs and native packages
- the build and release pipeline in this repository (`.github/workflows/`, `meson.build`, `build-aux/`), including
  leaked secrets

Out of scope:

- flaws in Steam, Heroic, Lutris, Bottles, Faugus, umu-launcher, Proton, SteamGridDB or the games themselves; report
  those to their projects
- flaws in GTK, libadwaita, the GNOME runtime, Flatpak or the portals; report those upstream
- what a host command can do after you grant Pindu launcher `--talk-name=org.freedesktop.Flatpak` yourself (see
  [Starting games on the host](README.md#starting-games-on-the-host)); that access is a sandbox escape by design. A
  way for a launcher's files, a download or a style to choose that command is in scope
- that native packages have no sandbox: they run with your user's rights by design. Other flaws in them are in scope

## Related

- [`docs/security/SECURITY-AI.md`](docs/security/SECURITY-AI.md): how AI coding agents may work in this repository
  safely
