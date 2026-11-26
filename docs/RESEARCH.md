# Research notes

How this tool came to work the way it does: what was studied, what was found, how each
stage of the simulation is grounded, and which choices were judgement. It is written for
someone opening the repository for the first time.

The README says how to use the tool. This document says why it is built this way.

## 1. The aim

The target is the look of the black-and-white pictures in Christopher Alexander's *The
Timeless Way of Building* (Oxford University Press, 1979) **as they appear in the book**.
It is not the look of the original photographs. Whatever the book's makers did to bring
many different pictures to one consistent appearance is part of the target.

The output is meant for the screen at high resolution: the working assumption was 48
megapixel sources viewed on a 5K display.

The approach is to simulate the physical chain a picture went through, in physical units,
so that every setting means something and can be checked against published data. What
published data cannot settle is set by comparison with the book's own pages.

## 2. How the work was done

Research, building and testing were one continuous process, gone through many times:
look something up, build it, check it against published data, look at the result, and
let whatever looked wrong decide what to look up next. No stage of the tool was designed
up front, and several were rebuilt when later findings contradicted them (section 7).

**Sources.** A few rules were kept throughout:

- Only sources that were opened are cited. Each claim is marked as read at first hand
  (and how much of the source was read), confirmed in code or data, or inference.
- Primary sources are preferred: makers' datasheets and data books, standards, journal
  papers, patents, source code, trade manuals, library and museum records.
- Easy sources agree with each other, so the search also went beyond first-page results:
  through citation chains, other languages (Japanese, German, Swedish, French) and
  adjacent fields (astronomy, radiography, motion pictures, photogrammetry).
- New questions were put to data and code already in hand. Several of the most useful
  numbers came this way.
- Mistakes are recorded with their corrections.

**Checking.** Findings were built into the tool and checked by `tools/calibrate.py`,
which compares the simulation with datasheet figures.

**Judging.** What data could not settle was judged by eye, in two ways: blind pairs of
renders differing in one setting, and sheets that set renders beside the book's own
pages at the same scale. Section 6 says what was learned about the difference between
the two.

The questions the work went through, and what each changed:

| Question | What it changed |
| --- | --- |
| What makes film grain look organic | Grain moved from "a texture added to the output" to "random particles in the negative, shaped by everything after" |
| How noise can be shaped into grain | Real grain is white noise low-passed, with no preferred blob size |
| What data exists for black-and-white film and paper | Curves, sharpness, spectral response and a route from a catalogue granularity to a grain size, all with usable licences |
| What halftone, press and paper do | The press as a tone curve with three numbers and a small blur |
| How development sharpens edges | The effect became a sized quantity with a known place in the chain |
| What the book says about its own pictures | They were gathered from older publications and re-photographed from printed pages |
| What shape blur has in film | A sharp core with a long skirt, where a Gaussian has neither |
| Where the pictures were copied from | Two matched to their source pages, both gravure; the copy brought each picture to a common range |
| How much halation and lens flare the period's materials had | Period numbers for both; neither adopted |
| What films and development the period used | A measured law for grain against development; what Tri-X was in 1956 |
| What the book's pages measure | The tone range, softness and grain that the defaults are set against |

## 3. What the book's pictures are

This turned out to matter more than any film stock.

**They were gathered, not made for the book.** The acknowledgments (p. 549) say: "As for
the photographs, many of them come from books and magazines long out of print, and it has
not always been possible to find the names of the photographers." The credits (p. 551)
name about three dozen pictures by page: Henri Cartier-Bresson (seven), Eugene Atget,
Andre Kertesz, Erik Lundberg, Ernst Haas, Werner Bischof, Bruce Davidson, E. O. Hoppe,
Bernard Rudofsky and others. The photographs span roughly 1900 to 1965, most of them
35 mm work.

**They were re-photographed from printed pages.** Two pictures have been matched to
their sources by registering scans:

- Page 20 (Hoppe) is plate 235 of *Picturesque Great Britain* (Wasmuth, Berlin, 1926),
  whose introduction calls its plates photogravure.
- Page 46 (Haas) is page 128 of *The Family of Man* (1955), which the Museum of Modern
  Art's own press releases say was printed by rotogravure and sheet-fed gravure.

Cartier-Bresson's *The Decisive Moment* (1952) was also gravure. Every source page
identified so far is gravure, which holds a long tonal scale and has no halftone dots.

**The copy brought each picture to a common range.** The two matched pairs were adjusted
in opposite directions. The pale gravure plate gained contrast: tones lighter than about
0.27 density above its paper went to blank white, and the rest rose with a slope near
1.6. The dark page was lightened. That is what bringing every picture to one house look
does.

**The house look can be measured.** Across twelve picture pages, reduced to 900 px on
the long edge:

| Measure | Value |
| --- | --- |
| Display value at the 10th percentile of a page | 0.12 (0.11 to 0.14 across pages) |
| At the 25th percentile | 0.24 (0.22 to 0.27) |
| At the 50th and 75th | 0.55 and 0.78 |
| Share of a page in mid-tones (0.25 to 0.75) | about half |
| Share near black, and at paper white | about 6% and 7% to 10% |
| Width of the sharpest edges, 10 to 90% | 1.33 px (median edge 2.10 px) |
| Grain in smooth areas, by tone | Strongest in the darker mid-tones (0.3 to 0.5); about 0.7 of that at 0.5 to 0.7, 0.6 at 0.7 to 0.85, and almost none at paper white |
| Skies | Most are at or near paper white |

Four things stand out. The shadows are placed consistently and are more open than a
straight print from a negative. The lightest tenth of a page reaches paper white, and
skies usually go with it. Grain falls away in the light tones faster than it does in a
straight print, where a pale sky is the grainiest part of the picture. And the pages are
soft: their detail corresponds to a blur of 10 to 30 micrometres on a 24 mm width of
film. The middle of the tone range, by contrast, differs widely from page to page.

**The softness is of a particular kind.** The picture is soft while the texture over it
is crisp, and edges stay firm because each generation of copying softened slightly and
then raised contrast. This is what separates it from an out-of-focus picture.

**Limits.** The scans are uncalibrated, several come from one coarse scan of a late
printing, and none resolves the halftone screen. A better scan exists only as a library
loan and was not used. The numbers describe the book as its scans show it.

## 4. The pipeline, stage by stage

    scene -> lens and film (softness, optional halation)
          -> negative (curve, edge effects, grain) -> print on paper -> house tones
          -> optional: copied from a printed page, or halftone on uncoated stock

### 4.1 Units and the simulation grid

Lengths are micrometres on the film and millimetres on a page. The simulation runs on a
grid of fixed size (2,560 cells on the long edge) and is then enlarged to the output, so
the same settings give the same picture from a small file and from a 48 megapixel one.
The grid and the assumed width of film (24 mm, a moderate crop of a 35 mm frame) together
set how large the grain is in the frame. Both were chosen by eye: simulating at the full
pixel count of a large file gave grain too fine to read as film at screen size.

### 4.2 Colour response

The scene is weighted by the film's spectral sensitivity, integrated over three bands
that stand in for a camera's red, green and blue. This is an approximation, since the
scene's spectrum is gone once a photograph is RGB. Tri-X is strongly blue-sensitive, so
skies come out pale unless a filter is chosen.

### 4.3 Lens softness

*What it does.* Blurs the image on the film before development, so grain and edge
effects are laid over a softened picture and stay crisp.

*Its shape.* The blur has a sharp core and a long faint skirt, with a transfer function
`exp(-(f/fc)^n)` and `n` = 1.15. A fit of the 54 distinct film sharpness curves in the
data set used here found that 51 fall off closer to `exp(-f/fc)` than to a Gaussian, with
a median exponent of 1.15. That transfer function belongs to the point spread
`(1 + r^2/a^2)^(-3/2)`, the form astronomers devised for star images on photographic
plates (the Moffat function, with exponent 1.5). An independent film simulator models
emulsion scatter the same way, as a Gaussian core plus an exponential tail. In
comparison the long-tailed shape was preferred to a Gaussian of the same half-response
in every pair.

*Its amount.* 10 micrometres. The book's pages measure softer than a clean render, and
renders at 10 to 11 micrometres land on the book's edge widths.

### 4.4 Halation (optional, off)

Light that passes through the emulsion reflects off the back of the film base and
returns as a wide glow. The model is three reflections, each half as strong as the last,
with a first width of 65 micrometres for still-film base. Period sources put the classic
halo wider than that (a ring 3.58 times the base thickness in diameter) and show that
Kodak's 35 mm films carried a grey base of density 0.2 to 0.28, which leaves about a
sixth of an unprotected film's halation. In comparison a 5% glow could not be seen and a
12% glow was disliked, and a period-correct one would be fainter still. It stays off.

### 4.5 Film curve and exposure

Exposure becomes density along the film's published characteristic curve for the chosen
development time. A metered grey is placed where an exposure at the film's speed puts
it. `--auto-ev` sets the exposure from the picture's median and brightest tones, read
inside a border when the photo has one (section 4.12).

### 4.6 The films

- **`trix-1956` (default).** Tri-X curves with granularity 22 and a third more emulsion
  blur. Kodak's own data books class Tri-X in 1956 as medium grain, 75 to 90 lines/mm,
  medium sharpness; the Tri-X Pan behind the datasheet figures is classed fine grain, 100
  to 125 lines/mm, very high sharpness. The two numbers here are estimates from those
  classes, not measurements. In comparison this film was preferred to the datasheet one
  in every pair.
- **`trix`.** Kodak Tri-X Pan from its datasheet: granularity 17, quoted for development
  to a contrast index of 0.56. The 2007 datasheet says its sharpness and spectral curves
  were measured on the older Tri-X Pan 5063. The characteristic curves are the
  datasheet's T-Max Developer curves.
- **`doublex`.** Eastman Double-X 5222, an emulsion of 1959, from its own datasheet:
  granularity 14, a long-tailed sharpness curve and edge effects more than twice as
  strong as Tri-X's.

No source was found that names the film, developer or paper of any credited
photographer.

### 4.7 Development and edge effects

*Time.* `--dev` takes minutes or a period's recommended development, resolved through
each film's own curves: gamma 0.83 for Kodak's 1952 sheets, 0.71 for 1956, and the
contrast index of 0.56 at which granularity is quoted.

*Edge effects.* Where the negative is dense, development releases products that hold
back development nearby. Contrast rises at edges and in small detail, which is why a
film's sharpness curve can exceed 1.0 at low frequencies (it does for 35 of 37 stocks in
the data set). The model spreads an inhibitor from the developing density and subtracts
it from log exposure before the curve, so the effect fades in blocked shadows and blown
highlights. Its strength and reach are fitted to each film's published sharpness curve.

Nelson's measurements say the effect also grows with the density itself. That is built
(`--edge-density 1`) and off: in comparison the simpler rule was preferred, and the
book's pages say nothing either way.

### 4.8 Grain

Grain is treated as counting statistics in the negative. Each cell of the grid holds a
number of grains that follows from its area on the film; the number developed is drawn
from a Poisson distribution. This gives the variance of density that a catalogue
granularity figure describes, and its skew where grains are few. No texture is added and
no noise is filtered.

The step from a catalogue figure (the standard deviation of density through a 48
micrometre aperture at density 1.0) to a grain size follows the grain model of
spektrafilm, re-implemented here.

Granularity rises with development. Measured on Kodak Double-X at several developments,
it grew as the negative's contrast to the power 1.3 to 1.4, whatever the developer. The
tool applies 1.3, which adds about 10% at the default development. The exponent was
measured on one film; applying it to Tri-X is an assumption.

*Light tones.* In a straight print from a small negative, grain is most visible in
smooth light greys. The book's pages show the opposite trend, because every copy and
printing after the first cleaned up the light end. The tool therefore scales grain down
where the print is light (`--grain-light`, default 0.6). Grain is taken as the difference
between the print and the same print made without grain, so the picture itself is not
touched. The default brings the fall-off from dark mid-tones to light ones close to the
book's.

Two approaches were tried and abandoned before this one. See section 7.

### 4.9 Print

The negative is printed on Kodak Polymax Fine-Art paper using its published curves by
grade, with print exposure anchored so that a metered grey prints at a set density. The
default of grade 3 with slightly less exposure than that anchor was chosen by eye.

### 4.10 House tones

The step that carries the book's consistency. It uses the two things that hold from page
to page in the book (section 3): where the shadows sit, and that the lightest tenth
reaches paper white. The picture's own percentiles at the dark end and the light end are
moved to the book's along a smooth monotone curve. The middle of the range is left to
the picture, because the book's pages differ widely there.

Two limits keep the step from doing harm. No part of the scale is stretched by more than
a factor of 2 (one matched pair measures the 1979 copy at 1.6), so grain is not blown up
where a picture's tones bunch together. The limit holds working up from black and down
from white, so a picture whose light tenth already sits near white keeps paper white. And
the light end is compressed toward white, which is where the book's pages are cleanest.

A percentile within 0.005 of the one below it (on the black floor) or of white is left
where it is. Near those limits it fades out over a short range instead of dropping: the
curve is a blend of the curves with and without it. Dropping it outright would make the
whole picture jump, by as much as a fifth of the scale in the middle tones, whenever a
setting carries a percentile across the limit.

The two shadow targets are set at the lower quartile across the book's pages (0.11 and
0.22) where the medians are 0.12 and 0.24. With the medians, buildings lost a little
depth in their shadows.

Ten test photographs then measure a darkest quarter at 0.22 (book 0.23 to 0.24), 8% near
black (6%), 8% at paper white (7% to 10%), and edges of 1.30 px (1.33).

This step is per picture by design, as the copy in 1979 was. `--house 0` gives the
straight print.

### 4.11 Optional finishes

- **Copied from a printed page (`--repro copy`).** An early model of the book's chain:
  print, lose both ends of the scale, soften, raise contrast, print again. It was built
  before the book's pages were measured. Its output sits further from the book than the
  default does (28% of the picture near black against the book's 6%), and its premise
  that the first printing was a halftone is wrong for the pages identified. It is kept
  as an effect.
- **Press (`--repro book`).** The 1979 printing itself: density range squeezed into what
  ink on uncoated paper holds (solid density about 1.2), dot gain, softness of screen and
  paper. Faithful to how a page reflects light, and for that reason it looks hazy on a
  screen beside a scan whose levels have been set. Off.
- **Cream paper (`--view page`).** Off.

### 4.12 Borders

A photo can carry a border that is not part of the picture: the white paper of a
photographed print or instant picture, a mat, the black bars of a letterboxed frame.
Automatic exposure and the house tones both measure the picture, and a border counts in
both. On a photographed instant print whose paper is a tenth of the pixels, the paper
read as the brightest tones: the house tones put the lightest tenth at pure white and gave
the picture inside less range than it should have had.

`--frame keep`, the default, looks for such a border and measures inside it. A border is
a band of one even tone along two opposite sides, or all four. Each side is followed
inward, position by position, while the tone stays within 0.08 of the tone at the edge
(display values, after a 5 px smoothing that takes out paper texture). Across the middle
60% of the side, 85% of the positions must end on one straight line, tilted no more than
about 3 degrees; the band must be at least 0.5% of the picture deep and no more than a
quarter; the tone inside it must vary by no more than 0.03; and opposite sides must agree
in tone within 0.12. The picture area stops a margin of 0.4% of the long edge plus three pixels
inside the deepest point of each edge, which allows for a print photographed a degree or
two askew. Nothing is cut: the
border goes through the whole chain and stays in the result. `--frame strip` crops the
finished picture to that area; the picture's own pixels are the same as with `keep`.
`--frame off` does not look.

The test asks for two opposite sides because single bands are common in ordinary
photographs: a blown or overcast sky along the top, snow or water along the bottom, a
wall filling one side. The straight inner edge rules out dark corners and night scenes
whose black edges follow the subject. Synthetic photos of each kind are part of the tests,
with the border cases beside them.

The trade-off is that the test misses some real borders: one whose tone the picture
continues along much of its edge (a black bar beside a dark scene, a grey mat beside grey
tones), or one narrower than half a percent of the picture. Those are measured as before.
A missed border costs what it cost before the test existed; an invented one would move a
picture's exposure and tones for no reason, so doubtful cases are left alone.

### 4.13 Checks

`tools/calibrate.py` confirms that the simulated negatives read their datasheet
granularity through a 48 micrometre aperture at several pixel sizes, that their
sharpness follows the published curves, that film and paper curves pass through the
digitised points, that named developments reach their contrast figures, that a
picture supplied at two pixel counts renders the same, and that a picture in a mat is
measured as the picture alone.

`tools/audit.py` measures any set of renders against any set of reference pages: tone
percentiles, edge widths and texture at four scales.

## 5. Decisions and trade-offs

| Decision | Alternative | Basis | Trade-off |
| --- | --- | --- | --- |
| Simulate the chain in physical units | Tone curve plus overlaid grain texture | Overlaid textures read as regular and artificial | More code, slower; every number is checkable |
| Fixed simulation grid, enlarged to output | Simulate at the file's pixel count | By eye | Detail finer than the grid is given up on purpose |
| Poisson grain from a catalogue figure | Filtered or band-limited noise | Fitted models of real film; the grain model of an open simulator | No direct control of grain "size"; it follows from film width and grid |
| Finish at a darkroom print with house tones | Finish at the simulated press | By eye; measurement | Skips the halftone, which the scans cannot resolve anyway |
| Long-tailed softness | Gaussian | Fit of 54 curves; comparison | Needs a Fourier-domain filter |
| Softness of 10 micrometres | Sharper or softer | The book's edge widths | Softer than a blind eye picked in isolation |
| House tones on: shadows and white point set to the book's | A straight, harder print | The book's measured range, judged in context | More open shadows than a blind eye picked in isolation |
| The middle of the tone range left to the picture | Matching the whole range | The book's pages vary widely there; matching it darkened skies | Mid-tone share stays below the book's median |
| Grain reduced in light tones | The film's own behaviour | The book's grain by tone | Departs from what a straight print would show |
| 1956 Tri-X as the default film | Datasheet Tri-X | Kodak's 1956 classes; comparison | Its two numbers are estimates |
| Grain follows development | Fixed granularity | One measured law | Exponent taken from another film |
| Edge effects by tone difference only | Weighted by density, after Nelson | Comparison | Departs from the literature |
| No halation, no lens flare | Period-correct amounts | Comparison; a dislike of haze | Less physically complete |
| Copy and press stages optional | Either as default | Measurement; comparison | The literal chain of the book is not the default path |
| Measure inside a uniform border, keep it in the picture | Measure the whole photo; crop the border | A border counted as picture skews exposure and tones | Some real borders are missed; cropping is a choice (`--frame strip`) |

## 6. What was learned about judging

Blind pairs are good at questions with a right answer for the eye, such as which of two
blur shapes looks more like film. They proved unreliable for questions of degree. Shown
pairs in isolation, the eye picked the harder print and the sharper picture every time.
Shown the same renders beside the book's pages, it picked the book's tone range and the
book's softness. Fatigue also sets in: after several dozen pairs the answers stop being
consistent with each other.

The working rule that came out of this: measurements against the reference pages carry
more weight than a blind preference and settle close calls, and any comparison meant to
set a default shows the references alongside the candidates.

## 7. Dead ends and corrections

- **Simulated halftone dots.** An early version drew the dot screen. A perfect grid
  sampled without care produced a visible rectangular lattice, and the result looked
  mechanical. The screen is not simulated now.
- **Grain from shaped noise.** Noise with a cloud-like spectrum looked mottled; noise
  limited to one band of frequencies gave grain of a single blob size, which was
  unpleasant to look at. Fitted models of real film show power falling steadily with
  frequency and no preferred size, which is what counting statistics give.
- **"No open simulator has black-and-white film data."** Wrong. Several do, and one of
  them supplies this tool's data.
- **"The scans preserve the page's ink black."** A units mistake between encoded and
  linear values. The scans have had their levels set.
- **"The first printing was a halftone."** Assumed until two pages were matched to their
  sources. Both are gravure.
- **"The book's look is a harder copy."** The first model of the book's chain raised
  contrast and lost both ends. Measurement showed the book's pages have more mid-tone
  and more open shadows than a straight print.
- **Halation as the source of the book's softness.** The long skirt of film blur
  suggested it. Comparison showed the shape of the softness matters and a glow does not.
- **A fixed tone curve for the copy.** The two matched pairs disagree with any single
  curve. The copy adapted to each picture.
- **Matching the whole tone range.** The first version of the house-tone step moved
  every part of a picture's range to the book's median values. On a picture that is half
  sky it pulled the sky down into the mid-greys and stretched a narrow band of tones
  five-fold, which multiplied the grain there. The book's own skies are mostly white, and
  its middle tones vary too much from page to page to be a target. The step now sets
  only the shadows and the white point, with a limit on stretch.

## 8. Open questions

- The halftone screen ruling of the book, and how much of its texture is film grain and
  how much is print structure. A macro photograph of a page would settle both.
- Which publications the seven Cartier-Bresson pictures and the rest were copied from.
- Who made the copies for the publisher between 1977 and 1979, and how.
- Whether Tri-X follows the grain-against-development exponent measured on Double-X.
- How much light a 1950s emulsion passed into its base, which sets the strength of
  period halation.
- Measured granularity for films Kodak classed as medium or coarse grain.

## 9. Sources

Read in full or in the relevant part unless marked otherwise.

**The book and its pictures**

- Christopher Alexander, *The Timeless Way of Building*, Oxford University Press, 1979:
  copyright page, acknowledgments, credits.
- [The Art of Seeing](https://www.daidalos.org/en/articles/alexander/), Daidalos.
- [Christopher Alexander and Center for Environmental Structure archive](https://christopher-alexander-ces-archive.org/book/the-timeless-way-of-building/), records for the book.
- E. O. Hoppe, [*Picturesque Great Britain*](https://archive.org/details/picturesquegreat0000eoho), Wasmuth, 1926.
- Museum of Modern Art, press releases on *The Family of Man*, 1955: [no. 44](https://www.moma.org/docs/press_archives/1945/releases/MOMA_1955_0060_44.pdf), [21 June](https://www.moma.org/docs/press_archives/1958/releases/MOMA_1955_0073_56.pdf).
- Richard Benson, [*The Printed Picture*](https://printedpicture.artgallery.yale.edu/the-book/photography-in-ink-relief-and-intaglio-printing), on relief and intaglio printing.
- Fondation Henri Cartier-Bresson, [bibliography](https://www.henricartierbresson.org/en/hcb/bibliography/); Art Institute of Chicago, [A Note on the Prints and Magazines](https://archive.artic.edu/cartier-bresson/prints).

**Film, paper and development data**

- Kodak, F-4017, Tri-X 320 and 400: [2021](https://kodakprofessional.com/sites/default/files/wysiwyg/film/f4017_trix_320400.pdf) and [2007](https://125px.com/docs/film/kodak/f4017-400TX-2007.pdf) editions; F-9, [Tri-X Pan](https://125px.com/docs/film/kodak/f9-Tri-X_Pan-199906.pdf), 1999.
- [spectral_film_lut](https://github.com/JanLohse/spectral_film_lut) by Jan Lohse (MIT): digitised curves for Tri-X, Double-X, Polymax and the 54 sharpness curves.
- *Kodak Films* data books, [5th edition, 1951](https://archive.org/details/kodak-films-5) and [7th edition, 1958](https://archive.org/details/kodak-films); Kodak [AF-1](https://archive.org/details/kodak-films-color-and-b-w), 1981; Kodak [F-20, Understanding Graininess and Granularity](https://archive.org/details/f-20-understanding-graininess-and-granularity), 1979.
- J. C. Trinder, [Aerial Film Granularity and Its Influence on Visual Performance](https://www.isprs.org/proceedings/XXIII/congress/part1/167_XXIII-B1.pdf), 14th Congress of the International Society for Photogrammetry, 1980.

**Grain**

- A. Newson and others, [Realistic Film Grain Rendering](https://www.ipol.im/pub/art/2017/192/), IPOL, 2017 (abstract), and its [code](https://github.com/alasdairnewson/film_grain_rendering_gpu).
- [spektrafilm](https://github.com/andreavolpato/spektrafilm) by Andrea Volpato (code GPL v3): grain model, inhibitor model, scatter and halation. The models are re-implemented here; no code is copied.
- [darktable](https://github.com/darktable-org/darktable), `grain.c` and `negadoctor.c`; the [AV1 specification](https://github.com/AOMediaCodec/av1-spec), film grain synthesis.
- [PGNM](https://github.com/miya9756/pgnm), fitted grain models of real films.

**Sharpness, edge effects and blur**

- B. Rajkowski and P. Nowak, Optica Applicata 31, 2001: [Model examinations of edge effects in light-sensitive materials](https://opticaapplicata.pwr.edu.pl/files/pdf/2001/no1/optappl_3101p185.pdf) and [Modelling of edge effects taking account of the diffusion phenomenon](https://opticaapplicata.pwr.edu.pl/files/pdf/2001/no2/optappl_3102p489.pdf).
- [Filmulator](https://github.com/CarVac/filmulator-gui), a simulation of development.
- Gilmore, Models of the Point Spread Function of Photographic Emulsions, [J. Opt. Soc. Am. 57, 1967](https://doi.org/10.1364/josa.57.000075) (abstract); Trujillo and others, [the Moffat PSF](https://arxiv.org/abs/astro-ph/0109067) (excerpt).
- Steve Yedlin, [Display Prep Demo FAQ](https://www.yedlin.net/NerdyFilmTechStuff/DispPrepDemoFAQ/).
- Templin and others, [Perceptually-motivated Stereoscopic Film Grain](https://resources.mpi-inf.mpg.de/FilmGrain/paper.pdf), 2014 (in part).

**Halation and flare**

- Huse and Chambers, Journal of the Society of Motion Picture Engineers 17, 1931, on the [grey base](https://archive.org/details/journalofsociety17socirich).
- L. P. Clerc, [*Photography: Theory and Practice*](https://archive.org/details/in.ernet.dli.2015.463855), 3rd edition, 1944, sections on halation and lens reflections.
- C. E. K. Mees, [*The Theory of the Photographic Process*](https://archive.org/details/dli.ernet.212981), 1942, on turbidity.
- Jones and Condit, J. Opt. Soc. Am., [1941](https://archive.org/download/sim_optical-society-of-america-journal_1941-11_31_11/sim_optical-society-of-america-journal_1941-11_31_11_djvu.txt) and [1948](https://archive.org/download/sim_optical-society-of-america-journal_1948-02_38_2/sim_optical-society-of-america-journal_1948-02_38_2.pdf), on flare (excerpts).

**Printing**

- J. D. Corey, [Making PMT Halftone Prints](https://www.osti.gov/servlets/purl/7107710), Bendix, 1977.
- US Navy, *Illustrator Draftsman*, [Halftones](https://draftingmanuals.tpub.com/14065/css/Halftones-Cont-61.htm); Penn State University Press, [Halftones](https://www.psupress.org/books/author_resources/author_digsub/halftones.html).
- Offset process standards for uncoated paper ([FOGRA47](https://www.color.org/chardata/fogra47.xalter) and the German process standard), seen as search excerpts only.
- [exp-tone-curves-print](https://github.com/vetal32/exp-tone-curves-print), a print model in code.
