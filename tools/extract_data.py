#!/usr/bin/env python3
"""Extract the digitised Kodak Tri-X 400, Eastman Double-X 5222 and Polymax Fine-Art data from a local
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


# ---- films ---------------------------------------------------------------------
# Three-band stand-in for a camera's R, G, B channels (an approximation: the real
# scene spectrum is unknown once a photo is RGB).
BANDS = {"r": (605.0, 32.0), "g": (540.0, 32.0), "b": (455.0, 24.0)}
FILTERS = {"none": None, "yellow": 495.0, "orange": 550.0, "red": 600.0}  # 50% cut-on, nm
grid = np.arange(380.0, 701.0, 1.0)


def film_record(py_file, name, extra, granularity_curve=True):
    src = open(f"{SRC}/{py_file}").read()
    sens = field(src, "log_sensitivity")
    wl = np.array([p[0] for p in sens])
    s = 10.0 ** np.array([p[1] for p in sens])
    S = np.interp(grid, wl, s, left=s[0], right=0.0)

    def weights(cut_on):
        t = 1.0 if cut_on is None else 1.0 / (1.0 + np.exp(-(grid - cut_on) / 6.0))
        out = {}
        for ch, (mu, sd) in BANDS.items():
            band = np.exp(-0.5 * ((grid - mu) / sd) ** 2)
            out[ch] = float((S * t * band).sum() / band.sum())
        tot = sum(out.values())
        return {k: round(v / tot, 4) for k, v in out.items()}

    rec = {
        "name": name,
        "source": f"JanLohse/spectral_film_lut, {py_file} (MIT)",
        "curves_logH_density": {k: v for k, v in keyed_blocks(src, "curve").items()},
        "log_sensitivity_nm": sens,
        "mtf_cycles_per_mm": field(src, "mtf"),
    }
    if granularity_curve:
        rec["granularity_curve_logH_density"] = field(src, "rms_curve")
        rec["granularity_curve_logH_sigma"] = field(src, "rms_density")
    rec["rms_granularity"] = int(re.search(r"rms=(\d+)", src).group(1))
    rec["rgb_weights"] = {n: weights(c) for n, c in FILTERS.items()}
    rec["rgb_weights_note"] = ("three Gaussian bands (R 605/32, G 540/32, B 455/24 nm) integrated against the "
                               "digitised sensitivity; filters are sigmoid cut-ons at 495, 550, 600 nm")
    rec.update(extra)
    return rec


json.dump(film_record(
    "bw_negative_film/kodak_trix_400.py", "Kodak Tri-X 400 (datasheet digitisation, file dated 2007)",
    {"iso": 400, "default_dev": 7,
     # Kodak quotes the granularity for development "to a contrast index of 0.56"
     "granularity_contrast_index": 0.56}),
    open("data/trix400.json", "w"), indent=1)

# Eastman Double-X 5222, an emulsion of 1959. Its file repeats Tri-X's granularity-against-density
# table, so that table is left out; the single figure of 14 is Kodak's.
json.dump(film_record(
    "bw_negative_film/kodak_5222.py", "Eastman Double-X 5222 (datasheet digitisation)",
    {"iso": 250, "default_dev": 6, "granularity_dev": 6}, granularity_curve=False),
    open("data/doublex5222.json", "w"), indent=1)

# ---- Polymax Fine-Art paper -----------------------------------------------------
poly = open(f"{SRC}/bw_print_film/kodak_polymax_fine_art.py").read()
curves = keyed_blocks(poly, "curve")
json.dump({
    "name": "Kodak Professional Polymax Fine-Art Paper (datasheet digitisation, file dated 1998)",
    "source": "JanLohse/spectral_film_lut, bw_print_film/kodak_polymax_fine_art.py (MIT)",
    "curves_logH_density": {("unfiltered" if k == "None" else k): v for k, v in curves.items()},
}, open("data/polymax.json", "w"), indent=1)

for f in ("trix400", "doublex5222"):
    t = json.load(open(f"data/{f}.json"))
    print(t["name"], "curves:", {k: len(v) for k, v in t["curves_logH_density"].items()})
    print("  RGB weights:", t["rgb_weights"]["none"], " granularity:", t["rms_granularity"], " sharpness points:", len(t["mtf_cycles_per_mm"]))
print("Polymax grades:", {k: len(v) for k, v in json.load(open("data/polymax.json"))["curves_logH_density"].items()})
