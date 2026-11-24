# alexander

A command-line tool that gives a digital photograph the look of the black-and-white
pictures in Christopher Alexander's *The Timeless Way of Building*.

It does this by simulating the chain those pictures went through, with each stage in
physical units: micrometres on a 35mm negative and millimetres on a printed page. The
simulation runs on a grid 2,560 px across and is then enlarged to the size of your file,
so the same settings give the same picture at 1,000 px and at 48 megapixels.

    scene -> Tri-X negative (curve, edge effects, grain) -> print on paper
          -> optional: halftone on uncoated stock -> optional: cream paper

## Use

Needs Python 3 with `numpy`, `scipy` and `pillow` (`pip install -r requirements.txt`).

```bash
python3 twb2.py photo.jpg -o out.png --auto-ev
```

That renders a darkroom print: the negative developed, with grain, printed on grade 3
paper, saved at the size of the input. Add `--out-long-edge 5120` to save at a chosen size.

Useful switches:

| Switch | What it does | Default |
| --- | --- | --- |
| `--auto-ev` | Sets exposure from the picture; `--ev` adds stops on top | off |
| `--grade` | Paper grade, -1 (soft) to 5 (hard) | 3 |
| `--print-ev` | Print exposure in stops; more is darker | -0.5 |
| `--detail-px` | Long edge of the grid the film and print are simulated on before enlarging. Lower is coarser and softer; 0 uses the input's own size | 2560 |
| `--film-mm` | Long edge of the piece of film the picture fills: 36 is a whole 35mm frame, 18 is half-frame, 56 is medium format. Smaller means more grain | 24 |
| `--dev` | Development time in minutes, 6 to 11; longer is more contrast | 7 |
| `--grain` | Granularity relative to the datasheet figure | 1.0 |
| `--edge`, `--no-edge` | Strength of development edge effects | 1.0 |
| `--filter` | Lens filter: `none`, `yellow`, `orange`, `red` | none |
| `--repro book` | Adds the printing press: weak ink, tone compression, dot gain, softness | off |
| `--view page` | With `--repro book`, shows true ink on cream paper | off |
| `--bits 16` | 16-bit output | 8 |

A harder alternative to the default tone is `--grade 4.25 --print-ev 0`.

`python3 twb2.py --help` lists everything.

## What each stage does

1. **Colour response.** The photo is weighted by Tri-X's spectral sensitivity, which is
   strong in blue. Blue skies come out pale unless a filter is chosen.
2. **Film curve.** Exposure becomes density along the Tri-X characteristic curve for the
   chosen development time. A metered grey is placed where an ISO 400 exposure puts it.
3. **Edge effects.** Dense areas hold back development nearby, which lifts contrast at
   edges and in small details. Applied to exposure before the curve, so it fades in
   blocked shadows and blown highlights.
4. **Grain.** Each cell of the simulation grid holds a number of grains that follows from
   its area on the film. The number developed is drawn from a Poisson distribution. No
   texture is added and no noise is filtered: grain strength comes from the film format
   and its apparent size from the grid.
5. **Print.** The negative is printed on Kodak Polymax paper of the chosen grade.
6. **Press (optional).** The print's density range is squeezed into what ink on uncoated
   paper can hold, dots gain on the press, and the screen and paper soften detail by a
   fixed distance on the page.

## Where the numbers come from

| Quantity | Source |
| --- | --- |
| Film curves, spectral sensitivity, sharpness curve, granularity 17 | Kodak Tri-X 400 datasheet, as digitised in [spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) |
| Paper curves by grade | Kodak Polymax Fine-Art datasheet, same digitisation |
| Grain statistics, and the step from a catalogue granularity to a grain size | The grain model in [spektrafilm](https://github.com/andreavolpato/spektrafilm) (development branch), re-implemented here |
| Edge-effect mechanism | spektrafilm's inhibitor model; strength and reach are fitted to the Tri-X sharpness curve by `tools/fit_mtf.py` |
| Solid ink density 1.20 on uncoated cream paper | Values published for the German offset process standard (paper type 5) |

Tuned by eye or against reference scans, with no source: enlarger and copy-lens blur, the
smallest printable dot, the dot gain left uncorrected, the scan levels and the cream
paper colour.

The film data is the current Tri-X (file dated 2007) and the paper is a 1998 product.
The pictures in the book were made on earlier materials.

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
