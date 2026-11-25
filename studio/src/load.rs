//! Reading photos as `load_linear` in twb2.py does: turned upright from the EXIF
//! orientation, an embedded colour profile converted to sRGB, then resized with Lanczos in
//! gamma space. Runs on worker threads.

use crate::library::{all_weightings, film_index, Levels, Proxy};
use crate::params::Settings;
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

/// Photos in `paths`, folders walked into. Hidden files are skipped.
pub fn collect(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for p in paths {
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
    for p in rgba.chunks_exact_mut(4) {
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
}

pub fn open(path: &Path) -> Result<Opened, String> {
    let (path, look) = match provenance::resolve(path) {
        Some(r) => (r.source, Some(r.look)),
        None => (path.to_path_buf(), None),
    };
    let px = decode(&path)?;
    let (pw, ph) = resize::fit_long(px.w, px.h, PROXY_PX.min(px.w.max(px.h)));
    let rgba = resize::rgba8(&px.rgba, px.w, px.h, pw, ph);
    let mut levels: Levels = [[(0.5, 0.9); 4]; 3];
    for (film, filter) in all_weightings() {
        let weights = film_weights()[film_index(film)][filter as usize];
        levels[film_index(film)][filter as usize] = crate::tone::exposure_levels(&rgba, pw as usize, ph as usize, weights);
    }
    Ok(Opened { path, look, original: (px.w, px.h), proxy: Proxy { w: pw, h: ph, rgba }, levels })
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
