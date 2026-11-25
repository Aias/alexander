//! Film data and the curve mathematics of `twb2.py`, ported exactly.
//! Everything here runs on the CPU in double precision and produces small tables
//! and numbers for the GPU stages.

use serde_json::Value;

pub const FILM_DMAX: f64 = 3.4;
pub const GRAIN_UNIFORMITY: f64 = 0.6;
pub const APERTURE_UM2: f64 = std::f64::consts::PI * 24.0 * 24.0;

const TRIX_JSON: &str = include_str!("../../data/trix400.json");
const TRIX_FIT: &str = include_str!("../../data/trix400_mtf_fit.json");
const DOUBLEX_JSON: &str = include_str!("../../data/doublex5222.json");
const DOUBLEX_FIT: &str = include_str!("../../data/doublex5222_mtf_fit.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FilmKind {
    Trix1956,
    Trix,
    DoubleX,
}

impl FilmKind {
    pub const ALL: [FilmKind; 3] = [FilmKind::Trix1956, FilmKind::Trix, FilmKind::DoubleX];
    pub fn cli(self) -> &'static str {
        match self {
            FilmKind::Trix1956 => "trix-1956",
            FilmKind::Trix => "trix",
            FilmKind::DoubleX => "doublex",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FilmKind::Trix1956 => "Tri-X, as in 1956",
            FilmKind::Trix => "Tri-X Pan (datasheet)",
            FilmKind::DoubleX => "Double-X 5222",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum Filter {
    None,
    Yellow,
    Orange,
    Red,
}

impl Filter {
    pub const ALL: [Filter; 4] = [Filter::None, Filter::Yellow, Filter::Orange, Filter::Red];
    pub fn cli(self) -> &'static str {
        match self {
            Filter::None => "none",
            Filter::Yellow => "yellow",
            Filter::Orange => "orange",
            Filter::Red => "red",
        }
    }
}

/// One film: digitised datasheet data plus the fitted sharpness model.
#[derive(Clone, Debug)]
pub struct Film {
    pub curves: Vec<(f64, Vec<(f64, f64)>)>, // (minutes, [(log exposure, density)]) sorted by minutes
    pub rms_granularity: f64,
    pub weights: [[f64; 3]; 4], // by Filter, r g b
    pub iso: f64,
    pub default_dev: f64,
    pub granularity_ci: Option<f64>,
    pub granularity_dev: Option<f64>,
    pub emulsion_blur: f64,
    pub kappa: f64,
    pub inhibitor_sigma_um: f64,
    pub inhibitor_tail_um: f64,
    pub inhibitor_tail_weight: f64,
    pub optical_sigma_um: f64,
    pub optical_exponent: f64,
}

fn pairs(v: &Value) -> Vec<(f64, f64)> {
    v.as_array()
        .map(|a| a.iter().map(|p| (p[0].as_f64().unwrap(), p[1].as_f64().unwrap())).collect())
        .unwrap_or_default()
}

impl Film {
    pub fn load(kind: FilmKind) -> Film {
        let (data, fit) = match kind {
            FilmKind::Trix1956 | FilmKind::Trix => (TRIX_JSON, TRIX_FIT),
            FilmKind::DoubleX => (DOUBLEX_JSON, DOUBLEX_FIT),
        };
        let d: Value = serde_json::from_str(data).expect("film data");
        let f: Value = serde_json::from_str(fit).expect("film fit");
        let mut curves: Vec<(f64, Vec<(f64, f64)>)> = d["curves_logH_density"]
            .as_object()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.parse::<f64>().unwrap(), pairs(v)))
            .collect();
        curves.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut weights = [[0.0; 3]; 4];
        for (i, flt) in Filter::ALL.iter().enumerate() {
            let w = &d["rgb_weights"][flt.cli()];
            weights[i] = [w["r"].as_f64().unwrap(), w["g"].as_f64().unwrap(), w["b"].as_f64().unwrap()];
        }
        let mut film = Film {
            curves,
            rms_granularity: d["rms_granularity"].as_f64().unwrap(),
            weights,
            iso: d["iso"].as_f64().unwrap_or(400.0),
            default_dev: d["default_dev"].as_f64().unwrap_or(7.0),
            granularity_ci: d["granularity_contrast_index"].as_f64(),
            granularity_dev: d["granularity_dev"].as_f64(),
            emulsion_blur: 1.0,
            kappa: f["kappa"].as_f64().unwrap(),
            inhibitor_sigma_um: f["inhibitor_sigma_um"].as_f64().unwrap(),
            inhibitor_tail_um: f["inhibitor_tail_um"].as_f64().unwrap(),
            inhibitor_tail_weight: f["inhibitor_tail_weight"].as_f64().unwrap(),
            optical_sigma_um: f["optical_sigma_um"][0].as_f64().unwrap(),
            optical_exponent: f["optical_exponent"].as_f64().unwrap_or(2.0),
        };
        if kind == FilmKind::Trix1956 {
            // Kodak's 1956 classes for Tri-X: medium grain, 75-90 lines/mm (estimates)
            film.rms_granularity = 22.0;
            film.emulsion_blur = 1.33;
        }
        film
    }

    /// log10 lux-seconds given to an 18% grey by a meter at the film's speed.
    pub fn logh_grey(&self) -> f64 {
        -1.75 + (400.0 / self.iso).log10()
    }

    pub fn dev_times(&self) -> Vec<f64> {
        self.curves.iter().map(|c| c.0).collect()
    }

    /// The characteristic curve at a development time, as density on the grid `curve_x()`.
    pub fn curve(&self, dev_min: f64) -> Vec<f64> {
        let x = curve_x();
        let tables: Vec<Vec<f64>> = self
            .curves
            .iter()
            .map(|(_, pts)| {
                let px: Vec<f64> = pts.iter().map(|p| p.0).collect();
                let py: Vec<f64> = pts.iter().map(|p| p.1).collect();
                let n = px.len();
                let end = (py[n - 1] - py[n - 3]) / (px[n - 1] - px[n - 3]);
                x.iter()
                    .map(|&xi| {
                        if xi > px[n - 1] {
                            py[n - 1] + end * 0.6 * (1.0 - (-(xi - px[n - 1]) / 0.6).exp())
                        } else {
                            interp(xi, &px, &py)
                        }
                    })
                    .collect()
            })
            .collect();
        let times = self.dev_times();
        let t = dev_min.max(times[0]).min(*times.last().unwrap());
        let hi = times.iter().position(|&v| v >= t).unwrap();
        let lo = hi.saturating_sub(1);
        let f = if hi == lo { 0.0 } else { (t - times[lo]) / (times[hi] - times[lo]) };
        (0..x.len()).map(|i| (1.0 - f) * tables[lo][i] + f * tables[hi][i]).collect()
    }

    pub fn granularity_contrast_index(&self) -> f64 {
        match self.granularity_ci {
            Some(ci) => ci,
            None => contrast_index(&curve_x(), &self.curve(self.granularity_dev.unwrap_or(self.default_dev))),
        }
    }

    /// Development time at which `measure` of the curve reaches `target`.
    pub fn dev_for(&self, measure: fn(&[f64], &[f64]) -> f64, target: f64) -> f64 {
        let x = curve_x();
        let times = self.dev_times();
        let (mut lo, mut hi) = (times[0], *times.last().unwrap());
        if measure(&x, &self.curve(lo)) >= target {
            return lo;
        }
        if measure(&x, &self.curve(hi)) <= target {
            return hi;
        }
        for _ in 0..30 {
            let mid = 0.5 * (lo + hi);
            if measure(&x, &self.curve(mid)) < target {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        (0.5 * (lo + hi) * 100.0).round() / 100.0
    }

    /// Area of one developed grain reproducing a catalogue granularity figure.
    pub fn grain_area_um2(rms_granularity: f64) -> f64 {
        let sigma = rms_granularity / 1000.0;
        sigma * sigma * APERTURE_UM2 / (1.0 * (FILM_DMAX - GRAIN_UNIFORMITY * 1.0))
    }
}

/// The log-exposure grid every film curve is tabulated on: -4.5 to 2.0 in steps of 0.01.
pub const CURVE_X0: f64 = -4.5;
pub const CURVE_DX: f64 = 0.01;
pub const CURVE_N: usize = 651;

pub fn curve_x() -> Vec<f64> {
    (0..CURVE_N).map(|i| CURVE_X0 + CURVE_DX * i as f64).collect()
}

/// numpy.interp: linear interpolation, clamped at both ends.
pub fn interp(x: f64, xp: &[f64], fp: &[f64]) -> f64 {
    let n = xp.len();
    if x <= xp[0] {
        return fp[0];
    }
    if x >= xp[n - 1] {
        return fp[n - 1];
    }
    // largest j with xp[j] <= x
    let (mut lo, mut hi) = (0usize, n - 1);
    while hi - lo > 1 {
        let mid = (lo + hi) / 2;
        if xp[mid] <= x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    if x == xp[lo] || xp[lo + 1] == xp[lo] {
        return fp[lo];
    }
    fp[lo] + (fp[lo + 1] - fp[lo]) * (x - xp[lo]) / (xp[lo + 1] - xp[lo])
}

/// numpy.gradient for unevenly or evenly spaced x (second order inside, first order at ends).
fn gradient(y: &[f64], x: &[f64]) -> Vec<f64> {
    let n = y.len();
    let mut g = vec![0.0; n];
    g[0] = (y[1] - y[0]) / (x[1] - x[0]);
    g[n - 1] = (y[n - 1] - y[n - 2]) / (x[n - 1] - x[n - 2]);
    for i in 1..n - 1 {
        let hd = x[i + 1] - x[i];
        let hs = x[i] - x[i - 1];
        g[i] = (hs * hs * y[i + 1] + (hd * hd - hs * hs) * y[i] - hd * hd * y[i - 1]) / (hs * hd * (hd + hs));
    }
    g
}

/// Slope of the curve's straight part, taken between net densities 0.6 and 1.0.
pub fn curve_gamma(cx: &[f64], cy: &[f64]) -> f64 {
    let base = cy.iter().cloned().fold(f64::INFINITY, f64::min);
    let g = gradient(cy, cx);
    let (mut s, mut n) = (0.0, 0);
    for i in 0..cy.len() {
        let net = cy[i] - base;
        if net > 0.6 && net < 1.0 {
            s += g[i];
            n += 1;
        }
    }
    s / n as f64
}

/// Kodak's contrast index: the slope of the straight line through two points of the curve
/// that lie 0.2 and 2.2 units from one point on the base-plus-fog line.
pub fn contrast_index(cx: &[f64], cy: &[f64]) -> f64 {
    let base = cy.iter().cloned().fold(f64::INFINITY, f64::min);
    let hit = |c: f64, r: f64| -> Option<(f64, f64)> {
        let d: Vec<f64> = cx.iter().zip(cy).map(|(&x, &y)| ((x - c).hypot(y - base)) - r).collect();
        for i in 0..d.len() - 1 {
            if d[i] < 0.0 && d[i + 1] >= 0.0 && cx[i] >= c {
                let t = -d[i] / (d[i + 1] - d[i]);
                return Some((cx[i] + t * (cx[i + 1] - cx[i]), cy[i] + t * (cy[i + 1] - cy[i])));
            }
        }
        None
    };
    let mut best: Option<(f64, f64)> = None;
    let stop = cx[cx.len() - 1] - 2.2;
    let mut k = 0usize;
    loop {
        let c = cx[0] + 0.005 * k as f64;
        if c >= stop {
            break;
        }
        k += 1;
        if let (Some(a), Some(b)) = (hit(c, 0.2), hit(c, 2.2)) {
            let err = ((a.1 - base).atan2(a.0 - c) - (b.1 - base).atan2(b.0 - c)).abs();
            if best.map_or(true, |bst| err < bst.0) {
                best = Some((err, (b.1 - base) / (b.0 - c)));
            }
        }
    }
    best.map(|b| b.1).unwrap_or(f64::NAN)
}

/// Development as the user gives it: minutes, or a period's recommended development.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Dev {
    Minutes(f64),
    Norm1952,
    Norm1956,
    Norm1981,
}

impl Dev {
    pub fn cli(self) -> String {
        match self {
            Dev::Minutes(m) => format!("{}", (m * 100.0).round() / 100.0),
            Dev::Norm1952 => "norm-1952".into(),
            Dev::Norm1956 => "norm-1956".into(),
            Dev::Norm1981 => "norm-1981".into(),
        }
    }
}

impl Film {
    pub fn resolve_dev(&self, dev: Option<Dev>) -> f64 {
        match dev {
            None => self.default_dev,
            Some(Dev::Minutes(m)) => m,
            Some(Dev::Norm1952) => self.dev_for(curve_gamma, 0.83),
            Some(Dev::Norm1956) => self.dev_for(curve_gamma, 0.71),
            Some(Dev::Norm1981) => self.dev_for(contrast_index, self.granularity_contrast_index()),
        }
    }

    /// Factor on granularity for the chosen development (1 at the datasheet's own).
    pub fn development_grain(&self, dev_min: f64, grain_dev: f64) -> f64 {
        if grain_dev <= 0.0 {
            return 1.0;
        }
        (contrast_index(&curve_x(), &self.curve(dev_min)) / self.granularity_contrast_index()).powf(grain_dev)
    }
}
