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


twb2.select_film("trix")     # sections 1 to 3 check Tri-X Pan against its datasheet

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
p = twb2.prepare(twb2.parser().parse_args(["x", "-o", "x", "--film", "trix", "--grain", "0", "--lens-um", "0"]))   # the datasheet describes the film alone
cx, cy = twb2.film_curve(p.dev)
base = cy.min()
x = np.arange(4096) * um / 1000.0                                  # mm
worst = 0.0
for f, want in twb2.FILM["mtf_cycles_per_mm"]:
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
pts = twb2.FILM["curves_logH_density"]["7"]
cx, cy = twb2.film_curve(7.0)
check("film curve at 7 min, worst point", max(abs(np.interp(a, cx, cy) - b) for a, b in pts), 0.0, 0.01)
pts = twb2.POLYMAX["curves_logH_density"]["3"]
px, py = twb2.paper_curve(3.0)
check("paper curve at grade 3, worst point", max(abs(np.interp(a, px, py) - b) for a, b in pts), 0.0, 0.02)

# 4. The same picture supplied at two pixel counts
print("4. resolution independence (the same picture supplied at 1500 px and at 4500 px, default settings)")
import contextlib
import io
import tempfile


def scene(width):
    """A synthetic scene with gradients, edges and texture, as an 8-bit image."""
    h = width * 2 // 3
    yy, xx = np.mgrid[0:h, 0:width].astype(np.float32) / width
    img = 0.05 + 0.5 * xx + 0.25 * np.sin(18 * xx) * np.sin(14 * yy) + 0.3 * ((xx - 0.6) ** 2 + (yy - 0.3) ** 2 < 0.02)
    img += 0.2 * (np.floor(xx * 12) % 2) * (yy > 0.45)
    return Image.fromarray((np.clip(img, 0.01, 1.0) ** (1 / 2.2) * 255).astype(np.uint8)).convert("RGB")


def two_sizes(extra):
    """The same scene supplied at 1500 px and at 4500 px, rendered to 1500 px."""
    outs = []
    with tempfile.TemporaryDirectory() as tmp:
        for width in (1500, 4500):
            src, dst = os.path.join(tmp, f"in{width}.png"), os.path.join(tmp, f"out{width}.png")
            scene(width).save(src)
            with contextlib.redirect_stdout(io.StringIO()):
                twb2.main([src, "-o", dst, "--auto-ev", "--out-long-edge", "1500", *extra])
            outs.append(np.asarray(Image.open(dst).convert("L"), dtype=np.float32) / 255)
    return outs


tex = lambda im: float((im - ndi.gaussian_filter(im, 2.0))[(im > 0.3) & (im < 0.8)].std())
# The film and print chain alone: the house-tone step is set aside because it reads the
# picture's own tone percentiles, which this scene's flat steps make jumpy.
a, b = two_sizes(["--house", "0"])
for q in (5, 25, 50, 75, 95):
    check(f"percentile {q}", float(np.percentile(b, q)), float(np.percentile(a, q)), 0.01)
check("fine texture in the greys", tex(b), tex(a), 0.002)
check("average difference after smoothing away grain",
      float(np.abs(ndi.gaussian_filter(a, 4.0) - ndi.gaussian_filter(b, 4.0)).mean()), 0.0, 0.005)
# With every default on, house tones included
a, b = two_sizes([])
check("with house tones: median", float(np.percentile(b, 50)), float(np.percentile(a, 50)), 0.01)
check("with house tones: average difference after smoothing away grain",
      float(np.abs(ndi.gaussian_filter(a, 4.0) - ndi.gaussian_filter(b, 4.0)).mean()), 0.0, 0.01)

# 5. Double-X: the same checks of granularity, sharpness and curve against its own data
print("5. Eastman Double-X 5222 (datasheet granularity 14)")
p = twb2.prepare(twb2.parser().parse_args(["x", "-o", "x", "--film", "doublex", "--grain", "0", "--lens-um", "0"]))
k = 10
D = twb2.add_grain(np.full((k * 300, k * 300), 1.0, np.float32), side / k, 1.0, np.random.default_rng(0))
check("rms granularity", float(D.reshape(300, k, 300, k).mean(axis=(1, 3)).std() * 1000), 14.0, 0.4)
cx, cy = twb2.film_curve(p.dev)
base = cy.min()
um = 1.5
x = np.arange(4096) * um / 1000.0
worst = 0.0
for f, want in twb2.FILM["mtf_cycles_per_mm"]:
    logH = twb2.LOGH_GREY + 0.10 * np.sin(2 * np.pi * f * x)
    E = 0.18 * 10 ** (logH - twb2.LOGH_GREY)
    rgb = np.repeat(np.repeat(E[None, :, None], 64, 0), 3, 2).astype(np.float32)
    D, _ = twb2.expose_and_develop(rgb, p, um, np.random.default_rng(0))
    back = np.interp(D[32, 400:-400] + base, cy, cx)
    xs = x[400:-400]
    worst = max(worst, abs(2 * np.abs(np.mean((back - back.mean()) * np.exp(-2j * np.pi * f * xs))) / 0.10 - want))
check("largest deviation from the datasheet sharpness curve", worst, 0.0, 0.06)
pts = twb2.FILM["curves_logH_density"]["6"]
check("film curve at 6 min, worst point", max(abs(np.interp(a, cx, cy) - b) for a, b in pts), 0.0, 0.01)

# 6. Development and grain
print("6. development")
p = twb2.prepare(twb2.parser().parse_args(["x", "-o", "x", "--film", "trix", "--dev", "norm-1981", "--grain-dev", "1.3"]))
check("Tri-X contrast index at the development its granularity is quoted for", twb2.contrast_index(*twb2.film_curve(p.dev)), 0.56, 0.005)
check("granularity factor at that development", twb2.development_grain(p), 1.0, 0.01)
p = twb2.prepare(twb2.parser().parse_args(["x", "-o", "x", "--film", "trix", "--dev", "11", "--grain-dev", "1.3"]))
check("granularity factor at 11 minutes, (0.86 / 0.56)^1.3", twb2.development_grain(p), (0.860 / 0.56) ** 1.3, 0.02)
p = twb2.prepare(twb2.parser().parse_args(["x", "-o", "x", "--film", "trix", "--dev", "norm-1952"]))
check("gamma of the 1952 norm", twb2.curve_gamma(*twb2.film_curve(p.dev)), 0.83, 0.005)
twb2.select_film("trix-1956")
D = twb2.add_grain(np.full((k * 300, k * 300), 1.0, np.float32), side / k, 1.0, np.random.default_rng(0))
check("granularity of the 1956 Tri-X setting", float(D.reshape(300, k, 300, k).mean(axis=(1, 3)).std() * 1000), 22.0, 0.5)
twb2.select_film("trix")

# 7. A border round the picture: found, measured inside, kept or stripped
print("7. a border round the picture (the scene of section 4 in a dark mat)")
with tempfile.TemporaryDirectory() as tmp:
    pic = np.asarray(scene(1200))
    h, w = pic.shape[:2]
    sheet = np.full((h + 150, w + 110, 3), 12, np.uint8)
    sheet[50:50 + h, 40:40 + w] = pic
    paths = {k: os.path.join(tmp, f"{k}.png") for k in ("pic", "sheet", "keep", "strip", "off", "pic_out", "pic_off")}
    Image.fromarray(pic).save(paths["pic"])
    Image.fromarray(sheet).save(paths["sheet"])
    logs = {}
    for k, src, extra in (("keep", "sheet", []), ("strip", "sheet", ["--frame", "strip"]), ("off", "sheet", ["--frame", "off"]),
                          ("pic_out", "pic", []), ("pic_off", "pic", ["--frame", "off"])):
        buf = io.StringIO()
        with contextlib.redirect_stdout(buf):
            twb2.main([paths[src], "-o", paths[k], "--auto-ev", "--out-long-edge", "1000", *extra])
        logs[k] = buf.getvalue()
    rgb = twb2.load_linear(paths["sheet"], 2560)
    area = twb2.find_frame(rgb)
    check("border found in the mat", float(area is not None), 1.0, 0)
    y0, y1, x0, x1 = twb2.area_px(rgb.shape, area)
    s = 2560 / max(sheet.shape[:2])
    check("picture area stays inside the picture (pixels of mat let in)",
          float(max(0, 40 * s - x0) + max(0, 50 * s - y0) + max(0, x1 - (40 + w) * s) + max(0, y1 - (50 + h) * s)), 0.0, 0)
    check("share of the picture kept in the area", (x1 - x0) * (y1 - y0) / (w * h * s * s), 1.0, 0.04)
    ev = lambda log: float(log.split("auto exposure ")[1].split()[0])
    check("auto exposure inside the mat against the picture alone, EV", ev(logs["keep"]), ev(logs["pic_out"]), 0.03)
    check("auto exposure without looking for it differs, EV", float(abs(ev(logs["off"]) - ev(logs["pic_out"])) > 0.1), 1.0, 0)
    keep, strip = (np.asarray(Image.open(paths[k]).convert("L")) for k in ("keep", "strip"))
    ky0, ky1, kx0, kx1 = twb2.area_px(keep.shape, area)
    check("stripped size: long edge as the area has it in the kept result", float(max(strip.shape)), float(max(ky1 - ky0, kx1 - kx0)), 1)
    check("house tones inside the mat against the picture alone, median",
          float(np.median(keep[ky0:ky1, kx0:kx1])), float(np.median(np.asarray(Image.open(paths["pic_out"]).convert("L"))[5:-5, 5:-5])), 2.0)
    check("a picture without a border is unchanged by looking for one",
          float(np.abs(np.asarray(Image.open(paths["pic_out"]), np.int16) - np.asarray(Image.open(paths["pic_off"]), np.int16)).max()), 0.0, 0)
    # at the size simulated, stripping is a crop of the kept result and nothing else
    for k, extra in (("keep", []), ("strip", ["--frame", "strip"])):
        with contextlib.redirect_stdout(io.StringIO()):
            twb2.main([paths["sheet"], "-o", paths[k], "--auto-ev", "--detail-px", "0", *extra])
    keep, strip = (np.asarray(Image.open(paths[k]).convert("L")) for k in ("keep", "strip"))
    ky0, ky1, kx0, kx1 = twb2.area_px(keep.shape, twb2.find_frame(twb2.load_linear(paths["sheet"])))
    check("stripped pixels against the kept result inside the area", float(np.abs(keep[ky0:ky1, kx0:kx1].astype(int) - strip).max()) if strip.shape == (ky1 - ky0, kx1 - kx0) else 99.0, 0.0, 0)

print("\nALL CHECKS PASSED" if ok else "\nSOME CHECKS FAILED")
sys.exit(0 if ok else 1)
