#!/usr/bin/env python3
"""Draw the addon's guide arrow (addon/Factoruide/Media/Arrow.tga): a light grey arrow pointing
up, shaded in relief (a lit left face and a darker right face along its ridge, rounded edges, a
dark outline and a soft drop shadow), on a transparent 256x256 canvas. The addon tints it (green
ahead, red behind) and turns it. `--preview out.png` also writes it tinted green on grey."""

import math
import struct
import sys
import zlib

SIZE = 256
SAMPLES = 3  # supersampling per axis, for smooth edges

# Arrow outline, in a 0..1 square, pointing up: head, shaft, notched tail.
SHAPE = [
    (0.50, 0.05),
    (0.92, 0.54),
    (0.65, 0.54),
    (0.65, 0.93),
    (0.50, 0.82),
    (0.35, 0.93),
    (0.35, 0.54),
    (0.08, 0.54),
]
OUTLINE = 0.012  # dark border width (fraction of the canvas)
BEVEL = 0.06  # width of the rounded edge
SHADOW = (0.02, 0.03)  # drop shadow offset (right, down)
SHADOW_BLUR = 0.035
SHADOW_ALPHA = 0.5


def inside(x, y, poly):
    hit = False
    j = len(poly) - 1
    for i, (xi, yi) in enumerate(poly):
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            hit = not hit
        j = i
    return hit


def edge_distance(x, y, poly):
    best = math.inf
    for i, (x1, y1) in enumerate(poly):
        x2, y2 = poly[(i + 1) % len(poly)]
        dx, dy = x2 - x1, y2 - y1
        t = max(0.0, min(1.0, ((x - x1) * dx + (y - y1) * dy) / (dx * dx + dy * dy)))
        best = min(best, math.hypot(x - (x1 + t * dx), y - (y1 + t * dy)))
    return best


def shade(x, y):
    """(light 0..1, alpha 0..1) of a point of the arrow, or of its shadow."""
    if inside(x, y, SHAPE):
        d = edge_distance(x, y, SHAPE)
        if d < OUTLINE:
            return 0.12, 1.0
        # Two faces along the ridge (x = 0.5): light from the upper left.
        side = max(-1.0, min(1.0, (x - 0.5) / 0.012))
        light = 0.86 - 0.2 * side
        # Brighter towards the tip.
        light *= 1.08 - 0.22 * y
        # Rounded edge: darker as it turns away, a highlight just inside on the lit side.
        r = min((d - OUTLINE) / BEVEL, 1.0)
        light *= 0.62 + 0.38 * math.sin(r * math.pi / 2)
        return min(light, 1.0), 1.0
    sx, sy = x - SHADOW[0], y - SHADOW[1]
    if inside(sx, sy, SHAPE):
        return 0.0, SHADOW_ALPHA
    d = edge_distance(sx, sy, SHAPE)
    if d < SHADOW_BLUR:
        return 0.0, SHADOW_ALPHA * (1 - d / SHADOW_BLUR) ** 2
    return 0.0, 0.0


def render():
    pixels = []
    for py in range(SIZE):
        row = []
        for px in range(SIZE):
            lum = alpha = 0.0
            for sy in range(SAMPLES):
                for sx in range(SAMPLES):
                    x = (px + (sx + 0.5) / SAMPLES) / SIZE
                    y = (py + (sy + 0.5) / SAMPLES) / SIZE
                    light, a = shade(x, y)
                    lum += light * a
                    alpha += a
            n = SAMPLES * SAMPLES
            grey = lum / alpha if alpha else 0.0
            row.append((int(round(255 * grey)), int(round(255 * alpha / n))))
        pixels.append(row)
    return pixels


def write_tga(out, pixels):
    data = bytearray()
    for row in pixels:
        for grey, alpha in row:
            data += bytes((grey, grey, grey, alpha))  # BGRA
    # Uncompressed true color, 32 bits, origin at the top left.
    header = struct.pack("<BBBHHBHHHHBB", 0, 0, 2, 0, 0, 0, 0, 0, SIZE, SIZE, 32, 0x28)
    with open(out, "wb") as f:
        f.write(header + data)


def write_preview(out, pixels, tint=(0.1, 1.0, 0.1), back=(90, 70, 50)):
    raw = bytearray()
    for row in pixels:
        raw.append(0)
        for grey, alpha in row:
            a = alpha / 255
            for c, b in zip(tint, back):
                raw.append(int(round(grey * c * a + b * (1 - a))))

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 2, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw))) + chunk(b"IEND", b"")
    with open(out, "wb") as f:
        f.write(png)


if __name__ == "__main__":
    args = sys.argv[1:]
    preview = None
    if "--preview" in args:
        i = args.index("--preview")
        preview = args[i + 1]
        del args[i : i + 2]
    image = render()
    write_tga(args[0] if args else "addon/Factoruide/Media/Arrow.tga", image)
    if preview:
        write_preview(preview, image)
