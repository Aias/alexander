//! Writing finished pictures, as `save` and `write_image` in twb2.py do: display values
//! Lanczos-resized to the size asked for, tagged as sRGB, greyscale when the picture is
//! neutral. Every file carries the record of what it was made from.

use crate::provenance::{self, Record};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Format {
    Jpeg,
    Png8,
    Png16,
}

impl Format {
    pub const ALL: [Format; 3] = [Format::Jpeg, Format::Png8, Format::Png16];
    pub fn ext(self) -> &'static str {
        match self {
            Format::Jpeg => "jpg",
            Format::Png8 | Format::Png16 => "png",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Format::Jpeg => "JPEG",
            Format::Png8 => "PNG",
            Format::Png16 => "PNG 16-bit",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Size {
    /// The original file's long edge.
    Original,
    /// The simulation grid, unresized.
    Grid,
    LongEdge(u32),
}

pub const JPEG_QUALITY: u8 = 96;

/// The picture to write: display values in [0, 1], four floats a pixel (alpha ignored).
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<f32>,
}

/// Where to write `source`'s export in `dir`: `<stem>-twb.<ext>`, numbered when taken.
/// Never a path that `taken` reports in use, so neither an existing file nor an original
/// is overwritten.
pub fn output_path(dir: &Path, source: &Path, format: Format, taken: &dyn Fn(&Path) -> bool) -> PathBuf {
    let stem = source.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| "photo".into());
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{stem}-twb.{}", format.ext()) } else { format!("{stem}-twb-{n}.{}", format.ext()) };
        let p = dir.join(name);
        if p != source && !p.exists() && !taken(&p) {
            return p;
        }
        n += 1;
    }
}

/// Resize and encode `pic` to `path`. `original_long` is the original file's long edge.
pub fn write(pic: &Picture, size: Size, original_long: u32, format: Format, record: &Record, path: &Path) -> Result<(), String> {
    let data = encode(pic, size, original_long, format, record)?;
    // written beside the target and moved into place, so a failed export leaves no half file
    let tmp = path.with_extension(format!("{}.part", format.ext()));
    let mut f = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    f.write_all(&data).map_err(|e| e.to_string())?;
    drop(f);
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        e.to_string()
    })
}

pub fn encode(pic: &Picture, size: Size, original_long: u32, format: Format, record: &Record) -> Result<Vec<u8>, String> {
    let (w, h) = (pic.w, pic.h);
    let long = match size {
        Size::Original => original_long.max(1),
        Size::Grid => w.max(h),
        Size::LongEdge(l) => l.max(1),
    };
    let (nw, nh) = crate::resize::fit_long(w, h, long);
    let neutral = pic.rgba.as_chunks::<4>().0.iter().all(|p| p[0] == p[1] && p[1] == p[2]);
    let channels = if neutral { 1 } else { 3 };
    let clip = |v: f32| v.clamp(0.0, 1.0);
    let planes: Vec<f32> = if neutral {
        pic.rgba.as_chunks::<4>().0.iter().map(|p| clip(p[1])).collect()
    } else {
        pic.rgba.as_chunks::<4>().0.iter().flat_map(|p| [clip(p[0]), clip(p[1]), clip(p[2])]).collect()
    };
    let values: Vec<f32> = crate::resize::f32s(&planes, w, h, nw, nh, channels).into_iter().map(clip).collect();
    let json = record.to_json();
    match format {
        Format::Png8 | Format::Png16 => {
            let mut out = Vec::new();
            {
                let mut enc = png::Encoder::new(&mut out, nw, nh);
                enc.set_color(if neutral { png::ColorType::Grayscale } else { png::ColorType::Rgb });
                let sixteen = format == Format::Png16;
                enc.set_depth(if sixteen { png::BitDepth::Sixteen } else { png::BitDepth::Eight });
                enc.set_source_srgb(png::SrgbRenderingIntent::Perceptual);
                enc.add_itxt_chunk(provenance::KEY.into(), json).map_err(|e| e.to_string())?;
                let mut wr = enc.write_header().map_err(|e| e.to_string())?;
                let bytes: Vec<u8> = if sixteen {
                    values.iter().flat_map(|v| ((v * 65535.0 + 0.5) as u16).to_be_bytes()).collect()
                } else {
                    values.iter().map(|v| (v * 255.0 + 0.5) as u8).collect()
                };
                wr.write_image_data(&bytes).map_err(|e| e.to_string())?;
                wr.finish().map_err(|e| e.to_string())?;
            }
            Ok(out)
        }
        Format::Jpeg => {
            let bytes: Vec<u8> = values.iter().map(|v| (v * 255.0 + 0.5) as u8).collect();
            let mut out = Vec::new();
            let mut enc = jpeg_encoder::Encoder::new(&mut out, JPEG_QUALITY);
            enc.set_sampling_factor(jpeg_encoder::SamplingFactor::R_4_4_4);
            let mut seg = provenance::JPEG_TAG.to_vec();
            seg.extend_from_slice(json.as_bytes());
            enc.add_app_segment(provenance::JPEG_APP, seg).map_err(|e| e.to_string())?;
            let ty = if neutral { jpeg_encoder::ColorType::Luma } else { jpeg_encoder::ColorType::Rgb };
            if nw > u16::MAX as u32 || nh > u16::MAX as u32 {
                return Err("too large for JPEG".into());
            }
            enc.encode(&bytes, nw as u16, nh as u16, ty).map_err(|e| e.to_string())?;
            Ok(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::params::Settings;

    fn ramp(w: u32, h: u32, tint: bool) -> Picture {
        let mut rgba = Vec::new();
        for _y in 0..h {
            for x in 0..w {
                let v = x as f32 / (w - 1) as f32;
                rgba.extend_from_slice(&[v, v * if tint { 0.9 } else { 1.0 }, v * if tint { 0.8 } else { 1.0 }, 1.0]);
            }
        }
        Picture { w, h, rgba }
    }

    fn record() -> Record {
        Record { source: "/photos/a b é.jpg".into(), look: Settings { grade: 2.25, ..Settings::default() } }
    }

    #[test]
    fn neutral_png_is_grey_srgb_and_resized() {
        let dir = std::env::temp_dir().join(format!("studio-export-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x-twb.png");
        write(&ramp(64, 32, false), Size::Original, 128, Format::Png8, &record(), &p).unwrap();
        let dec = png::Decoder::new(std::fs::File::open(&p).unwrap());
        let reader = dec.read_info().unwrap();
        let info = reader.info();
        assert_eq!((info.width, info.height), (128, 64));
        assert_eq!(info.color_type, png::ColorType::Grayscale);
        assert_eq!(info.bit_depth, png::BitDepth::Eight);
        assert!(info.srgb.is_some());
        assert_eq!(provenance::read(&p), Some(record()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sixteen_bit_and_tinted() {
        let data = encode(&ramp(20, 10, false), Size::Grid, 999, Format::Png16, &record()).unwrap();
        let img = image::load_from_memory(&data).unwrap();
        assert_eq!(img.color(), image::ColorType::L16);
        let px = img.into_luma16();
        assert_eq!(px.get_pixel(19, 0)[0], 65535);
        let data = encode(&ramp(20, 10, true), Size::LongEdge(40), 999, Format::Png8, &record()).unwrap();
        let img = image::load_from_memory(&data).unwrap();
        assert_eq!(img.color(), image::ColorType::Rgb8);
        assert_eq!((img.width(), img.height()), (40, 20));
    }

    #[test]
    fn jpeg_carries_record() {
        let dir = std::env::temp_dir().join(format!("studio-export-j-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("x-twb.jpg");
        write(&ramp(64, 48, false), Size::Grid, 0, Format::Jpeg, &record(), &p).unwrap();
        let img = image::open(&p).unwrap();
        assert_eq!(img.color(), image::ColorType::L8);
        assert_eq!(provenance::read(&p), Some(record()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn output_names_never_collide() {
        let dir = std::env::temp_dir().join(format!("studio-export-n-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("beach.jpg");
        let a = output_path(&dir, &src, Format::Jpeg, &|_| false);
        assert_eq!(a.file_name().unwrap(), "beach-twb.jpg");
        std::fs::write(&a, b"x").unwrap();
        let b = output_path(&dir, &src, Format::Jpeg, &|_| false);
        assert_eq!(b.file_name().unwrap(), "beach-twb-2.jpg");
        let c = output_path(&dir, &src, Format::Jpeg, &|p| p == b);
        assert_eq!(c.file_name().unwrap(), "beach-twb-3.jpg");
        // an original that happens to be named like an export is never the target
        let odd = dir.join("odd-twb.png");
        let d = output_path(&dir, &dir.join("odd.png"), Format::Png8, &|p| p == odd);
        assert_ne!(d, odd);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
