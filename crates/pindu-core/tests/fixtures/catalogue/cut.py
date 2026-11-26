#!/usr/bin/env python3
"""Cut Get Games' fixture catalogue from a real Flathub AppStream catalogue (#884).

Usage: python3 cut.py ACTIVE_DIR OUT_DIR
Keeps the listed components in the catalogue's order, byte for byte except that every
translated element (`xml:lang`) is dropped outside SuperTuxKart and Firefox, which keep theirs
so a parser meets them; and the 64x64 cached icon of each. Every other component and icon size
is dropped.
"""
import os, re, shutil, sys, xml.etree.ElementTree as ET

IDS = """
net.supertuxkart.SuperTuxKart org.srb2.SRB2 org.supertuxproject.SuperTux org.taisei_project.Taisei
com.valvesoftware.Steam com.heroicgameslauncher.hgl net.lutris.Lutris
org.libretro.RetroArch net.wz2100.wz2100 org.openttd.OpenTTD org.zdoom.UZDoom org.gnome.Chess
org.gnome.Aisleriot org.kde.kiriki com.github.tmewett.BrogueCE com.tuxpusher.TuxPusherAF
com.frogatto.Frogatto org.sonic3air.Sonic3AIR net.clonehero.CloneHero com.moonlight_stream.Moonlight
com.steamgriddb.SGDBoop io.github.benjamimgois.goverlay net.davidotek.pupgui2
org.phoenicis.playonlinux page.codeberg.Qwel.egui_picross
com.valvesoftware.Steam.Utility.InhibitScreensaver io.github.theforceengine.tfe.Debug
tv.kodi.Kodi org.mozilla.firefox org.gnome.Calculator org.freedesktop.Platform
""".split()
TRANSLATED = {"net.supertuxkart.SuperTuxKart", "org.mozilla.firefox"}
LANG = re.compile(r'[ \t]*<(\w+) xml:lang="[^"]*"[^>]*>.*?</\1>\n?', re.S)

active, out = sys.argv[1], sys.argv[2]
raw = open(os.path.join(active, "appstream.xml"), encoding="utf-8").read()
header = raw[: raw.index("<component ")]
footer = "</components>\n"
kept, found = [], set()
for m in re.finditer(r"<component[ >].*?</component>", raw, re.S):
    block = m.group(0)
    cid = re.search(r"<id>([^<]+)</id>", block).group(1)
    if cid in IDS and cid not in found:
        if cid not in TRANSLATED:
            block = LANG.sub("", block)
        kept.append("  " + block)
        found.add(cid)
missing = set(IDS) - found
if missing:
    sys.exit(f"not in the catalogue: {sorted(missing)}")
os.makedirs(os.path.join(out, "icons", "64x64"), exist_ok=True)
text = header.rstrip() + "\n" + "\n".join(kept) + "\n" + footer
open(os.path.join(out, "appstream.xml"), "w", encoding="utf-8").write(text)
ET.fromstring(text.encode())  # still well-formed
for block in kept:
    for icon in re.findall(r'<icon[^>]*type="cached"[^>]*width="64"[^>]*>([^<]+)</icon>', block):
        src = os.path.join(active, "icons", "64x64", icon)
        if os.path.exists(src):
            shutil.copyfile(src, os.path.join(out, "icons", "64x64", icon))
print(len(kept), "components,", len(os.listdir(os.path.join(out, "icons", "64x64"))), "icons")
