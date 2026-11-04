#!/usr/bin/env python3
"""Writes the small Windows programs the PE metadata tests read.

Run from this folder: `python3 make_fixtures.py`. Each program is a minimal, valid PE
file with a one-byte code section and, when it has any, a resource section:

- game.exe (64-bit): version info with a product name and a description, and an icon
  group with a 16×16 bitmap icon and a 256×256 PNG icon.
- setup.exe (32-bit): version info in German with only a description, and an icon group
  with only the 16×16 bitmap icon.
- plain.exe (64-bit): no resources at all.
"""

import struct
import zlib

RT_ICON, RT_GROUP_ICON, RT_VERSION = 3, 14, 16
FILE_ALIGN, SECTION_ALIGN = 0x200, 0x1000


def align(value, to):
    return (value + to - 1) // to * to


def png(size, rgba):
    """A solid-colour PNG, compressed with zlib."""
    raw = b"".join(b"\0" + bytes(rgba) * size for _ in range(size))

    def chunk(kind, data):
        body = kind + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def dib_icon(size, bgra):
    """A 32-bit bitmap icon as stored in RT_ICON: header, pixels, then the AND mask."""
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, 0, 0, 0, 0, 0)
    pixels = bytes(bgra) * size * size
    mask_row = align((size + 7) // 8, 4)
    return header + pixels + b"\0" * mask_row * size


def group_icon(entries):
    """A GRPICONDIR: (width, height, bit count, bytes, icon ID) per entry."""
    out = struct.pack("<HHH", 0, 1, len(entries))
    for width, height, bits, size, icon_id in entries:
        out += struct.pack(
            "<BBBBHHIH", width % 256, height % 256, 0, 0, 1, bits, size, icon_id
        )
    return out


def utf16(text):
    return text.encode("utf-16-le") + b"\0\0"


def block(key, value=b"", text=False, children=()):
    """One version-info block: length, value length, type, key, value, children."""
    head_len = 6 + len(utf16(key))
    out = bytearray(struct.pack("<HHH", 0, 0, 1 if text else 0) + utf16(key))
    out += b"\0" * (align(len(out), 4) - len(out))
    out += value
    for child in children:
        out += b"\0" * (align(len(out), 4) - len(out))
        out += child
    value_len = len(value) // 2 if text else len(value)
    struct.pack_into("<HH", out, 0, len(out), value_len)
    assert head_len <= len(out)
    return bytes(out)


def version_info(table, strings):
    fixed = struct.pack(
        "<13I", 0xFEEF04BD, 0x10000, 0x10000, 0, 0x10000, 0, 0x3F, 0, 0x40004, 1, 0, 0, 0
    )
    string_blocks = [block(k, utf16(v), text=True) for k, v in strings]
    lang = int(table[:4], 16)
    codepage = int(table[4:], 16)
    return block(
        "VS_VERSION_INFO",
        fixed,
        children=[
            block("StringFileInfo", text=True, children=[block(table, text=True, children=string_blocks)]),
            block(
                "VarFileInfo",
                text=True,
                children=[block("Translation", struct.pack("<HH", lang, codepage))],
            ),
        ],
    )


def resource_section(resources, rva):
    """The .rsrc section for {type: {id: {language: bytes}}}: directories, then data
    entries, then data."""
    types = sorted(resources)

    def directory(count):
        return struct.pack("<IIHHHH", 0, 0, 0, 0, 0, count)

    # Sizes first, to know every offset.
    dirs_size = 16 + 8 * len(types)
    for t in types:
        dirs_size += 16 + 8 * len(resources[t])
        for name in resources[t]:
            dirs_size += 16 + 8 * len(resources[t][name])
    leaves = [(t, n, l) for t in types for n in sorted(resources[t]) for l in sorted(resources[t][n])]
    entries_offset = dirs_size
    data_offset = align(entries_offset + 16 * len(leaves), 8)

    out = bytearray()
    next_dir = 16 + 8 * len(types)
    leaf_index = 0
    data_blobs = bytearray()
    level2, level3 = bytearray(), bytearray()
    # Level 1: types.
    out += directory(len(types))
    type_dirs = []
    for t in types:
        type_dirs.append(next_dir)
        out += struct.pack("<II", t, 0x80000000 | next_dir)
        next_dir += 16 + 8 * len(resources[t])
    # Level 2: names, then level 3: languages.
    name_dirs_start = next_dir
    for t in types:
        level2 += directory(len(resources[t]))
        for n in sorted(resources[t]):
            level2 += struct.pack("<II", n, 0x80000000 | next_dir)
            next_dir += 16 + 8 * len(resources[t][n])
    for t in types:
        for n in sorted(resources[t]):
            level3 += directory(len(resources[t][n]))
            for l in sorted(resources[t][n]):
                level3 += struct.pack("<II", l, entries_offset + 16 * leaf_index)
                leaf_index += 1
    assert len(out) + len(level2) == name_dirs_start
    out += level2 + level3
    assert len(out) == entries_offset
    for t, n, l in leaves:
        data = resources[t][n][l]
        offset = data_offset + len(data_blobs)
        out += struct.pack("<IIII", rva + offset, len(data), 0, 0)
        data_blobs += data + b"\0" * (align(len(data), 8) - len(data))
    out += b"\0" * (data_offset - len(out))
    out += data_blobs
    return bytes(out)


def pe(path, wide, resources):
    sections = [(b".text", b"\xc3")]
    if resources:
        sections.append((b".rsrc", None))
    optional_size = (112 if wide else 96) + 16 * 8
    headers_size = align(0x40 + 4 + 20 + optional_size + 40 * len(sections), FILE_ALIGN)

    rva = SECTION_ALIGN
    file_offset = headers_size
    table, bodies, rsrc = [], [], (0, 0)
    for name, body in sections:
        if body is None:
            body = resource_section(resources, rva)
            rsrc = (rva, len(body))
        raw_size = align(len(body), FILE_ALIGN)
        flags = 0x60000020 if name == b".text" else 0x40000040
        table.append(struct.pack("<8sIIIIIIHHI", name, len(body), rva, raw_size, file_offset, 0, 0, 0, 0, flags))
        bodies.append(body + b"\0" * (raw_size - len(body)))
        rva += align(len(body), SECTION_ALIGN)
        file_offset += raw_size

    dos = bytearray(0x40)
    dos[0:2] = b"MZ"
    struct.pack_into("<I", dos, 0x3C, 0x40)
    machine = 0x8664 if wide else 0x14C
    characteristics = 0x22 if wide else 0x102
    coff = struct.pack("<HHIIIHH", machine, len(sections), 0, 0, 0, optional_size, characteristics)
    if wide:
        optional = struct.pack(
            "<HBBIIIIIQIIHHHHHHIIIIHHQQQQII",
            0x20B, 14, 0, 0x200, 0x200, 0, SECTION_ALIGN, SECTION_ALIGN, 0x140000000,
            SECTION_ALIGN, FILE_ALIGN, 6, 0, 0, 0, 6, 0, 0, rva, headers_size, 0, 2, 0x8160,
            0x100000, 0x1000, 0x100000, 0x1000, 0, 16,
        )
    else:
        optional = struct.pack(
            "<HBBIIIIIIIIIHHHHHHIIIIHHIIIIII",
            0x10B, 14, 0, 0x200, 0x200, 0, SECTION_ALIGN, SECTION_ALIGN, SECTION_ALIGN * 2,
            0x400000, SECTION_ALIGN, FILE_ALIGN, 6, 0, 0, 0, 6, 0, 0, rva, headers_size, 0, 2,
            0x8140, 0x100000, 0x1000, 0x100000, 0x1000, 0, 16,
        )
    data_dirs = bytearray(16 * 8)
    struct.pack_into("<II", data_dirs, 2 * 8, *rsrc)
    header = bytes(dos) + b"PE\0\0" + coff + optional + bytes(data_dirs) + b"".join(table)
    assert len(optional) + len(data_dirs) == optional_size
    with open(path, "wb") as out:
        out.write(header + b"\0" * (headers_size - len(header)) + b"".join(bodies))


small = dib_icon(16, (0x33, 0x66, 0x99, 0xFF))
large = png(256, (0x99, 0x66, 0x33, 0xFF))
english, german = 0x409, 0x407

pe(
    "game.exe",
    True,
    {
        RT_ICON: {1: {english: small}, 2: {english: large}},
        RT_GROUP_ICON: {1: {english: group_icon([(16, 16, 32, len(small), 1), (256, 256, 32, len(large), 2)])}},
        RT_VERSION: {
            1: {
                english: version_info(
                    "040904b0",
                    [
                        ("CompanyName", "Pindu Test Studio"),
                        ("FileDescription", "Celeste Launcher"),
                        ("ProductName", "Celeste"),
                    ],
                )
            }
        },
    },
)
pe(
    "setup.exe",
    False,
    {
        RT_ICON: {1: {german: small}},
        RT_GROUP_ICON: {7: {german: group_icon([(16, 16, 32, len(small), 1)])}},
        RT_VERSION: {1: {german: version_info("040704b0", [("FileDescription", "Celeste Setup")])}},
    },
)
pe("plain.exe", True, {})
