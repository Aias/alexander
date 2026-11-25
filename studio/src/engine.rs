//! The simulation on the GPU. `Engine::render` brings a photo's finished picture up to date
//! for a plan, redoing only the stages whose numbers changed since the last call, and
//! `Engine::draw` puts that picture on a canvas at any zoom.
//!
//! Stage by stage this follows `render()` in twb2.py. Three things are done differently,
//! for speed, and are held to the Python results by tests/render.rs:
//!   - the long-tailed lens blur is a sum of Gaussians (see plan::soften) in place of an FFT;
//!   - Gaussians wider than a couple of pixels are drawn on reduced copies of the picture;
//!   - grain comes from a counter-based random number per pixel, so its pattern differs
//!     from numpy's while its statistics are the same.

use crate::film::{CURVE_DX, CURVE_N, CURVE_X0};
use crate::gpu::{Gpu, Pass, Precision, Tex};
use crate::params::{Repro, View};
use crate::plan::{gauss_kernel, Plan, ViewPlan, FULL_RES_SIGMA};
use crate::tone::{self, HOUSE_LUT_N, HOUSE_PCT, PAPER_CREAM, PAPER_DX, PAPER_N, PAPER_X0};
use wgpu::TextureFormat as Format;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    Optics,
    Develop,
    Grain,
    Enlarge,
    Paper,
    Finish,
}

/// A photo on the GPU: its pixels and, once rendered, the kept result of every stage.
pub struct Photo {
    pub src: Tex,
    cache: Option<Cache>,
}

impl Photo {
    pub fn size(&self) -> (u32, u32) {
        (self.src.w, self.src.h)
    }
    pub fn rendered(&self) -> bool {
        self.cache.is_some()
    }
    /// The picture's own tone percentiles, as measured for the house tone step.
    pub fn percentiles(&self) -> Option<[f64; 5]> {
        self.cache.as_ref().and_then(|c| c.own.as_ref().map(|o| o.1))
    }
}

type CopyKey = (bool, [u32; 9]);

struct Cache {
    plan: Plan,
    soft: Tex,         // exposure after lens, emulsion and halation, before the exposure gain
    d0: Tex,           // developed density without grain
    neg: Tex,          // transmission of the negative: with grain, without
    negb: Option<Tex>, // the same through the enlarger lens
    r: Tex,            // the darkroom print
    fin: Tex,          // the finished picture at the photo's own size, with half-size copies
    fin_levels: u32,   // how many of those copies are current
    own: Option<(CopyKey, [f64; 5])>,
}

/// Where a canvas looks: `origin` is the picture position of its top-left corner and
/// `scale` the number of picture pixels per canvas pixel.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Canvas {
    pub origin: [f32; 2],
    pub scale: f32,
    pub show_source: bool,
}

struct Table {
    tex: Tex,
    data: Vec<f32>,
}

pub struct Engine {
    pub gpu: Gpu,
    free: Vec<Tex>,
    film: Table,
    paper: Table,
    house: Table,
    /// Milliseconds the last `render` took, waiting for the GPU included when it had to.
    pub last_ms: f32,
}

const POOL_BYTES: u64 = 512 << 20;

impl Engine {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue, precision: Precision) -> Engine {
        let gpu = Gpu::new(device, queue, precision);
        let table = |n: usize| Table { tex: gpu.table(&vec![0.0; n]), data: Vec::new() };
        Engine { film: table(CURVE_N), paper: table(PAPER_N), house: table(HOUSE_LUT_N), gpu, free: Vec::new(), last_ms: 0.0 }
    }

    /// An engine without a window. None when the machine has no usable GPU.
    pub fn headless(precision: Precision) -> Option<(Engine, String)> {
        let (device, queue, name) = crate::gpu::headless()?;
        Some((Engine::new(device, queue, precision), name))
    }

    /// Put a photo's pixels (8-bit sRGB, four bytes each) on the GPU.
    pub fn upload(&mut self, rgba: &[u8], w: u32, h: u32) -> Photo {
        let src = self.gpu.tex(w, h, Format::Rgba8UnormSrgb);
        self.gpu.write(&src, rgba, 4);
        Photo { src, cache: None }
    }

    /// Let go of a photo's kept stages. Its next render starts from the beginning.
    pub fn release(&mut self, photo: &mut Photo) {
        if let Some(c) = photo.cache.take() {
            for t in [Some(c.soft), Some(c.d0), Some(c.neg), c.negb, Some(c.r), Some(c.fin)].into_iter().flatten() {
                self.give(t);
            }
        }
        self.trim();
    }

    // ---------- scratch pictures ----------------------------------------------------
    fn take(&mut self, w: u32, h: u32, format: Format) -> Tex {
        if let Some(i) = self.free.iter().rposition(|t| t.w == w && t.h == h && t.format == format && t.mips == 1) {
            return self.free.remove(i);
        }
        self.gpu.tex(w, h, format)
    }

    fn give(&mut self, t: Tex) {
        if t.mips == 1 {
            self.free.push(t);
        }
    }

    fn trim(&mut self) {
        let mut total: u64 = self.free.iter().map(|t| t.bytes()).sum();
        while total > POOL_BYTES && !self.free.is_empty() {
            total -= self.free.remove(0).bytes();
        }
    }

    fn set_table(gpu: &Gpu, table: &mut Table, data: &[f32]) {
        if table.data != data {
            gpu.write_table(&table.tex, data);
            table.data = data.to_vec();
        }
    }

    // ---------- blurs ----------------------------------------------------------------
    fn blur_numbers(w: u32, h: u32, sigma: f64, dir: [f32; 2]) -> Vec<[f32; 4]> {
        let k = if sigma < 0.15 { vec![1.0] } else { gauss_kernel(sigma) };
        let r = k.len() / 2;
        let mut v = vec![[1.0 / w as f32, 1.0 / h as f32, 0.0, k[r] as f32], [dir[0], dir[1], 0.0, 0.0]];
        let mut pairs: Vec<[f32; 2]> = Vec::new();
        let mut i = 1;
        while i <= r {
            let a = k[r + i];
            let b = if i < r { k[r + i + 1] } else { 0.0 };
            let s = a + b;
            pairs.push([(i as f64 + if s > 0.0 { b / s } else { 0.0 }) as f32, s as f32]);
            i += 2;
        }
        v[0][2] = pairs.len() as f32;
        for c in pairs.chunks(2) {
            let second = c.get(1).copied().unwrap_or([0.0, 0.0]);
            v.push([c[0][0], c[0][1], second[0], second[1]]);
        }
        v
    }

    /// Gaussian blur at the picture's own size: one pass along each axis.
    fn blur_sep(&mut self, enc: &mut wgpu::CommandEncoder, src: &Tex, sigma: f64) -> Tex {
        let sigma = sigma.min(24.0);
        let tmp = self.take(src.w, src.h, src.format);
        let out = self.take(src.w, src.h, src.format);
        self.gpu.pass(enc, Pass::Blur, &tmp.view, src.format, &Self::blur_numbers(src.w, src.h, sigma, [1.0, 0.0]), &[&src.view], &[]);
        self.gpu.pass(enc, Pass::Blur, &out.view, src.format, &Self::blur_numbers(src.w, src.h, sigma, [0.0, 1.0]), &[&tmp.view], &[]);
        self.give(tmp);
        out
    }

    /// The reduced copy a blur of `sigma` is drawn on (0 is the picture itself, each step
    /// halves it) and the blur left to apply there. Halving with the (1 3 3 1) filter and
    /// enlarging again with linear interpolation are themselves blurs of known variance,
    /// which is taken off.
    fn level_for(sigma: f64, w: u32, h: u32) -> (usize, f64) {
        if sigma < FULL_RES_SIGMA {
            return (0, sigma);
        }
        let deepest = ((w.min(h) as f64 / 4.0).log2().floor()).max(0.0) as usize;
        let l = ((sigma / 1.19).log2().floor() as usize).min(deepest);
        if l == 0 {
            return (0, sigma);
        }
        let f2 = 4f64.powi(l as i32);
        let s2 = sigma * sigma / f2 - 0.25 - 1.0 / 6.0 + 0.25 / f2;
        (l, s2.max(0.04).sqrt())
    }

    fn pyramid(&mut self, enc: &mut wgpu::CommandEncoder, base: &Tex, levels: &mut Vec<Tex>, upto: usize) {
        while levels.len() < upto {
            let (sw, sh) = levels.last().map_or((base.w, base.h), |s| (s.w, s.h));
            let t = self.take(sw.div_ceil(2), sh.div_ceil(2), base.format);
            let src = levels.last().unwrap_or(base);
            self.gpu.pass(enc, Pass::Down, &t.view, base.format, &[[1.0 / sw as f32, 1.0 / sh as f32, 0.0, 0.0]], &[&src.view], &[]);
            levels.push(t);
        }
    }

    /// Gaussian blur of `base`, drawn on the cheapest reduced copy. Returns the blurred
    /// picture and how many times smaller than `base` it is.
    fn blur_level(&mut self, enc: &mut wgpu::CommandEncoder, base: &Tex, levels: &mut Vec<Tex>, sigma: f64) -> (Tex, f32) {
        let (l, s) = Self::level_for(sigma, base.w, base.h);
        if l == 0 {
            return (self.blur_sep(enc, base, s), 1.0);
        }
        self.pyramid(enc, base, levels, l);
        (self.blur_sep(enc, &levels[l - 1], s), (1u32 << l) as f32)
    }

    /// Gaussian blur of `base`, returned at its own size.
    fn blur_full(&mut self, enc: &mut wgpu::CommandEncoder, base: &Tex, sigma: f64) -> Tex {
        let mut levels = Vec::new();
        let (t, f) = self.blur_level(enc, base, &mut levels, sigma);
        let out = if f == 1.0 {
            t
        } else {
            let o = self.take(base.w, base.h, base.format);
            self.combine(enc, &o, &[(&t, f, 1.0)]);
            self.give(t);
            o
        };
        for l in levels {
            self.give(l);
        }
        out
    }

    /// Weighted sum into `target` of pictures that may be smaller than it by a factor.
    fn combine(&mut self, enc: &mut wgpu::CommandEncoder, target: &Tex, parts: &[(&Tex, f32, f32)]) {
        let mut acc: Option<Tex> = None;
        let mut i = 0;
        loop {
            let room = if acc.is_some() { 5 } else { 6 };
            let chunk = &parts[i..(i + room).min(parts.len())];
            i += chunk.len();
            let last = i >= parts.len();
            let held = if last { None } else { Some(self.take(target.w, target.h, target.format)) };
            let mut numbers = vec![[0.0f32; 4]];
            let mut views: Vec<&wgpu::TextureView> = Vec::new();
            if let Some(a) = &acc {
                let s = a.per_pixel(1.0);
                numbers.push([s[0], s[1], 1.0, 0.0]);
                views.push(&a.view);
            }
            for (t, factor, weight) in chunk {
                let s = t.per_pixel(*factor);
                numbers.push([s[0], s[1], *weight, 0.0]);
                views.push(&t.view);
            }
            numbers[0][0] = views.len() as f32;
            let out = held.as_ref().unwrap_or(target);
            self.gpu.pass(enc, Pass::Combine, &out.view, target.format, &numbers, &views, &[]);
            drop(views);
            if let Some(a) = acc.take() {
                self.give(a);
            }
            acc = held;
            if last {
                break;
            }
        }
    }

    // ---------- stages -----------------------------------------------------------------
    fn optics(&mut self, enc: &mut wgpu::CommandEncoder, src: &Tex, plan: &Plan, soft: &Tex) {
        let (w, h) = plan.grid;
        let f1 = self.gpu.f1;
        let o = &plan.optics;
        let e = self.take(w, h, f1);
        self.gpu.pass(enc, Pass::Expose, &e.view, f1, &[[o.weights[0], o.weights[1], o.weights[2], o.scale as f32]], &[&src.view], &[]);
        // lens and emulsion: the parts of the blur, each on its cheapest copy
        let mut levels = Vec::new();
        let mut blurred: Vec<(Tex, f32, f32)> = Vec::new();
        let mut plain = 0.0f32;
        for &(sigma, weight) in &o.blur.0 {
            if sigma < 0.15 {
                plain += weight as f32;
            } else {
                let (t, f) = self.blur_level(enc, &e, &mut levels, sigma);
                blurred.push((t, f, weight as f32));
            }
        }
        let halo = o.halation > 0.0;
        let held = if halo { Some(self.take(w, h, f1)) } else { None };
        {
            let mut parts: Vec<(&Tex, f32, f32)> = Vec::new();
            if plain > 0.0 || blurred.is_empty() {
                parts.push((&e, 1.0, if blurred.is_empty() { 1.0 } else { plain }));
            }
            parts.extend(blurred.iter().map(|(t, f, w)| (t, *f, *w)));
            self.combine(enc, held.as_ref().unwrap_or(soft), &parts);
        }
        for t in blurred.into_iter().map(|b| b.0).chain(levels) {
            self.give(t);
        }
        self.give(e);
        if let Some(s) = held {
            // halation: three reflections from the back of the film base, each half as
            // strong as the last and wider by the square root of its number
            let mut levels = Vec::new();
            let glow: Vec<(Tex, f32)> = o.halation_sigmas.iter().map(|&sg| self.blur_level(enc, &s, &mut levels, sg)).collect();
            let hh = o.halation as f32;
            {
                let mut parts: Vec<(&Tex, f32, f32)> = vec![(&s, 1.0, 1.0 / (1.0 + hh))];
                for (k, (t, f)) in glow.iter().enumerate() {
                    parts.push((t, *f, hh * [1.0, 0.5, 0.25][k] / 1.75 / (1.0 + hh)));
                }
                self.combine(enc, soft, &parts);
            }
            for t in glow.into_iter().map(|g| g.0).chain(levels) {
                self.give(t);
            }
            self.give(s);
        }
    }

    fn develop(&mut self, enc: &mut wgpu::CommandEncoder, plan: &Plan, soft: &Tex, d0: &Tex) {
        let d = &plan.develop;
        let f1 = self.gpu.f1;
        Self::set_table(&self.gpu, &mut self.film, &d.curve);
        let grid = [CURVE_X0 as f32, (1.0 / CURVE_DX) as f32, CURVE_N as f32, d.k];
        if !d.edge_on || d.k == 0.0 {
            self.gpu.pass(enc, Pass::Develop, &d0.view, f1, &[[d.gain, d.logh_grey, d.base, 0.0], grid], &[&soft.view], &[&self.film.tex.view]);
            return;
        }
        // edge effects: density releases an inhibitor that spreads sideways and holds back
        // development nearby; only the difference from the local value acts
        let plain = self.take(soft.w, soft.h, f1);
        self.gpu.pass(enc, Pass::Develop, &plain.view, f1, &[[d.gain, d.logh_grey, d.base, 0.0], grid], &[&soft.view], &[&self.film.tex.view]);
        let mut levels = Vec::new();
        let (core, fc) = self.blur_level(enc, &plain, &mut levels, d.sigma_core);
        let (tail, ft) = self.blur_level(enc, &plain, &mut levels, d.sigma_tail);
        let (sc, st) = (core.per_pixel(fc), tail.per_pixel(ft));
        self.gpu.pass(
            enc,
            Pass::Develop,
            &d0.view,
            f1,
            &[[d.gain, d.logh_grey, d.base, 1.0], grid, [d.tail_weight, d.edge_density, d.ref_density, 0.0], [sc[0], sc[1], st[0], st[1]]],
            &[&soft.view, &plain.view, &core.view, &tail.view],
            &[&self.film.tex.view],
        );
        for t in [plain, core, tail].into_iter().chain(levels) {
            self.give(t);
        }
    }

    fn tone_numbers(plan: &Plan, step: u32) -> [[f32; 4]; 4] {
        let f = &plan.finish;
        let press = step == 5 || step == 6;
        let (solid, tau, d_out) = if press { (f.press_solid, f.press_tau, f.press_ink) } else { (f.copy_solid, f.copy_tau, f.copy_ink) };
        [
            [step as f32, f.black, solid, tau],
            [f.copy_dmax_in, d_out, f.copy_drop, f.copy_fill],
            [f.copy_gain, f.min_dot, f.dot_gain, plan.optics.scale as f32],
            [HOUSE_LUT_N as f32, 0.0, 0.0, 0.0],
        ]
    }

    fn tone(&mut self, enc: &mut wgpu::CommandEncoder, plan: &Plan, step: u32, src: &Tex, target: &wgpu::TextureView, format: Format) {
        self.gpu.pass(enc, Pass::Tone, target, format, &Self::tone_numbers(plan, step), &[&src.view], &[&self.house.tex.view]);
    }

    fn copy_key(plan: &Plan) -> CopyKey {
        let f = &plan.finish;
        if f.repro != Repro::Copy {
            return (false, [0; 9]);
        }
        (true, [f.black, f.copy_solid, f.copy_tau, f.copy_dmax_in, f.copy_ink, f.copy_drop, f.copy_fill, f.copy_gain, f.copy_sigma as f32].map(f32::to_bits))
    }

    /// Everything after the darkroom print. Returns the encoder to go on recording with:
    /// measuring the picture's percentiles for the house tones needs the GPU's results,
    /// which ends one encoder and starts another.
    fn finish(&mut self, mut enc: wgpu::CommandEncoder, plan: &Plan, r: &Tex, fin: &Tex, own: &mut Option<(CopyKey, [f64; 5])>) -> wgpu::CommandEncoder {
        let f = &plan.finish;
        let (w, h) = plan.grid;
        let f1 = self.gpu.f1;
        let mut cur: Option<Tex> = None;
        if f.repro == Repro::Copy {
            // printed in a book, re-photographed from the page and printed again
            let a = self.take(w, h, f1);
            self.tone(&mut enc, plan, 1, r, &a.view, f1);
            let b = if f.copy_sigma >= 0.15 {
                let b = self.blur_full(&mut enc, &a, f.copy_sigma);
                self.give(a);
                b
            } else {
                a
            };
            let c = self.take(w, h, f1);
            self.tone(&mut enc, plan, 2, &b, &c.view, f1);
            self.give(b);
            cur = Some(c);
        }
        if f.house > 0.0 {
            // the tone range the book's pages share: the picture's own percentiles, read
            // back from every third pixel, moved toward the book's along a smooth curve
            let key = Self::copy_key(plan);
            let pct = match own.as_ref() {
                Some((k, p)) if *k == key => *p,
                _ => {
                    let small = self.take(w.div_ceil(3), h.div_ceil(3), Format::R32Float);
                    self.tone(&mut enc, plan, 3, cur.as_ref().unwrap_or(r), &small.view, Format::R32Float);
                    self.gpu.submit(enc);
                    enc = self.gpu.encoder();
                    let bytes = self.gpu.read(&small, 4);
                    let values: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
                    self.give(small);
                    let p = tone::percentiles(&values, &HOUSE_PCT);
                    let p = [p[0], p[1], p[2], p[3], p[4]];
                    *own = Some((key, p));
                    p
                }
            };
            Self::set_table(&self.gpu, &mut self.house, &tone::house_lut(&pct, f.house));
            let y = self.take(w, h, f1);
            self.tone(&mut enc, plan, 4, cur.as_ref().unwrap_or(r), &y.view, f1);
            if let Some(c) = cur.replace(y) {
                self.give(c);
            }
        }
        if f.repro == Repro::Book {
            // copy camera, halftone screen, press and paper
            let mut x = if f.press_copy_sigma >= 0.15 { Some(self.blur_full(&mut enc, cur.as_ref().unwrap_or(r), f.press_copy_sigma)) } else { None };
            let a = self.take(w, h, f1);
            self.tone(&mut enc, plan, 5, x.as_ref().or(cur.as_ref()).unwrap_or(r), &a.view, f1);
            for t in [x.take(), cur.take()].into_iter().flatten() {
                self.give(t);
            }
            let a2 = if f.press_cell_sigma >= 0.15 {
                let b = self.blur_full(&mut enc, &a, f.press_cell_sigma);
                self.give(a);
                b
            } else {
                a
            };
            let r2 = self.take(w, h, f1);
            self.tone(&mut enc, plan, 6, &a2, &r2.view, f1);
            self.give(a2);
            let r3 = if f.press_scatter_sigma >= 0.15 {
                let b = self.blur_full(&mut enc, &r2, f.press_scatter_sigma);
                self.give(r2);
                b
            } else {
                r2
            };
            cur = Some(r3);
        }
        let x = cur.as_ref().unwrap_or(r);
        if plan.optics.scale > 1 {
            self.tone(&mut enc, plan, 7, x, &level_view(fin, 0), f1);
        } else {
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &x.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &fin.texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: fin.w, height: fin.h, depth_or_array_layers: 1 },
            );
        }
        if let Some(c) = cur {
            self.give(c);
        }
        enc
    }

    /// Bring `photo` up to date for `plan`. Returns the first stage that had to be redone,
    /// or None when the kept result was already current.
    pub fn render(&mut self, photo: &mut Photo, plan: &Plan) -> Option<Stage> {
        let start = std::time::Instant::now();
        assert_eq!(plan.source, photo.size(), "plan made for another photo");
        if photo.cache.as_ref().is_some_and(|c| c.plan.grid != plan.grid) {
            self.release(photo);
        }
        let (f1, f2) = (self.gpu.f1, self.gpu.f2);
        let (w, h) = plan.grid;
        let first = match &photo.cache {
            None => Some(Stage::Optics),
            Some(c) => {
                let p = &c.plan;
                if p.optics != plan.optics {
                    Some(Stage::Optics)
                } else if p.develop != plan.develop {
                    Some(Stage::Develop)
                } else if p.grain != plan.grain {
                    Some(Stage::Grain)
                } else if p.enlarger_sigma != plan.enlarger_sigma {
                    Some(Stage::Enlarge)
                } else if p.paper != plan.paper {
                    Some(Stage::Paper)
                } else if p.finish != plan.finish {
                    Some(Stage::Finish)
                } else {
                    None
                }
            }
        };
        let first = first?;
        if photo.cache.is_none() {
            photo.cache = Some(Cache {
                plan: plan.clone(),
                soft: self.take(w, h, f1),
                d0: self.take(w, h, f1),
                neg: self.take(w, h, f2),
                negb: None,
                r: self.take(w, h, f1),
                fin: self.gpu.tex_with_mips(plan.source.0, plan.source.1, f1),
                fin_levels: 0,
                own: None,
            });
        }
        let mut cache = photo.cache.take().unwrap();
        let mut enc = self.gpu.encoder();
        if first <= Stage::Optics {
            self.optics(&mut enc, &photo.src, plan, &cache.soft);
        }
        if first <= Stage::Develop {
            self.develop(&mut enc, plan, &cache.soft, &cache.d0);
        }
        if first <= Stage::Grain {
            let g = &plan.grain;
            self.gpu.pass(&mut enc, Pass::Grain, &cache.neg.view, f2, &[[if g.on { g.grains_per_px } else { 0.0 }, g.seed as f32, 0.0, 0.0]], &[&cache.d0.view], &[]);
        }
        if first <= Stage::Enlarge {
            if let Some(t) = cache.negb.take() {
                self.give(t);
            }
            if plan.enlarger_sigma >= 0.15 {
                cache.negb = Some(self.blur_full(&mut enc, &cache.neg, plan.enlarger_sigma));
            }
        }
        if first <= Stage::Paper {
            let p = &plan.paper;
            Self::set_table(&self.gpu, &mut self.paper, &p.curve);
            self.gpu.pass(
                &mut enc,
                Pass::Paper,
                &cache.r.view,
                f1,
                &[[p.offset, p.dmin, p.grain_light, 0.0], [PAPER_X0 as f32, (1.0 / PAPER_DX) as f32, PAPER_N as f32, 0.0]],
                &[&cache.negb.as_ref().unwrap_or(&cache.neg).view],
                &[&self.paper.tex.view],
            );
            cache.own = None;
        }
        let enc = self.finish(enc, plan, &cache.r, &cache.fin, &mut cache.own);
        self.gpu.submit(enc);
        cache.fin_levels = 1;
        cache.plan = plan.clone();
        photo.cache = Some(cache);
        self.trim();
        self.last_ms = start.elapsed().as_secs_f32() * 1000.0;
        Some(first)
    }

    fn canvas_numbers(view: &ViewPlan, c: &Canvas, level: u32, raw: bool) -> [[f32; 4]; 4] {
        let mode = match view.mode {
            View::Print | View::White => 0.0,
            View::Scan => 1.0,
            View::Page => 2.0,
        };
        let cream = PAPER_CREAM.map(|v| tone::srgb_to_lin(v) as f32);
        [
            [c.origin[0], c.origin[1], c.scale, level as f32],
            [mode, tone::lin_to_srgb(view.solid as f64) as f32, view.scan_black, view.scan_white],
            [cream[0], cream[1], cream[2], if raw { 0.0 } else { 1.0 }],
            [if c.show_source { 1.0 } else { 0.0 }, if raw { 1.0 } else { 0.0 }, 0.0, 0.0],
        ]
    }

    /// Draw the finished picture (or the photo itself) into `target` as the canvas asks.
    /// `raw` writes display values unconverted, for a float target that is read back.
    pub fn draw(&mut self, photo: &mut Photo, view: &ViewPlan, canvas: &Canvas, target: &wgpu::TextureView, format: Format, raw: bool) {
        let mut enc = self.gpu.encoder();
        let mut level = 0;
        if let (Some(c), false) = (photo.cache.as_mut(), canvas.show_source) {
            if canvas.scale >= 2.0 {
                level = (canvas.scale.log2().floor() as u32).min(c.fin.mips - 1);
            }
            // half-size averages of the finished picture, made when a view first needs them
            while c.fin_levels <= level {
                let l = c.fin_levels;
                let (sw, sh) = ((c.fin.w >> (l - 1)).max(1), (c.fin.h >> (l - 1)).max(1));
                self.gpu.pass(&mut enc, Pass::Mip, &level_view(&c.fin, l), c.fin.format, &[[1.0 / sw as f32, 1.0 / sh as f32, 0.0, 0.0]], &[&level_view(&c.fin, l - 1)], &[]);
                c.fin_levels += 1;
            }
        }
        let fin = match (&photo.cache, canvas.show_source) {
            (Some(c), false) => &c.fin.view,
            _ => &photo.src.view,
        };
        let show_source = canvas.show_source || photo.cache.is_none();
        let numbers = Self::canvas_numbers(view, &Canvas { show_source, ..*canvas }, level, raw);
        self.gpu.pass(&mut enc, Pass::Canvas, target, format, &numbers, &[fin, &photo.src.view], &[]);
        self.gpu.submit(enc);
    }

    /// The finished picture as display values, four floats per pixel, for saving.
    pub fn read_display(&mut self, photo: &mut Photo, view: &ViewPlan) -> (u32, u32, Vec<f32>) {
        let (w, h) = photo.size();
        let target = self.take(w, h, Format::Rgba32Float);
        self.draw(photo, view, &Canvas { origin: [0.0, 0.0], scale: 1.0, show_source: false }, &target.view, Format::Rgba32Float, true);
        let bytes = self.gpu.read(&target, 16);
        self.give(target);
        (w, h, bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
    }
}

fn level_view(t: &Tex, level: u32) -> wgpu::TextureView {
    t.texture.create_view(&wgpu::TextureViewDescriptor { base_mip_level: level, mip_level_count: Some(1), ..Default::default() })
}
