#!/usr/bin/env python3
"""Reference values from twb2.py for the Rust port's tests.

    python3 studio/tests/make_fixtures.py

Writes studio/tests/fixtures/derived.json (curves and derived numbers) and, for a
synthetic scene, the scene itself and full renders without grain as 16-bit PNGs.
"""
import contextlib, io, json, math, os, sys

import numpy as np
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, ROOT)
import twb2  # noqa: E402

OUT = os.path.join(ROOT, "studio", "tests", "fixtures")
os.makedirs(OUT, exist_ok=True)
d = {"films": {}, "paper": {}, "misc": {}}
for name in twb2.FILMS:
    twb2.select_film(name)
    f = {"logh_grey": twb2.LOGH_GREY, "granularity_ci": twb2.granularity_contrast_index(), "devs": {}}
    for dev in (None, 9.0, "norm-1952", "norm-1956", "norm-1981"):
        t = twb2.resolve_dev(dev if dev is None or isinstance(dev, str) else str(dev))
        cx, cy = twb2.film_curve(t)
        base = float(cy.min())
        class P: pass
        p = P(); p.dev = t; p.grain_dev = 1.3
        f["devs"][str(dev)] = {
            "minutes": t, "curve_every_25": [float(v) for v in cy[::25]], "base": base,
            "ref": float(np.interp(twb2.LOGH_GREY, cx, cy) - base),
            "contrast_index": float(twb2.contrast_index(cx, cy)), "gamma": twb2.curve_gamma(cx, cy),
            "development_grain": twb2.development_grain(p),
            "grain_area": float(twb2.grain_area_um2(twb2.FILM["rms_granularity"])),
        }
    d["films"][name] = f
twb2.select_film("trix-1956")
for grade in (3.0, 2.55, 0.9, 4.25):
    px, py = twb2.paper_curve(grade)
    dmin = float(py.min())
    ref = 0.681
    offset = float(np.interp(dmin + 0.745, py, px)) + ref + (-0.5) * math.log10(2.0)
    d["paper"][str(grade)] = {"curve_every_30": [float(v) for v in py[::30]], "dmin": dmin, "range": twb2.print_range(grade), "offset_ref0681": offset}
def tau(d_in, d_out):
    lo, hi = 0.05, 50.0
    for _ in range(60):
        t = 0.5 * (lo + hi)
        lo, hi = (t, hi) if t * (1 - math.exp(-d_in / t)) < d_out else (lo, t)
    return t
d["misc"]["tau_2.1_1.6"] = tau(twb2.print_range(3.0), 1.6)
d["misc"]["tau_2.1_1.2"] = tau(twb2.print_range(3.0), 1.2)
# house curve: for given own percentiles, the curve's display value out at sample inputs
from scipy.interpolate import PchipInterpolator
cases = []
for own in ([0.085, 0.094, 0.145, 0.93, 0.96], [0.09, 0.2, 0.38, 0.8, 0.9], [0.086, 0.0861, 0.12, 0.9999, 1.0]):
    for strength in (1.0, 0.5):
        want = (1 - strength) * np.array(own) + strength * np.array(twb2.HOUSE_VAL)
        xs, ys = [0.0], [0.0]
        for x, y in zip(own, want):
            if x - xs[-1] > 0.005 and x < 0.995:
                xs.append(float(x)); ys.append(float(y))
        xs.append(1.0); ys.append(1.0)
        for i in range(1, len(xs)):
            ys[i] = min(ys[i], ys[i - 1] + twb2.HOUSE_MAX_SLOPE * (xs[i] - xs[i - 1])); ys[i] = max(ys[i], ys[i - 1] + 1e-4)
        v = np.linspace(0, 1, 41)
        cases.append({"own": own, "strength": strength, "out": [float(t) for t in np.clip(PchipInterpolator(xs, ys)(v), 0, 1)]})
d["misc"]["house"] = cases
json.dump(d, open(os.path.join(OUT, "derived.json"), "w"), indent=1)

# ---- a synthetic scene and its renders without grain ----
W, H = 384, 256
yy, xx = np.mgrid[0:H, 0:W].astype(np.float32) / W
img = 0.05 + 0.5 * xx + 0.25 * np.sin(18 * xx) * np.sin(14 * yy) + 0.3 * ((xx - 0.6) ** 2 + (yy - 0.3) ** 2 < 0.02)
img += 0.2 * (np.floor(xx * 12) % 2) * (yy > 0.45)
grey = np.clip(img, 0.01, 1.0) ** (1 / 2.2)
rgb = np.stack([np.clip(grey * 1.05, 0, 1), grey, np.clip(grey * (0.8 + 0.4 * yy / yy.max()), 0, 1)], -1)
scene = os.path.join(OUT, "scene.png")
Image.fromarray((rgb * 255 + 0.5).astype(np.uint8), "RGB").save(scene)
FILM_MM = str(W * 9.375 / 1000)                      # the default 9.375 um per cell at this size
CASES = {
    "default": [],
    "straight": ["--house", "0", "--lens-exponent", "2", "--lens-um", "8"],
    "doublex": ["--film", "doublex", "--halation", "0.06"],
    "nelson_red": ["--edge-density", "1", "--filter", "red", "--dev", "norm-1956", "--grade", "2.55"],
    "copy": ["--repro", "copy"],
    "book": ["--repro", "book", "--page-mm", str(W / 300 * 25.4 * 2)],
    "book_page": ["--repro", "book", "--view", "page"],
}
meta = {}
for name, extra in CASES.items():
    out = os.path.join(OUT, f"render_{name}.png")
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        twb2.main([scene, "-o", out, "--auto-ev", "--grain", "0", "--detail-px", "0", "--film-mm", FILM_MM, "--bits", "16", *extra])
    meta[name] = {"flags": extra, "log": buf.getvalue().strip().splitlines()[0]}
    print(name, meta[name]["log"])
json.dump({"film_mm": float(FILM_MM), "cases": meta}, open(os.path.join(OUT, "renders.json"), "w"), indent=1)

# ---- flat bands with grain: the statistics a different random source must reproduce ----
BW, BH, LEVELS = 80, 240, [0.08, 0.2, 0.35, 0.5, 0.7, 0.9]
bands = np.concatenate([np.full((BH, BW), v, np.float32) for v in LEVELS], 1)
bands_png = os.path.join(OUT, "bands.png")
Image.fromarray((np.stack([bands] * 3, -1) * 255 + 0.5).astype(np.uint8), "RGB").save(bands_png)
GRAIN_CASES = {
    "default": ["--film-mm", "4.5"],
    "coarse": ["--film-mm", "4.5", "--film", "trix", "--grain", "2", "--grain-light", "1", "--dev", "9"],
    "few_grains": ["--film-mm", "1.2", "--grain-light", "1"],
}
stats = {"levels": LEVELS, "band_px": BW, "height": BH, "margin": 16, "cases": {}}
for name, extra in GRAIN_CASES.items():
    out = os.path.join(OUT, "_grain.png")
    with contextlib.redirect_stdout(io.StringIO()):
        twb2.main([bands_png, "-o", out, "--house", "0", "--detail-px", "0", "--bits", "16", *extra])
    v = np.asarray(Image.open(out), dtype=np.float64) / 65535
    os.remove(out)
    rows = []
    for i in range(len(LEVELS)):
        patch = v[16:BH - 16, i * BW + 16:(i + 1) * BW - 16]
        m = patch.mean()
        rows.append({"mean": float(m), "std": float(patch.std()), "skew": float(((patch - m) ** 3).mean() / max(patch.std() ** 3, 1e-12)),
                     "neighbour": float(np.corrcoef(patch[:, :-1].ravel(), patch[:, 1:].ravel())[0, 1])})
    stats["cases"][name] = {"flags": extra, "bands": rows}
    print(name, [(round(r["mean"], 3), round(r["std"], 4)) for r in rows])
json.dump(stats, open(os.path.join(OUT, "grain.json"), "w"), indent=1)
