# Get Games' fixture catalogue

A cut of Flathub's real AppStream catalogue for Get Games' tests (#884, GG-1; `docs/get-games.md`, section 6). The
folder is laid out as a Flatpak installation: `appstream/flathub/x86_64/active/` holds `appstream.xml` and
`icons/64x64/`, as `/var/lib/flatpak/` and `~/.local/share/flatpak/` do.

## How it was cut

From the system installation's catalogue on 2026-11-26 (Flathub's `appstream2` branch as Flatpak 1.18.4 deployed it the
day before: 4,864 components, 52 MB), with `cut.py` in this folder:

```sh
python3 cut.py /var/lib/flatpak/appstream/flathub/x86_64/active \
  appstream/flathub/x86_64/active
```

It keeps the 31 components it lists, in the catalogue's order and byte for byte, except that translated elements
(`xml:lang`) are dropped from all but SuperTuxKart and Firefox, which keep theirs so a parser meets them. Of the icons it
keeps only the 64x64 cached icon of each kept component (29 of them: Steam's screensaver add-on and the runtime have
none); the 128x128 icons and every other app's icon are gone. Flatpak deploys an uncompressed `appstream.xml` beside
`appstream.xml.gz`; only the uncompressed file is here.

## What each component is for

| Rule (`docs/get-games.md`, 6.3) | Components |
| --- | --- |
| A game: `Game` category, an application | `net.supertuxkart.SuperTuxKart`, `org.srb2.SRB2`, `org.supertuxproject.SuperTux`, `org.taisei_project.Taisei` (Pindu OS's picks), `org.libretro.RetroArch`, `net.wz2100.wz2100`, `org.openttd.OpenTTD`, `org.zdoom.UZDoom`, `org.gnome.Chess`, `org.gnome.Aisleriot`, `org.kde.kiriki`, `com.github.tmewett.BrogueCE`, `com.tuxpusher.TuxPusherAF`, `com.frogatto.Frogatto`, `org.sonic3air.Sonic3AIR`, `net.clonehero.CloneHero` |
| Additional categories become shelves | `ArcadeGame`, `ActionGame`, `StrategyGame`, `Simulation`, `Shooter`, `BoardGame`, `CardGame`, `Emulator` among the games above; BrogueCE and SRB2 have no additional category |
| The three stores, on the Launchers shelf (7.3) | `com.valvesoftware.Steam`, `com.heroicgameslauncher.hgl`, `net.lutris.Lutris` (`Game`, `PackageManager`) |
| Not a game though it has `Game` | `com.steamgriddb.SGDBoop`, `io.github.benjamimgois.goverlay` (Pindu OS's blocklist), `net.davidotek.pupgui2` (on GG-1's proposed blocklist) |
| A game category on an app that streams games | `com.moonlight_stream.Moonlight` (`Game`, `Network`, `RemoteAccess`) |
| An application in the old type name `desktop` | `org.phoenicis.playonlinux` (`Game`, `Utility`), `org.mozilla.firefox` (an extra) |
| `Game` on a component that is not an application | `page.codeberg.Qwel.egui_picross` (`generic`), `com.valvesoftware.Steam.Utility.InhibitScreensaver` and `io.github.theforceengine.tfe.Debug` (`addon`) |
| A runtime | `org.freedesktop.Platform` |
| Extras: not games, offered by the lists file | `tv.kodi.Kodi`, `org.mozilla.firefox` |
| Neither a game nor an extra | `org.gnome.Calculator` |

Facts the app page shows (6.7), as they occur here:

- **Verification:** `<custom><value key="flathub::verification::verified">true</value>` on verified apps; SuperTux,
  OpenTTD, Aisleriot, Frogatto, Sonic 3 A.I.R., Clone Hero, Steam, PlayOnLinux and the picross game have none.
- **Proprietary licence:** `LicenseRef-proprietary` (Steam, Clone Hero), with or without `=<URL>`.
- **Age rating:** `<content_rating type="oars-1.1">` with `<content_attribute>` children, or empty (no violence, no
  in-app purchases and so on); `oars-1.0` on older entries.
- **Entities:** `&amp;` in Frogatto's name and Sonic 3 A.I.R.'s summary.
- **Missing parts:** neither add-on has screenshots or a `content_rating`; Steam's screensaver add-on has no icon.

The catalogue carries no end-of-life state. Flathub keeps it in the repository summary, which
`flatpak remote-ls --app --all --columns=application,options` prints (`../flatpak/remote-ls-eol.stdout`); none of the
804 components with the `Game` category was end-of-life on the day of the cut.
