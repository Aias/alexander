"""Generate studio/macos/icon.png, the 1024 px source of the app icon.

The icon is the seal from the cover of The Timeless Way of Building, drawn in
white on a full-bleed dark square that the system masks to its icon shape.
seal.svg is a vector tracing of the seal in black on a transparent ground; its
coverage becomes the white of the icon.

    python3 studio/macos/make_icon.py

Requires numpy, Pillow and resvg-py.
"""

import io
from pathlib import Path

import numpy as np
import resvg_py
from PIL import Image

SIZE = 1024
# macOS masks a full-bleed icon to its own shape at 824/1024 scale, so a 410 px
# radius lands on the 660 px circle of the system's circular marks.
SEAL_RADIUS = 410
# Outer edge of the seal's fine border, in seal.svg's 1024-unit view box.
SVG_OUTER = 481.5


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

    out = Path(__file__).with_name("icon.png")
    Image.fromarray(np.round(grey * 255).astype(np.uint8), "L").convert("RGB").save(out, optimize=True)
    print(out)


if __name__ == "__main__":
    main()
