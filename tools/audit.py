#!/usr/bin/env python3
"""Measure renders against reference pages: tone range, edge softness and grain, with
every image reduced to the same size first.

    python3 tools/audit.py --refs refs/*.webp --renders out/*.png

The figures printed for the references are the ones the tool's house tone range
(HOUSE_VAL in twb2.py) and default softness were set from. Scans are uncalibrated, so
these describe pages as their scans show them.
"""
import argparse

import numpy as np
from PIL import Image
from scipy import ndimage as ndi

LONG_EDGE = 900          # every image is compared at this size
TONE_PCT = (2, 10, 25, 50, 75, 90, 98)


def load(path):
    im = Image.open(path).convert("L")
    s = LONG_EDGE / max(im.size)
    a = np.asarray(im.resize((round(im.width * s), round(im.height * s)), Image.LANCZOS), np.float32) / 255
    my, mx = int(a.shape[0] * 0.04), int(a.shape[1] * 0.04)          # stay off page edges
    return a[my:a.shape[0] - my, mx:a.shape[1] - mx]


def edge_widths(a):
    """10-90% widths, in pixels, of clear dark-to-light edges, measured along the gradient
    at the centre line of each edge."""
    g = ndi.gaussian_filter(a, 0.7)
    gy, gx = np.gradient(g)
    mag = np.hypot(gx, gy)
    peak = (mag == ndi.maximum_filter(mag, size=5)) & (mag >= np.percentile(mag, 97))
    t, tt, out = np.arange(-8, 9), np.linspace(-8, 8, 321), []
    for y, x in zip(*np.where(peak)):
        dy, dx = gy[y, x] / mag[y, x], gx[y, x] / mag[y, x]
        yy, xx = y + t * dy, x + t * dx
        if yy.min() < 0 or xx.min() < 0 or yy.max() > a.shape[0] - 1 or xx.max() > a.shape[1] - 1:
            continue
        prof = ndi.map_coordinates(a, [yy, xx], order=1)
        lo, hi = prof[:4].mean(), prof[-4:].mean()
        if hi - lo < 0.35:
            continue
        n = np.interp(tt, t, (prof - lo) / (hi - lo))
        i = j = int(np.argmin(np.abs(tt)))
        while i > 0 and n[i] > 0.1:
            i -= 1
        while j < len(n) - 1 and n[j] < 0.9:
            j += 1
        out.append(tt[j] - tt[i])
    return np.array(out) if out else np.array([np.nan])


def texture(a):
    """Standard deviation of texture in smooth mid-greys, in four bands of scale."""
    g1, g2, g4, g8 = (ndi.gaussian_filter(a, s) for s in (1.0, 2.0, 4.0, 8.0))
    smooth = ndi.gaussian_filter(np.hypot(*np.gradient(g4)), 6) < 0.004
    m = (g4 > 0.30) & (g4 < 0.80) & smooth
    if m.sum() < 3000:
        return [np.nan] * 4
    return [float(x[m].std()) for x in (a - g1, g1 - g2, g2 - g4, g4 - g8)]


def measure(path):
    a = load(path)
    w = edge_widths(a)
    return (list(np.percentile(a, TONE_PCT))
            + [100 * float(((a > 0.25) & (a < 0.75)).mean()), 100 * float((a <= 0.10).mean()), 100 * float((a >= 0.96).mean())]
            + [float(np.nanpercentile(w, 10)), float(np.nanmedian(w))] + texture(a))


COLS = [f"p{q}" for q in TONE_PCT] + ["mids%", "black%", "white%", "edge10", "edgeMed", "tex<1", "tex1-2", "tex2-4", "tex4-8"]


def block(title, paths):
    M = np.array([measure(p) for p in paths])
    print(f"\n{title}: {len(paths)} images")
    print("            " + " ".join(f"{c:>7s}" for c in COLS))
    for name, row in (("median", np.nanmedian(M, 0)), ("quartile 1", np.nanpercentile(M, 25, 0)), ("quartile 3", np.nanpercentile(M, 75, 0))):
        print(f"{name:11s} " + " ".join(f"{v:7.1f}" if 7 <= i <= 9 else (f"{v:7.4f}" if i >= 12 else f"{v:7.2f}") for i, v in enumerate(row)))


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--refs", nargs="+", default=[], help="reference pages")
    ap.add_argument("--renders", nargs="+", default=[], help="renders to compare")
    a = ap.parse_args()
    print(f"All images reduced to {LONG_EDGE} px on the long edge. Tones are display values, 0 to 1.")
    print("mids = share between 0.25 and 0.75; black = at or below 0.10; white = at or above 0.96.")
    print("edge10, edgeMed = 10-90% width in px of the sharpest tenth of strong edges, and their median.")
    print("tex = texture in smooth mid-greys at scales below 1 px, 1-2, 2-4 and 4-8 px.")
    if a.refs:
        block("References", a.refs)
    if a.renders:
        block("Renders", a.renders)
