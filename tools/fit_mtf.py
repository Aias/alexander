#!/usr/bin/env python3
"""Fit the film-stage sharpness model to a film's digitised MTF curve.

    python3 tools/fit_mtf.py            # Tri-X 400
    python3 tools/fit_mtf.py doublex    # Eastman Double-X 5222

Model, for a small exposure modulation at f cycles/mm:
    MTF(f) = M_opt(f) * (1 + kappa * (1 - S(f)))
M_opt : light scatter in the emulsion. For Tri-X a two-Gaussian mixture, of which the tool
        uses the first; for other films exp(-ln 2 (f / f50)^n), whose n = 2 is a Gaussian
        and whose lower values have a sharp core and a long skirt
S     : how the development inhibitor spreads: a Gaussian core (sigma_i) plus a wide
        Gaussian tail (200 um, 3% weight), after spektrafilm's inhibitor model
kappa : edge-effect strength = (log-exposure shift per unit density) x (film gradient)
"""
import json
import math
import sys

import numpy as np
from scipy.optimize import least_squares

FILES = {"trix": "trix400", "doublex": "doublex5222"}
film = sys.argv[1] if len(sys.argv) > 1 else "trix"
d = json.load(open(f"data/{FILES[film]}.json"))
f = np.array([p[0] for p in d["mtf_cycles_per_mm"]])
m = np.array([p[1] for p in d["mtf_cycles_per_mm"]])
TAIL_UM, TAIL_W = 200.0, 0.03


def G(f, sigma_um):                       # transfer function of a Gaussian blur
    return np.exp(-2 * np.pi ** 2 * (sigma_um / 1000.0) ** 2 * f ** 2)


def shaped(f, sigma_um, n):               # same half-response frequency as G, other shape
    f50 = math.sqrt(math.log(2.0) / (2.0 * math.pi ** 2)) / (sigma_um / 1000.0)
    return np.exp(-math.log(2.0) * (f / f50) ** n)


def spread(f, si):
    return (1 - TAIL_W) * G(f, si) + TAIL_W * G(f, TAIL_UM)


def report(fit, values):
    print(f"rms error {np.sqrt(np.mean(fit.fun**2)):.4f}")
    print(" f c/mm   data   model")
    for fi, mi, yi in zip(f, m, values):
        print(f"{fi:6.1f}  {mi:.3f}  {yi:.3f}")


if film == "trix":
    def model(p, f):
        kappa, s1, s2, c, si = p
        return (c * G(f, s1) + (1 - c) * G(f, s2)) * (1 + kappa * (1 - spread(f, si)))

    best = None
    for si0 in (10, 20, 30, 45):
        r = least_squares(lambda p: model(p, f) - m, x0=[0.2, 2.0, 6.0, 0.5, si0],
                          bounds=([0, 0.3, 0.3, 0, 5], [1.0, 12, 25, 1, 80]))
        if best is None or r.cost < best.cost:
            best = r
    kappa, s1, s2, c, si = best.x
    print(f"kappa {kappa:.3f}   inhibitor core sigma {si:.1f} um   optical: {c:.2f} x G({s1:.2f} um) + {1-c:.2f} x G({s2:.2f} um)")
    report(best, model(best.x, f))
    out = {"kappa": round(float(kappa), 4), "inhibitor_sigma_um": round(float(si), 2),
           "inhibitor_tail_um": TAIL_UM, "inhibitor_tail_weight": TAIL_W,
           "optical_sigma_um": [round(float(s1), 3), round(float(s2), 3)], "optical_weight": round(float(c), 4),
           "rms_error": round(float(np.sqrt(np.mean(best.fun ** 2))), 4)}
else:
    def model(p, f):
        kappa, s, n, si = p
        return shaped(f, s, n) * (1 + kappa * (1 - spread(f, si)))

    best = None
    for si0 in (10, 20, 30, 45):
        for n0 in (1.0, 1.5, 2.0):
            r = least_squares(lambda p: model(p, f) - m, x0=[0.3, 4.0, n0, si0],
                              bounds=([0, 0.3, 0.6, 5], [1.5, 25, 2.2, 80]))
            if best is None or r.cost < best.cost:
                best = r
    kappa, s, n, si = best.x
    print(f"kappa {kappa:.3f}   inhibitor core sigma {si:.1f} um   optical: sigma {s:.2f} um, exponent {n:.2f}")
    report(best, model(best.x, f))
    out = {"kappa": round(float(kappa), 4), "inhibitor_sigma_um": round(float(si), 2),
           "inhibitor_tail_um": TAIL_UM, "inhibitor_tail_weight": TAIL_W,
           "optical_sigma_um": [round(float(s), 3)], "optical_exponent": round(float(n), 3),
           "rms_error": round(float(np.sqrt(np.mean(best.fun ** 2))), 4)}
json.dump(out, open(f"data/{FILES[film]}_mtf_fit.json", "w"), indent=1)
