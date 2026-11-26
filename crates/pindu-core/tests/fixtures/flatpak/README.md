# Captured flatpak output

Real output of the host's `flatpak` for Get Games' fake backend to replay (#884, GG-1; `docs/get-games.md`, section 7).
Each case is three files: `CASE.stdout`, `CASE.stderr` and `CASE.status` (the exit status). Captured on 2026-11-26 with
Flatpak 1.18.4, `LANG=C.UTF-8`, standard input from `/dev/null` and both streams piped, as the launcher runs it; into a
throwaway user installation (`FLATPAK_USER_DIR`), so `--user` stands where Get Games passes `--system` on Pindu OS.
The output does not otherwise differ between the two.

| Case | Command | What it shows |
| --- | --- | --- |
| `install` | `flatpak install --user -y flathub com.voxdsp.Snowboarder` | One operation (its runtime was installed): `Installing…` without a count, then percentages |
| `install-runtime` | `flatpak install --user -y flathub io.github.riftr.intelpy` | A first install: two GL extensions, the runtime's locale, the runtime, then the app, `Installing 1/5…` to `5/5…` |
| `install-cancelled` | the same, `SIGTERM` after 4 s | Ends during `4/5` with status 1 and nothing on standard error; the three operations that finished stay installed |
| `install-unknown` | `flatpak install --user -y flathub com.example.DoesNotExist` | `error: Nothing matches … in remote flathub` |
| `install-offline` | `install` with no network (`unshare -rn`) | `error: Unable to load summary from remote flathub: … Could not connect to server` |
| `install-no-space` | `flatpak install --user -y flathub com.github.tmewett.BrogueCE` on a 12 MB tmpfs | `error: Failed to install …: … min-free-space-size 500MB would be exceeded, at least 1.5 MB requested`, after `0%` |
| `uninstall` | `flatpak uninstall --user --noninteractive -y com.voxdsp.Snowboarder` | `Uninstalling app/…` |
| `uninstall-delete-data` | `flatpak uninstall --user --noninteractive -y --delete-data io.github.riftr.intelpy` | The app, and with it the runtime, its locale and the GL extensions it pulled in, which nothing else used |
| `uninstall-not-installed` | `uninstall` again | `error: No installed refs found for ‘com.voxdsp.Snowboarder’` |
| `remote-info` | `flatpak remote-info --user flathub com.voxdsp.Snowboarder` | `Download Size: 408.7 kB`, `Installed Size: 530.9 kB` and the runtime |
| `remote-info-unknown` | `flatpak remote-info --user flathub com.example.DoesNotExist` | `error: Error searching remote flathub: Can't find ref …` |
| `remote-info-offline` | `remote-info` with no network | `error: Unable to load summary from remote flathub: …` |
| `appstream-refresh` | `flatpak update --appstream --user --noninteractive` | Nothing printed, status 0 |
| `appstream-refresh-offline` | the same with no network | Also nothing printed and status 0: only the catalogue's age tells whether it was refreshed |
| `remote-ls-eol.stdout` | `flatpak remote-ls --system flathub --app --all --columns=application,options`, cut to the fixture catalogue's apps and three end-of-life ones | `eol=` and `eol-rebase=` in the options column; without `--all` end-of-life apps are left out |

`noninteractive/` holds the same installs with `--noninteractive -y`, which Add a Store passes. That mode prints one
`Installing app/…` line as each operation starts and nothing else: no count, no percentage, and its errors start with
`Error:`, capitalised. `install-already` shows `Skipping: … is already installed` on standard error with status 0.

The captures were taken with `-y` and without `--noninteractive` because only that mode reports progress; the
maintainer chose it for Get Games' install on #884.
