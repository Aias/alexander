# Data

`trix400.json`, `doublex5222.json` and `polymax.json` are digitised from Kodak datasheets by
Jan Lohse and taken from https://github.com/JanLohse/spectral_film_lut (MIT licence, see
`LICENSE-spectral_film_lut.txt`). `tools/extract_data.py` regenerates them from a checkout.
The `*_mtf_fit.json` files are written by `tools/fit_mtf.py`.

The Tri-X curves and granularity are from the datasheet's 2007 edition; that edition says
its sharpness and spectral sensitivity curves were measured on the older Tri-X Pan 5063.
The characteristic curves are the datasheet's T-Max Developer curves at 6, 7, 9 and 11
minutes, and the granularity of 17 is quoted for development to a contrast index of 0.56.

Double-X 5222 dates from 1959. Its source file repeats Tri-X's table of granularity against
density, so that table is left out here; the single figure of 14 is Kodak's.

Polymax is a 1998 variable-contrast paper.
