#!/usr/bin/env python3
"""Generate the extension's PNG icons (16/32/48/128) with no third-party deps.

Pure-Python PNG encoder + a tiny supersampled rasterizer. The mark is a
niutero-green rounded square with a white reference-bracket pair `[ . ]`. Run
from anywhere; writes into ./icons next to this file.
"""
import os
import struct
import zlib

OUT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "icons")
GREEN = (0x1F, 0x8A, 0x5B)
WHITE = (0xFF, 0xFF, 0xFF)
SIZES = (16, 32, 48, 128)
SS = 4  # supersampling factor per axis


def rounded_rect(x, y, r):
    """Inside the full-canvas rounded rect (unit square, corner radius r)?"""
    if x < r and y < r:
        return (x - r) ** 2 + (y - r) ** 2 <= r * r
    if x > 1 - r and y < r:
        return (x - (1 - r)) ** 2 + (y - r) ** 2 <= r * r
    if x < r and y > 1 - r:
        return (x - r) ** 2 + (y - (1 - r)) ** 2 <= r * r
    if x > 1 - r and y > 1 - r:
        return (x - (1 - r)) ** 2 + (y - (1 - r)) ** 2 <= r * r
    return 0.0 <= x <= 1.0 and 0.0 <= y <= 1.0


def in_rect(x, y, x0, x1, y0, y1):
    return x0 <= x <= x1 and y0 <= y <= y1


def glyph(x, y):
    """White reference-brackets `[ . ]` over the green field."""
    w = 0.085
    # left bracket: stem + top/bottom serifs
    if in_rect(x, y, 0.26, 0.26 + w, 0.28, 0.72):
        return True
    if in_rect(x, y, 0.26, 0.43, 0.28, 0.28 + w):
        return True
    if in_rect(x, y, 0.26, 0.43, 0.72 - w, 0.72):
        return True
    # right bracket (mirrored)
    if in_rect(x, y, 0.74 - w, 0.74, 0.28, 0.72):
        return True
    if in_rect(x, y, 0.57, 0.74, 0.28, 0.28 + w):
        return True
    if in_rect(x, y, 0.57, 0.74, 0.72 - w, 0.72):
        return True
    # centre dot
    if (x - 0.5) ** 2 + (y - 0.5) ** 2 <= 0.052 ** 2:
        return True
    return False


def render(size):
    px = bytearray(size * size * 4)
    n = SS * SS
    for oy in range(size):
        for ox in range(size):
            ar = ag = ab = cover = 0.0
            for sy in range(SS):
                for sx in range(SS):
                    x = (ox + (sx + 0.5) / SS) / size
                    y = (oy + (sy + 0.5) / SS) / size
                    if not rounded_rect(x, y, 0.22):
                        continue
                    cover += 1
                    r, g, b = WHITE if glyph(x, y) else GREEN
                    ar += r
                    ag += g
                    ab += b
            i = (oy * size + ox) * 4
            if cover > 0:
                px[i] = round(ar / cover)
                px[i + 1] = round(ag / cover)
                px[i + 2] = round(ab / cover)
            px[i + 3] = round(cover / n * 255)
    return png(size, size, px)


def png(w, h, rgba):
    def chunk(typ, data):
        return (
            struct.pack(">I", len(data))
            + typ
            + data
            + struct.pack(">I", zlib.crc32(typ + data) & 0xFFFFFFFF)
        )

    raw = bytearray()
    for y in range(h):
        raw.append(0)  # filter: none
        raw += rgba[y * w * 4 : (y + 1) * w * 4]
    sig = b"\x89PNG\r\n\x1a\n"
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0)  # 8-bit RGBA
    return (
        sig
        + chunk(b"IHDR", ihdr)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def main():
    os.makedirs(OUT, exist_ok=True)
    for s in SIZES:
        path = os.path.join(OUT, f"icon-{s}.png")
        with open(path, "wb") as f:
            f.write(render(s))
        print("wrote", path)


if __name__ == "__main__":
    main()
