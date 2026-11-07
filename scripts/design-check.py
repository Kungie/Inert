#!/usr/bin/env python3
"""Checks rendered browser screenshots against the Yön A (Stencil) design file.

The design file's tokens (Claude Design, "Yön A"): background #000000, text
#EDEDED, headings #FFFFFF, muted #9A9A9A / #8A8A8A, borders #222222 /
#3A3A3A, accents #B48CFF / #7F9CFF / #B9C6FF.

For each screenshot it reports how much of the browser's own interface is
pure black, which design tokens appear as solid pixels, and how many solid
pixels fall outside the palette (anti-aliasing excluded: only colors that
cover at least 0.05% of the image count as "solid"). Areas showing web
content (Servo/WebKit pages) are excluded with --chrome-only rectangles.

Standard library only. Usage: scripts/design-check.py <png>...
"""

import json
import struct
import sys
import zlib
from collections import Counter

TOKENS = {
    "#000000": "background",
    "#EDEDED": "text",
    "#FFFFFF": "heading",
    "#9A9A9A": "muted",
    "#8A8A8A": "dim",
    "#222222": "line",
    "#3A3A3A": "line strong",
    "#B48CFF": "accent purple",
    "#7F9CFF": "edge blue",
    "#B9C6FF": "secure blue",
}
# Colors the browser adds on top of the design file: status colors and three
# near-black elevation shades for panels, raised controls and hover
# (theme.rs PANEL, RAISED, HOVER; the design file's own divider is #1A1A1A).
STATUS = {"#FFB547": "warn", "#FF7A7A": "danger", "#5DCAA5": "ok",
          "#080808": "panel", "#101010": "raised", "#181818": "hover", "#191919": "hover", "#1A1A1A": "divider"}


def read_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, w = 8, b"", 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos : pos + 8])
        body = data[pos + 8 : pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, ctype, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert depth == 8 and ctype in (2, 6) and interlace == 0, "only 8-bit RGB/RGBA"
            bpp = 4 if ctype == 6 else 3
        elif kind == b"IDAT":
            idat += body
        pos += 12 + n
    raw = zlib.decompress(idat)
    stride = w * bpp
    rows, prev, i = [], bytearray(stride), 0
    for _ in range(h):
        f = raw[i]
        line = bytearray(raw[i + 1 : i + 1 + stride])
        i += 1 + stride
        for x in range(stride):
            a = line[x - bpp] if x >= bpp else 0
            b = prev[x]
            c = prev[x - bpp] if x >= bpp else 0
            if f == 1:
                line[x] = (line[x] + a) & 255
            elif f == 2:
                line[x] = (line[x] + b) & 255
            elif f == 3:
                line[x] = (line[x] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[x] = (line[x] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(bytes(line))
        prev = line
    return w, h, bpp, rows


def check(path, chrome_only=None):
    w, h, bpp, rows = read_png(path)
    counts = Counter()
    for y, row in enumerate(rows):
        if chrome_only is not None and y >= chrome_only:
            break
        for x in range(0, w * bpp, bpp):
            counts["#%02X%02X%02X" % (row[x], row[x + 1], row[x + 2])] += 1
    total = sum(counts.values())
    solid = {c: n for c, n in counts.items() if n >= total * 0.0005}
    on = {c: n for c, n in solid.items() if c in TOKENS or c in STATUS}
    off = {c: n for c, n in solid.items() if c not in TOKENS and c not in STATUS}
    return {
        "file": path.split("/")[-1],
        "pixels": total,
        "black_pct": round(100 * counts["#000000"] / total, 1),
        "tokens_seen": sorted(TOKENS[c] for c in on if c in TOKENS),
        "solid_on_palette_pct": round(100 * sum(on.values()) / max(1, sum(solid.values())), 1),
        "solid_off_palette": sorted(off, key=lambda c: -off[c])[:5],
    }


if __name__ == "__main__":
    args = sys.argv[1:]
    out = []
    for a in args:
        # "file.png:rows" limits the check to the first N rows (browser chrome
        # above a web page whose colors are the site's, not ours).
        path, _, limit = a.partition(":")
        out.append(check(path, int(limit) if limit else None))
    print(json.dumps(out, indent=1))
