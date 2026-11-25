"""Generate studio/macos/icon.png, the 1024 px source of the app icon.

The icon is the seal from the cover of The Timeless Way of Building, drawn in
white on a dark tile. seal.png is a photograph of the seal, centred on its
rings; the engraving is thresholded from it and the rings are redrawn as
exact circles.

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
SEAL_RADIUS = 300
# Radii in seal.png pixels, measured from the photograph's radial ink profile.
PHOTO_OUTER = 133.5
ENGRAVING = 124.0
INNER_RING = (121.5, 127.0)
OUTER_RING = (131.5, 133.5)


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


def engraving(scale: float) -> np.ndarray:
    """Ink coverage of the photographed seal, resampled onto the canvas."""
    photo = Image.open(Path(__file__).with_name("seal.png")).convert("RGB")
    rgb = np.asarray(photo, dtype=np.float64)
    # Red ink against cream paper: the red-green gap separates them cleanly.
    ink = np.clip((rgb[..., 0] - rgb[..., 1] - 30) / 130, 0.0, 1.0)
    side = round(photo.width * scale)
    ink = np.asarray(Image.fromarray(ink.astype(np.float32), "F").resize((side, side), Image.BICUBIC))
    # Steepen the resampled edge to about a pixel wide, so the cut reads crisp.
    ink = np.clip((ink - 0.5) * 8 + 0.5, 0.0, 1.0)
    canvas = np.zeros((SIZE, SIZE))
    o = (SIZE - side) // 2
    canvas[o : o + side, o : o + side] = ink
    return canvas


def main() -> None:
    scale = SEAL_RADIUS / PHOTO_OUTER
    c = (SIZE - 1) / 2
    y, x = np.mgrid[0:SIZE, 0:SIZE].astype(np.float64)
    r = np.hypot(x - c, y - c)

    def ring(inner: float, outer: float) -> np.ndarray:
        mid, half = (inner + outer) / 2 * scale, (outer - inner) / 2 * scale
        return np.clip(0.5 - (np.abs(r - mid) - half), 0.0, 1.0)

    inside = np.clip(ENGRAVING * scale + 0.5 - r, 0.0, 1.0)
    seal = np.maximum.reduce([engraving(scale) * inside, ring(*INNER_RING), ring(*OUTER_RING)])

    # A faint fall of light from the top, as on the system's dark icons.
    ground = 0.15 - 0.07 * (y / (SIZE - 1))
    grey = ground + (0.96 - ground) * seal
    alpha = rounded_mask(SIZE, TILE, RADIUS)

    rgba = np.empty((SIZE, SIZE, 4), dtype=np.uint8)
    rgba[..., 0:3] = np.round(grey * 255)[..., None].astype(np.uint8)
    rgba[..., 3] = np.round(alpha * 255).astype(np.uint8)

    out = Path(__file__).with_name("icon.png")
    Image.fromarray(rgba, "RGBA").save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
