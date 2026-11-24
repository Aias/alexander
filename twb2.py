#!/usr/bin/env python3
"""twb2: the look of the photographs in "The Timeless Way of Building", built as the
physical chain those pictures went through, in physical units.

    scene -> Tri-X negative (curve, edge effects, grain) -> print on paper
          -> halftone on uncoated stock -> scan or page view

Every length is in micrometres on the film or millimetres on the printed page, so the
same settings give the same picture at 1,000 px and at 48 MP.

    python3 twb2.py photo.jpg -o out.png --auto-ev                              # darkroom print
    python3 twb2.py photo.jpg -o out.png --auto-ev --repro book                 # + printing press
    python3 twb2.py photo.jpg -o out.png --auto-ev --repro book --view page     # + cream paper

Sourced numbers (README.md lists where each comes from):
  film curve, colour response, granularity 17, sharpness curve  Kodak Tri-X 400 datasheet
  paper curves by grade                                          Kodak Polymax Fine-Art datasheet
  grain statistics, granularity -> grain size                    spektrafilm (dev branch), re-implemented
  edge-effect mechanism; strength and size                       spektrafilm / fitted to the Tri-X curve
  ink density 1.20, dot gain about 0.2 on uncoated paper         German offset process standard
Tuned, not sourced: enlarger and copy-lens blur, smallest printable dot, scan levels.
"""
import argparse
import json
import math
import os

import numpy as np
from PIL import Image, ImageCms, ImageOps, PngImagePlugin
from scipy import ndimage as ndi

F = np.float32
HERE = os.path.dirname(os.path.abspath(__file__))
TRIX = json.load(open(os.path.join(HERE, "data", "trix400.json")))
FIT = json.load(open(os.path.join(HERE, "data", "trix400_mtf_fit.json")))
POLYMAX = json.load(open(os.path.join(HERE, "data", "polymax.json")))

LOGH_GREY = -1.75            # log10 lux-seconds given to an 18% grey by a meter at ISO 400
APERTURE_UM2 = math.pi * 24.0 ** 2   # the 48 um aperture granularity is quoted for
FILM_DMAX = 3.4              # net density the emulsion can reach (top of the datasheet's granularity plot)
GRAIN_UNIFORMITY = 0.6       # black-and-white value used by spektrafilm
PAPER_CREAM = (0.957, 0.937, 0.882)   # display colour of the uncoated stock [tuned]


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


# ---------- the negative ----------------------------------------------------------
def film_curve(dev_min):
    """Tri-X characteristic curve at a development time (minutes), as arrays of log
    exposure and density, interpolated between the datasheet's 6, 7, 9 and 11 minutes."""
    x = np.arange(-4.5, 2.0001, 0.01)
    table = {}
    for t, pts in TRIX["curves_logH_density"].items():
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


def grain_area_um2(rms_granularity):
    """Area of one developed grain that reproduces a catalogue granularity figure
    (standard deviation of density x 1000 through a 48 um aperture at net density 1.0)."""
    sigma = rms_granularity / 1000.0
    return sigma ** 2 * APERTURE_UM2 / (1.0 * (FILM_DMAX - GRAIN_UNIFORMITY * 1.0))


def add_grain(D, um, scale, rng):
    """Each pixel holds N grains; the number developed is Poisson. That gives the
    variance D (Dmax - u D) / N behind the catalogue figure, and its skew at small N."""
    N = (um * um) / grain_area_um2(TRIX["rms_granularity"] * scale)
    prob = np.clip(D / FILM_DMAX, 0.0, 0.98)
    sat = 1.0 - GRAIN_UNIFORMITY * prob
    return (rng.poisson(N * prob / sat).astype(F) * (FILM_DMAX / N) * sat).astype(F)


def film_exposure(rgb, p, weights=None):
    """Linear RGB -> the exposure the film receives, scaled so a metered grey is 0.18.
    `weights` overrides the film's colour response (used to show a plain conversion)."""
    w = weights or TRIX["rgb_weights"][p.filter]
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
    E = blur(E, FIT["optical_sigma_um"][0] / um)                    # light scatter in the emulsion
    logH = (np.log10(np.maximum(E, 1e-6) / 0.18) + LOGH_GREY).astype(F)
    del E

    cx, cy = film_curve(p.dev)
    base = float(cy.min())
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
        logH += (k * (D - spread)).astype(F)
        del spread
        D = (np.interp(logH, cx, cy) - base).astype(F)
    del logH

    if p.grain > 0:
        D = add_grain(D, um, p.grain, rng)

    ref = float(np.interp(LOGH_GREY, cx, cy) - base)                # density of a metered grey
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
    ap.add_argument("--work-long-edge", type=int, help="resample the input to this size before simulating")
    ap.add_argument("--out-long-edge", type=int, help="resample the result to this size when saving")
    ap.add_argument("--bits", type=int, choices=(8, 16), default=8)
    ap.add_argument("--seed", type=int, default=1)
    g = ap.add_argument_group("negative (Kodak Tri-X 400)")
    g.add_argument("--film-mm", type=float, default=36.0, help="long edge of the film frame: 36 = 35mm, 56 = 6x4.5, 120 = 4x5")
    g.add_argument("--filter", choices=sorted(TRIX["rgb_weights"]), default="none", help="lens filter")
    g.add_argument("--ev", type=float, default=0.0, help="exposure in stops")
    g.add_argument("--auto-ev", type=float, nargs="?", const=0.45, default=None, metavar="TARGET")
    g.add_argument("--dev", type=float, default=7.0, help="development time in minutes, 6 to 11 (longer = more contrast)")
    g.add_argument("--grain", type=float, default=1.0, help="granularity relative to the datasheet's 17")
    g.add_argument("--edge", type=float, default=1.0, help="edge-effect strength relative to the datasheet fit")
    g.add_argument("--no-edge", action="store_true", help="switch development edge effects off")
    g = ap.add_argument_group("print (Kodak Polymax Fine-Art)")
    g.add_argument("--grade", type=float, default=3.0, help="paper grade, -1 to 5")
    g.add_argument("--print-ev", type=float, default=-0.5, help="print exposure in stops (more = darker)")
    g.add_argument("--print-grey", type=float, default=0.745,
                   help="print density a metered grey gets before --print-ev is applied (0.745 = 18%% grey)")
    g.add_argument("--enlarger-um", type=float, default=2.3, help="enlarger lens blur on the film, micrometres [tuned]")
    g = ap.add_argument_group("book page (offset halftone on uncoated paper)")
    g.add_argument("--repro", choices=("none", "book"), default="none",
                   help="none: finish at the darkroom print (default). book: also run the printing press")
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


def main(argv=None):
    p = parser().parse_args(argv)
    if p.view is None:
        p.view = "scan" if p.repro == "book" else "print"
    rgb = load_linear(p.input, p.work_long_edge)
    save(render(rgb, p), p.output, p)


if __name__ == "__main__":
    main()
