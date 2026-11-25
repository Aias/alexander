//! A uniform border round the picture, as `find_frame` in twb2.py finds it: the paper of a
//! photographed print, a mat, letterbox or pillarbox bars. Counted as part of the picture,
//! such a border moves automatic exposure and the house tones (a white one reads as the
//! brightest tones, a black one as the deepest), so both are measured inside it instead.
//! The test prefers missing a doubtful border to inventing one.

use crate::tone::{lin_to_srgb, srgb_to_lin};
use serde::{Deserialize, Serialize};

const TOL: f64 = 0.08; // tone step (display value, after 5x5 smoothing) that ends a band
const REACH: f64 = 0.30; // how far into the picture a band is followed, as a share of it
const WIDEST: f64 = 0.25; // widest band accepted
const STRAIGHT: f64 = 0.85; // share of a side whose inner edge must lie on one straight line
const SLOPE: f64 = 0.05; // steepest inner edge, about 3 degrees
const EVEN: f64 = 0.03; // largest spread of tone inside a band (paper texture, soft shadow)
const MATCH: f64 = 0.12; // largest difference in tone between the sides of one frame

/// The picture inside a border, as fractions of the width and height, so it holds for the
/// photo at any size.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Area {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
}

impl Area {
    /// The area in pixels of a `w` x `h` picture: left, top, right, bottom (exclusive).
    pub fn pixels(&self, w: u32, h: u32) -> [u32; 4] {
        let at = |f: f64, n: u32| ((f * n as f64 + 0.5).floor().max(0.0) as u32).min(n);
        [at(self.left, w), at(self.top, h), at(self.right, w), at(self.bottom, h)]
    }
}

struct Side {
    ok: bool,
    tone: f64,
    inner: f64,
}

fn median(v: &mut [f64]) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        0.5 * (v[n / 2 - 1] + v[n / 2])
    }
}

/// One side, seen through `at(i, j)`: depth `i` from the outer edge (of `n`) at position
/// `j` along it (of `m`).
fn side(at: &dyn Fn(usize, usize) -> f64, n: usize, m: usize) -> Side {
    let reach = (n as f64 * REACH) as usize;
    let none = Side { ok: false, tone: 0.0, inner: 0.0 };
    // the band of the edge's own tone, followed inward position by position
    let d: Vec<f64> = (0..m)
        .map(|j| {
            let t = at(2, j);
            (0..reach).find(|&i| (at(i, j) - t).abs() > TOL).unwrap_or(reach) as f64
        })
        .collect();
    // judged on the middle of the side, away from the corners where neighbouring bands cross
    let (lo, hi) = (m * 2 / 10, m * 8 / 10);
    if hi - lo < 4 {
        return none;
    }
    let pos: Vec<f64> = (lo..hi).map(|j| j as f64).collect();
    let dd = &d[lo..hi];
    let half = (hi - lo) / 2;
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let mut slope = (median(&mut dd[half..].to_vec()) - median(&mut dd[..half].to_vec())) / (mean(&pos[half..]) - mean(&pos[..half]));
    let mut icpt = median(&mut dd.iter().zip(&pos).map(|(d, p)| d - slope * p).collect::<Vec<_>>());
    let tol = (0.003 * n as f64).max(2.0);
    // medians find the edge among outliers (picture tones that continue the band); a least
    // squares fit to the points near it then places it to a fraction of a pixel
    for _ in 0..2 {
        let near: Vec<(f64, f64)> = pos.iter().zip(dd).filter(|(p, d)| (*d - (icpt + slope * *p)).abs() <= 3.0 * tol).map(|(p, d)| (*p, *d)).collect();
        if near.len() < 2 {
            break;
        }
        let px = near.iter().map(|v| v.0).sum::<f64>() / near.len() as f64;
        let py = near.iter().map(|v| v.1).sum::<f64>() / near.len() as f64;
        let sxy: f64 = near.iter().map(|v| (v.0 - px) * (v.1 - py)).sum();
        let sxx: f64 = near.iter().map(|v| (v.0 - px) * (v.0 - px)).sum();
        slope = sxy / sxx;
        icpt = py - slope * px;
    }
    let line: Vec<f64> = pos.iter().map(|p| icpt + slope * p).collect();
    let straight = dd.iter().zip(&line).filter(|(d, l)| (*d - *l).abs() <= tol).count() as f64 / dd.len() as f64;
    let inner = icpt.max(icpt + slope * (m - 1) as f64); // deepest point of the edge across the whole side
    let (mut n_in, mut sum, mut sum_off) = (0usize, 0.0, 0.0);
    let mut offs = Vec::new();
    for (k, j) in (lo..hi).enumerate() {
        let t = at(2, j);
        for i in 0..reach {
            if i as f64 + 3.0 < line[k] {
                let v = at(i, j);
                n_in += 1;
                sum += v;
                sum_off += v - t;
                offs.push(v - t);
            }
        }
    }
    let min_line = line.iter().cloned().fold(f64::INFINITY, f64::min);
    let tone = if n_in > 0 { sum / n_in as f64 } else { 0.0 };
    let mut ok = straight >= STRAIGHT && slope.abs() <= SLOPE && min_line >= (0.005 * n as f64).max(4.0) && inner <= WIDEST * n as f64 && n_in > 0;
    if ok {
        let m_off = sum_off / n_in as f64;
        let spread = (offs.iter().map(|o| (o - m_off) * (o - m_off)).sum::<f64>() / n_in as f64).sqrt();
        ok = spread <= EVEN;
    }
    Side { ok, tone, inner }
}

/// The picture area inside a uniform border of an 8-bit sRGB picture (four bytes a pixel),
/// or None. A border is a band of one even tone along two opposite sides (or all four)
/// whose inner edge is straight; its sides must agree in tone. The area stops a small
/// margin inside the band's deepest point, which allows for a print photographed a degree
/// or two askew.
pub fn find(rgba: &[u8], w: usize, h: usize) -> Option<Area> {
    if w.min(h) < 64 {
        return None;
    }
    let lin: Vec<f64> = (0..256).map(|i| srgb_to_lin(i as f64 / 255.0) as f32 as f64).collect();
    let g: Vec<f64> = rgba.as_chunks::<4>().0.iter().map(|p| lin_to_srgb(0.2126 * lin[p[0] as usize] + 0.7152 * lin[p[1] as usize] + 0.0722 * lin[p[2] as usize])).collect();
    let s = box5(&g, w, h);
    let at = |x: usize, y: usize| s[y * w + x];
    let sides = [
        side(&|i, j| at(j, i), h, w),         // top
        side(&|i, j| at(j, h - 1 - i), h, w), // bottom
        side(&|i, j| at(i, j), w, h),         // left
        side(&|i, j| at(w - 1 - i, j), w, h), // right
    ];
    let pair = |a: usize, b: usize| sides[a].ok && sides[b].ok && (sides[a].tone - sides[b].tone).abs() <= MATCH;
    let pairs: Vec<[usize; 2]> = [[0, 1], [2, 3]].into_iter().filter(|p| pair(p[0], p[1])).collect();
    if pairs.is_empty() {
        return None;
    }
    let tones: Vec<f64> = pairs.iter().flatten().map(|&k| sides[k].tone).collect();
    let reference = tones.iter().sum::<f64>() / tones.len() as f64;
    // the smoothing ends a band up to two pixels early; past that, a small safety margin
    let margin = 3 + (0.004 * w.max(h) as f64).ceil() as usize;
    let e: Vec<usize> = sides.iter().map(|s| if s.ok && (s.tone - reference).abs() <= MATCH { s.inner.ceil() as usize + margin } else { 0 }).collect();
    let (x0, y0, x1, y1) = (e[2] as f64, e[0] as f64, w as f64 - e[3] as f64, h as f64 - e[1] as f64);
    if x1 - x0 < w as f64 / 2.0 || y1 - y0 < h as f64 / 2.0 {
        return None;
    }
    Some(Area { left: x0 / w as f64, top: y0 / h as f64, right: x1 / w as f64, bottom: y1 / h as f64 })
}

/// 5 x 5 mean with the edge pixels repeated beyond the picture (scipy's uniform_filter,
/// mode "nearest").
fn box5(g: &[f64], w: usize, h: usize) -> Vec<f64> {
    let clamp = |v: isize, n: usize| v.clamp(0, n as isize - 1) as usize;
    let mut rows = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            let k = (-2..=2).map(|d| g[clamp(y as isize + d, h) * w + x]).sum::<f64>();
            rows[y * w + x] = k / 5.0;
        }
    }
    let mut out = vec![0.0; w * h];
    for y in 0..h {
        for x in 0..w {
            out[y * w + x] = (-2..=2).map(|d| rows[y * w + clamp(x as isize + d, w)]).sum::<f64>() / 5.0;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_and_framed() {
        let (w, h) = (200usize, 150usize);
        let mut rgba = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let inside = (20..185).contains(&x) && (12..130).contains(&y);
                let v = if inside { ((x * 7 + y * 13) % 200) as u8 } else { 240 };
                rgba[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[v, v, v, 255]);
            }
        }
        let a = find(&rgba, w, h).expect("a white border");
        let [x0, y0, x1, y1] = a.pixels(w as u32, h as u32);
        assert!(x0 >= 20 && y0 >= 12 && x1 <= 185 && y1 <= 130, "{:?}", [x0, y0, x1, y1]);
        let flat = vec![128u8; w * h * 4];
        assert_eq!(find(&flat, w, h), None);
    }
}
