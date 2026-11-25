//! Lanczos resizing, as PIL's `Image.LANCZOS` does for `twb2.py`.

use fast_image_resize::{images::Image, FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer};

fn options() -> ResizeOptions {
    ResizeOptions::new().resize_alg(ResizeAlg::Convolution(FilterType::Lanczos3)).use_alpha(false)
}

/// The size a picture of `w` x `h` takes when its long edge is `long`, rounded as PIL does.
pub fn fit_long(w: u32, h: u32, long: u32) -> (u32, u32) {
    let s = long as f64 / w.max(h) as f64;
    (((w as f64 * s).round() as u32).max(1), ((h as f64 * s).round() as u32).max(1))
}

/// Resize 8-bit RGBA. The values are resized as stored, in gamma space, like PIL on an
/// RGB image.
pub fn rgba8(src: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    if (w, h) == (nw, nh) {
        return src.to_vec();
    }
    let mut buf = src.to_vec();
    let s = Image::from_slice_u8(w, h, &mut buf, PixelType::U8x4).expect("source size");
    let mut d = Image::new(nw, nh, PixelType::U8x4);
    Resizer::new().resize(&s, &mut d, &options()).expect("resize");
    d.into_vec()
}

/// Resize planes of floats, `channels` values a pixel (1 or 3).
pub fn f32s(src: &[f32], w: u32, h: u32, nw: u32, nh: u32, channels: usize) -> Vec<f32> {
    if (w, h) == (nw, nh) {
        return src.to_vec();
    }
    let ty = if channels == 1 { PixelType::F32 } else { PixelType::F32x3 };
    let mut sbuf = src.to_vec();
    let mut dbuf = vec![0f32; nw as usize * nh as usize * channels];
    {
        let s = Image::from_slice_u8(w, h, bytemuck::cast_slice_mut(&mut sbuf), ty).expect("source size");
        let mut d = Image::from_slice_u8(nw, nh, bytemuck::cast_slice_mut(&mut dbuf), ty).expect("target size");
        Resizer::new().resize(&s, &mut d, &options()).expect("resize");
    }
    dbuf
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_edge_rounds_like_pil() {
        assert_eq!(fit_long(4000, 3000, 2560), (2560, 1920));
        assert_eq!(fit_long(3000, 4000, 640), (480, 640));
        assert_eq!(fit_long(1001, 333, 500), (500, 166));
    }

    #[test]
    fn flat_stays_flat() {
        let src = vec![0.25f32; 40 * 30];
        let out = f32s(&src, 40, 30, 17, 13, 1);
        assert!(out.iter().all(|v| (v - 0.25).abs() < 1e-5));
        let rgba = vec![100u8; 40 * 30 * 4];
        let out = rgba8(&rgba, 40, 30, 90, 70);
        assert!(out.iter().all(|&v| v == 100));
    }
}
