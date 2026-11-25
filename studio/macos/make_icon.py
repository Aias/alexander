"""Generate studio/macos/icon.png, the 1024 px source of the app icon.

The icon is drawn from code so the repository carries no third-party artwork
and anyone can regenerate it bit for bit: the noise uses a fixed seed.

    python3 studio/macos/make_icon.py

Requires numpy and Pillow.
"""

from pathlib import Path

import numpy as np
from PIL import Image

SIZE = 1024
# Apple's icon grid: the tile is 824 px inside the 1024 canvas, the rest is
# left transparent so the Dock's shadow and spacing match other apps.
TILE = 824
RADIUS = 185
SEED = 5222


def rounded_mask(size: int, tile: int, radius: int) -> np.ndarray:
    """Antialiased coverage of a rounded square centred in the canvas."""
    c = (size - 1) / 2
    half = tile / 2
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    # Signed distance to a rounded box, so the edge can be antialiased.
    qx = np.abs(x - c) - (half - radius)
    qy = np.abs(y - c) - (half - radius)
    outside = np.hypot(np.maximum(qx, 0), np.maximum(qy, 0))
    inside = np.minimum(np.maximum(qx, qy), 0)
    dist = outside + inside - radius
    return np.clip(0.5 - dist, 0.0, 1.0)


def grain(rng: np.random.Generator, size: int) -> np.ndarray:
    """Clumped noise: fine noise plus a coarser layer, like silver grain."""
    fine = rng.standard_normal((size, size))
    coarse = rng.standard_normal((size // 2, size // 2))
    coarse = np.kron(coarse, np.ones((2, 2)))[:size, :size]
    g = 0.65 * fine + 0.35 * coarse
    return g / g.std()


def main() -> None:
    rng = np.random.default_rng(SEED)
    y, x = np.mgrid[0:SIZE, 0:SIZE].astype(np.float64) / (SIZE - 1)

    # A dark print: deep shadows with a faint light falling from the upper
    # left, the kind of low-key tonality a black-and-white print has.
    light = np.exp(-(((x - 0.32) ** 2) / 0.08 + ((y - 0.30) ** 2) / 0.12))
    horizon = 0.05 * np.clip(1.0 - np.abs(y - 0.62) * 6.0, 0.0, 1.0)
    tone = 0.06 + 0.22 * light + horizon

    # Grain is strongest in the mid-tones, as on film.
    amount = 0.045 * (0.4 + 4.0 * tone * (1.0 - tone))
    tone = tone + amount * grain(rng, SIZE)

    # A thin paper border so the tile reads as a print rather than a flat dark
    # square.
    c = (SIZE - 1) / 2
    inset = TILE / 2 - 46
    border = (np.abs(x * (SIZE - 1) - c) > inset) | (np.abs(y * (SIZE - 1) - c) > inset)
    paper = 0.86 + 0.015 * grain(rng, SIZE)
    tone = np.where(border, paper, tone)

    # Tones are display values already: no gamma lift, the print stays dark.
    grey = np.clip(tone, 0.0, 1.0)
    alpha = rounded_mask(SIZE, TILE, RADIUS)

    rgba = np.empty((SIZE, SIZE, 4), dtype=np.uint8)
    rgba[..., 0:3] = np.round(grey * 255)[..., None].astype(np.uint8)
    rgba[..., 3] = np.round(alpha * 255).astype(np.uint8)

    out = Path(__file__).with_name("icon.png")
    Image.fromarray(rgba, "RGBA").save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
