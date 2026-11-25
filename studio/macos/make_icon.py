"""Generate studio/macos/icon.png, the 1024 px source of the app icon.

The icon is the seal from the cover of The Timeless Way of Building, drawn in
white on a dark tile. seal.svg is a vector tracing of the seal in black on a
transparent ground; its coverage becomes the white of the icon.

    python3 studio/macos/make_icon.py

Requires numpy, Pillow and resvg-py.
"""

import io
from pathlib import Path

import numpy as np
import resvg_py
from PIL import Image

SIZE = 1024
# Apple's icon grid: the tile is 824 px inside the 1024 canvas, the rest is
# left transparent so the Dock's shadow and spacing match other apps.
TILE = 824
RADIUS = 185
SEAL_RADIUS = 300
# Outer edge of the seal's fine border, in seal.svg's 1024-unit view box.
SVG_OUTER = 481.5


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


def seal() -> np.ndarray:
    """Ink coverage of seal.svg, rendered centred on the canvas."""
    side = round(SIZE * SEAL_RADIUS / SVG_OUTER)
    svg = str(Path(__file__).with_name("seal.svg"))
    png = bytes(resvg_py.svg_to_bytes(svg_path=svg, width=side, height=side))
    ink = np.asarray(Image.open(io.BytesIO(png)).getchannel("A"), dtype=np.float64) / 255
    canvas = np.zeros((SIZE, SIZE))
    o = (SIZE - side) // 2
    canvas[o : o + side, o : o + side] = ink
    return canvas


def main() -> None:
    y = np.mgrid[0:SIZE, 0:SIZE][0] / (SIZE - 1)
    # A faint fall of light from the top, as on the system's dark icons.
    ground = 0.15 - 0.07 * y
    grey = ground + (0.96 - ground) * seal()
    alpha = rounded_mask(SIZE, TILE, RADIUS)

    rgba = np.empty((SIZE, SIZE, 4), dtype=np.uint8)
    rgba[..., 0:3] = np.round(grey * 255)[..., None].astype(np.uint8)
    rgba[..., 3] = np.round(alpha * 255).astype(np.uint8)

    out = Path(__file__).with_name("icon.png")
    Image.fromarray(rgba, "RGBA").save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
