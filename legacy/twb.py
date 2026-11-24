#!/usr/bin/env python3
"""Emulate the reproduction chain behind the photographs in
"The Timeless Way of Building": camera negative -> copy print ->
halftone on uncoated paper -> page scan.

Every stage is a knob. Stages marked [LR] have a direct Lightroom
equivalent; stages marked [x] do not and exist to test fidelity.

    python3 twb.py photo.jpg -o out.png
    python3 twb.py photo.jpg -o out.png --no-screen      # Lightroom-only subset
"""
import argparse
import math

import numpy as np
from PIL import Image
from scipy import ndimage as ndi

F = np.float32


# ---------- helpers ----------------------------------------------------------
def srgb_to_lin(x):
    return np.where(x <= 0.04045, x / 12.92, ((x + 0.055) / 1.055) ** 2.4)


def lin_to_srgb(x):
    x = np.clip(x, 0, None)
    return np.where(x <= 0.0031308, x * 12.92, 1.055 * x ** (1 / 2.4) - 0.055)


def resize(a, size, resample=Image.LANCZOS):
    """Resize a float array (H,W) to size=(w,h)."""
    return np.asarray(Image.fromarray(a.astype(F), mode="F").resize(size, resample), dtype=F)


def load_rgb(path, crop=None):
    im = Image.open(path).convert("RGB")
    if crop:
        cw, ch = (float(v) for v in crop.split(":"))
        w, h = im.size
        tw, th = (w, round(w * ch / cw)) if w * ch / cw <= h else (round(h * cw / ch), h)
        l, t = (w - tw) // 2, (h - th) // 2
        im = im.crop((l, t, l + tw, t + th))
    return np.asarray(im, dtype=F) / 255


# ---------- stage 1: the "negative" ------------------------------------------
MIX_HUES = np.array([0, 30, 60, 120, 180, 240, 270, 300, 360]) / 360


def to_mono(rgb, mix, blue_sens=0.0):
    """[LR] B&W conversion with a hue mixer (red,orange,yellow,green,aqua,blue,purple,magenta).

    blue_sens shifts the film's colour response toward blue, as with older emulsions
    shot without a yellow filter: 0 = modern luminance, 1 = strongly blue-sensitive.
    Blue skies go pale, reds and foliage go darker."""
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    wr, wg, wb = (np.array([0.299, 0.587, 0.114]) * (1 - blue_sens) + np.array([0.18, 0.32, 0.50]) * blue_sens)
    y = wr * r + wg * g + wb * b
    mx, mn = rgb.max(-1), rgb.min(-1)
    c = mx - mn
    cs = np.where(c > 1e-6, c, 1)
    h = np.select([mx == r, mx == g], [((g - b) / cs) % 6, (b - r) / cs + 2], (r - g) / cs + 4) / 6
    w = np.interp(h, MIX_HUES, list(mix) + [mix[0]])
    return np.clip(y + 0.6 * c * w / 100, 0, 1).astype(F)


def exposure(x, ev):
    """[LR] Exposure in stops, applied in linear light, with a soft shoulder so
    pushed highlights roll off the way film and paper do instead of clipping hard."""
    if ev == 0:
        return x
    lin = srgb_to_lin(x) * 2.0 ** ev
    knee = 0.7
    lin = np.where(lin > knee, knee + (1 - knee) * np.tanh((lin - knee) / (1 - knee)), lin)
    return np.clip(lin_to_srgb(lin), 0, 1).astype(F)


def auto_exposure(x, target):
    """Stops of exposure a printer might choose: halfway between putting the median
    tone on `target` and putting the brightest 2% just under paper white.
    Limited to -1..+2.5 stops."""
    lin = lambda v: float(srgb_to_lin(np.float64(v)))
    med, top = float(np.median(x)), float(np.percentile(x, 98))
    ev_mid = math.log2(lin(target) / max(lin(med), 1e-4))
    ev_top = math.log2(lin(0.90) / max(lin(top), 1e-4))
    return float(np.clip((ev_mid + ev_top) / 2, -1.0, 2.5)), med, top


def grain(x, amount, size, seed):
    """[LR] Film grain added before the contrast stage, so the curve shapes it."""
    if amount <= 0:
        return x
    n = np.random.default_rng(seed).standard_normal(x.shape).astype(F)
    if size > 0:
        n = ndi.gaussian_filter(n, size)
    return x + amount * n / n.std()


# ---------- stage 2: the copy print ------------------------------------------
def tone(x, bp, wp, contrast, pivot):
    """[LR] Levels clip, then an S-curve. Stands in for Contrast/Whites/Blacks/Shadows."""
    x = np.clip((x - bp) / (wp - bp), 0, 1)
    if pivot != 0.5:
        x = x ** (math.log(0.5) / math.log(pivot))
    xg = x ** contrast
    return (xg / (xg + (1 - x) ** contrast + 1e-9)).astype(F)


def clarity(x, amount, radius):
    """[LR] Local contrast: wide-radius unsharp mask."""
    if amount == 0:
        return x
    return np.clip(x + amount * (x - ndi.gaussian_filter(x, radius)), 0, 1)


def vignette(x, amount):
    """[LR] Darken corners."""
    if amount == 0:
        return x
    h, w = x.shape
    yy, xx = np.mgrid[0:h, 0:w].astype(F)
    r2 = ((xx - w / 2) / (w / 2)) ** 2 + ((yy - h / 2) / (h / 2)) ** 2
    return x * (1 - amount * r2 / 2)


# ---------- stage 3: halftone on paper, then the page scan --------------------
def halftone_scan(x, period, o, neutral=True, scan_blur=None):
    """[x] Screen the image into dots, spread the ink, scan it, shrink to file size.

    period        dot spacing in output pixels
    o.dot_gain    how much dots grow on absorbent paper (0 = none, 0.1 = heavy)
    o.ink_blur    softness of dot edges, as a fraction of the dot period
    o.bitonal     scan as pure black/white (how most circulating PDFs were made)
    o.skew        page rotation on the scanner glass, degrees
    o.warp        slow drift of the dot lattice (paper stretch, page curl), in dot periods
    o.dot_noise   ragged dots from paper fibre and uneven inking
    o.scan_aa     scanner optics: low-pass before sampling, in output px. 0 lets the
                  dot lattice alias against the pixel grid (hard rectangular moire)
    neutral       pre-compensate so this stage adds texture and clips the extremes
                  but leaves the mid-tones where the tone stage put them
    """
    scan_blur = o.scan_blur if scan_blur is None else scan_blur
    if neutral:
        # measure this stage's own tone response on a grey chart, then invert it
        levels = np.linspace(0, 1, 41, dtype=F)
        chart = np.repeat(np.repeat(levels[None, :], 48, 0), 48, 1)
        got = halftone_scan(chart, period, o, neutral=False, scan_blur=0)
        resp = np.maximum.accumulate(got.reshape(48, 41, 48)[8:-8, :, 8:-8].mean(axis=(0, 2)))
        x = np.interp(x, resp + np.arange(41) * 1e-6, levels).astype(F)
    h, w = x.shape
    u = int(min(8, max(2, math.ceil(6 / period))))  # oversampling factor
    big = resize(x, (w * u, h * u), Image.BICUBIC)
    P = period * u
    rng = np.random.default_rng(o.seed + 7)
    th = math.radians(o.angle + o.skew)
    yy = np.arange(h * u, dtype=F)[:, None]
    xx = np.arange(w * u, dtype=F)[None, :]
    a = (xx * math.cos(th) + yy * math.sin(th)) / P
    b = (-xx * math.sin(th) + yy * math.cos(th)) / P
    if o.warp > 0:
        for arr in (a, b):
            n = ndi.gaussian_filter(rng.standard_normal((h, w), dtype=F), o.warp_scale)
            arr += o.warp * resize(n / n.std(), (w * u, h * u), Image.BICUBIC)
    spot = (np.cos(2 * np.pi * a) + np.cos(2 * np.pi * b)) / 2
    del a, b
    # rank-transform the spot function so ink coverage tracks tone linearly
    g = (np.arange(256, dtype=F) + 0.5) / 256
    ref = np.sort(((np.cos(2 * np.pi * g)[:, None] + np.cos(2 * np.pi * g)[None, :]) / 2).ravel())
    rank = np.searchsorted(ref, spot.ravel()).reshape(spot.shape).astype(F) / ref.size
    del spot
    if o.dot_noise > 0:
        n = ndi.gaussian_filter(rng.standard_normal(rank.shape, dtype=F), 0.3 * P)
        rank += o.dot_noise * n / n.std()
    ink = (rank + (1 - big) > 1).astype(F)          # 1 = ink
    del rank, big
    ink = ndi.gaussian_filter(ink, o.ink_blur * P)  # ink spread
    if o.bitonal:
        ink = (ink > 0.5 - o.dot_gain).astype(F)
    else:
        ink = np.clip((ink - (0.5 - o.dot_gain)) * 4 + 0.5, 0, 1)
    if o.scan_aa > 0:
        ink = ndi.gaussian_filter(ink, o.scan_aa * u)
    cov = ink.reshape(h, u, w, u).mean(axis=(1, 3))  # sample back to file size
    out = 1 - cov
    if scan_blur > 0:
        out = ndi.gaussian_filter(out, scan_blur)
    return out.astype(F)


def scan_artifacts(x, mottle, mottle_size, banding, band_period, seed):
    """[x] Blotchy unevenness and horizontal moire bands, confined to the greys."""
    if mottle <= 0 and banding <= 0:
        return x
    w = 4 * x * (1 - x)                              # zero in pure black and white
    if mottle > 0:
        n = ndi.gaussian_filter(np.random.default_rng(seed + 1).standard_normal(x.shape).astype(F), mottle_size)
        x = x + mottle * w * n / n.std()
    if banding > 0:
        yy = np.arange(x.shape[0], dtype=F)[:, None]
        x = x + banding * w * np.sin(2 * np.pi * yy / band_period)
    return np.clip(x, 0, 1)


# ---------- stage 4: the grain as it appears in the scans ----------------------
# Radial power spectrum measured on flat patches of the reference scans (porch sky,
# barbershop wall). It falls steadily from large soft clouds to fine specks, with no
# preferred blob size, which is what keeps it from looking regular.
GRAIN_F = np.array([0.004, 0.04, 0.09, 0.16, 0.25, 0.35, 0.45, 0.71])
GRAIN_PSD = np.array([1.60, 1.60, 0.25, 0.105, 0.080, 0.030, 0.009, 0.003])
GRAIN_SPLIT = 0.12   # c/px: below this is soft cloudiness, above it is fine grain


def book_grain(x, amount, speckle, clouds, seed):
    """[~LR] Grain synthesised to match the reference scans: their measured spectrum,
    fine grain with heavier-than-Gaussian tails (occasional strong specks), and a
    strength that peaks in the mid-greys and fades toward solid black and paper white.

    amount   strength in the mid-greys (the scans measure about 0.03)
    speckle  0 = smooth Gaussian fine grain, 1 = the scans' speckiness
    clouds   weight of the large soft blotches relative to the fine grain (1 = as measured)
    """
    if amount <= 0:
        return x
    h, w = x.shape
    rng = np.random.default_rng(seed + 3)
    spec = np.fft.rfft2(rng.standard_normal((h, w)))
    f = np.hypot(np.fft.fftfreq(h)[:, None], np.fft.rfftfreq(w)[None, :])
    psd = np.exp(np.interp(np.log(np.maximum(f, 1e-4)), np.log(GRAIN_F), np.log(GRAIN_PSD)))
    psd[0, 0] = 0
    low = np.fft.irfft2(spec * np.sqrt(np.where(f < GRAIN_SPLIT, psd * clouds, 0)), s=(h, w))
    high = np.fft.irfft2(spec * np.sqrt(np.where(f >= GRAIN_SPLIT, psd, 0)), s=(h, w))
    sd_high = high.std()
    z = high / sd_high
    if speckle > 0:
        z = z - 0.03 * speckle * (z * z - 1)        # lean slightly toward dark specks
        z = z * (1 + 0.09 * speckle * z * z)        # heavier tails: occasional strong specks
        z /= z.std()
    n = low + z * sd_high
    n /= n.std()
    weight = 0.15 + 0.85 * 4 * x * (1 - x)          # strongest in the mid-greys
    return np.clip(x + amount * weight * n.astype(F), 0, 1)


# ---------- pipeline ----------------------------------------------------------
def process(rgb, p):
    x = to_mono(rgb, p.mix, p.blue_sens)
    ev = p.ev
    if p.auto_ev is not None:
        auto, med, top = auto_exposure(x, p.auto_ev)
        ev += auto
        print(f"auto exposure {auto:+.2f} EV (median {med:.2f}, brightest 2% at {top:.2f})")
    x = exposure(x, ev)
    h, w = x.shape
    s = p.long_edge / max(h, w)
    x = resize(x, (round(w * s), round(h * s)))
    if p.soften > 0:
        x = ndi.gaussian_filter(x, p.soften)
    x = grain(x, p.grain, p.grain_size, p.seed)
    x = tone(x, p.bp, p.wp, p.contrast, p.pivot)
    x = clarity(x, p.clarity, 0.025 * p.long_edge)
    x = vignette(x, p.vignette)
    screen = p.screen
    if screen > 0:
        period = p.long_edge / screen                 # dot spacing in output px
        x = halftone_scan(x, period, p, neutral=not p.raw_screen)
        x = scan_artifacts(x, p.mottle, p.mottle_size, p.banding, p.band_period, p.seed)
    x = book_grain(np.clip(x, 0, 1), p.book_grain, p.speckle, p.clouds, p.seed)
    x = p.floor + (p.ceil - p.floor) * np.clip(x, 0, 1)
    return x


def to_image(x, paper):
    if not paper:
        return Image.fromarray((np.clip(x, 0, 1) * 255 + 0.5).astype(np.uint8), "L")
    ink, sheet = np.array([28, 25, 22], F), np.array([247, 241, 226], F)
    rgb = ink + (sheet - ink) * np.clip(x, 0, 1)[..., None]
    return Image.fromarray((rgb + 0.5).astype(np.uint8), "RGB")


def parser():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("input")
    ap.add_argument("-o", "--output", required=True)
    ap.add_argument("--long-edge", type=int, default=900, help="output size in px (default 900)")
    ap.add_argument("--crop", help="centre-crop to an aspect ratio first, e.g. 2:3")
    g = ap.add_argument_group("negative")
    g.add_argument("--mix", type=lambda s: [float(v) for v in s.split(",")],
                   default=[-10, 0, 0, -10, 20, 30, 0, 0], metavar="R,O,Y,G,A,B,P,M",
                   help="B&W mixer, -100..100 per hue")
    g.add_argument("--blue-sens", type=float, default=0.0,
                   help="0..1, shift the film response toward blue: pale skies, darker reds and foliage")
    g.add_argument("--ev", type=float, default=0.0, help="exposure in stops")
    g.add_argument("--auto-ev", type=float, nargs="?", const=0.45, default=None, metavar="TARGET",
                   help="pick exposure from the median tone (aimed at TARGET, default 0.45) and the "
                        "brightest 2%% (aimed just under white); --ev adds to it")
    g.add_argument("--soften", type=float, default=0.45, help="blur sigma in output px")
    g.add_argument("--grain", type=float, default=0.0, help="extra film grain added before the contrast stage (0-0.2)")
    g.add_argument("--grain-size", type=float, default=0.45, help="grain clump sigma in output px")
    g.add_argument("--seed", type=int, default=1)
    g = ap.add_argument_group("copy print")
    g.add_argument("--bp", type=float, default=0.04, help="input level clipped to black")
    g.add_argument("--wp", type=float, default=0.92, help="input level clipped to white")
    g.add_argument("--contrast", type=float, default=1.2, help="S-curve strength, 1 = none")
    g.add_argument("--pivot", type=float, default=0.5, help="tone that stays mid-grey")
    g.add_argument("--clarity", type=float, default=0.25)
    g.add_argument("--vignette", type=float, default=0.10)
    g = ap.add_argument_group("halftone + scan")
    g.add_argument("--screen", type=float, default=0,
                   help="halftone lines along the long edge. Off by default: it leaves a faint regular "
                        "ripple that the scans mostly lack. Try 0.9 x --long-edge for visible dots, or a "
                        "value close to --long-edge for the diamond moire seen in the porch scan")
    g.add_argument("--no-screen", dest="screen", action="store_const", const=0,
                   help="skip the halftone and scan stages (Lightroom-only subset)")
    g.add_argument("--angle", type=float, default=45)
    g.add_argument("--raw-screen", action="store_true",
                   help="let dot gain darken the mid-tones instead of compensating for it")
    g.add_argument("--dot-gain", type=float, default=0.03)
    g.add_argument("--ink-blur", type=float, default=0.18)
    g.add_argument("--grey-scan", dest="bitonal", action="store_false", help="scan in greyscale instead of 1-bit")
    g.add_argument("--skew", type=float, default=1.5, help="page rotation on the scanner, degrees")
    g.add_argument("--warp", type=float, default=0.5, help="slow drift of the dot lattice, in dot periods")
    g.add_argument("--warp-scale", type=float, default=40.0, help="distance over which the lattice drifts, px")
    g.add_argument("--dot-noise", type=float, default=0.0, help="ragged dots from paper fibre and inking")
    g.add_argument("--scan-aa", type=float, default=0.55,
                   help="scanner low-pass before sampling, output px; 0 gives hard rectangular moire")
    g.add_argument("--scan-blur", type=float, default=0.0, help="softness of the scan in output px")
    g.add_argument("--mottle", type=float, default=0.0, help="blotchy unevenness in the greys")
    g.add_argument("--mottle-size", type=float, default=6.0, help="blotch size in output px")
    g.add_argument("--banding", type=float, default=0.0, help="horizontal moire bands (reference 1 has about 0.02)")
    g.add_argument("--band-period", type=float, default=36.0, help="band spacing in output px")
    g = ap.add_argument_group("grain matched to the scans")
    g.add_argument("--book-grain", type=float, default=0.038, help="strength in the mid-greys (scans measure ~0.03)")
    g.add_argument("--speckle", type=float, default=0.3, help="0 = smooth, 1 = as specky as the scans")
    g.add_argument("--clouds", type=float, default=0.0,
                   help="large soft blotches. Off by default: they were measured from scan patches that "
                        "contain real shading and moire, and read as artificial in smooth skies")
    g = ap.add_argument_group("output")
    g.add_argument("--floor", type=float, default=0.07, help="darkest output value")
    g.add_argument("--ceil", type=float, default=1.0, help="brightest output value")
    g.add_argument("--paper", action="store_true", help="tone as warm ink on cream paper")
    return ap


if __name__ == "__main__":
    p = parser().parse_args()
    out = process(load_rgb(p.input, p.crop), p)
    to_image(out, p.paper).save(p.output)
    print(f"wrote {p.output} {out.shape[1]}x{out.shape[0]}")
