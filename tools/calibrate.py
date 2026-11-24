#!/usr/bin/env python3
"""Checks that the simulated negative reproduces the datasheet numbers it was built from,
and that results do not depend on pixel count.

    python3 tools/calibrate.py
"""
import json
import os
import sys

import numpy as np
from PIL import Image
from scipy import ndimage as ndi

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
import twb2

ok = True


def check(name, got, want, tol):
    global ok
    good = abs(got - want) <= tol
    ok &= good
    print(f"  {'PASS' if good else 'FAIL'}  {name}: got {got:.4f}, want {want:.4f} +/- {tol}")


# 1. Granularity: a uniform negative at net density 1.0, read through a square aperture
#    with the same area as the 48 um circle the catalogue figure is quoted for
print("1. granularity at net density 1.0 through a 48 um aperture's area (datasheet: 17)")
side = np.sqrt(twb2.APERTURE_UM2)
for k in (20, 10, 5):
    um = side / k
    D = twb2.add_grain(np.full((k * 300, k * 300), 1.0, np.float32), um, 1.0, np.random.default_rng(0))
    read = D.reshape(300, k, 300, k).mean(axis=(1, 3))
    check(f"pixel {um:.2f} um: mean density", float(D.mean()), 1.0, 0.01)
    check(f"pixel {um:.2f} um: rms granularity", float(read.std() * 1000), 17.0, 0.4)
D = twb2.add_grain(np.full((2000, 2000), 1.0, np.float32), 4.2, 1.0, np.random.default_rng(0))
z = (D - D.mean()) / D.std()
print(f"   at 4.2 um pixels (48 MP on 35mm): per-pixel sd {D.std():.3f} density, skew {float((z**3).mean()):+.2f}")

# 2. Sharpness: sinusoidal exposures through emulsion blur and development
print("2. sharpness of the developed negative against the datasheet curve")
um = 1.5
p = twb2.parser().parse_args(["x", "-o", "x", "--grain", "0"])
cx, cy = twb2.film_curve(p.dev)
base = cy.min()
x = np.arange(4096) * um / 1000.0                                  # mm
worst = 0.0
for f, want in twb2.TRIX["mtf_cycles_per_mm"]:
    logH = twb2.LOGH_GREY + 0.10 * np.sin(2 * np.pi * f * x)       # small modulation about a metered grey
    E = 0.18 * 10 ** (logH - twb2.LOGH_GREY)
    rgb = np.repeat(np.repeat(E[None, :, None], 64, 0), 3, 2).astype(np.float32)
    D, _ = twb2.expose_and_develop(rgb, p, um, np.random.default_rng(0))
    row = D[32, 400:-400] + base
    back = np.interp(row, cy, cx)                                  # density -> effective log exposure
    xs = x[400:-400]
    amp = 2 * np.abs(np.mean((back - back.mean()) * np.exp(-2j * np.pi * f * xs)))
    got = amp / 0.10
    worst = max(worst, abs(got - want))
    print(f"   {f:5.1f} c/mm  datasheet {want:.3f}  simulated {got:.3f}")
check("largest deviation from the datasheet curve", worst, 0.0, 0.05)

# 3. Tone: the film and paper curves pass through the digitised points
print("3. curves")
pts = twb2.TRIX["curves_logH_density"]["7"]
cx, cy = twb2.film_curve(7.0)
check("film curve at 7 min, worst point", max(abs(np.interp(a, cx, cy) - b) for a, b in pts), 0.0, 0.01)
pts = twb2.POLYMAX["curves_logH_density"]["3"]
px, py = twb2.paper_curve(3.0)
check("paper curve at grade 3, worst point", max(abs(np.interp(a, px, py) - b) for a, b in pts), 0.0, 0.02)

# 4. The same picture at two pixel counts
print("4. resolution independence (press on, the same picture at 1000 px and at 3000 px reduced to 1000)")
photos = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "photos")
found = sorted(f for f in os.listdir(photos) if f.lower().endswith((".jpg", ".jpeg", ".png", ".tif", ".tiff"))) if os.path.isdir(photos) else []
if found:
    src = os.path.join(photos, found[0])
    load = lambda work: twb2.load_linear(src, work)
else:                                   # no photo to hand: a synthetic scene with gradients, edges and texture
    def load(work):
        h = work * 2 // 3
        yy, xx = np.mgrid[0:h, 0:work].astype(np.float32) / work
        img = 0.05 + 0.5 * xx + 0.25 * np.sin(18 * xx) * np.sin(14 * yy) + 0.3 * ((xx - 0.6) ** 2 + (yy - 0.3) ** 2 < 0.02)
        img += 0.2 * (np.floor(xx * 12) % 2) * (yy > 0.45)
        return np.repeat(np.clip(img, 0.01, 1.0)[..., None], 3, -1).astype(np.float32)
    src = "x"
outs = []
for work in (1000, 3000):
    q = twb2.parser().parse_args([src, "-o", "x", "--ev", "0.8", "--repro", "book", "--page-mm", "200"])
    q.view = "scan"
    img = twb2.render(load(work), q)[..., 0]
    if work != 1000:
        k = work // 1000
        img = img[:img.shape[0] // k * k, :img.shape[1] // k * k].reshape(img.shape[0] // k, k, img.shape[1] // k, k).mean(axis=(1, 3))
    outs.append(img)
a, b = outs
h, w = min(a.shape[0], b.shape[0]), min(a.shape[1], b.shape[1])
a, b = a[:h, :w], b[:h, :w]
for q in (5, 25, 50, 75, 95):
    check(f"percentile {q}", float(np.percentile(b, q)), float(np.percentile(a, q)), 0.02)
tex = lambda im: float((im - ndi.gaussian_filter(im, 2.0))[(im > 0.3) & (im < 0.8)].std())
check("fine texture in the greys", tex(b), tex(a), 0.004)

print("\nALL CHECKS PASSED" if ok else "\nSOME CHECKS FAILED")
sys.exit(0 if ok else 1)
