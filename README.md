# alexander

A command-line tool that gives a digital photograph the look of the black-and-white
pictures in Christopher Alexander's *The Timeless Way of Building*.

It does this by simulating the chain those pictures went through, with each stage in
physical units: micrometres on a 35mm negative and millimetres on a printed page. The
simulation runs on a grid 2,560 px across and is then enlarged to the size of your file,
so the same settings give the same picture at 1,000 px and at 48 megapixels.

    scene -> lens and film (softness, optional halation)
          -> Tri-X negative (curve, edge effects, grain) -> print on paper
          -> optional: copied from a printed page, or halftone on uncoated stock

## Use

Needs Python 3 with `numpy`, `scipy` and `pillow` (`pip install -r requirements.txt`).

```bash
python3 twb2.py photo.jpg -o out.png --auto-ev
```

That renders a darkroom print: a slightly soft image on the negative, developed, with
grain, printed on grade 3 paper, saved at the size of the input. Add `--out-long-edge 5120` to save at a chosen size.

Useful switches:

| Switch | What it does | Default |
| --- | --- | --- |
| `--auto-ev` | Sets exposure from the picture; `--ev` adds stops on top | off |
| `--grade` | Paper grade, -1 (soft) to 5 (hard) | 3 |
| `--print-ev` | Print exposure in stops; more is darker | -0.5 |
| `--detail-px` | Long edge of the grid the film and print are simulated on before enlarging. Lower is coarser and softer; 0 uses the input's own size | 2560 |
| `--lens-um` | Softness of the picture on the film, as a blur radius in micrometres. It acts before development, so grain stays crisp over the softened picture. 0 is off; the reference scans, at the size of a book page, sit between 10 and 30 | 5 |
| `--lens-exponent` | Shape of that softness. 2 is a Gaussian. Lower values keep a sharp core and add a long faint skirt; published film sharpness curves sit near 1.15 | 2 |
| `--halation` | Light reflected back from the film base as a wide glow around bright areas, as a fraction of the light. 0.02 to 0.08 covers films with weak protection against it | 0 (off) |
| `--film-mm` | Long edge of the piece of film the picture fills: 36 is a whole 35mm frame, 18 is half-frame, 56 is medium format. Smaller means more grain | 24 |
| `--dev` | Development time in minutes, 6 to 11; longer is more contrast | 7 |
| `--grain` | Granularity relative to the datasheet figure | 1.0 |
| `--edge`, `--no-edge` | Strength of development edge effects | 1.0 |
| `--edge-density` | How edge effects grow with the density of the negative. 1 follows Nelson's measurement; 0 uses the tone difference alone | 1 |
| `--filter` | Lens filter: `none`, `yellow`, `orange`, `red` | none |
| `--repro copy` | Shows the print as it looks after being printed in a book, re-photographed from the page and printed again, which is how the book's own pictures were made. Tuned with `--copy-blur`, `--copy-gain`, `--copy-drop`, `--copy-fill` | off |
| `--repro book` | Adds the 1979 printing press: weak ink, tone compression, dot gain, softness | off |
| `--view page` | With `--repro book`, shows true ink on cream paper | off |
| `--bits 16` | 16-bit output | 8 |

A harder alternative to the default tone is `--grade 4.25 --print-ev 0`.

`python3 twb2.py --help` lists everything.

## What each stage does

1. **Colour response.** The photo is weighted by Tri-X's spectral sensitivity, which is
   strong in blue. Blue skies come out pale unless a filter is chosen.
   The image is then softened slightly, as a lens and a film do. With `--lens-exponent`
   below 2 the softness has a sharp core and a long skirt, and `--halation` adds the glow
   of light reflected from the back of the film.
2. **Film curve.** Exposure becomes density along the Tri-X characteristic curve for the
   chosen development time. A metered grey is placed where an ISO 400 exposure puts it.
3. **Edge effects.** Dense areas hold back development nearby, which lifts contrast at
   edges and in small details. Applied to exposure before the curve, so it fades in
   blocked shadows and blown highlights. The change at an edge grows with the density of
   the negative as well as with the difference across the edge, so fringes are stronger in
   the light parts of the picture than in its shadows.
4. **Grain.** Each cell of the simulation grid holds a number of grains that follows from
   its area on the film. The number developed is drawn from a Poisson distribution. No
   texture is added and no noise is filtered: grain strength comes from the film format
   and its apparent size from the grid.
5. **Print.** The negative is printed on Kodak Polymax paper of the chosen grade.
6. **Copied from a printed page (optional).** The print is printed once, which loses the
   palest highlights and closes up the deepest shadows; it is re-photographed, which
   softens detail and raises contrast; and it is printed again. The result is shown with
   solid ink as the print's own black.
7. **Press (optional).** The print's density range is squeezed into what ink on uncoated
   paper can hold, dots gain on the press, and the screen and paper soften detail by a
   fixed distance on the page.

## Where the numbers come from

| Quantity | Source |
| --- | --- |
| Film curves, spectral sensitivity, sharpness curve, granularity 17 | Kodak Tri-X 400 datasheet, as digitised in [spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) |
| Paper curves by grade | Kodak Polymax Fine-Art datasheet, same digitisation |
| Grain statistics, and the step from a catalogue granularity to a grain size | The grain model in [spektrafilm](https://github.com/andreavolpato/spektrafilm) (development branch), re-implemented here |
| Edge-effect mechanism | spektrafilm's inhibitor model; strength and reach are fitted to the Tri-X sharpness curve by `tools/fit_mtf.py` |
| Edge effects proportional to density and to the density difference | Nelson's measurement (Photogr. Sci. Eng. 15, 1971), as reported by Rajkowski and Nowak, [Optica Applicata 31(1), 2001](https://opticaapplicata.pwr.edu.pl/files/pdf/2001/no1/optappl_3101p185.pdf) |
| Halation as light reflected from the film base, about 65 micrometres wide on still film | The halation model in spektrafilm, re-implemented here |
| Long-tailed softness, exponent near 1.15 | A fit of the 54 distinct film sharpness curves in spectral_film_lut: 51 fall off closer to exp(-f/fc) than to a Gaussian. Tri-X is the most Gaussian of them |
| Highlight dots that drop out and shadow dots that fill in at each printing (5% and 95% nominal, 15% and 85% on a small press) | J. D. Corey, [Making PMT Halftone Prints](https://www.osti.gov/servlets/purl/7107710), Bendix, 1977 |
| Solid ink density 1.20 on uncoated cream paper | Values published for the German offset process standard (paper type 5) |

Tuned by eye or against reference scans, with no source: lens softness, halation
strength, enlarger and copy-lens blur, the blur and contrast of the copy stage, the
smallest printable dot, the dot gain left uncorrected, the scan levels and the cream
paper colour.

The film curves and granularity come from Kodak's Tri-X datasheet in its 2007 edition.
That edition states that its sharpness and spectral sensitivity curves were measured on
the older Tri-X Pan 5063, so those two belong to the film sold before the 2000s
revision. The paper is a 1998 product.

The pictures in the book are older than any of this. Its acknowledgments say that many
of the photographs "come from books and magazines long out of print", and the credits
name photographers who worked from the 1900s to the 1960s. They were re-photographed
from printed pages, which is what `--repro copy` imitates.

## Checks

```bash
python3 tools/calibrate.py
```

This confirms that the simulated negative reads the datasheet granularity of 17 through
a 48 micrometre aperture at three pixel sizes, that its sharpness follows the published
curve, that the film and paper curves pass through the digitised points, and that a
picture supplied at 1,500 px and at 4,500 px renders the same.

## Files

- `twb2.py`: the tool.
- `tools/calibrate.py`: the checks above.
- `tools/fit_mtf.py`: fits the sharpness model to the Tri-X curve and writes `data/trix400_mtf_fit.json`.
- `tools/extract_data.py`: rebuilds `data/*.json` from a checkout of spectral_film_lut.
- `tools/sheets.py`: builds contact sheets for a 5K display. It expects test photos in `photos/`.
- `compare.py`: tone and texture statistics for comparing renders with reference images.
- `legacy/twb.py`: an earlier, hand-tuned version that simulates halftone dots and scan artefacts.

## Licence

The code is released under the MIT licence; see `LICENSE`.

`data/trix400.json` and `data/polymax.json` are derived from
[spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) by Jan Lohse, also
under the MIT licence. Its text is in `data/LICENSE-spectral_film_lut.txt`.
