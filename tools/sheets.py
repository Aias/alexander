#!/usr/bin/env python3
"""Contact sheets sized for a 5K display (5120 x 2880). View them at 100%.

    python3 tools/sheets.py stages looks overview film full

The sheets were built around ten test photos, which are not part of this repository.
Put your own in photos/ and edit PHOTOS, the photo names inside each function and the
crop boxes to match.
"""
import contextlib
import io
import os
import sys
import time

import numpy as np
from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
sys.path.insert(0, ROOT)
import twb2

OUT = os.path.join(ROOT, "sheets")
os.makedirs(OUT, exist_ok=True)
FONT = ImageFont.load_default(size=30)
SMALL = ImageFont.load_default(size=22)
PHOTOS = ["courtyard", "citrus", "lake", "rooftops", "workshop", "barn", "shrub", "maple", "stucco", "sailboat"]
BOOK_PX = 1717          # a 200 mm page at the display's 218 pixels per inch


def render(name, work, quiet=True, **kw):
    args = [os.path.join(ROOT, "photos", name + ".jpg"), "-o", "x", "--auto-ev"]
    p = twb2.parser().parse_args(args)
    for k, v in kw.items():
        setattr(p, k, v)
    if p.view is None:
        p.view = "scan" if p.repro == "book" else "print"
    rgb = twb2.load_linear(p.input, work)
    with contextlib.redirect_stdout(io.StringIO()) if quiet else contextlib.nullcontext():
        return np.clip(twb2.render(rgb, p), 0, 1)


def to_img(a):
    return Image.fromarray((a * 255 + 0.5).astype(np.uint8), "RGB")


def compose(tiles, cols, title, path, note=""):
    """tiles: list of (label, PIL image). Writes a labelled grid."""
    tw, th = max(t.width for _, t in tiles), max(t.height for _, t in tiles)
    rows = (len(tiles) + cols - 1) // cols
    gap, top = 16, 96
    W, H = cols * tw + (cols + 1) * gap, top + rows * (th + 46 + gap)
    sheet = Image.new("RGB", (W, H), (255, 255, 255))
    d = ImageDraw.Draw(sheet)
    d.text((gap, 14), title, fill=(0, 0, 0), font=FONT)
    if note:
        d.text((gap, 54), note, fill=(90, 90, 90), font=SMALL)
    for i, (label, t) in enumerate(tiles):
        x, y = gap + (i % cols) * (tw + gap), top + (i // cols) * (th + 46 + gap)
        d.text((x, y), label, fill=(0, 0, 0), font=SMALL)
        sheet.paste(t, (x, y + 34))
    twb2.write_image(np.asarray(sheet, dtype=np.float32) / 255, path)
    print("wrote", path, sheet.size, f"{os.path.getsize(path) / 1e6:.1f} MB")


def crop(a, box):
    x, y, w, h = box
    return a[y:y + h, x:x + w]


def overview():
    tiles = []
    for n in PHOTOS:
        im = to_img(render(n, BOOK_PX))
        tiles.append((n, im.resize((1000, round(im.height * 1000 / im.width)), Image.LANCZOS)))
    compose(tiles, 5, "Darkroom print, default settings, all ten photos", os.path.join(OUT, "1_overview.png"),
            "Film, development, grain and print on paper. No printing press, no paper colour.")


LOOKS = [("A", "grade 4.25, print exposure 0", {"grade": 4.25, "print_ev": 0.0}),
         ("Between", "grade 3.6, print exposure -0.25", {"grade": 3.6, "print_ev": -0.25}),
         ("B", "grade 3, print exposure -0.5 (current default)", {"grade": 3.0, "print_ev": -0.5})]


def looks():
    """The two tone settings the user picked, and the midpoint, on five photos."""
    tiles = []
    for tag, desc, kw in LOOKS:
        for n in ("courtyard", "rooftops", "sailboat", "workshop", "maple"):
            im = to_img(render(n, BOOK_PX, **kw))
            tiles.append((f"{tag}: {desc}", im.resize((1000, round(im.height * 1000 / im.width)), Image.LANCZOS)))
    compose(tiles, 5, "Your two tone picks as darkroom prints, and the midpoint between them",
            os.path.join(OUT, "7_two_looks.png"),
            "Top row matches your 'grade 4, +0.0' tile. Bottom row matches your 'grade 3, -0.5' tile. Press step off.")


CROP = (1990, 640, 1680, 920)      # rooftops at 5120 px: sky, towers and roofs


def film():
    v = [("35mm Tri-X, normal development (the default)", {}),
         ("Half-frame 18 mm: the same film enlarged twice as far", {"film_mm": 18.0}),
         ("Medium format 56 mm", {"film_mm": 56.0}),
         ("35mm, development edge effects off", {"no_edge": True}),
         ("35mm, edge effects doubled", {"edge": 2.0}),
         ("35mm, underexposed 1.5 stops and developed 11 minutes (pushed)", {"ev": -1.5, "dev": 11.0})]
    tiles = []
    for label, kw in v:
        t = time.time()
        a = render("rooftops", 5120, **kw)
        tiles.append((label, to_img(crop(a, CROP))))
        print(f"  {label}: {time.time() - t:.0f} s")
    compose(tiles, 3, "The negative and the darkroom print: no book page yet. Pixel for pixel at 5120 px wide.",
            os.path.join(OUT, "2_film.png"),
            "Source photo is 2000 px enlarged to 5120, so fine subject detail is softer than a 48 MP original would give.")


def page():
    v = [("Darkroom print (no book page)", {"repro": "none"}),
         ("Book page, picture 200 mm long (the book's full-page size; the default)", {}),
         ("Book page, picture 100 mm long", {"page_mm": 100.0}),
         ("Book page, picture 400 mm long (a large plate)", {"page_mm": 400.0}),
         ("Book page 200 mm, full press dot gain left uncorrected (0.20)", {"dot_gain": 0.20}),
         ("Book page 200 mm, shown as true ink on cream paper", {"view": "page"})]
    tiles = []
    for label, kw in v:
        t = time.time()
        a = render("rooftops", 5120, **kw)
        tiles.append((label, to_img(crop(a, CROP))))
        print(f"  {label}: {time.time() - t:.0f} s")
    compose(tiles, 3, "From darkroom print to book page. Pixel for pixel at 5120 px wide.",
            os.path.join(OUT, "3_page.png"),
            "The page stages are fixed in millimetres, so a larger reproduction keeps more detail.")


def tone():
    tiles = []
    for grade in (2.0, 3.0, 4.0):
        for pev in (-0.5, 0.0, 0.5):
            a = render("courtyard", BOOK_PX, grade=grade, print_ev=pev)
            im = to_img(a)
            mark = "  (default)" if (grade, pev) == (3.0, 0.0) else ""
            tiles.append((f"Paper grade {grade:.0f}, print exposure {pev:+.1f} stop{mark}",
                          im.resize((1000, round(im.height * 1000 / im.width)), Image.LANCZOS)))
    compose(tiles, 3, "Print contrast and exposure, book look", os.path.join(OUT, "4_tone.png"),
            "Rows: paper grade 2, 3, 4. Columns: lighter, normal, darker print.")


def full():
    for n, kw, tag in (("rooftops", {}, "print"), ("sailboat", {}, "print"), ("courtyard", {}, "print"),
                       ("workshop", {}, "print")):
        t = time.time()
        a = render(n, 5120, **kw)
        path = os.path.join(OUT, f"full_{n}_{tag}_5120.png")
        twb2.write_image(a, path)
        print("wrote", path, a.shape[1], "x", a.shape[0], f"{os.path.getsize(path) / 1e6:.1f} MB, {time.time() - t:.0f} s")


def stages():
    """One photo after each step of the chain, each step added to the ones before."""
    import copy
    name, work = "rooftops", 5120
    p = twb2.parser().parse_args([os.path.join(ROOT, "photos", name + ".jpg"), "-o", "x", "--auto-ev"])
    rgb = twb2.load_linear(p.input, work)
    long_edge = max(rgb.shape[:2])
    um = p.film_mm * 1000.0 / long_edge
    mm = twb2.page_size_mm(long_edge, p) / long_edge
    grey = lambda a: np.clip(a, 0, 1)[..., None].repeat(3, -1)
    quiet = lambda: contextlib.redirect_stdout(io.StringIO())

    def negative(**kw):
        q = copy.copy(p)
        for k, v in kw.items():
            setattr(q, k, v)
        with quiet():
            D, ref = twb2.expose_and_develop(rgb, q, um, np.random.default_rng(1))
        return twb2.print_on_paper(D, ref, q, um)

    with quiet():
        lum = twb2.film_exposure(rgb, p, weights={"r": 0.2126, "g": 0.7152, "b": 0.0722})
        film = twb2.film_exposure(rgb, p)
    R6 = negative()
    with quiet():
        R7, solid = twb2.halftone_on_page(R6, p, mm)
    p.view = "page"
    out = [
        ("1  Your photo", np.clip(twb2.lin_to_srgb(rgb), 0, 1)),
        ("2  Plain black-and-white, exposure set automatically", grey(twb2.lin_to_srgb(lum))),
        ("3  + the film's colour response (Tri-X sees blue strongly)", grey(twb2.lin_to_srgb(film))),
        ("4  + film curve and paper curve (negative printed on grade 3 paper)", grey(twb2.lin_to_srgb(negative(no_edge=True, grain=0.0)))),
        ("5  + development edge effects", grey(twb2.lin_to_srgb(negative(grain=0.0)))),
        ("6  + film grain  = the darkroom print, the default finish", grey(twb2.lin_to_srgb(R6))),
        ("7  optional: + printing press (weak ink, tone squeeze, dot gain, softness)", grey(twb2.lin_to_srgb(R7))),
        ("8  optional: + cream paper", np.clip(twb2.view(R7, solid, p), 0, 1)),
    ]
    print("  average change each step makes to the picture (display values, 0 to 1):")
    lumi = lambda a: 0.2126 * a[..., 0] + 0.7152 * a[..., 1] + 0.0722 * a[..., 2]
    for (l0, a0), (l1, a1) in zip(out[1:], out[2:]):
        d = np.abs(lumi(a1) - lumi(a0))
        print(f"    step {l1.split()[0]}: mean {d.mean():.3f}, largest 1% above {np.percentile(d, 99):.3f}")
    frames = [(l, to_img(a).resize((1250, round(a.shape[0] * 1250 / a.shape[1])), Image.LANCZOS)) for l, a in out]
    compose(frames, 4, "The chain one step at a time. Each picture includes every step before it.",
            os.path.join(OUT, "5_steps_whole.png"), "Whole frame, reduced. The default finish is number 6. Numbers 7 and 8 are optional extras.")
    box = (2250, 800, 1250, 800)
    compose([(l, to_img(crop(a, box))) for l, a in out], 4,
            "The chain one step at a time, pixel for pixel. Each picture includes every step before it.",
            os.path.join(OUT, "6_steps_detail.png"),
            "A 1250 x 800 window of a 5120 px render. Steps 5 and 6 only show at this magnification.")


if __name__ == "__main__":
    for job in sys.argv[1:]:
        {"overview": overview, "film": film, "page": page, "tone": tone, "full": full, "stages": stages, "looks": looks}[job]()
