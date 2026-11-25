//! Reading photos as `load_linear` in twb2.py does: turned upright from the EXIF
//! orientation, an embedded colour profile converted to sRGB, then resized with Lanczos in
//! gamma space. Runs on worker threads.

use crate::frame::Area;
use crate::library::{all_weightings, film_index, Levels, Proxy};
use crate::params::{Frame, Settings};
use crate::provenance;
use crate::resize;
use image::{ImageDecoder, ImageReader};
use std::path::{Path, PathBuf};

/// Long edge of the copy every photo keeps in memory, rendered for thumbnails.
pub const PROXY_PX: u32 = 640;

/// A decoded photo: 8-bit sRGB, four bytes a pixel, alpha always opaque.
pub struct Pixels {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Extensions offered when opening and picked up from folders.
pub fn extensions() -> &'static [&'static str] {
    if cfg!(target_os = "macos") {
        &["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp", "heic", "heif", "avif", "dng", "cr2", "cr3", "nef", "arw", "raf", "orf", "rw2", "pef", "srw"]
    } else {
        &["jpg", "jpeg", "png", "tif", "tiff", "webp", "bmp"]
    }
}

pub fn is_photo(path: &Path) -> bool {
    path.extension().map(|e| e.to_string_lossy().to_lowercase()).is_some_and(|e| extensions().contains(&e.as_str()))
}

/// Photos in `paths`, folders walked into. Hidden files are skipped. Paths come back
/// absolute and with links resolved, so one file is one entry however it was named, and
/// the source recorded in an export still finds it from another working directory.
pub fn collect(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
        let p = &std::fs::canonicalize(p).unwrap_or_else(|_| p.clone());
        if p.is_dir() {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(p).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
            entries.retain(|e| !e.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.')));
            entries.sort();
            out.extend(collect(&entries));
        } else if is_photo(p) {
            out.push(p.clone());
        }
    }
    out
}

/// The photo at its own size, upright and in sRGB.
pub fn decode(path: &Path) -> Result<Pixels, String> {
    match decode_with_image(path) {
        Ok(p) => Ok(p),
        Err(e) => decode_with_sips(path).ok_or(e),
    }
}

fn decode_with_image(path: &Path) -> Result<Pixels, String> {
    let reader = ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let mut decoder = reader.into_decoder().map_err(|e| e.to_string())?;
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);
    let icc = decoder.icc_profile().ok().flatten();
    let mut img = image::DynamicImage::from_decoder(decoder).map_err(|e| e.to_string())?;
    img.apply_orientation(orientation);
    let gray = matches!(img.color(), image::ColorType::L8 | image::ColorType::L16 | image::ColorType::La8 | image::ColorType::La16);
    let mut rgba = img.into_rgba8();
    for p in rgba.as_chunks_mut::<4>().0 {
        p[3] = 255;
    }
    if let (Some(icc), false) = (icc, gray) {
        to_srgb(&icc, &mut rgba);
    }
    let (w, h) = rgba.dimensions();
    Ok(Pixels { w, h, rgba: rgba.into_raw() })
}

/// Convert from the embedded profile to sRGB. A profile qcms cannot read is ignored, as
/// twb2.py ignores one PIL cannot.
fn to_srgb(icc: &[u8], rgba: &mut [u8]) {
    let Some(src) = qcms::Profile::new_from_slice(icc, false) else { return };
    let dst = qcms::Profile::new_sRGB();
    if let Some(t) = qcms::Transform::new(&src, &dst, qcms::DataType::RGBA8, qcms::Intent::Perceptual) {
        use rayon::prelude::*;
        rgba.par_chunks_mut(4 * 4096).for_each(|c| t.apply(c));
    }
}

/// macOS reads HEIC and camera raw files the image crate cannot; `sips` turns them into
/// a PNG that keeps their colour profile.
#[cfg(target_os = "macos")]
fn decode_with_sips(path: &Path) -> Option<Pixels> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let tmp = std::env::temp_dir().join(format!("alexander-studio-{}-{}.png", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
    let ok = std::process::Command::new("/usr/bin/sips")
        .args(["-s", "format", "png"])
        .arg(path)
        .arg("--out")
        .arg(&tmp)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    let out = if ok { decode_with_image(&tmp).ok() } else { None };
    let _ = std::fs::remove_file(&tmp);
    out
}

#[cfg(not(target_os = "macos"))]
fn decode_with_sips(_path: &Path) -> Option<Pixels> {
    None
}

/// Long edge of the simulation grid for a look: `detail_px`, or the original's own size
/// when that is 0, never more than the device can hold.
pub fn grid_long(detail_px: u32, original: (u32, u32), max_dim: u32) -> u32 {
    let own = original.0.max(original.1);
    if detail_px == 0 { own } else { detail_px }.min(max_dim).max(1)
}

pub fn grid_size(original: (u32, u32), long: u32) -> (u32, u32) {
    resize::fit_long(original.0, original.1, long)
}

/// Everything the library needs to know about a photo when it is first opened.
pub struct Opened {
    /// The file to work from: the one opened, or the original an export was made from.
    pub path: PathBuf,
    pub look: Option<Settings>,
    pub original: (u32, u32),
    pub proxy: Proxy,
    pub levels: Levels,
    /// The picture inside a uniform border, and its levels measured there.
    pub frame: Option<(Area, Levels)>,
}

pub fn open(path: &Path) -> Result<Opened, String> {
    if let Some(r) = provenance::resolve(path) {
        if let Ok(px) = decode(&r.source) {
            // a record that does not describe this picture (made by hand, or the original
            // replaced since) is ignored rather than opening some other photo in its place
            let o = measure(r.source, Some(r.look), px);
            // an export with its border stripped has the shape of the picture inside it
            let (w, h) = o.original;
            let strip = o.look.as_ref().is_some_and(|l| l.frame == Frame::Strip);
            let shape = match (&o.frame, strip) {
                (Some((a, _)), true) => {
                    let [x0, y0, x1, y1] = a.pixels(w, h);
                    (x1 - x0, y1 - y0)
                }
                _ => (w, h),
            };
            if image::image_dimensions(path).is_ok_and(|(ew, eh)| same_shape((ew, eh), shape)) {
                return Ok(o);
            }
        }
    }
    Ok(measure(path.to_path_buf(), None, decode(path)?))
}

/// Whether a picture of size `a` can be `b` resized: the same aspect up to rounding.
fn same_shape(a: (u32, u32), b: (u32, u32)) -> bool {
    let (long_a, long_b) = (a.0.max(a.1), b.0.max(b.1));
    if (a.0 >= a.1) != (b.0 >= b.1) && a.0 != a.1 && b.0 != b.1 {
        return false;
    }
    let short_a = a.0.min(a.1) as f64;
    let short_b = b.0.min(b.1) as f64 * long_a as f64 / long_b as f64;
    (short_a - short_b).abs() <= 1.0 + 1e-6 * short_a
}

/// The proxy and exposure levels of a decoded photo. The levels are measured on the photo
/// at the default simulation grid, the picture `film_exposure` in twb2.py measures, so
/// automatic exposure agrees with `--auto-ev`; the small proxy alone reads the brightest
/// tones a little higher. A look with another `detail_px` still uses these levels, so a
/// thumbnail and its full render always share one exposure.
fn measure(path: PathBuf, look: Option<Settings>, px: Pixels) -> Opened {
    let long = grid_long(Settings::default().detail_px, (px.w, px.h), u32::MAX);
    let (gw, gh) = resize::fit_long(px.w, px.h, long);
    let grid = resize::rgba8(&px.rgba, px.w, px.h, gw, gh);
    drop(px.rgba);
    let area = crate::frame::find(&grid, gw as usize, gh as usize);
    let measure_in = |rect: [u32; 4]| {
        let mut levels: Levels = [[(0.5, 0.9); 4]; 3];
        for (film, filter) in all_weightings() {
            let weights = film_weights()[film_index(film)][filter as usize];
            levels[film_index(film)][filter as usize] = crate::tone::exposure_levels_in(&grid, gw as usize, gh as usize, weights, rect);
        }
        levels
    };
    let levels = measure_in([0, 0, gw, gh]);
    let frame = area.map(|a| (a, measure_in(a.pixels(gw, gh))));
    let (pw, ph) = resize::fit_long(gw, gh, PROXY_PX.min(gw.max(gh)));
    let rgba = resize::rgba8(&grid, gw, gh, pw, ph);
    Opened { path, look, original: (px.w, px.h), proxy: Proxy { w: pw, h: ph, rgba }, levels, frame }
}

fn film_weights() -> &'static [[[f64; 3]; 4]; 3] {
    static W: std::sync::OnceLock<[[[f64; 3]; 4]; 3]> = std::sync::OnceLock::new();
    W.get_or_init(|| crate::film::FilmKind::ALL.map(|k| crate::film::Film::load(k).weights))
}

/// The photo at the size of its simulation grid.
pub fn full(path: &Path, long: u32) -> Result<Pixels, String> {
    let px = decode(path)?;
    let (w, h) = resize::fit_long(px.w, px.h, long);
    let rgba = resize::rgba8(&px.rgba, px.w, px.h, w, h);
    Ok(Pixels { w, h, rgba })
}

#[cfg(test)]
mod tests {
    use super::same_shape;

    #[test]
    fn shapes() {
        assert!(same_shape((3000, 2000), (3000, 2000)));
        assert!(same_shape((2560, 1707), (3000, 2000)));
        assert!(same_shape((480, 640), (3000, 4000)));
        assert!(!same_shape((2000, 3000), (3000, 2000)));
        assert!(!same_shape((3000, 1500), (3000, 2000)));
        assert!(same_shape((500, 500), (2000, 2000)));
    }
}
