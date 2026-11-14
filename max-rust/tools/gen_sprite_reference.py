#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Generate sprite/palette fixtures + reference dumps for the Rust tests.

Development aid, not part of the build. Reproduces the reference behaviour of
the C++ oracle (`max-cpp-port/src/gfx.cpp`, `resource_manager.cpp`,
`world.cpp`) for the *pure* parts of `max_assets::sprite` and `max_render`:

  * extracts the raw RES bodies of TANK / S_TANK / COMMTWR from
    `max-original/MAX.RES` into
    `max_assets/tests/data/sprite_<tag>.bin` (the sprite container format is
    described in `max-original-dasm/formats/spw.md`);
  * decodes them with the row-RLE decoder of spw.md §4 and dumps headers of
    every frame plus the full pixel rows of selected frames into
    `max_assets/tests/data/sprite_ref.txt` (hex palette indices, `..` =
    transparent pixel, `#` = darkened pixel for shadow sheets);
  * copies `max-original/COLOR.PAL` to `max_render/tests/data/COLOR.PAL` and
    dumps the parts of it the Rust palette/team-LUT code is checked against
    into `max_render/tests/data/palette_ref.txt`.

Usage (from the repository root):
    python max-rust/tools/gen_sprite_reference.py [max-original/MAX.RES]
"""

import importlib.util
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MAX_RUST = os.path.dirname(HERE)
REPO = os.path.dirname(MAX_RUST)

DEFAULT_RES = os.path.join(REPO, "max-original", "MAX.RES")
DEFAULT_PAL = os.path.join(REPO, "max-original", "COLOR.PAL")
RES_PARSER = os.path.join(REPO, "max-original-dasm", "tools", "res_parser.py")

ASSETS_DATA = os.path.join(MAX_RUST, "crates", "max_assets", "tests", "data")
RENDER_DATA = os.path.join(MAX_RUST, "crates", "max_render", "tests", "data")

# Team colour slots (resource_manager.cpp: ResourceManager_BuildColorTables).
TEAM_COLOR_INDICES = {
    "red": (56, 57, 58, 59, 60, 61, 62, 63),
    "green": (32, 33, 34, 35, 36, 37, 38, 39),
    "blue": (48, 49, 50, 51, 52, 53, 54, 55),
    "gray": (255, 161, 172, 169, 216, 213, 212, 207),
    "derelict": (216, 215, 214, 213, 212, 211, 210, 209),
}


# ---------------------------------------------------------------------------
# RES container: uses the project's reference parser, max-original-dasm/tools/res_parser.py
# ---------------------------------------------------------------------------
def load_res_parser():
    """Imports `res_parser.py` (the reference RES reader) as a module."""
    spec = importlib.util.spec_from_file_location("max_res_parser", RES_PARSER)
    if spec is None or spec.loader is None:  # pragma: no cover - defensive
        raise SystemExit("cannot import reference parser: %s" % RES_PARSER)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def res_bodies(path, tags):
    """tag -> (offset, size, body bytes) using the reference RES parser."""
    parser = load_res_parser()
    out = {}
    with parser.ResArchive(path) as archive:
        for tag in tags:
            if tag not in archive.by_name:
                raise SystemExit("resource %s not found in %s" % (tag, path))
            entry = archive.by_name[tag]
            out[tag] = (entry.offset, entry.size, archive.read(tag))
    return out


# ---------------------------------------------------------------------------
# Sprite container (formats/spw.md §3-§4)
# ---------------------------------------------------------------------------
class SpriteError(Exception):
    pass


def u16(b, o):
    return struct.unpack_from("<H", b, o)[0]


def i16(b, o):
    return struct.unpack_from("<h", b, o)[0]


def u32(b, o):
    return struct.unpack_from("<I", b, o)[0]


def decode_sprite(data, shadow):
    """Return (image_count, [frame dicts]) — canonical decode of spw.md §7.2.

    Each frame: width, height, hotx, hoty, rows (list of (skip, count, pixels)).
    A shadow frame's runs carry no pixel bytes, only the pixel count.
    """
    if len(data) < 2:
        raise SpriteError("sprite shorter than image_count")
    count = u16(data, 0)
    if count == 0:
        raise SpriteError("image_count == 0")
    if len(data) < 2 + 4 * count:
        raise SpriteError("offset table truncated")
    frames = []
    for index in range(count):
        foff = u32(data, 2 + 4 * index)
        if foff + 8 > len(data):
            raise SpriteError("frame %d offset 0x%X out of bounds" % (index, foff))
        width = i16(data, foff)
        height = i16(data, foff + 2)
        hotx = i16(data, foff + 4)
        hoty = i16(data, foff + 6)
        if width <= 0 or height <= 0:
            raise SpriteError("frame %d has non-positive dimensions" % index)
        if foff + 8 + 4 * height > len(data):
            raise SpriteError("frame %d row table truncated" % index)
        row_offsets = [u32(data, foff + 8 + 4 * y) for y in range(height)]
        rows = []
        for y, roff in enumerate(row_offsets):
            if roff >= len(data):
                raise SpriteError("frame %d row %d offset out of bounds" % (index, y))
            pos = roff
            x = 0
            runs = []
            while True:
                if pos >= len(data):
                    raise SpriteError("frame %d row %d unterminated" % (index, y))
                if data[pos] == 0xFF:
                    break
                if pos + 1 >= len(data):
                    raise SpriteError("frame %d row %d run header truncated" % (index, y))
                skip = data[pos]
                cnt = data[pos + 1]
                pos += 2
                x += skip
                if shadow:
                    pixels = []
                else:
                    if pos + cnt > len(data):
                        raise SpriteError("frame %d row %d pixel payload truncated" % (index, y))
                    pixels = list(data[pos:pos + cnt])
                    pos += cnt
                x += cnt
                if x > width:
                    raise SpriteError(
                        "frame %d row %d overruns width (%d > %d)" % (index, y, x, width))
                runs.append((skip, cnt, pixels))
            rows.append(runs)
        frames.append({
            "width": width,
            "height": height,
            "hotx": hotx,
            "hoty": hoty,
            "rows": rows,
        })
    return count, frames


def expand_rows(frame, shadow):
    """Rasterise a frame to `height` lists of tokens ('.' or hex idx, '#' shadow)."""
    width = frame["width"]
    out = []
    for runs in frame["rows"]:
        row = [".."] * width
        x = 0
        for skip, cnt, pixels in runs:
            x += skip
            for i in range(cnt):
                if shadow:
                    row[x + i] = "#"
                else:
                    row[x + i] = "%02x" % pixels[i]
            x += cnt
        out.append(row)
    return out


# ---------------------------------------------------------------------------
# Reference dumps
# ---------------------------------------------------------------------------
SPRITES = [
    # tag, shadow, frames whose rows are dumped in full
    ("TANK", False, (0, 1, 23)),
    ("S_TANK", True, (0, 1, 23)),
    ("COMMTWR", False, (0, 9)),
]


def write_sprite_fixtures():
    os.makedirs(ASSETS_DATA, exist_ok=True)
    bodies = res_bodies(DEFAULT_RES, [tag for tag, _, _ in SPRITES])
    lines = [
        "# M.A.X. sprite reference dump — generated by max-rust/tools/gen_sprite_reference.py",
        "# Source: max-original/MAX.RES (bodies extracted by max-original-dasm/tools/res_parser.py);",
        "# format: max-original-dasm/formats/spw.md",
        "# Pixels: two-digit hex palette index; '..' = transparent; '#' = darkened (shadow).",
        "",
    ]
    for tag, shadow, dump_frames in SPRITES:
        offset, size, blob = bodies[tag]
        path = os.path.join(ASSETS_DATA, "sprite_%s.bin" % tag)
        with open(path, "wb") as fp:
            fp.write(blob)
        count, frames = decode_sprite(blob, shadow)
        lines.append("sprite %s bytes=%d count=%d shadow=%d" % (tag, size, count, int(shadow)))
        for index, frame in enumerate(frames):
            lines.append("frame %d %d %d %d %d" % (
                index, frame["width"], frame["height"], frame["hotx"], frame["hoty"]))
            if index in dump_frames:
                for y, row in enumerate(expand_rows(frame, shadow)):
                    lines.append("row %d %s" % (y, " ".join(row)))
        lines.append("")
        print("  %-8s offset=0x%08X %6d bytes, %3d frames -> %s" % (
            tag, offset, size, count, path))

    ref = os.path.join(ASSETS_DATA, "sprite_ref.txt")
    with open(ref, "w", encoding="utf-8", newline="\n") as fp:
        fp.write("\n".join(lines) + "\n")
    print("  reference dump -> %s (%d lines)" % (ref, len(lines)))


def write_palette_fixtures():
    os.makedirs(RENDER_DATA, exist_ok=True)
    blob = open(DEFAULT_PAL, "rb").read()
    if len(blob) < 768:
        raise SystemExit("COLOR.PAL too short")
    with open(os.path.join(RENDER_DATA, "COLOR.PAL"), "wb") as fp:
        fp.write(blob)
    lines = [
        "# M.A.X. palette reference dump — generated by max-rust/tools/gen_sprite_reference.py",
        "# Source: max-original/COLOR.PAL (256 x RGB, 6 bit per component).",
        "# palette8 = raw * 4 (color.cpp Color_SetSystemPaletteEntry).",
        "",
    ]
    for index in range(256):
        r, g, b = blob[index * 3:index * 3 + 3]
        lines.append("palette %d %d %d %d %d %d %d" % (index, r, g, b, r * 4, g * 4, b * 4))
    lines.append("")
    # Team colour LUT: slots 32..39 and 48..63 replaced by the team's 8 colours,
    # everything else identity (resource_manager.cpp ResourceManager_BuildColorTables).
    for name, colors in TEAM_COLOR_INDICES.items():
        lut = list(range(256))
        j = 0
        k = 32
        while j < 24:
            if j == 8:
                k += 8
            lut[k] = colors[j & 7]
            k += 1
            j += 1
        lines.append("team %s %s" % (name, " ".join("%d" % v for v in lut)))
    lines.append("")
    ref = os.path.join(RENDER_DATA, "palette_ref.txt")
    with open(ref, "w", encoding="utf-8", newline="\n") as fp:
        fp.write("\n".join(lines) + "\n")
    print("  COLOR.PAL -> %s" % os.path.join(RENDER_DATA, "COLOR.PAL"))
    print("  reference dump -> %s (%d lines)" % (ref, len(lines)))


def main(argv):
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.reconfigure(encoding="utf-8", errors="replace")
        except (AttributeError, ValueError):
            pass
    print("sprite fixtures:")
    write_sprite_fixtures()
    print("palette fixtures:")
    write_palette_fixtures()
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
