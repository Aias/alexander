//! From settings to the numbers each GPU stage needs. One `Plan` describes one render.
//! Stages compare their part of the plan with the last one drawn and skip work when
//! nothing upstream changed.

use crate::film::{contrast_index, curve_x, interp, Dev, Film, FilmKind, CURVE_N};
use crate::params::{Repro, Settings, View};
use crate::tone::{compress_tau, Paper, PAPER_N};

/// A blur as a weighted sum of Gaussians. `sigma` 0 stands for the unblurred image.
#[derive(Clone, Debug, PartialEq)]
pub struct Mixture(pub Vec<(f64, f64)>);

#[derive(Clone, Debug, PartialEq)]
pub struct OpticsPlan {
    pub weights: [f32; 3],
    pub scale: u32, // the grid is the source enlarged by this (book press on small pages)
    pub blur: Mixture,
    pub halation: f64,
    pub halation_sigmas: [f64; 3],
}

#[derive(Clone, Debug, PartialEq)]
pub struct DevelopPlan {
    pub gain: f32,
    pub curve: Vec<f32>,
    pub base: f32,
    pub logh_grey: f32,
    pub ref_density: f32,
    pub edge_on: bool,
    pub k: f32,
    pub tail_weight: f32,
    pub sigma_core: f64,
    pub sigma_tail: f64,
    pub edge_density: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GrainPlan {
    pub on: bool,
    pub grains_per_px: f32,
    pub seed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PaperPlan {
    pub curve: Vec<f32>,
    pub dmin: f32,
    pub offset: f32,
    pub grain_light: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FinishPlan {
    pub house: f64,
    pub repro: Repro,
    pub black: f32, // reflectance of the print's own black
    // copied from a printed page
    pub copy_solid: f32,
    pub copy_tau: f32, // 0 when no compression is needed
    pub copy_dmax_in: f32,
    pub copy_ink: f32,
    pub copy_drop: f32,
    pub copy_fill: f32,
    pub copy_gain: f32,
    pub copy_sigma: f64,
    // book press
    pub press_solid: f32,
    pub press_tau: f32,
    pub press_ink: f32,
    pub press_copy_sigma: f64,
    pub press_cell_sigma: f64,
    pub press_scatter_sigma: f64,
    pub min_dot: f32,
    pub dot_gain: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ViewPlan {
    pub mode: View,
    pub solid: f32,
    pub scan_black: f32,
    pub scan_white: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub source: (u32, u32), // the photo's own grid
    pub grid: (u32, u32),   // simulation size: the source times `optics.scale`
    pub optics: OpticsPlan,
    pub develop: DevelopPlan,
    pub grain: GrainPlan,
    pub enlarger_sigma: f64,
    pub paper: PaperPlan,
    pub finish: FinishPlan,
    pub view: ViewPlan,
    pub dev_minutes: f64,
    pub auto_ev: f64,
    pub um: f64,
}

/// Turns settings into plans. Holds the film data and remembers the slow derived values
/// (development time for a period norm, contrast index, the blur fit) between calls, since
/// a slider drag changes one of them at most.
pub struct Planner {
    films: Vec<(FilmKind, Film)>,
    dev: Option<((FilmKind, Option<Dev>), f64)>,
    grain_dev: Option<((FilmKind, u64, u64), f64)>,
    paper: Option<(u64, Paper)>,
    blur: Option<([u64; 5], Mixture)>,
}

impl Default for Planner {
    fn default() -> Self {
        Self::new()
    }
}

impl Planner {
    pub fn new() -> Planner {
        Planner { films: FilmKind::ALL.iter().map(|&k| (k, Film::load(k))).collect(), dev: None, grain_dev: None, paper: None, blur: None }
    }

    pub fn film(&self, kind: FilmKind) -> &Film {
        &self.films.iter().find(|f| f.0 == kind).unwrap().1
    }

    /// Development time in minutes for the settings.
    pub fn dev_minutes(&mut self, s: &Settings) -> f64 {
        let key = (s.film, s.dev);
        if let Some((k, v)) = &self.dev {
            if *k == key {
                return *v;
            }
        }
        let v = self.film(s.film).resolve_dev(s.dev);
        self.dev = Some((key, v));
        v
    }

    /// `source` is the photo's grid in pixels, `auto_ev` the automatic exposure measured
    /// for it (ignored when automatic exposure is off), `max_dim` the largest texture the
    /// device can hold.
    pub fn plan(&mut self, s: &Settings, source: (u32, u32), auto_ev: f64, max_dim: u32) -> Plan {
        let dev = self.dev_minutes(s);
        let gkey = (s.film, dev.to_bits(), s.grain_dev.to_bits());
        let dev_grain = match &self.grain_dev {
            Some((k, v)) if *k == gkey => *v,
            _ => {
                let v = self.film(s.film).development_grain(dev, s.grain_dev);
                self.grain_dev = Some((gkey, v));
                v
            }
        };
        if self.paper.as_ref().is_none_or(|p| p.0 != s.grade.to_bits()) {
            self.paper = Some((s.grade.to_bits(), Paper::new(s.grade)));
        }
        let film = self.films.iter().find(|f| f.0 == s.film).unwrap().1.clone();
        let paper = &self.paper.as_ref().unwrap().1;

        let long_src = source.0.max(source.1) as f64;
        let page_mm = s.page_mm.unwrap_or(long_src / s.ppi * 25.4);
        let scale = if s.repro == Repro::Book {
            let want = ((1.2 * (page_mm / long_src) / s.paper_scatter_mm.max(1e-6)).ceil() as i64).clamp(1, 4) as u32;
            want.min((max_dim as f64 / long_src).floor().max(1.0) as u32)
        } else {
            1
        };
        let grid = (source.0 * scale, source.1 * scale);
        let long_edge = long_src * scale as f64;
        let um = s.film_mm * 1000.0 / long_edge;
        let mm = page_mm / long_edge;

        // lens softness and scatter in the emulsion
        let w = film.weights[s.filter as usize];
        let opt = film.optical_sigma_um * film.emulsion_blur / um;
        let lens = s.lens_um.max(0.0) / um;
        let max_sigma = long_edge / 3.0;
        let bkey = [lens.to_bits(), s.lens_exponent.to_bits(), opt.to_bits(), film.optical_exponent.to_bits(), max_sigma.to_bits()];
        let blur = match &self.blur {
            Some((k, m)) if *k == bkey => m.clone(),
            _ => {
                let m = soften(lens, s.lens_exponent, opt, film.optical_exponent, max_sigma);
                self.blur = Some((bkey, m.clone()));
                m
            }
        };
        let hs = s.halation_um / um;
        let optics = OpticsPlan {
            weights: [w[0] as f32, w[1] as f32, w[2] as f32],
            scale,
            blur,
            halation: s.halation.max(0.0),
            halation_sigmas: [hs, hs * 2f64.sqrt(), hs * 3f64.sqrt()],
        };

        // development
        let auto_ev = if s.auto_ev { auto_ev } else { 0.0 };
        let cx = curve_x();
        let cy = film.curve(dev);
        let base = cy.iter().cloned().fold(f64::INFINITY, f64::min);
        let logh_grey = film.logh_grey();
        let ref_density = interp(logh_grey, &cx, &cy) - base;
        let gamma = crate::film::curve_gamma(&cx, &cy);
        debug_assert_eq!(cy.len(), CURVE_N);
        let develop = DevelopPlan {
            gain: 2f64.powf(s.ev + auto_ev) as f32,
            curve: cy.iter().map(|&v| v as f32).collect(),
            base: base as f32,
            logh_grey: logh_grey as f32,
            ref_density: ref_density as f32,
            edge_on: !s.no_edge,
            k: (s.edge * film.kappa / gamma) as f32,
            tail_weight: film.inhibitor_tail_weight as f32,
            sigma_core: film.inhibitor_sigma_um / um,
            sigma_tail: film.inhibitor_tail_um / um,
            edge_density: s.edge_density.max(0.0) as f32,
        };

        // grain
        let grain = GrainPlan {
            on: s.grain > 0.0,
            grains_per_px: if s.grain > 0.0 { (um * um / Film::grain_area_um2(film.rms_granularity * s.grain * dev_grain)) as f32 } else { 0.0 },
            seed: s.seed,
        };

        // print
        let paper_plan = PaperPlan {
            curve: paper.curve.iter().map(|&v| v as f32).collect(),
            dmin: paper.dmin as f32,
            offset: paper.offset(ref_density, s.print_grey, s.print_ev) as f32,
            grain_light: if s.grain > 0.0 { s.grain_light.clamp(0.0, 1.0) as f32 } else { 1.0 },
        };
        debug_assert_eq!(paper_plan.curve.len(), PAPER_N);

        // finish
        let black = 10f64.powf(-paper.range);
        let press_solid = 10f64.powf(-s.ink_density);
        let finish = FinishPlan {
            house: s.house.clamp(0.0, 1.0),
            repro: s.repro,
            black: black as f32,
            copy_solid: 10f64.powf(-s.copy_ink) as f32,
            copy_tau: compress_tau(paper.range, s.copy_ink).unwrap_or(0.0) as f32,
            copy_dmax_in: paper.range as f32,
            copy_ink: s.copy_ink as f32,
            copy_drop: s.copy_drop as f32,
            copy_fill: s.copy_fill as f32,
            copy_gain: s.copy_gain as f32,
            copy_sigma: s.copy_blur / 1000.0 * long_edge,
            press_solid: press_solid as f32,
            press_tau: compress_tau(paper.range, s.ink_density).unwrap_or(0.0) as f32,
            press_ink: s.ink_density as f32,
            press_copy_sigma: s.copy_blur_mm / mm,
            press_cell_sigma: 0.25 * (25.4 / s.lpi) / mm,
            press_scatter_sigma: s.paper_scatter_mm / mm,
            min_dot: s.min_dot as f32,
            dot_gain: s.dot_gain as f32,
        };
        let view = ViewPlan {
            mode: s.view_mode(),
            solid: if s.repro == Repro::Book { press_solid as f32 } else { 0.0 },
            scan_black: s.scan_black as f32,
            scan_white: s.scan_white as f32,
        };
        let _ = contrast_index;
        Plan {
            source,
            grid,
            optics,
            develop,
            grain,
            enlarger_sigma: s.enlarger_um.max(0.0) / um,
            paper: paper_plan,
            finish,
            view,
            dev_minutes: dev,
            auto_ev,
            um,
        }
    }
}

/// Widest Gaussian drawn at full size. Wider ones are drawn on a reduced copy.
pub const FULL_RES_SIGMA: f64 = 2.38;

/// Sampled, normalised Gaussian, as `scipy.ndimage.gaussian_filter` with truncate 3.5 builds it.
pub fn gauss_kernel(sigma: f64) -> Vec<f64> {
    let r = (3.5 * sigma + 0.5) as i64;
    let mut w: Vec<f64> = (-r..=r).map(|i| (-0.5 * (i as f64 / sigma).powi(2)).exp()).collect();
    let s: f64 = w.iter().sum();
    w.iter_mut().for_each(|v| *v /= s);
    w
}

/// Response of a Gaussian, as the engine draws it, at frequency f (cycles per pixel).
fn kernel_response(sigma: f64, f: f64) -> f64 {
    if sigma < 0.15 {
        return 1.0;
    }
    if sigma >= FULL_RES_SIGMA {
        return (-2.0 * std::f64::consts::PI.powi(2) * sigma * sigma * f * f).exp(); // drawn at reduced size; close to ideal
    }
    let w = gauss_kernel(sigma);
    let r = (w.len() / 2) as i64;
    (-r..=r).zip(&w).map(|(i, &v)| v * (2.0 * std::f64::consts::PI * f * i as f64).cos()).sum()
}

/// Transfer function exp(-ln 2 (f / f50)^n) of `blur_shaped`: n = 2 is the Gaussian of that sigma.
fn shaped_response(sigma: f64, n: f64, f: f64) -> f64 {
    if sigma < 0.15 {
        return 1.0;
    }
    let f50 = (2f64.ln() / (2.0 * std::f64::consts::PI.powi(2))).sqrt() / sigma;
    (-(2f64.ln()) * (f / f50).powf(n)).exp()
}

/// The lens softness (sigma `lens`, exponent `n_lens`) followed by scatter in the emulsion
/// (`emul`, `n_emul`), all in pixels, as a mixture of Gaussians no wider than `max_sigma`.
/// A long-tailed blur has no separable kernel, but Gaussians an octave apart reproduce its
/// transfer function closely, and all but the narrowest two or three are drawn on reduced
/// copies of the picture at almost no cost.
pub fn soften(lens: f64, n_lens: f64, emul: f64, n_emul: f64, max_sigma: f64) -> Mixture {
    let gauss = |n: f64| (n - 2.0).abs() < 1e-6;
    let (lens, emul) = (if lens < 0.15 { 0.0 } else { lens }, if emul < 0.15 { 0.0 } else { emul });
    // the Gaussian part, applied to every component
    let mut g2 = 0.0;
    if gauss(n_lens) {
        g2 += lens * lens;
    }
    if gauss(n_emul) {
        g2 += emul * emul;
    }
    let tails: Vec<(f64, f64)> = [(lens, n_lens), (emul, n_emul)].into_iter().filter(|&(s, n)| s > 0.0 && !gauss(n)).collect();
    if tails.is_empty() {
        let s = g2.sqrt();
        return Mixture(vec![(if s < 0.15 { 0.0 } else { s }, 1.0)]);
    }
    let scale = tails.iter().map(|t| t.0).fold(0.0, f64::max);
    let pi2 = std::f64::consts::PI.powi(2);
    let want = |f: f64| tails.iter().map(|&(s, n)| shaped_response(s, n, f)).product::<f64>() * (-2.0 * pi2 * g2 * f * f).exp();
    // Frequencies for the fit: evenly up to the grid's limit, and again on a logarithmic
    // scale so the slow fall at low frequencies (the long skirt) is fitted too.
    let fit_f: Vec<f64> = (0..120).map(|i| 0.5 * (i as f64 + 0.5) / 120.0).chain((0..40).map(|i| 0.5 * 10f64.powf(-3.0 * (1.0 - i as f64 / 40.0)))).collect();
    let fit_b: Vec<f64> = fit_f.iter().map(|&f| want(f)).collect();
    let test_f: Vec<f64> = (0..=100).map(|i| 0.5 * i as f64 / 100.0).collect();
    let test_b: Vec<f64> = test_f.iter().map(|&f| want(f)).collect();
    let top = (80.0 * scale).min(max_sigma.max(1.0));
    let mut best: Option<(f64, Vec<(f64, f64)>)> = None;
    for base in [0.2, 0.23, 0.26, 0.3, 0.34, 0.39] {
        let mut cand: Vec<f64> = vec![g2.sqrt()];
        let mut m = base * scale;
        while m <= top {
            cand.push((m * m + g2).sqrt());
            m *= 2.0;
        }
        let n = cand.len();
        // normal equations of: least squares on the transfer function, weights summing to one
        let cols: Vec<Vec<f64>> = cand.iter().map(|&s| fit_f.iter().map(|&f| kernel_response(s, f)).collect()).collect();
        let mut g = vec![vec![0.0; n]; n];
        let mut h = vec![0.0; n];
        for i in 0..n {
            for j in i..n {
                let v: f64 = cols[i].iter().zip(&cols[j]).map(|(a, b)| a * b).sum::<f64>() + 900.0;
                g[i][j] = v;
                g[j][i] = v;
            }
            h[i] = cols[i].iter().zip(&fit_b).map(|(a, b)| a * b).sum::<f64>() + 900.0;
        }
        let w = nnls(&g, &h);
        let total: f64 = w.iter().sum();
        let mut parts: Vec<(f64, f64)> = cand.iter().zip(&w).filter(|(_, &w)| w / total > 2e-4).map(|(&s, &w)| (if s < 0.15 { 0.0 } else { s }, w)).collect();
        let t: f64 = parts.iter().map(|c| c.1).sum();
        parts.iter_mut().for_each(|c| c.1 /= t);
        // judged on the error below 0.4 cycles per pixel, with a small charge for work at full size
        let (mut err, mut taps) = (0.0f64, 0usize);
        for (k, &f) in test_f.iter().enumerate() {
            if f <= 0.4 {
                let got: f64 = parts.iter().map(|&(s, w)| w * kernel_response(s, f)).sum();
                err = err.max((got - test_b[k]).abs());
            }
        }
        for &(s, _) in &parts {
            if (0.15..FULL_RES_SIGMA).contains(&s) {
                taps += 2 * ((3.5 * s + 0.5) as usize) + 1;
            }
        }
        let score = err + 0.001 * taps as f64;
        if best.as_ref().is_none_or(|b| score < b.0) {
            best = Some((score, parts));
        }
    }
    Mixture(best.unwrap().1)
}

/// Largest error of a mixture against the transfer function it stands for, up to
/// `f_max` cycles per pixel (for tests).
pub fn soften_error(lens: f64, n_lens: f64, emul: f64, n_emul: f64, f_max: f64) -> f64 {
    let m = soften(lens, n_lens, emul, n_emul, 1e9);
    (0..400)
        .map(|i| {
            let f = f_max * i as f64 / 399.0;
            let want = shaped_response(lens, n_lens, f) * shaped_response(emul, n_emul, f);
            let got: f64 = m.0.iter().map(|&(s, w)| w * kernel_response(s, f)).sum();
            (want - got).abs()
        })
        .fold(0.0, f64::max)
}

/// Non-negative least squares (Lawson and Hanson) from the normal equations: minimises
/// x'Gx/2 - h'x over x >= 0 for a small dense system.
fn nnls(g: &[Vec<f64>], h: &[f64]) -> Vec<f64> {
    let n = h.len();
    let mut x = vec![0.0; n];
    let mut passive = vec![false; n];
    let solve = |set: &[usize]| -> Vec<f64> {
        let k = set.len();
        let mut a = vec![vec![0.0; k + 1]; k];
        for (r, &jr) in set.iter().enumerate() {
            for (c, &jc) in set.iter().enumerate() {
                a[r][c] = g[jr][jc];
            }
            a[r][r] *= 1.0 + 1e-12;
            a[r][k] = h[jr];
        }
        for c in 0..k {
            let p = (c..k).max_by(|&i, &j| a[i][c].abs().partial_cmp(&a[j][c].abs()).unwrap()).unwrap();
            a.swap(c, p);
            // row c is not changed while the others are reduced by it
            let pivot = a[c][c..=k].to_vec();
            for (r, row) in a.iter_mut().enumerate().take(k) {
                if r != c && pivot[0] != 0.0 {
                    let f = row[c] / pivot[0];
                    for (v, pv) in row[c..=k].iter_mut().zip(&pivot) {
                        *v -= f * pv;
                    }
                }
            }
        }
        (0..k).map(|r| if a[r][r] != 0.0 { a[r][k] / a[r][r] } else { 0.0 }).collect()
    };
    for _ in 0..(10 * n) {
        let grad: Vec<f64> = (0..n).map(|j| h[j] - (0..n).map(|i| g[j][i] * x[i]).sum::<f64>()).collect();
        let Some(j) = (0..n).filter(|&j| !passive[j] && grad[j] > 1e-9).max_by(|&p, &q| grad[p].partial_cmp(&grad[q]).unwrap()) else {
            break;
        };
        passive[j] = true;
        loop {
            let set: Vec<usize> = (0..n).filter(|&j| passive[j]).collect();
            let z = solve(&set);
            if z.iter().all(|&v| v > 0.0) {
                for (r, &j) in set.iter().enumerate() {
                    x[j] = z[r];
                }
                break;
            }
            let mut alpha = f64::INFINITY;
            for (r, &j) in set.iter().enumerate() {
                if z[r] <= 0.0 {
                    alpha = alpha.min(x[j] / (x[j] - z[r]));
                }
            }
            if !alpha.is_finite() {
                alpha = 0.0;
            }
            for (r, &j) in set.iter().enumerate() {
                x[j] += alpha * (z[r] - x[j]);
                if x[j] <= 1e-12 {
                    x[j] = 0.0;
                    passive[j] = false;
                }
            }
            if set.iter().all(|&j| !passive[j]) {
                break;
            }
        }
    }
    x
}
