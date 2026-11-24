#!/usr/bin/env python3
"""Extract the digitised Kodak Tri-X 400 and Polymax Fine-Art data from a local
checkout of JanLohse/spectral_film_lut (MIT) into data/*.json.

    python3 tools/extract_data.py /path/to/spectral_film_lut/src/spectral_film_lut

The source files are read as text and parsed; nothing in them is executed.
"""
import json
import re
import sys

import numpy as np

SRC = sys.argv[1]
PAIR = re.compile(r"(-?[0-9.]+):\s*(-?[0-9.]+)")


def pairs(text):
    return sorted((float(a), float(b)) for a, b in PAIR.findall(text))


def keyed_blocks(src, var):
    """{key: [(x, y), ...]} for a `var = {key: {x: y, ...}, ...}` literal."""
    body = re.search(var + r" = \{(.*?)\n\}\n", src, re.S).group(1)
    parts = re.split(r"\n    (None|-?\d+): \{", "\n" + body)
    return {parts[i]: pairs(parts[i + 1]) for i in range(1, len(parts), 2)}


def field(src, name):
    m = re.search(name + r"=\[\s*\{(.*?)\}", src, re.S)
    return pairs(m.group(1)) if m else []


# ---- Tri-X 400 ---------------------------------------------------------------
trix = open(f"{SRC}/bw_negative_film/kodak_trix_400.py").read()
sens = field(trix, "log_sensitivity")
wl = np.array([p[0] for p in sens])
s = 10.0 ** np.array([p[1] for p in sens])
grid = np.arange(380.0, 701.0, 1.0)
S = np.interp(grid, wl, s, left=s[0], right=0.0)

# Three-band stand-in for a camera's R, G, B channels (an approximation: the real
# scene spectrum is unknown once a photo is RGB).
BANDS = {"r": (605.0, 32.0), "g": (540.0, 32.0), "b": (455.0, 24.0)}
FILTERS = {"none": None, "yellow": 495.0, "orange": 550.0, "red": 600.0}  # 50% cut-on, nm


def weights(cut_on):
    t = 1.0 if cut_on is None else 1.0 / (1.0 + np.exp(-(grid - cut_on) / 6.0))
    out = {}
    for ch, (mu, sd) in BANDS.items():
        band = np.exp(-0.5 * ((grid - mu) / sd) ** 2)
        out[ch] = float((S * t * band).sum() / band.sum())
    tot = sum(out.values())
    return {k: round(v / tot, 4) for k, v in out.items()}


json.dump({
    "name": "Kodak Tri-X 400 (datasheet digitisation, file dated 2007)",
    "source": "JanLohse/spectral_film_lut, bw_negative_film/kodak_trix_400.py (MIT)",
    "curves_logH_density": {k: v for k, v in keyed_blocks(trix, "curve").items()},
    "log_sensitivity_nm": sens,
    "mtf_cycles_per_mm": field(trix, "mtf"),
    "granularity_curve_logH_density": field(trix, "rms_curve"),
    "granularity_curve_logH_sigma": field(trix, "rms_density"),
    "rms_granularity": 17,
    "rgb_weights": {name: weights(c) for name, c in FILTERS.items()},
    "rgb_weights_note": "three Gaussian bands (R 605/32, G 540/32, B 455/24 nm) integrated against the "
                        "digitised sensitivity; filters are sigmoid cut-ons at 495, 550, 600 nm",
}, open("data/trix400.json", "w"), indent=1)

# ---- Polymax Fine-Art paper -----------------------------------------------------
poly = open(f"{SRC}/bw_print_film/kodak_polymax_fine_art.py").read()
curves = keyed_blocks(poly, "curve")
json.dump({
    "name": "Kodak Professional Polymax Fine-Art Paper (datasheet digitisation, file dated 1998)",
    "source": "JanLohse/spectral_film_lut, bw_print_film/kodak_polymax_fine_art.py (MIT)",
    "curves_logH_density": {("unfiltered" if k == "None" else k): v for k, v in curves.items()},
}, open("data/polymax.json", "w"), indent=1)

t = json.load(open("data/trix400.json"))
print("Tri-X curves:", {k: len(v) for k, v in t["curves_logH_density"].items()})
print("Tri-X RGB weights:", t["rgb_weights"])
print("Polymax grades:", {k: len(v) for k, v in json.load(open("data/polymax.json"))["curves_logH_density"].items()})
