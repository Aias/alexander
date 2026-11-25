# Alexander Studio

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

Open photos with Open… (⌘O), by dropping files or folders on the window, or by naming
them on the command line. The library shows every photo; click to select, ⌘-click to add
or remove one, ⇧-click to select a run. The panel on the right edits the look of every
selected photo at once. Original shows the photo as it is; Film applies the process.
Moving a knob changes that knob on all selected photos and leaves their other knobs as
they are, so photos can share some settings and differ in others.

A look is a set of numbers and nothing else. Every render, on screen or exported, starts
again from the original file, so changing a look never processes a processed picture.

Three views: all photos (G), the selected photos side by side (N), and one photo large
with a filmstrip (E, Return, or double-click).

| Keys | Does |
| --- | --- |
| ⌘O | Open photos |
| ⇧⌘E | Export the selected photos |
| G / N / E or Return | All photos / selected photos / single photo |
| Esc | Back to all photos |
| Arrow keys | Next or previous photo; ⇧ extends the selection in the library |
| ⌘A / ⌘D | Select all / none |
| Delete or Backspace | Remove from the library (the file stays on disk) |
| V | Switch the process on or off for the selection |
| \\ | Before and after |
| ⇧⌘C / ⇧⌘V | Copy and paste a look |
| ⇧⌘R | Reset the selection to the original |
| ⌘Z / ⇧⌘Z | Undo and redo changes to looks |
| Z, or double-click | Single view: fit or 100% |
| ⌘0 / ⌘1 | Single view: fit / 100% |
| ⌘+ / ⌘− | Zoom in and out; in the library, larger and smaller thumbnails |
| Scroll or pinch | Single view: zoom about the pointer |
| Drag | Single view: move the photo |
| Tab | Show or hide the panel |

Double-clicking a knob or its name restores its default. The panel's … menu also saves
and loads looks as JSON and copies the look as `twb2.py` flags. In the single view 100%
means one pixel of the simulation grid to one pixel of the screen.

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
shape, so a file with a hand-made or stale record opens as itself.

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

- Files opened from the Finder with Open With, or dropped on the Dock icon, arrive as
  macOS open-document events, which the windowing library does not pass on. Drop them on
  the window or use Open… instead.
- HEIC and camera raw files are read through `sips`, which ships with macOS. That path
  has not been tested on a Mac yet; raw files come out as `sips` develops them.
- JPEG exports carry no embedded ICC profile; untagged JPEG is read as sRGB by browsers
  and photo apps, which is what the values are. PNG exports carry the sRGB chunk.
- There is no native menu bar; less frequent commands are in the panel's … menu.
- The library lasts as long as the window; looks live on in exported files and in saved
  JSON, not in a catalogue.
- Exporting reads each finished picture back from the GPU on the interface thread, so the
  window pauses for a moment per photo while an export runs.
- Removing photos from the library cannot be undone; their looks go with them.
- Speed has been measured only on a software GPU, where absolute numbers mean little.
