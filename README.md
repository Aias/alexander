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
grain, printed on grade 3 paper, its tones then brought to the range the book's own pages
share, saved at the size of the input.

The film, the shape of the softness, the edge-effect rule and the optional stages were
chosen by blind comparison of renders. The tone range and the amount of softness were
set against the book's own pages, measured from scans and judged with the pages alongside. Add `--out-long-edge 5120` to save at a chosen size.

Useful switches:

| Switch | What it does | Default |
| --- | --- | --- |
| `--auto-ev` | Sets exposure from the picture; `--ev` adds stops on top | off |
| `--grade` | Paper grade, -1 (soft) to 5 (hard) | 3 |
| `--print-ev` | Print exposure in stops; more is darker | -0.5 |
| `--detail-px` | Long edge of the grid the film and print are simulated on before enlarging. Lower is coarser and softer; 0 uses the input's own size | 2560 |
| `--lens-um` | Softness of the picture on the film, as a blur radius in micrometres. It acts before development, so grain stays crisp over the softened picture. 0 is off; the reference scans, at the size of a book page, sit between 10 and 30 | 10 |
| `--lens-exponent` | Shape of that softness. 2 is a Gaussian. Lower values keep a sharp core and add a long faint skirt; published film sharpness curves sit near 1.15 | 1.15 |
| `--halation` | Light reflected back from the film base as a wide glow around bright areas, as a fraction of the light. 0.02 to 0.08 covers films with weak protection against it | 0 (off) |
| `--film` | `trix-1956`: Tri-X with the coarser grain and lower sharpness Kodak classed it at in 1956 (its two numbers are estimates). `trix`: Kodak Tri-X Pan from its datasheet. `doublex`: Eastman Double-X 5222, an emulsion of 1959 | trix-1956 |
| `--film-mm` | Long edge of the piece of film the picture fills: 36 is a whole 35mm frame, 18 is half-frame, 56 is medium format. Smaller means more grain | 24 |
| `--dev` | Development time in minutes (Tri-X 6 to 11, Double-X 4 to 12); longer is more contrast. Or a period's recommended development: `norm-1952`, `norm-1956`, `norm-1981`. A longer development wants a softer `--grade` | 7 (Double-X 6) |
| `--grain` | Granularity relative to the datasheet figure | 1.0 |
| `--grain-dev` | How granularity grows with development: the power of the negative's contrast it follows. 1.3 is the value measured on Double-X; 0 leaves it fixed | 1.3 |
| `--edge`, `--no-edge` | Strength of development edge effects | 1.0 |
| `--edge-density` | How edge effects grow with the density of the negative. 0 uses the tone difference alone; 1 follows Nelson's measurement, which makes fringes stronger in the light parts of the picture | 0 |
| `--filter` | Lens filter: `none`, `yellow`, `orange`, `red` | none |
| `--house` | Sets the picture's shadows and white point to those the book's own pages share, from 0 (a straight darkroom print) to 1 (matched). The book's pages keep their shadows more open than a straight print | 1 |
| `--grain-light` | Grain kept in the light tones, relative to a straight print. The book's pages are cleaner in their light tones | 0.6 |
| `--repro copy` | Shows the print as it looks after being printed in a book, re-photographed from the page and printed again, which is how the book's own pictures were made. Tuned with `--copy-blur`, `--copy-gain`, `--copy-drop`, `--copy-fill` | off |
| `--repro book` | Adds the 1979 printing press: weak ink, tone compression, dot gain, softness | off |
| `--view page` | With `--repro book`, shows true ink on cream paper | off |
| `--bits 16` | 16-bit output | 8 |

A harder alternative to the default tone is `--grade 4.25 --print-ev 0`.

`python3 twb2.py --help` lists everything.

The research behind every stage, its sources and the decisions taken are written up in
[docs/RESEARCH.md](docs/RESEARCH.md).

## What each stage does

1. **Colour response.** The photo is weighted by Tri-X's spectral sensitivity, which is
   strong in blue. Blue skies come out pale unless a filter is chosen.
   The image is then softened slightly, as a lens and a film do. The softness has a sharp
   core and a long faint skirt (`--lens-exponent 2` makes it a plain Gaussian), and
   `--halation` adds the glow of light reflected from the back of the film.
2. **Film curve.** Exposure becomes density along the film's characteristic curve for the
   chosen development time. A metered grey is placed where an exposure at the film's
   speed puts it.
3. **Edge effects.** Dense areas hold back development nearby, which lifts contrast at
   edges and in small details. Applied to exposure before the curve, so it fades in
   blocked shadows and blown highlights. With `--edge-density 1` the change at an edge
   also grows with the density of the negative, as Nelson measured, so fringes are
   stronger in the light parts of the picture than in its shadows.
4. **Grain.** Each cell of the simulation grid holds a number of grains that follows from
   its area on the film. The number developed is drawn from a Poisson distribution. No
   texture is added and no noise is filtered: grain strength comes from the film format
   and its apparent size from the grid. Granularity also rises with development, as
   measured on real film: at the default 7 minutes it is about 10% above the datasheet
   figure, which is quoted for a shorter development. In the light tones of the print it
   is scaled down, as the book's pages show less grain there than a straight print does.
5. **Print.** The negative is printed on Kodak Polymax paper of the chosen grade.
6. **House tones.** The book's makers copied every source to a common look:
   a pale picture gained contrast and a dark one was lightened. `--house` sets the
   picture's shadows and white point to the book's along a smooth curve, leaves the
   middle of the range to the picture, and never stretches tones by more than twice.
7. **Copied from a printed page (optional).** The print is printed once, which loses the
   palest highlights and closes up the deepest shadows; it is re-photographed, which
   softens detail and raises contrast; and it is printed again. The result is shown with
   solid ink as the print's own black.
8. **Press (optional).** The print's density range is squeezed into what ink on uncoated
   paper can hold, dots gain on the press, and the screen and paper soften detail by a
   fixed distance on the page.

## Where the numbers come from

| Quantity | Source |
| --- | --- |
| Film curves, spectral sensitivity, sharpness curve, granularity 17 | Kodak Tri-X 400 datasheet, as digitised in [spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) |
| Paper curves by grade | Kodak Polymax Fine-Art datasheet, same digitisation |
| The same four quantities for Double-X 5222 (granularity 14) | Eastman Double-X datasheet, same digitisation. Its sharpness fit has a long-tailed emulsion blur (exponent 1.11) |
| Granularity growing as the negative's contrast to the power 1.3 to 1.4, whatever the developer | J. C. Trinder, [Aerial Film Granularity and Its Influence on Visual Performance](https://www.isprs.org/proceedings/XXIII/congress/part1/167_XXIII-B1.pdf), 1980, measured on Kodak Double-X Aerographic. Applying it to Tri-X is an assumption |
| Granularity 17 quoted for development to a contrast index of 0.56 | Kodak F-4017 and F-9 datasheets |
| Recommended development by period, as negative gamma: 0.80 to 0.87 in 1952, 0.68 to 0.75 in 1956 | Time-gamma curves in *Kodak Films*, [5th](https://archive.org/details/kodak-films-5) and [7th](https://archive.org/details/kodak-films) editions |
| Tri-X classed medium grain, 75 to 90 lines/mm in 1956; fine grain, 100 to 125 lines/mm in 1981 | *Kodak Films*, 7th edition, and Kodak [AF-1](https://archive.org/details/kodak-films-color-and-b-w), 1981. The granularity of 22 and the wider blur of `--film trix-1956` are estimates from those classes |
| Grain statistics, and the step from a catalogue granularity to a grain size | The grain model in [spektrafilm](https://github.com/andreavolpato/spektrafilm) (development branch), re-implemented here |
| Edge-effect mechanism | spektrafilm's inhibitor model; strength and reach are fitted to the Tri-X sharpness curve by `tools/fit_mtf.py` |
| Edge effects proportional to density and to the density difference | Nelson's measurement (Photogr. Sci. Eng. 15, 1971), as reported by Rajkowski and Nowak, [Optica Applicata 31(1), 2001](https://opticaapplicata.pwr.edu.pl/files/pdf/2001/no1/optappl_3101p185.pdf) |
| Halation as light reflected from the film base, about 65 micrometres wide on still film | The halation model in spektrafilm, re-implemented here |
| Long-tailed softness, exponent near 1.15 | A fit of the 54 distinct film sharpness curves in spectral_film_lut: 51 fall off closer to exp(-f/fc) than to a Gaussian. Tri-X is the most Gaussian of them |
| Highlight dots that drop out and shadow dots that fill in at each printing (5% and 95% nominal, 15% and 85% on a small press) | J. D. Corey, [Making PMT Halftone Prints](https://www.osti.gov/servlets/purl/7107710), Bendix, 1977 |
| Shadows and white point of the book's pages (display values 0.08, 0.11 and 0.22 at the 2nd, 10th and 25th percentiles; 0.95 and 1.00 at the 90th and 98th) | Twelve picture pages of the book, measured from scans. The 10th and 25th are the lower quartile across pages (the medians are 0.12 and 0.24) so that shadows keep a little more depth |
| Grain falling away in light tones | The same pages: texture in smooth areas is strongest at display values 0.3 to 0.5 and about 0.6 to 0.7 of that in lighter greys |
| Softness of 10 micrometres | The book's pages measure edge widths of 1.33 px (sharpest tenth) and 2.10 px (median) at 900 px on the long edge; renders at 10 to 11 land closest |
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
picture supplied at 1,500 px and at 4,500 px renders the same. It repeats the film checks
for Double-X, and checks the contrast index and gamma of the named developments.

## Files

- `twb2.py`: the tool.
- `tools/calibrate.py`: the checks above.
- `tools/audit.py`: measures renders against reference pages (tone range, edge softness, grain by scale).
- `docs/RESEARCH.md`: the research, sources, technical overview and decisions.
- `tools/fit_mtf.py`: fits the sharpness model to a film's curve and writes its `data/*_mtf_fit.json`.
- `tools/extract_data.py`: rebuilds `data/*.json` from a checkout of spectral_film_lut.
- `tools/sheets.py`: builds contact sheets for a 5K display. It expects test photos in `photos/`.
- `compare.py`: tone and texture statistics for comparing renders with reference images.
- `legacy/twb.py`: an earlier, hand-tuned version that simulates halftone dots and scan artefacts.

## Licence

The code is released under the MIT licence; see `LICENSE`.

`data/trix400.json` and `data/polymax.json` are derived from
[spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) by Jan Lohse, also
under the MIT licence. Its text is in `data/LICENSE-spectral_film_lut.txt`.
