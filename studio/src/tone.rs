//! Paper, tone and curve helpers of `twb2.py`, ported exactly.

use crate::film::interp;
use serde_json::Value;

const POLYMAX_JSON: &str = include_str!("../../data/polymax.json");

pub const PAPER_X0: f64 = -0.5;
pub const PAPER_DX: f64 = 0.005;
pub const PAPER_N: usize = 901;
pub const PAPER_CREAM: [f64; 3] = [0.957, 0.937, 0.882];

/// The tone range the book's pages share (see docs/RESEARCH.md): display values at these
/// percentiles of a page.
pub const HOUSE_PCT: [f64; 5] = [2.0, 10.0, 25.0, 90.0, 98.0];
pub const HOUSE_VAL: [f64; 5] = [0.08, 0.11, 0.22, 0.95, 1.00];
pub const HOUSE_MAX_SLOPE: f64 = 2.0;

pub fn srgb_to_lin(x: f64) -> f64 {
    if x <= 0.04045 {
        x / 12.92
    } else {
        ((x + 0.055) / 1.055).powf(2.4)
    }
}

pub fn lin_to_srgb(x: f64) -> f64 {
    let x = x.max(0.0);
    if x <= 0.0031308 {
        x * 12.92
    } else {
        1.055 * x.powf(1.0 / 2.4) - 0.055
    }
}

pub fn paper_x() -> Vec<f64> {
    (0..PAPER_N).map(|i| PAPER_X0 + PAPER_DX * i as f64).collect()
}

/// Polymax curve at a (possibly fractional) grade, on the grid `paper_x()`.
pub fn paper_curve(grade: f64) -> Vec<f64> {
    let d: Value = serde_json::from_str(POLYMAX_JSON).expect("paper data");
    let x = paper_x();
    let mut tables: Vec<(f64, Vec<f64>)> = d["curves_logH_density"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| k.as_str() != "unfiltered")
        .map(|(k, v)| {
            let pts: Vec<(f64, f64)> = v.as_array().unwrap().iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect();
            let px: Vec<f64> = pts.iter().map(|p| p.0).collect();
            let py: Vec<f64> = pts.iter().map(|p| p.1).collect();
            (k.parse::<f64>().unwrap(), x.iter().map(|&xi| interp(xi, &px, &py)).collect())
        })
        .collect();
    tables.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
    let g = grade.max(tables[0].0).min(tables.last().unwrap().0);
    let hi = tables.iter().position(|t| t.0 >= g).unwrap();
    let lo = hi.saturating_sub(1);
    let f = if hi == lo { 0.0 } else { (g - tables[lo].0) / (tables[hi].0 - tables[lo].0) };
    let mut y: Vec<f64> = (0..x.len()).map(|i| (1.0 - f) * tables[lo].1[i] + f * tables[hi].1[i]).collect();
    for i in 1..y.len() {
        if y[i] < y[i - 1] {
            y[i] = y[i - 1]; // np.maximum.accumulate
        }
    }
    y
}

pub struct Paper {
    pub curve: Vec<f64>,
    pub dmin: f64,
    pub range: f64,
}

impl Paper {
    pub fn new(grade: f64) -> Paper {
        let curve = paper_curve(grade);
        let dmin = curve.iter().cloned().fold(f64::INFINITY, f64::min);
        let dmax = curve.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        Paper { curve, dmin, range: dmax - dmin }
    }

    /// Log exposure offset that prints a metered grey (negative density `ref_density`) at
    /// `print_grey` above the paper's white, shifted by `print_ev` stops.
    pub fn offset(&self, ref_density: f64, print_grey: f64, print_ev: f64) -> f64 {
        interp(self.dmin + print_grey, &self.curve, &paper_x()) + ref_density + print_ev * 2f64.log10()
    }
}

/// tau of `compress_range`: t = d_out (1 - e^(-d/tau)) / (1 - e^(-d_in/tau)). None when no compression is needed.
pub fn compress_tau(d_in_max: f64, d_out_max: f64) -> Option<f64> {
    if d_out_max >= d_in_max {
        return None;
    }
    let (mut lo, mut hi) = (0.05f64, 50.0f64);
    let mut tau = 1.0;
    for _ in 0..60 {
        tau = 0.5 * (lo + hi);
        if tau * (1.0 - (-d_in_max / tau).exp()) < d_out_max {
            lo = tau;
        } else {
            hi = tau;
        }
    }
    Some(tau)
}

/// scipy's PchipInterpolator: a monotone cubic through the knots.
pub struct Pchip {
    x: Vec<f64>,
    y: Vec<f64>,
    d: Vec<f64>,
}

impl Pchip {
    pub fn new(x: &[f64], y: &[f64]) -> Pchip {
        let n = x.len();
        let h: Vec<f64> = (0..n - 1).map(|i| x[i + 1] - x[i]).collect();
        let m: Vec<f64> = (0..n - 1).map(|i| (y[i + 1] - y[i]) / h[i]).collect();
        let mut d = vec![0.0; n];
        if n == 2 {
            d[0] = m[0];
            d[1] = m[0];
        } else {
            for k in 1..n - 1 {
                if m[k - 1] * m[k] <= 0.0 {
                    d[k] = 0.0;
                } else {
                    let w1 = 2.0 * h[k] + h[k - 1];
                    let w2 = h[k] + 2.0 * h[k - 1];
                    d[k] = (w1 + w2) / (w1 / m[k - 1] + w2 / m[k]);
                }
            }
            let sign = |v: f64| -> i32 { (v > 0.0) as i32 - (v < 0.0) as i32 };
            let edge = |h0: f64, h1: f64, m0: f64, m1: f64| -> f64 {
                let d = ((2.0 * h0 + h1) * m0 - h0 * m1) / (h0 + h1);
                if sign(d) != sign(m0) {
                    0.0
                } else if sign(m0) != sign(m1) && d.abs() > 3.0 * m0.abs() {
                    3.0 * m0
                } else {
                    d
                }
            };
            d[0] = edge(h[0], h[1], m[0], m[1]);
            d[n - 1] = edge(h[n - 2], h[n - 3], m[n - 2], m[n - 3]);
        }
        Pchip { x: x.to_vec(), y: y.to_vec(), d }
    }

    pub fn eval(&self, v: f64) -> f64 {
        let n = self.x.len();
        let mut k = 0;
        while k < n - 2 && v >= self.x[k + 1] {
            k += 1;
        }
        let h = self.x[k + 1] - self.x[k];
        let t = (v - self.x[k]) / h;
        let (t2, t3) = (t * t, t * t * t);
        (2.0 * t3 - 3.0 * t2 + 1.0) * self.y[k]
            + (t3 - 2.0 * t2 + t) * h * self.d[k]
            + (-2.0 * t3 + 3.0 * t2) * self.y[k + 1]
            + (t3 - t2) * h * self.d[k + 1]
    }
}

pub const HOUSE_LUT_N: usize = 2048;

/// The house-tone curve as a table: display value of the print in, linear reflectance out.
/// `own` holds the print's display values at `HOUSE_PCT`.
pub fn house_lut(own: &[f64; 5], strength: f64) -> Vec<f32> {
    let mut xs = vec![0.0f64];
    let mut ys = vec![0.0f64];
    for i in 0..5 {
        let want = (1.0 - strength) * own[i] + strength * HOUSE_VAL[i];
        if own[i] - xs[xs.len() - 1] > 0.005 && own[i] < 0.995 {
            xs.push(own[i]);
            ys.push(want);
        }
    }
    xs.push(1.0);
    ys.push(1.0);
    for i in 1..xs.len() {
        ys[i] = ys[i].min(ys[i - 1] + HOUSE_MAX_SLOPE * (xs[i] - xs[i - 1]));
        ys[i] = ys[i].max(ys[i - 1] + 1e-4);
    }
    let p = Pchip::new(&xs, &ys);
    (0..HOUSE_LUT_N)
        .map(|i| {
            let v = i as f64 / (HOUSE_LUT_N - 1) as f64;
            srgb_to_lin(p.eval(v).clamp(0.0, 1.0)) as f32
        })
        .collect()
}

/// A table that leaves tones alone (display value in, linear reflectance out).
pub fn identity_lut() -> Vec<f32> {
    (0..HOUSE_LUT_N).map(|i| srgb_to_lin(i as f64 / (HOUSE_LUT_N - 1) as f64) as f32).collect()
}

/// numpy.percentile (linear interpolation) for values in [0, 1], through a fine histogram.
pub fn percentiles(values: &[f32], pcts: &[f64]) -> Vec<f64> {
    const BINS: usize = 1 << 16;
    let mut hist = vec![0u32; BINS];
    for &v in values {
        let b = ((v.clamp(0.0, 1.0) as f64) * (BINS - 1) as f64 + 0.5) as usize;
        hist[b] += 1;
    }
    let n = values.len() as f64;
    pcts.iter()
        .map(|&p| {
            let rank = p / 100.0 * (n - 1.0); // position in the sorted array
            let (lo, hi) = (rank.floor(), rank.ceil());
            let at = |r: f64| -> f64 {
                let mut acc = 0.0;
                for (b, &c) in hist.iter().enumerate() {
                    acc += c as f64;
                    if acc > r {
                        return b as f64 / (BINS - 1) as f64;
                    }
                }
                1.0
            };
            let (a, b) = (at(lo), at(hi));
            a + (b - a) * (rank - lo)
        })
        .collect()
}

/// Where a photo's tones sit once the film's colour response has turned it to grey: the
/// median and the 98th percentile, as display values. `rgba` is 8-bit sRGB, four bytes a
/// pixel; large photos are read at every second pixel, which moves neither figure.
pub fn exposure_levels(rgba: &[u8], w: usize, h: usize, weights: [f64; 3]) -> (f64, f64) {
    exposure_levels_in(rgba, w, h, weights, [0, 0, w as u32, h as u32])
}

/// `exposure_levels` read inside `area` alone (left, top, right, bottom in pixels).
pub fn exposure_levels_in(rgba: &[u8], w: usize, h: usize, weights: [f64; 3], area: [u32; 4]) -> (f64, f64) {
    use rayon::prelude::*;
    let lin: Vec<f32> = (0..256).map(|i| srgb_to_lin(i as f64 / 255.0) as f32).collect();
    let step = if w * h > 1_500_000 { 2 } else { 1 };
    let wt = [weights[0] as f32, weights[1] as f32, weights[2] as f32];
    let [x0, y0, x1, y1] = area.map(|v| v as usize);
    let rows: Vec<usize> = (y0..y1).step_by(step).collect();
    let values: Vec<f32> = rows
        .par_iter()
        .flat_map_iter(|&y| {
            let row = &rgba[y * w * 4..(y + 1) * w * 4];
            let lin = &lin;
            (x0..x1).step_by(step).map(move |x| {
                let p = &row[x * 4..x * 4 + 3];
                let e = wt[0] * lin[p[0] as usize] + wt[1] * lin[p[1] as usize] + wt[2] * lin[p[2] as usize];
                lin_to_srgb(e as f64) as f32
            })
        })
        .collect();
    let p = percentiles(&values, &[50.0, 98.0]);
    (p[0], p[1])
}

/// The automatic exposure of `film_exposure`: stops to add so the picture's median and its
/// brightest tones land where a careful exposure would put them.
pub fn auto_ev(median: f64, top: f64, target: f64) -> f64 {
    let lin = srgb_to_lin;
    let auto = 0.5 * ((lin(target) / lin(median).max(1e-4)).log2() + (lin(0.90) / lin(top).max(1e-4)).log2());
    auto.clamp(-1.0, 2.5)
}
