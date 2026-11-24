#!/usr/bin/env python3
"""Fit the film-stage sharpness model to the digitised Tri-X MTF curve.

Model, for a small exposure modulation at f cycles/mm:
    MTF(f) = M_opt(f) * (1 + kappa * (1 - S(f)))
M_opt : light scatter in the emulsion, a two-Gaussian mixture
S     : how the development inhibitor spreads: a Gaussian core (sigma_i) plus a wide
        Gaussian tail (200 um, 3% weight), after spektrafilm's inhibitor model
kappa : edge-effect strength = (log-exposure shift per unit density) x (film gradient)
"""
import json

import numpy as np
from scipy.optimize import least_squares

d = json.load(open("data/trix400.json"))
f = np.array([p[0] for p in d["mtf_cycles_per_mm"]])
m = np.array([p[1] for p in d["mtf_cycles_per_mm"]])
TAIL_UM, TAIL_W = 200.0, 0.03


def G(f, sigma_um):                       # transfer function of a Gaussian blur
    return np.exp(-2 * np.pi ** 2 * (sigma_um / 1000.0) ** 2 * f ** 2)


def model(p, f):
    kappa, s1, s2, c, si = p
    opt = c * G(f, s1) + (1 - c) * G(f, s2)
    spread = (1 - TAIL_W) * G(f, si) + TAIL_W * G(f, TAIL_UM)
    return opt * (1 + kappa * (1 - spread))


best = None
for si0 in (10, 20, 30, 45):
    r = least_squares(lambda p: model(p, f) - m, x0=[0.2, 2.0, 6.0, 0.5, si0],
                      bounds=([0, 0.3, 0.3, 0, 5], [1.0, 12, 25, 1, 80]))
    if best is None or r.cost < best.cost:
        best = r
kappa, s1, s2, c, si = best.x
print(f"kappa {kappa:.3f}   inhibitor core sigma {si:.1f} um   optical: {c:.2f} x G({s1:.2f} um) + {1-c:.2f} x G({s2:.2f} um)")
print(f"rms error {np.sqrt(np.mean(best.fun**2)):.4f}")
print(" f c/mm   data   model")
for fi, mi, yi in zip(f, m, model(best.x, f)):
    print(f"{fi:6.1f}  {mi:.3f}  {yi:.3f}")
json.dump({"kappa": round(float(kappa), 4), "inhibitor_sigma_um": round(float(si), 2),
           "inhibitor_tail_um": TAIL_UM, "inhibitor_tail_weight": TAIL_W,
           "optical_sigma_um": [round(float(s1), 3), round(float(s2), 3)], "optical_weight": round(float(c), 4),
           "rms_error": round(float(np.sqrt(np.mean(best.fun ** 2))), 4)},
          open("data/trix400_mtf_fit.json", "w"), indent=1)
