#!/usr/bin/env python3
"""twb2: the look of the photographs in "The Timeless Way of Building", built as the
physical chain those pictures went through, in physical units.

    scene -> lens and film (softness, halation) -> Tri-X negative (curve, edge effects, grain)
          -> print on paper -> optional: copied from a printed page, or halftone on uncoated stock

Every length is in micrometres on the film or millimetres on the printed page, and the
simulation runs on a grid of fixed size (--detail-px) before being enlarged to the output,
so the same settings give the same picture at 1,000 px and at 48 MP.

    python3 twb2.py photo.jpg -o out.png --auto-ev                              # darkroom print
    python3 twb2.py photo.jpg -o out.png --auto-ev --repro copy                 # + copied from a printed page
    python3 twb2.py photo.jpg -o out.png --auto-ev --repro book                 # + printing press
    python3 twb2.py photo.jpg -o out.png --auto-ev --repro book --view page     # + cream paper

Sourced numbers (README.md lists where each comes from):
  film curve, colour response, granularity 17, sharpness curve  Kodak Tri-X 400 datasheet
  the same for Double-X 5222 (granularity 14)                    Eastman Double-X datasheet
  granularity grows as contrast to the power 1.3                 Trinder 1980, measured on Double-X
  paper curves by grade                                          Kodak Polymax Fine-Art datasheet
  grain statistics, granularity -> grain size                    spektrafilm (dev branch), re-implemented
  edge-effect mechanism; strength and size                       spektrafilm / fitted to the Tri-X curve
  edge effects grow with density                                 Nelson 1971, as reported by Rajkowski and Nowak 2001
  halation mechanism and its 65 um width on still-film base      spektrafilm, re-implemented
  long-tailed blur (exponent near 1.15)                          fit of 54 published film sharpness curves
  dots that drop out and fill in when a picture is printed       Corey, "Making PMT Halftone Prints", 1977
  ink density 1.20, dot gain about 0.2 on uncoated paper         German offset process standard
Tuned, not sourced: lens softness, halation strength, enlarger and copy-lens blur, the copy
stage's blur and contrast, smallest printable dot, scan levels.
"""
import argparse
import json
import math
import os

import numpy as np
from PIL import Image, ImageCms, ImageOps, PngImagePlugin
from scipy import ndimage as ndi
from scipy.interpolate import PchipInterpolator

F = np.float32
HERE = os.path.dirname(os.path.abspath(__file__))
POLYMAX = json.load(open(os.path.join(HERE, "data", "polymax.json")))

# The films. "trix-1956" is the Tri-X data with the grain and sharpness Kodak's 1956 sheets
# class it at: medium grain against today's fine, 75-90 lines/mm against 100-125. Its two
# numbers are estimates from those classes, not measurements.
FILMS = {
    "trix": {"data": "trix400.json", "fit": "trix400_mtf_fit.json"},
    "trix-1956": {"data": "trix400.json", "fit": "trix400_mtf_fit.json", "rms_granularity": 22, "emulsion_blur": 1.33},
    "doublex": {"data": "doublex5222.json", "fit": "doublex5222_mtf_fit.json"},
}
# Development Kodak recommended in each period, as the gamma of the negative; None stands for
# the contrast index the datasheet's granularity is quoted at.
DEV_NORMS = {"norm-1952": 0.83, "norm-1956": 0.71, "norm-1981": None}
FILM = FIT = None
LOGH_GREY = -1.75            # log10 lux-seconds given to an 18% grey by a meter at ISO 400


def select_film(name):
    """Make one of FILMS the film every later call simulates."""
    global FILM, FIT, LOGH_GREY
    spec = FILMS[name]
    FILM = json.load(open(os.path.join(HERE, "data", spec["data"])))
    FIT = json.load(open(os.path.join(HERE, "data", spec["fit"])))
    FILM.update({k: v for k, v in spec.items() if k not in ("data", "fit")})
    LOGH_GREY = -1.75 + math.log10(400.0 / FILM.get("iso", 400))   # a slower film is given more light


select_film("trix")
APERTURE_UM2 = math.pi * 24.0 ** 2   # the 48 um aperture granularity is quoted for
FILM_DMAX = 3.4              # net density the emulsion can reach (top of the datasheet's granularity plot)
GRAIN_UNIFORMITY = 0.6       # black-and-white value used by spektrafilm
PAPER_CREAM = (0.957, 0.937, 0.882)   # display colour of the uncoated stock [tuned]
# The tone range the book's picture pages share: display values at these percentiles of a
# page, as the median over twelve pages measured from scans. The darkest tenth and quarter
# sit within a few hundredths of these values on every page measured.
HOUSE_PCT = (2, 10, 25, 50, 75, 90, 98)
HOUSE_VAL = (0.08, 0.12, 0.24, 0.55, 0.78, 0.95, 1.00)


# ---------- colour and file handling -------------------------------------------
def srgb_to_lin(x):
    return np.where(x <= 0.04045, x / 12.92, ((x + 0.055) / 1.055) ** 2.4)


def lin_to_srgb(x):
    x = np.clip(x, 0, None)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * x ** (1 / 2.4) - 0.055)


def load_linear(path, work_long_edge=None):
    """Read a photo as linear-light RGB. Honours orientation and converts an embedded
    colour profile (Display P3 from a phone or a Mac, Adobe RGB) to sRGB first."""
    im = ImageOps.exif_transpose(Image.open(path))
    icc = im.info.get("icc_profile")
    im = im.convert("RGB")
    if icc:
        try:
            import io
            src = ImageCms.ImageCmsProfile(io.BytesIO(icc))
            im = ImageCms.profileToProfile(im, src, ImageCms.createProfile("sRGB"), outputMode="RGB")
        except Exception:
            pass
    if work_long_edge and work_long_edge != max(im.size):
        s = work_long_edge / max(im.size)
        im = im.resize((round(im.width * s), round(im.height * s)), Image.LANCZOS)
    return srgb_to_lin(np.asarray(im, dtype=F) / 255).astype(F)


def blur(x, sigma_px):
    """Gaussian blur. Very wide blurs are done on a reduced copy."""
    if sigma_px < 0.15:
        return x
    if sigma_px <= 12:
        return ndi.gaussian_filter(x, sigma_px, mode="reflect", truncate=3.5)
    k = int(sigma_px // 4)
    h, w = x.shape
    H, W = math.ceil(h / k) * k, math.ceil(w / k) * k
    xp = np.pad(x, ((0, H - h), (0, W - w)), mode="reflect")
    small = xp.reshape(H // k, k, W // k, k).mean(axis=(1, 3))
    small = ndi.gaussian_filter(small, sigma_px / k, mode="reflect", truncate=3.5)
    big = np.asarray(Image.fromarray(small.astype(F), mode="F").resize((W, H), Image.BICUBIC), dtype=F)
    return big[:h, :w]


def blur_shaped(x, sigma_px, exponent):
    """Blur whose transfer function is exp(-ln 2 (f / f50)^n). With n = 2 it is the Gaussian
    of the given sigma. A lower n keeps the frequency of half response and changes the
    shape: a sharp core with a long, faint skirt. Published film sharpness curves fall off
    with n near 1.15."""
    if sigma_px < 0.15:
        return x
    if abs(exponent - 2.0) < 1e-6:
        return blur(x, sigma_px)
    pad = int(min(min(x.shape) - 1, math.ceil(40 * sigma_px) + 16))   # the skirt reaches far
    xp = np.pad(x, pad, mode="reflect")
    f = np.hypot(np.fft.fftfreq(xp.shape[0])[:, None], np.fft.rfftfreq(xp.shape[1])[None, :])
    f50 = math.sqrt(math.log(2.0) / (2.0 * math.pi ** 2)) / sigma_px
    H = np.exp(-math.log(2.0) * np.power(f / f50, exponent))
    out = np.fft.irfft2(np.fft.rfft2(xp) * H, s=xp.shape)
    return out[pad:-pad, pad:-pad].astype(F)


# ---------- the negative ----------------------------------------------------------
def film_curve(dev_min):
    """The film's characteristic curve at a development time (minutes), as arrays of log
    exposure and density, interpolated between the times the datasheet gives."""
    x = np.arange(-4.5, 2.0001, 0.01)
    table = {}
    for t, pts in FILM["curves_logH_density"].items():
        px = np.array([p[0] for p in pts]); py = np.array([p[1] for p in pts])
        y = np.interp(x, px, py)
        end = (py[-1] - py[-3]) / (px[-1] - px[-3])          # slope where the data stops
        over = x > px[-1]                                     # shoulder: slope dies away
        y[over] = py[-1] + end * 0.6 * (1 - np.exp(-(x[over] - px[-1]) / 0.6))
        table[float(t)] = y
    times = sorted(table)
    t = min(max(dev_min, times[0]), times[-1])
    hi = next(i for i, v in enumerate(times) if v >= t)
    lo = max(hi - 1, 0)
    f = 0.0 if hi == lo else (t - times[lo]) / (times[hi] - times[lo])
    return x, (1 - f) * table[times[lo]] + f * table[times[hi]]


def contrast_index(cx, cy):
    """Kodak's contrast index: the slope of the straight line through two points of the
    curve that lie 0.2 and 2.2 units from one point on the base-plus-fog line."""
    base = float(cy.min())

    def hit(c, r):                          # where the curve is r away from (c, base)
        d = np.hypot(cx - c, cy - base) - r
        i = np.where((d[:-1] < 0) & (d[1:] >= 0) & (cx[:-1] >= c))[0]
        if len(i) == 0:
            return None
        i = int(i[0]); t = -d[i] / (d[i + 1] - d[i])
        return cx[i] + t * (cx[i + 1] - cx[i]), cy[i] + t * (cy[i + 1] - cy[i])

    best = None
    for c in np.arange(cx[0], cx[-1] - 2.2, 0.005):
        a, b = hit(c, 0.2), hit(c, 2.2)
        if a is None or b is None:
            continue
        err = abs(math.atan2(a[1] - base, a[0] - c) - math.atan2(b[1] - base, b[0] - c))
        if best is None or err < best[0]:
            best = (err, (b[1] - base) / (b[0] - c))
    return best[1]


def curve_gamma(cx, cy):
    """Slope of the curve's straight part, taken between net densities 0.6 and 1.0."""
    net = cy - cy.min()
    return float(np.gradient(cy, cx)[(net > 0.6) & (net < 1.0)].mean())


def dev_times():
    return sorted(float(t) for t in FILM["curves_logH_density"])


def dev_for(measure, target):
    """Development time at which `measure` of the curve reaches `target`, within the
    range of times the datasheet covers."""
    lo, hi = dev_times()[0], dev_times()[-1]
    if measure(*film_curve(lo)) >= target:
        return lo
    if measure(*film_curve(hi)) <= target:
        return hi
    for _ in range(30):
        mid = 0.5 * (lo + hi)
        lo, hi = (mid, hi) if measure(*film_curve(mid)) < target else (lo, mid)
    return round(0.5 * (lo + hi), 2)


def granularity_contrast_index():
    """Contrast index of the development the datasheet's granularity figure is quoted for."""
    if "granularity_contrast_index" in FILM:
        return FILM["granularity_contrast_index"]
    return contrast_index(*film_curve(FILM["granularity_dev"]))


def resolve_dev(dev):
    """Minutes from --dev: a number, a period norm, or the film's default when not given."""
    if dev is None:
        return float(FILM["default_dev"])
    if dev in DEV_NORMS:
        g = DEV_NORMS[dev]
        return dev_for(contrast_index, granularity_contrast_index()) if g is None else dev_for(curve_gamma, g)
    return float(dev)


def development_grain(p):
    """Factor on granularity for the chosen development. Measured on Double-X, granularity
    at density 1.0 grows as the negative's contrast to a power of 1.3 to 1.4, whatever the
    developer (Trinder 1980). The factor is 1 at the development the datasheet figure is for."""
    if p.grain_dev <= 0:
        return 1.0
    return float((contrast_index(*film_curve(p.dev)) / granularity_contrast_index()) ** p.grain_dev)


def grain_area_um2(rms_granularity):
    """Area of one developed grain that reproduces a catalogue granularity figure
    (standard deviation of density x 1000 through a 48 um aperture at net density 1.0)."""
    sigma = rms_granularity / 1000.0
    return sigma ** 2 * APERTURE_UM2 / (1.0 * (FILM_DMAX - GRAIN_UNIFORMITY * 1.0))


def add_grain(D, um, scale, rng):
    """Each pixel holds N grains; the number developed is Poisson. That gives the
    variance D (Dmax - u D) / N behind the catalogue figure, and its skew at small N."""
    N = (um * um) / grain_area_um2(FILM["rms_granularity"] * scale)
    prob = np.clip(D / FILM_DMAX, 0.0, 0.98)
    sat = 1.0 - GRAIN_UNIFORMITY * prob
    return (rng.poisson(N * prob / sat).astype(F) * (FILM_DMAX / N) * sat).astype(F)


def film_exposure(rgb, p, weights=None):
    """Linear RGB -> the exposure the film receives, scaled so a metered grey is 0.18.
    `weights` overrides the film's colour response (used to show a plain conversion)."""
    w = weights or FILM["rgb_weights"][p.filter]
    E = (w["r"] * rgb[..., 0] + w["g"] * rgb[..., 1] + w["b"] * rgb[..., 2]).astype(F)
    ev = p.ev
    if p.auto_ev is not None:
        enc = lin_to_srgb(E)
        med, top = float(np.median(enc)), float(np.percentile(enc, 98))
        lin = lambda v: float(srgb_to_lin(np.float64(v)))
        auto = 0.5 * (math.log2(lin(p.auto_ev) / max(lin(med), 1e-4)) + math.log2(lin(0.90) / max(lin(top), 1e-4)))
        auto = float(np.clip(auto, -1.0, 2.5))
        ev += auto
        print(f"auto exposure {auto:+.2f} EV (median {med:.2f}, brightest 2% at {top:.2f})")
    return E * F(2.0 ** ev)


def expose_and_develop(rgb, p, um, rng):
    """Linear RGB -> net density of the developed negative, grain included."""
    E = film_exposure(rgb, p)
    # softness of the image that reaches the film, then light scatter in the emulsion.
    # Both act on light before development, so grain stays crisp over the softened picture
    # and the film and paper curves firm up the softened edges.
    opt, opt_n = FIT["optical_sigma_um"][0] * FILM.get("emulsion_blur", 1.0), FIT.get("optical_exponent", 2.0)
    if abs(p.lens_exponent - 2.0) < 1e-6 and abs(opt_n - 2.0) < 1e-6:
        E = blur(E, math.hypot(p.lens_um, opt) / um)
    else:
        E = blur_shaped(E, p.lens_um / um, p.lens_exponent)
        E = blur_shaped(E, opt / um, opt_n)
    if p.halation > 0:
        # halation: light that passed through the emulsion is reflected by the back of the
        # film base and returns as a wide, faint glow. Three reflections, each half as strong
        # as the last and wider by the square root of its number.
        w = (1.0, 0.5, 0.25)
        glow = sum(wk / sum(w) * blur(E, p.halation_um * math.sqrt(k) / um) for k, wk in enumerate(w, 1))
        E = ((E + p.halation * glow) / (1.0 + p.halation)).astype(F)
        del glow
    logH = (np.log10(np.maximum(E, 1e-6) / 0.18) + LOGH_GREY).astype(F)
    del E

    cx, cy = film_curve(p.dev)
    base = float(cy.min())
    ref = float(np.interp(LOGH_GREY, cx, cy) - base)                # density of a metered grey
    D = (np.interp(logH, cx, cy) - base).astype(F)

    if not p.no_edge:
        # development edge effects: density releases an inhibitor that spreads sideways and
        # holds back development nearby. Only the difference from the local value acts,
        # so even areas stay on the datasheet curve.
        mid = (cy - base > 0.6) & (cy - base < 1.0)
        gamma = float(np.gradient(cy, cx)[mid].mean())
        k = p.edge * FIT["kappa"] / gamma
        tw = FIT["inhibitor_tail_weight"]
        spread = (1 - tw) * blur(D, FIT["inhibitor_sigma_um"] / um) + tw * blur(D, FIT["inhibitor_tail_um"] / um)
        # Nelson's measurement: the change at an edge grows with the density itself as well
        # as with the difference across the edge. The fitted strength holds at a metered grey.
        if p.edge_density > 0:
            k = k * np.power(np.maximum(D, 0.0) / ref, p.edge_density)
        logH += (k * (D - spread)).astype(F)
        del spread
        D = (np.interp(logH, cx, cy) - base).astype(F)
    del logH

    if p.grain > 0:
        D = add_grain(D, um, p.grain * development_grain(p), rng)

    return D, ref


# ---------- the print ---------------------------------------------------------------
def paper_curve(grade):
    """Polymax curve at a (possibly fractional) grade, on a common log-exposure grid."""
    x = np.arange(-0.5, 4.0001, 0.005)
    table = {}
    for g, pts in POLYMAX["curves_logH_density"].items():
        if g == "unfiltered":
            continue
        px = np.array([q[0] for q in pts]); py = np.array([q[1] for q in pts])
        table[float(g)] = np.interp(x, px, py)
    grades = sorted(table)
    g = min(max(grade, grades[0]), grades[-1])
    hi = next(i for i, v in enumerate(grades) if v >= g)
    lo = max(hi - 1, 0)
    f = 0.0 if hi == lo else (g - grades[lo]) / (grades[hi] - grades[lo])
    y = (1 - f) * table[grades[lo]] + f * table[grades[hi]]
    return x, np.maximum.accumulate(y)


def print_on_paper(D, ref, p, um):
    """Negative density -> print reflectance relative to the paper's white."""
    T = np.power(F(10.0), -D)
    T = blur(T, p.enlarger_um / um)                                  # enlarger lens
    Dn = -np.log10(np.maximum(T, 1e-6))
    del T
    px, py = paper_curve(p.grade)
    dmin = float(py.min())
    target = dmin + p.print_grey                                     # density a metered grey prints at
    offset = float(np.interp(target, py, px)) + ref + p.print_ev * math.log10(2.0)
    Dp = np.interp(offset - Dn, px, py).astype(F)
    return np.power(F(10.0), -(Dp - dmin)).astype(F)


# ---------- the book page -------------------------------------------------------------
def print_range(grade):
    _, py = paper_curve(grade)
    return float(py.max() - py.min())


def compress_range(d, d_in_max, d_out_max):
    """Map density 0..d_in_max onto 0..d_out_max with slope 1 at the highlight end and a
    smooth roll-off toward the shadows: t = d_out_max (1 - e^(-d/tau)) / (1 - e^(-d_in_max/tau))."""
    if d_out_max >= d_in_max:
        return d
    lo, hi = 0.05, 50.0                    # solve tau (1 - e^(-d_in_max/tau)) = d_out_max
    for _ in range(60):
        tau = 0.5 * (lo + hi)
        lo, hi = (tau, hi) if tau * (1 - math.exp(-d_in_max / tau)) < d_out_max else (lo, tau)
    return (d_out_max * (1 - np.exp(-d / tau)) / (1 - math.exp(-d_in_max / tau))).astype(F)


def smoothstep(lo, hi, x):
    t = np.clip((x - lo) / (hi - lo), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def printed_once(t, drop, fill):
    """Tone value (0 paper, 1 solid) after one printing: dots smaller than `drop` fail to
    print, and tones above `fill` close up into solid ink."""
    t = t * smoothstep(0.6 * drop, 1.4 * drop, t)
    return 1.0 - (1.0 - t) * smoothstep(0.6 * (1.0 - fill), 1.4 * (1.0 - fill), 1.0 - t)


def s_curve(x, gain):
    """Contrast gain about the middle of 0..1 with both ends held."""
    x = np.clip(x, 0.0, 1.0)
    return np.where(x < 0.5, 0.5 * np.power(2.0 * x, gain), 1.0 - 0.5 * np.power(2.0 * (1.0 - x), gain))


def copied_from_page(R, p, long_edge):
    """The darkroom print as it looks after being printed in a book or magazine,
    re-photographed from that page, and printed again. Each printing loses the two ends
    of the scale; the copy between them softens detail and raises contrast. The result is
    shown the way the reference scans are, with solid ink as the print's own black."""
    rng = print_range(p.grade)
    black, solid = 10.0 ** -rng, 10.0 ** -p.copy_ink
    # first printing: the print's long density range is fitted to what ink can hold
    d = compress_range(-np.log10(np.clip(R, black, 1.0)), rng, p.copy_ink)
    t = ((1.0 - np.power(F(10.0), -d)) / (1.0 - solid)).astype(F)   # dot asked for: 0 paper, 1 solid
    del d
    t = printed_once(t, p.copy_drop, p.copy_fill)
    # the copy: its lens, and the old screen blurred away; then contrasty copy film
    t = blur(t.astype(F), p.copy_blur / 1000.0 * long_edge)
    vb = float(lin_to_srgb(np.float64(black)))
    v = (lin_to_srgb(1.0 - t * (1.0 - black)) - vb) / (1.0 - vb)    # tone as the eye sees it, 0 black to 1 white
    v = vb + s_curve(v, p.copy_gain) * (1.0 - vb)
    t = np.clip((1.0 - srgb_to_lin(v)) / (1.0 - black), 0.0, 1.0)
    # second printing
    t = printed_once(t, p.copy_drop, p.copy_fill)
    return (1.0 - t * (1.0 - black)).astype(F)


def house_tones(R, strength):
    """Pull a picture's tones toward the range the book's pages share. The book's makers
    copied every source to a common look: a pale picture gained contrast and a dark one was
    lightened. Here the picture's own percentiles are moved part of the way (`strength`, 0 to
    1) to the book's, along a smooth curve that keeps their order."""
    v = lin_to_srgb(np.clip(R, 0.0, 1.0))
    own = np.percentile(v[::3, ::3], HOUSE_PCT)
    want = (1.0 - strength) * own + strength * np.array(HOUSE_VAL)
    xs, ys = [0.0], [0.0]
    for x, y in zip(own, want):
        if x - xs[-1] > 0.01 and x < 0.99:               # skip percentiles that sit on the black floor or on white
            xs.append(float(x)); ys.append(float(max(y, ys[-1] + 1e-4)))
    xs.append(1.0); ys.append(1.0)
    return srgb_to_lin(np.clip(PchipInterpolator(xs, ys)(v), 0.0, 1.0)).astype(F)


def halftone_on_page(R, p, mm):
    """Print reflectance -> page reflectance relative to the paper, after the copy
    camera, the halftone screen, the press and the paper."""
    solid = 10.0 ** -p.ink_density
    R = blur(R, p.copy_blur_mm / mm)
    # The camera operator fits the print's long density range into the short range the
    # press can hold: highlights and mid-tones keep their contrast, shadows are compressed.
    d = -np.log10(np.clip(R, 1e-4, 1.0))
    d = compress_range(d, print_range(p.grade), p.ink_density)
    a = ((1.0 - np.power(F(10.0), -d)) / (1.0 - solid)).astype(F)    # ink coverage asked for
    del d
    a = blur(a, 0.25 * (25.4 / p.lpi) / mm)                          # one screen cell cannot hold finer detail
    if p.min_dot > 0:                                                # the smallest dots do not print
        a = np.where(a < p.min_dot, a * (a / p.min_dot) ** 2, a)
    a = np.clip(a + 4.0 * p.dot_gain * a * (1.0 - a), 0.0, 1.0)      # dots spread on the press and in the paper
    R = (1.0 - a * (1.0 - solid)).astype(F)
    return blur(R, p.paper_scatter_mm / mm), solid


# ---------- viewing ---------------------------------------------------------------------
def view(R, solid, p):
    """Reflectance -> display values. 'scan' sets levels the way the reference scans
    were made; 'page' shows true ink on cream paper; 'print' shows the darkroom print."""
    if p.view == "scan":
        e, ek = lin_to_srgb(R), float(lin_to_srgb(np.float64(solid)))
        out = p.scan_black + (e - ek) / (p.scan_white - ek) * (1.0 - p.scan_black)
        return np.clip(out, 0, 1)[..., None].repeat(3, -1)
    if p.view == "page":
        paper = srgb_to_lin(np.array(PAPER_CREAM))
        return lin_to_srgb(np.clip(R, 0, 1)[..., None] * paper)
    return lin_to_srgb(np.clip(R, 0, 1))[..., None].repeat(3, -1)


def page_size_mm(long_edge_px, p):
    """Long edge of the picture on the page. Unless set, the page is as large as the
    file prints at --ppi, so the press softens a large file no more than a small one."""
    return p.page_mm if p.page_mm else long_edge_px / p.ppi * 25.4


def render(rgb, p):
    long_edge = max(rgb.shape[:2])
    page_mm = page_size_mm(long_edge, p)
    # Small renders have pixels coarser than the page's finest blur. Work on a finer
    # grid in that case and average back, so small and large renders agree.
    k = 1
    if p.repro == "book":
        k = int(min(4, max(1, math.ceil(1.2 * (page_mm / long_edge) / p.paper_scatter_mm))))
    if k > 1:
        rgb = np.stack([np.asarray(Image.fromarray(rgb[..., c], mode="F").resize(
            (rgb.shape[1] * k, rgb.shape[0] * k), Image.BICUBIC), dtype=F) for c in range(3)], -1)
        rgb = np.clip(rgb, 0, None)
        long_edge *= k
    um = p.film_mm * 1000.0 / long_edge             # size of one pixel on the film
    mm = page_mm / long_edge                        # size of one pixel on the page
    rng = np.random.default_rng(p.seed)
    D, ref = expose_and_develop(rgb, p, um, rng)
    del rgb
    R = print_on_paper(D, ref, p, um)
    solid = 0.0
    if p.repro == "copy":
        R = copied_from_page(R, p, long_edge)
    if p.house > 0:
        R = house_tones(R, p.house)
    if p.repro == "book":
        R, solid = halftone_on_page(R, p, mm)
    if k > 1:
        h, w = R.shape
        R = R.reshape(h // k, k, w // k, k).mean(axis=(1, 3))
    return view(R, solid, p)


def save(out, path, p):
    out = np.clip(out, 0, 1)
    if p.out_long_edge and p.out_long_edge != max(out.shape[:2]):
        s = p.out_long_edge / max(out.shape[:2])
        size = (round(out.shape[1] * s), round(out.shape[0] * s))
        out = np.stack([np.asarray(Image.fromarray(out[..., c].astype(F), mode="F").resize(size, Image.LANCZOS))
                        for c in range(3)], -1)
    write_image(out, path, p.bits)
    print(f"wrote {path} {out.shape[1]}x{out.shape[0]}")


def write_image(out, path, bits=8):
    """Save display values in [0, 1], tagged as sRGB. Neutral pictures are stored as greyscale."""
    out = np.clip(out, 0, 1)
    neutral = out.ndim == 2 or (np.array_equal(out[..., 0], out[..., 1]) and np.array_equal(out[..., 1], out[..., 2]))
    grey = out if out.ndim == 2 else out[..., 1]
    tag = PngImagePlugin.PngInfo()
    tag.add(b"sRGB", bytes([0]))                     # tells viewers the values use the sRGB curve
    is_png = path.lower().endswith(".png")
    if bits == 16:
        Image.fromarray((grey * 65535 + 0.5).astype(np.uint16)).save(path, **({"pnginfo": tag} if is_png else {}))
    elif neutral and is_png:
        Image.fromarray((grey * 255 + 0.5).astype(np.uint8), "L").save(path, pnginfo=tag)
    else:
        icc = ImageCms.ImageCmsProfile(ImageCms.createProfile("sRGB")).tobytes()
        rgb = out if out.ndim == 3 else out[..., None].repeat(3, -1)
        kw = {"quality": 96, "subsampling": 0} if path.lower().endswith((".jpg", ".jpeg")) else {}
        Image.fromarray((rgb * 255 + 0.5).astype(np.uint8), "RGB").save(path, icc_profile=icc, **kw)


def parser():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input")
    ap.add_argument("-o", "--output", required=True)
    ap.add_argument("--detail-px", type=int, default=2560,
                   help="long edge of the grid the negative and print are simulated on. The result is then "
                        "enlarged to the output size, so grain and detail keep the same size relative to the "
                        "picture whatever the file's pixel count. Lower is coarser and softer; 0 simulates at "
                        "the input's own size")
    ap.add_argument("--out-long-edge", type=int, help="long edge of the saved file (default: the input's)")
    ap.add_argument("--bits", type=int, choices=(8, 16), default=8)
    ap.add_argument("--seed", type=int, default=1)
    g = ap.add_argument_group("negative")
    g.add_argument("--film", choices=sorted(FILMS), default="trix-1956",
                   help="trix-1956: Tri-X with the coarser grain and lower sharpness Kodak classed it at in "
                        "1956 (default, preferred in blind comparison; its two numbers are estimates). "
                        "trix: Kodak Tri-X Pan from its datasheet. doublex: Eastman Double-X 5222, an "
                        "emulsion of 1959")
    g.add_argument("--film-mm", type=float, default=24.0,
                   help="long edge of the piece of film the picture fills: 36 = a whole 35mm frame, 24 = a "
                        "moderate crop of one (default), 18 = half-frame, 56 = 6x4.5")
    g.add_argument("--filter", choices=sorted(FILM["rgb_weights"]), default="none", help="lens filter")
    g.add_argument("--lens-um", type=float, default=8.0,
                   help="softness of the picture on the film, as a blur radius in micrometres: lens, focus, "
                        "camera movement and losses from copying. Applied before development, so grain is "
                        "not softened. 0 is off; the reference scans, at the size of a book page, sit "
                        "between 10 and 30 [tuned]")
    g.add_argument("--lens-exponent", type=float, default=1.15,
                   help="shape of that softness. 2 is a Gaussian; lower values keep a sharp core and add a "
                        "long faint skirt, which lowers the contrast of middle-sized detail and keeps fine "
                        "edges. Published film sharpness curves sit near 1.15, the default")
    g.add_argument("--halation", type=float, default=0.0,
                   help="light reflected back from the film base as a wide glow around bright areas, as a "
                        "fraction of the light. 0 is off; 0.02 to 0.08 covers films with weak "
                        "anti-halation protection [tuned]")
    g.add_argument("--halation-um", type=float, default=65.0,
                   help="width of the first reflection on the film, micrometres (65 for still-film base)")
    g.add_argument("--ev", type=float, default=0.0, help="exposure in stops")
    g.add_argument("--auto-ev", type=float, nargs="?", const=0.45, default=None, metavar="TARGET")
    g.add_argument("--dev", default=None,
                   help="development time in minutes (Tri-X 6 to 11, default 7; Double-X 4 to 12, default 6; "
                        "longer = more contrast), or a period's recommended development: norm-1952, "
                        "norm-1956 or norm-1981. A longer development wants a softer --grade")
    g.add_argument("--grain", type=float, default=1.0, help="granularity relative to the film's datasheet figure")
    g.add_argument("--grain-dev", type=float, default=1.3,
                   help="how granularity grows with development: the power of the negative's contrast it "
                        "follows. 1.3 is the value measured on Double-X (default); 0 leaves it fixed")
    g.add_argument("--edge", type=float, default=1.0, help="edge-effect strength relative to the datasheet fit")
    g.add_argument("--edge-density", type=float, default=0.0,
                   help="how edge effects grow with the density of the negative. 0 uses the difference "
                        "across an edge alone (default, preferred in blind comparison); 1 follows Nelson's "
                        "measurement, in which the change is also proportional to the density")
    g.add_argument("--no-edge", action="store_true", help="switch development edge effects off")
    g = ap.add_argument_group("print (Kodak Polymax Fine-Art)")
    g.add_argument("--grade", type=float, default=3.0, help="paper grade, -1 to 5")
    g.add_argument("--print-ev", type=float, default=-0.5, help="print exposure in stops (more = darker)")
    g.add_argument("--print-grey", type=float, default=0.745,
                   help="print density a metered grey gets before --print-ev is applied (0.745 = 18%% grey)")
    g.add_argument("--enlarger-um", type=float, default=2.3, help="enlarger lens blur on the film, micrometres [tuned]")
    g.add_argument("--house", type=float, default=0.0,
                   help="pull the picture's tones toward the range the book's own pages share, from 0 (off) "
                        "to 1 (matched). The book's pages keep their shadows more open than a straight print")
    g = ap.add_argument_group("copied from a printed page (with --repro copy)")
    g.add_argument("--copy-blur", type=float, default=0.35,
                   help="softness of the copy, in thousandths of the picture's long edge. The reference "
                        "scans measure 0.4 to 1.25 at the size of a book page; the default is lower, for "
                        "viewing at full screen [tuned]")
    g.add_argument("--copy-gain", type=float, default=1.2, help="contrast gain of the copy film [tuned]")
    g.add_argument("--copy-drop", type=float, default=0.05,
                   help="highlight dot that fails to print at each printing (0.05 nominal, 0.15 on a small press)")
    g.add_argument("--copy-fill", type=float, default=0.95,
                   help="shadow tone that closes up into solid ink at each printing (0.95 nominal, 0.85 on a small press)")
    g.add_argument("--copy-ink", type=float, default=1.6,
                   help="solid ink density on the source page; lower values close up more of the shadows [tuned]")
    g = ap.add_argument_group("book page (offset halftone on uncoated paper)")
    g.add_argument("--repro", choices=("none", "copy", "book"), default="none",
                   help="none: finish at the darkroom print (default). copy: the print as it looks after "
                        "being printed, re-photographed from the page and printed again, which is how the "
                        "book's own pictures were made. book: run the 1979 printing press on uncoated paper")
    g.add_argument("--page-mm", type=float, default=None,
                   help="long edge of the picture on the page. Default: the size the file prints at --ppi. "
                        "The book's own full-page pictures are about 200")
    g.add_argument("--ppi", type=float, default=300.0, help="pixels per inch used to size the page when --page-mm is not given")
    g.add_argument("--lpi", type=float, default=133.0, help="halftone screen ruling, lines per inch")
    g.add_argument("--ink-density", type=float, default=1.20, help="solid black on the stock (1.20 uncoated cream, 1.85 coated)")
    g.add_argument("--dot-gain", type=float, default=0.08,
                   help="dot gain left uncorrected, as tone value increase at a 50%% dot. The press adds about "
                        "0.2 on uncoated paper; the halftone was shot to allow for most of it [tuned]")
    g.add_argument("--min-dot", type=float, default=0.03, help="smallest dot that prints [tuned]")
    g.add_argument("--copy-blur-mm", type=float, default=0.03, help="copy camera blur on the page [tuned]")
    g.add_argument("--paper-scatter-mm", type=float, default=0.06, help="light spread inside the paper")
    g = ap.add_argument_group("viewing")
    g.add_argument("--view", choices=("print", "scan", "white", "page"), default=None,
                   help="print: the darkroom print (default). With --repro book: scan = levels set like the "
                        "reference scans (default), white = true ink on white paper, page = true ink on cream paper")
    g.add_argument("--scan-black", type=float, default=0.07, help="display value given to solid ink [tuned]")
    g.add_argument("--scan-white", type=float, default=0.97, help="encoded page value sent to white [tuned]")
    return ap


def prepare(p):
    """Settle the settings that depend on one another: the film, its development, the view."""
    select_film(p.film)
    p.dev = resolve_dev(p.dev)
    if p.view is None:
        p.view = "scan" if p.repro == "book" else "print"
    return p


def main(argv=None):
    p = prepare(parser().parse_args(argv))
    with Image.open(p.input) as im:
        original = max(im.size)
    if p.out_long_edge is None:
        p.out_long_edge = original
    rgb = load_linear(p.input, p.detail_px or original)
    save(render(rgb, p), p.output, p)


if __name__ == "__main__":
    main()
