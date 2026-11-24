#!/usr/bin/env python3
"""Measure how close rendered images are to the reference scans, and build
side-by-side sheets.

    python3 compare.py --refs refs/*  --test out/a.png out/b.png
    python3 compare.py --sheet sheet.png --panels src.jpg out/a.png refs/1.webp --labels source ours reference
"""
import argparse
import os

import numpy as np
from PIL import Image, ImageDraw
from scipy import ndimage as ndi

KEYS = ["p1", "p5", "p50", "p95", "p99", "lo", "hi", "tex_mid", "tex_dark", "tex_lite", "ac1"]
HELP = {
    "p1": "1st percentile (black floor)", "p5": "5th percentile", "p50": "median",
    "p95": "95th percentile", "p99": "99th percentile (white point)",
    "lo": "share of pixels below 10%", "hi": "share of pixels above 90%",
    "tex_mid": "texture strength in flat mid-greys", "tex_dark": "texture strength in blacks",
    "tex_lite": "texture strength in whites", "ac1": "texture coarseness (0 = pixel-fine, 1 = blobby)",
}


def gray(path, height=None):
    im = Image.open(path).convert("L")
    if height:
        im = im.resize((round(im.width * height / im.height), height), Image.LANCZOS)
    return np.asarray(im, dtype=np.float32) / 255


def metrics(a):
    blur = ndi.gaussian_filter(a, 2.0)
    r = a - blur
    grad = ndi.gaussian_gradient_magnitude(a, 3.0)
    flat = grad < np.median(grad)
    mid = (blur > 0.3) & (blur < 0.7) & flat
    dark, lite = blur < 0.12, blur > 0.92

    def sd(m):
        return float(r[m].std()) if m.sum() > 500 else float("nan")

    pair = mid[:, 1:] & mid[:, :-1]
    ac1 = float((r[:, 1:] * r[:, :-1])[pair].mean() / (r[mid] ** 2).mean()) if pair.sum() > 500 else float("nan")
    p = np.percentile(a, [1, 5, 50, 95, 99])
    return dict(p1=p[0], p5=p[1], p50=p[2], p95=p[3], p99=p[4], lo=float((a < 0.1).mean()),
                hi=float((a > 0.9).mean()), tex_mid=sd(mid), tex_dark=sd(dark), tex_lite=sd(lite), ac1=ac1)


def table(refs, tests, height):
    R = [metrics(gray(p, height)) for p in refs]
    print(f"{'':28s}" + "".join(f"{k:>9s}" for k in KEYS))
    for p, m in zip(refs, R):
        print(f"{'ref ' + os.path.basename(p):28s}" + "".join(f"{m[k]:9.3f}" for k in KEYS))
    lo = {k: np.nanmin([m[k] for m in R]) for k in KEYS}
    hi = {k: np.nanmax([m[k] for m in R]) for k in KEYS}
    print(f"{'ref range low':28s}" + "".join(f"{lo[k]:9.3f}" for k in KEYS))
    print(f"{'ref range high':28s}" + "".join(f"{hi[k]:9.3f}" for k in KEYS))
    print()
    for p in tests:
        m = metrics(gray(p, height))
        cells = ""
        for k in KEYS:
            flag = " " if (np.isnan(m[k]) or lo[k] <= m[k] <= hi[k]) else ("v" if m[k] < lo[k] else "^")
            cells += f"{m[k]:8.3f}{flag}"
        print(f"{os.path.basename(p):28s}" + cells)
    print("\n  v = below the reference range, ^ = above it")
    for k in KEYS:
        print(f"  {k:9s} {HELP[k]}")


def best_patch(a, size):
    """Top-left corner of the window richest in flat mid-greys."""
    blur = ndi.gaussian_filter(a, 2.0)
    mid = ((blur > 0.3) & (blur < 0.7)).astype(np.float32)
    score = ndi.uniform_filter(mid, size)
    h, w = a.shape
    half = size // 2
    score[:half] = score[-half:] = 0
    score[:, :half] = score[:, -half:] = 0
    y, x = np.unravel_index(score.argmax(), score.shape)
    return max(0, min(w - size, x - half)), max(0, min(h - size, y - half))


def sheet(out, panels, labels, height, zoom, patch):
    tiles = []
    for path, label in zip(panels, labels):
        im = Image.open(path).convert("RGB")
        im = im.resize((round(im.width * height / im.height), height), Image.LANCZOS)
        g = np.asarray(im.convert("L"), dtype=np.float32) / 255
        x, y = best_patch(g, patch)
        z = im.crop((x, y, x + patch, y + patch)).resize((patch * zoom, patch * zoom), Image.NEAREST)
        w = max(im.width, z.width)
        tile = Image.new("RGB", (w, height + z.height + 46), "white")
        tile.paste(im, ((w - im.width) // 2, 24))
        tile.paste(z, ((w - z.width) // 2, height + 34))
        d = ImageDraw.Draw(tile)
        d.text((4, 6), label, fill="black")
        d.rectangle([(w - im.width) // 2 + x, 24 + y, (w - im.width) // 2 + x + patch, 24 + y + patch], outline=(255, 0, 0))
        tiles.append(tile)
    W = sum(t.width for t in tiles) + 12 * (len(tiles) + 1)
    H = max(t.height for t in tiles) + 24
    canvas = Image.new("RGB", (W, H), "white")
    x = 12
    for t in tiles:
        canvas.paste(t, (x, 12))
        x += t.width + 12
    canvas.save(out)
    print("wrote", out, canvas.size)


def grid(out, panels, labels, cols, width):
    """Plain labelled grid, `cols` images per row, each scaled to `width` px."""
    ims = []
    for p in panels:
        im = Image.open(p).convert("RGB")
        ims.append(im.resize((width, round(im.height * width / im.width)), Image.LANCZOS))
    rows = [ims[i:i + cols] for i in range(0, len(ims), cols)]
    heights = [max(im.height for im in r) + 22 for r in rows]
    canvas = Image.new("RGB", (cols * (width + 8) + 8, sum(heights) + 8), "white")
    d = ImageDraw.Draw(canvas)
    y = 8
    for ri, r in enumerate(rows):
        for ci, im in enumerate(r):
            x = 8 + ci * (width + 8)
            d.text((x, y), labels[ri * cols + ci] if ri * cols + ci < len(labels) else "", fill="black")
            canvas.paste(im, (x, y + 14))
        y += heights[ri]
    canvas.save(out)
    print("wrote", out, canvas.size)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--refs", nargs="*", default=[])
    ap.add_argument("--test", nargs="*", default=[])
    ap.add_argument("--height", type=int, default=900, help="normalise every image to this height first")
    ap.add_argument("--sheet")
    ap.add_argument("--panels", nargs="*", default=[])
    ap.add_argument("--labels", nargs="*", default=[])
    ap.add_argument("--grid", help="write a plain grid of --panels to this file")
    ap.add_argument("--cols", type=int, default=3)
    ap.add_argument("--width", type=int, default=600)
    ap.add_argument("--zoom", type=int, default=4)
    ap.add_argument("--patch", type=int, default=120)
    a = ap.parse_args()
    if a.refs:
        table(a.refs, a.test, a.height)
    if a.grid:
        grid(a.grid, a.panels, a.labels or [os.path.basename(p) for p in a.panels], a.cols, a.width)
    if a.sheet:
        labels = a.labels or [os.path.basename(p) for p in a.panels]
        sheet(a.sheet, a.panels, labels, a.height, a.zoom, a.patch)
