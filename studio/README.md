# Alexander

A desktop studio for the film and print simulation of `twb2.py`. Open any number of
photos, choose some, and give them the look of the pictures in *The Timeless Way of
Building* with every knob of the command-line tool live on screen. The simulation runs on
the GPU; the app is written in Rust with egui and wgpu and is meant for macOS, though it
also builds and runs on Linux.

## Build and run

Needs a current stable Rust toolchain.

```bash
cd studio
cargo run --release                      # an empty studio
cargo run --release -- ~/Pictures/trip   # photos or folders to open at launch
```

The release build is the one to judge speed by; the debug build optimises dependencies
but not the app. The macOS bundle is made by the scripts in `macos/`.

## Working in it

Open photos with Open… (⌘O), by dropping files or folders on the window or on the Dock
icon, with Open With in the Finder, or by naming them on the command line. On macOS, Open from Photos… (⇧⌘O) shows the system's Photos
picker, with the library's albums and search, and takes any number of photos. The picker
hands each photo over in its original format for a moment only, so the app copies it to
`~/Library/Caches/studio.alexander.app/Photos`, in a folder per photo. The same photo
picked again lands in the same place and opens once. Exports of these copies go to
Pictures unless another folder is chosen. The library shows every photo; click to select, ⌘-click to add
or remove one, ⇧-click to select a run. The panel on the right edits the look of every
selected photo at once. Original shows the photo as it is; Film applies the process.
Moving a knob changes that knob on all selected photos and leaves their other knobs as
they are, so photos can share some settings and differ in others. The histogram at the
top of the panel counts the tones of the focused photo as it is shown, from its 640 px
copy. The triangle in each of its top corners lights when shadows or highlights clip to
pure black or white; clicking either, or J, marks those values in Survey and Detail,
highlights red and shadows blue.

A rail of tools runs down the window's right edge, as in Lightroom. Edit shows the knobs in
the panel and Crop the crop; clicking the tool the panel shows hides the panel. Reference
shows the reference or hides it.

The library is remembered between launches: its photos with their looks, virtual copies
and stars, the selection, the view, the thumbnail size and whether cells are filled. The app writes it to
`~/Library/Application Support/studio.alexander.app/library.json` (on Linux,
`~/.local/share/alexander-studio`) at most once a second while it changes and when it
quits, and opens the photos again from their files at the next launch. A photo whose file
has gone shows as an empty tile with a mark, its error in the tooltip.

A look is a set of numbers and nothing else. Every render, on screen or exported, starts
again from the original file, so changing a look never processes a processed picture.

Three views, named as in Lightroom: Grid, all photos (G); Survey, the selected photos side
by side (N); and Detail, one photo large (E, Return, or double-click). Survey and Detail
have a filmstrip. The bar under the view counts the photos and the selection, names the
focused photo, holds the compare switches, and holds the view's own settings: in Grid
and Survey, Fit or Fill, and in Grid the thumbnail size; in Detail the zoom. Fit shows
each photo whole inside its cell; Fill covers the cell with it and cuts off what is over.

A cell names its photo; on macOS the SF Symbol for film beside the name marks a photo
the process is on for. A star marks the photos worth keeping. It shows in the top-right corner of a starred photo's
cell, and under the pointer in that corner of any other, where a click stars that photo
alone. In Detail the star in the bar under the photo, or S, stars the photo shown.
Starred Only at the left of the bar shows only the starred photos; the arrow keys, ⇧-click and
Select All then go through those alone. Opening photos shows every photo again.

Before (`\`) shows the photo itself in place of its render. Split (Y) shows both, in
Survey and Detail, with the photo itself left of a divider and the render right of it. Drag
the divider to move it, double-click it to put it back in the middle, and press Swap to
put the photo itself on the right. Moving the divider draws the picture again without
rendering it again.

A virtual copy is a second entry for the same file with a look of its own, so that two
looks of one photo can stand side by side in Survey. It follows its photo in the library
and is named for it, as in "IMG_1414.jpeg · Copy 1". Opening an export of a photo already
in the library shows its look on the copy that has it, on the photo when the photo has no
look, or else on a new virtual copy.

The reference (⇧R, or Reference in the rail; as Lightroom's Reference View) shows a picture
to work against, such as a scanned page of the book, beside the photo being edited. Its
pictures are the ones in the app's data folder under `References`, in order of their
names, and it goes through them with ‹ and ›. Add…, or dropping pictures on its own
window, copies pictures there; right-clicking the picture shows it in the Finder or moves
it to the Trash. It floats over the main window, where it can be moved, resized and
closed, or Pop Out puts it in a window of its own, which can stand anywhere, on another
screen too.

Right-clicking a photo opens a menu of what can be done to it: open it in Detail, show
it in the Finder, copy its file path, copy its finished picture as an image, make a
virtual copy, star it, switch the
process on or off, copy, paste or reset its look, export it, or remove it from the
library. As in the Finder, a photo outside the selection becomes the selection first, so
the commands act on every photo selected. Copy Image copies the photo clicked at the size
of its simulation grid, cut to the picture inside its border when the look strips it.

On macOS the menu bar holds every command with its key: File (with Open Recent, the
folders photos were opened from lately), Edit, Photo, View and Window. Keys with ⌘ go to
the menu; plain keys go to the window, which leaves them to text being typed.

| Keys | Does |
| --- | --- |
| ⌘O | Open photos |
| ⇧⌘O | Open from the Photos library (macOS) |
| ⇧⌘E | Export the selected photos |
| G / N / E or Return | Grid / Survey / Detail |
| Esc | Back to the grid |
| Arrow keys | Next or previous photo; ⇧ extends the selection in the library |
| ⌘A / ⌘D | Select all / none |
| Delete or Backspace | Remove from the library (the file stays on disk; ⌘Z puts it back) |
| V | Switch the process on or off for the selection |
| S | Star the selection, or in Detail the photo shown, or take the stars away |
| ⌘' | Make a virtual copy of each selected photo |
| ⌘R | Show the selected photos in the Finder |
| ⇧R | Show or hide the reference |
| \\ | Before |
| Y | Split before and after |
| J | Mark clipped highlights red and clipped shadows blue |
| R | Crop the focused photo; Return or R again keeps the crop, Esc leaves it |
| ⇧⌘C / ⇧⌘V | Copy and paste a look |
| ⇧⌘R | Reset the film settings of the selection to the defaults |
| ⌘Z / ⇧⌘Z | Undo and redo changes to looks |
| Z, or click | Detail: fit, or 100% about the point clicked |
| ⌘0 / ⌘1 | Detail: fit / 100% |
| ⌘+ / ⌘− | Zoom in and out; in the library, larger and smaller thumbnails |
| Scroll or pinch | Detail: zoom about the pointer |
| Drag | Detail: move the photo |
| Tab | Show or hide the panel |

Border, in the Exposure section, deals with a uniform border round the picture, such as
the paper of a photographed print or letterbox bars, found when the photo is opened as
`twb2.py --frame` finds it. Keep (the default) measures exposure and tones inside the
border and leaves it in the picture; Strip does the same and shows and exports only the
picture inside, in every view and thumbnail, without rendering again; Off does not look.

Crop (R, or Crop in the rail) works on one photo at a time, in Detail, and holds the panel
while it lasts. It shows the whole photo with a level frame over it: drag the frame or its
handles to move or size it. Dragging just outside a corner, where the pointer becomes a
curved arrow, turns the photo under the frame, as does Straighten, up to 45° either way.
Moving to another photo in Detail gives the photo left its crop and goes on to crop the next. The frame
keeps the photo's own aspect unless another one is chosen, or Free, and never leaves the
photo, shrinking when a turn would take a corner off it. Return, R or Done keeps the crop
as one step undo takes back; Esc or Cancel leaves the photo as it was. The crop is part of
the look, so exports, thumbnails and the histogram show it, but presets and pasted looks
leave each photo's own crop alone, and Reset takes it away. A stripped border is a crop
of the picture inside it, at no angle.

Presets head the panel, closed until opened. Resting the pointer on one shows it on the focused photo, and a
click gives it to every selected photo, in one step that undo takes back. Four come with
the app: Default, Harder (grade 4.25 at print exposure 0), Copied from a Page and Book
Page (ink on cream paper). New Preset… keeps the focused photo's look under a name, as a
look file in the app's data folder under `Presets`; right-clicking one of these updates it
with the current look, shows it in the Finder or deletes it.

The knobs are grouped as the process runs: Film, Exposure, Lens, Grain, Edge Effects
(switched on and off at its heading), Print, Reproduction and Simulation. Clicking a
heading opens or closes it. A setting off its default underlines its name under the
pointer, and clicking the name restores the default; double-clicking a slider does the
same. A slider's value, shown to two decimals at most, can also be dragged or typed. Stock, Filter, Border and Finish are
rows of toggles; the full names of the stocks and finishes are in their tooltips. Copy, Paste and Reset sit under the knobs, and the
panel's ••• menu saves and loads looks as JSON and copies the look as `twb2.py` flags. In
Detail 100% means one pixel of the simulation grid to one pixel of the screen.

## Exports

Export (⇧⌘E) writes each selected photo that has a look to a folder of your choice as
`<name>-twb.jpg` or `.png`, adding a number when that name is taken. A source file is never
overwritten. As in `twb2.py` the picture is resized with Lanczos to the original's long
edge (or kept at the simulation grid, or a long edge of your choice), stored as 8-bit
greyscale when it is neutral, tagged as sRGB, with options for 16-bit PNG and JPEG at
quality 96 without chroma subsampling.

Every export records the original's path and the look as JSON inside the file: a PNG
iTXt chunk named `alexander-studio`, or a JPEG APP11 segment that starts with the same
name. Opening an export whose original is still where it was opens the original with
that look, ready to change; when the original has gone, the export opens as a photo of
its own. A record is not followed when its path is relative or names a picture of another
shape, so a file with a hand-made or stale record opens as itself. An export with its
border stripped has the shape of the picture inside the border, and is matched to it.
With Strip, the exported size follows the picture area: at the original's size it keeps
the resolution it had in the whole photo.

## How it relates to twb2.py

The knobs, their names, defaults and meaning are those of `twb2.py`; a look copied as
flags reproduces the picture on the command line, grain pattern aside (the GPU draws grain
from a different random source with the same statistics). Photos are read as
`load_linear` reads them: turned upright from their EXIF orientation, converted from an
embedded colour profile to sRGB, and resized with Lanczos in gamma space to the
simulation grid (`detail_px`, 2560 px on the long edge by default). The engine's renders
are held to the Python tool's by `tests/render.rs`.

Automatic exposure is measured once, when a photo is opened, on the photo resized to the
default simulation grid (2560 px), for each film and filter: the picture `twb2.py
--auto-ev` measures. Thumbnails and full renders use the same exposure. With another
`detail_px` the command line measures a slightly different picture, and the two can differ
by a few thousandths of a stop.

## Speed and memory

Every photo keeps a 640 px copy in memory. The copies of photos whose thumbnails need a
new render go to the GPU and are rendered through the same engine; since the simulation
works in physical units, a thumbnail is a faithful miniature. Photos shown large also get
their full grid on the GPU. Both kinds are held in least-recently-used order under fixed
byte budgets (256 MB of thumbnails' photos, 1.2 GB of full grids, 192 MB of drawn
thumbnails); a thumbnail already drawn with the current look needs no GPU copy, and
uploads share the per-frame time budget with renders, so GPU memory follows what is being
rendered, not the size of the library or of the selection. Decoding, colour conversion,
resizing and encoding happen on worker threads. A file that cannot be read shows as an
empty tile with a mark; its error is in the tile's tooltip.

The engine keeps every stage of a render, and a change redoes only the stages after the
knob it touches: paper grade, for example, redoes the print and what follows, not the
negative. While a knob moves, the focused photo is rendered every frame and the other
visible photos catch up within a time budget per frame. A large picture shows the
thumbnail's render until its full grid has arrived.

## Tests

```bash
cargo test
cargo clippy --all-targets -- -D warnings
```

`tests/render.rs` compares GPU renders with `twb2.py` and checks that stages are reused,
`tests/borders.rs` that borders are found where `twb2.py` finds them and that a scene in a
mat renders as it does there, kept and stripped,
`tests/derived.rs` the curve mathematics, and `tests/library.rs` selection, looks, undo,
EXIF orientation and the round trip from an export back to its original. The export
encoder has unit tests in `src/export.rs`. Render tests are skipped on a machine without
a usable GPU.

Two environment variables help when checking the app without a person at it.
`STUDIO_TRACE=1` prints which stage each render of the focused photo started from, and
every sixty frames the time spent per frame and how many photos are on the GPU.
`STUDIO_SHOTS=<folder>` makes the app open the photos given on the command line, step
through its views, save a screenshot of each in that folder, export the selection there,
and quit.

## Known limits

- HEIC and camera raw files are read through `sips`, which ships with macOS, as a PNG
  that keeps the file's colour profile. Raw files come out as `sips` develops them.
- JPEG exports carry no embedded ICC profile; untagged JPEG is read as sRGB by browsers
  and photo apps, which is what the values are. PNG exports carry the sRGB chunk.
- Exporting reads each finished picture back from the GPU on the interface thread, so the
  window pauses for a moment per photo while an export runs.
- Speed has been measured only on a software GPU, where absolute numbers mean little.
