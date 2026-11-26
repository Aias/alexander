//! Step each slider across its range and measure how far the picture moves at each step.
//!
//!     cargo run --release --example sweep -- out.csv photo.jpg [photo.jpg …]
//!
//! Each photo is used at the default simulation grid. For each slider, every other setting
//! keeps its default (with grain off, so that it does not mask the slider, except for the
//! grain sliders), and the slider takes `STEPS` values from one end of its range to the
//! other, evenly spaced as the slider spaces them. A row gives the mean absolute change of
//! the display values from the step before; the 5th, 50th and 95th percentiles of the
//! picture; how far pixels stand from their neighbours, which follows grain where the
//! change does not (a new grain pattern changes every pixel); and the percentiles of the
//! print that the house tones read.
use alexander_studio::engine::Engine;
use alexander_studio::film::Dev;
use alexander_studio::gpu::Precision;
use alexander_studio::params::{Repro, Settings, View};
use alexander_studio::plan::Planner;
use alexander_studio::{load, resize, tone};
use std::io::Write;

const STEPS: usize = 161;

type Set = fn(&mut Settings, f64);

fn knobs() -> Vec<(&'static str, f64, f64, Settings, Set)> {
    let d = Settings { grain: 0.0, ..Settings::default() };
    let grain = Settings::default();
    let minutes = Settings { dev: Some(Dev::Minutes(7.0)), ..d.clone() };
    let copy = Settings { repro: Repro::Copy, ..d.clone() };
    let book = Settings { repro: Repro::Book, ..d.clone() };
    let fixed = Settings { auto_ev: false, ..d.clone() };
    vec![
        ("film_mm", 8.0, 72.0, grain.clone(), |s, v| s.film_mm = v),
        ("minutes", 6.0, 11.0, minutes, |s, v| s.dev = Some(Dev::Minutes(v))),
        ("auto_ev_target", 0.2, 0.7, d.clone(), |s, v| s.auto_ev_target = v),
        ("ev", -3.0, 3.0, d.clone(), |s, v| s.ev = v),
        ("ev_manual", -3.0, 3.0, fixed, |s, v| s.ev = v),
        ("lens_um", 0.0, 40.0, d.clone(), |s, v| s.lens_um = v),
        ("lens_exponent", 0.6, 2.0, d.clone(), |s, v| s.lens_exponent = v),
        ("halation", 0.0, 0.2, d.clone(), |s, v| s.halation = v),
        ("halation_um", 10.0, 200.0, Settings { halation: 0.1, ..d.clone() }, |s, v| s.halation_um = v),
        ("grain", 0.0, 3.0, grain.clone(), |s, v| s.grain = v),
        ("grain_light", 0.0, 1.0, grain.clone(), |s, v| s.grain_light = v),
        ("grain_dev", 0.0, 2.5, grain, |s, v| s.grain_dev = v),
        ("edge", 0.0, 3.0, d.clone(), |s, v| s.edge = v),
        ("edge_density", 0.0, 1.0, d.clone(), |s, v| s.edge_density = v),
        ("grade", -1.0, 5.0, d.clone(), |s, v| s.grade = v),
        ("print_ev", -2.5, 2.5, d.clone(), |s, v| s.print_ev = v),
        ("print_grey", 0.3, 1.3, d.clone(), |s, v| s.print_grey = v),
        ("enlarger_um", 0.0, 15.0, d.clone(), |s, v| s.enlarger_um = v),
        ("house", 0.0, 1.0, d.clone(), |s, v| s.house = v),
        ("copy_blur", 0.0, 2.0, copy.clone(), |s, v| s.copy_blur = v),
        ("copy_gain", 0.5, 2.0, copy.clone(), |s, v| s.copy_gain = v),
        ("copy_drop", 0.0, 0.3, copy.clone(), |s, v| s.copy_drop = v),
        ("copy_fill", 0.7, 1.0, copy.clone(), |s, v| s.copy_fill = v),
        ("copy_ink", 0.8, 2.5, copy, |s, v| s.copy_ink = v),
        ("ppi", 72.0, 600.0, book.clone(), |s, v| s.ppi = v),
        ("page_mm", 30.0, 400.0, book.clone(), |s, v| s.page_mm = Some(v)),
        ("lpi", 65.0, 200.0, book.clone(), |s, v| s.lpi = v),
        ("ink_density", 0.8, 2.2, book.clone(), |s, v| s.ink_density = v),
        ("dot_gain", 0.0, 0.3, book.clone(), |s, v| s.dot_gain = v),
        ("min_dot", 0.0, 0.1, book.clone(), |s, v| s.min_dot = v),
        ("copy_blur_mm", 0.0, 0.2, book.clone(), |s, v| s.copy_blur_mm = v),
        ("paper_scatter_mm", 0.0, 0.2, book.clone(), |s, v| s.paper_scatter_mm = v),
        ("scan_black", 0.0, 0.3, Settings { view: Some(View::Scan), ..book.clone() }, |s, v| s.scan_black = v),
        ("scan_white", 0.7, 1.0, Settings { view: Some(View::Scan), ..book }, |s, v| s.scan_white = v),
    ]
}

fn percentile(sorted: &[f32], p: f64) -> f32 {
    sorted[((sorted.len() - 1) as f64 * p) as usize]
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let only = std::env::var("SWEEP_ONLY").ok();
    let (mut engine, gpu) = Engine::headless(Precision::Half).expect("no usable GPU");
    eprintln!("rendering on {gpu}");
    let mut out = std::fs::File::create(&args[1]).expect("output");
    writeln!(out, "knob,photo,value,change,p05,p50,p95,texture,own").unwrap();
    let mut planner = Planner::new();
    for path in &args[2..] {
        let px = load::decode(std::path::Path::new(path)).expect("photo");
        let long = load::grid_long(Settings::default().detail_px, (px.w, px.h), engine.gpu.max_dim);
        let (w, h) = load::grid_size((px.w, px.h), long);
        let grid = resize::rgba8(&px.rgba, px.w, px.h, w, h);
        let name = std::path::Path::new(path).file_stem().unwrap().to_string_lossy().into_owned();
        let start = std::time::Instant::now();
        for (knob, lo, hi, base, set) in knobs() {
            if only.as_deref().is_some_and(|o| !o.split(',').any(|k| k == knob)) {
                continue;
            }
            let mut photo = engine.upload(&grid, w, h);
            let mut before: Option<Vec<f32>> = None;
            for i in 0..STEPS {
                let t = i as f64 / (STEPS - 1) as f64;
                let v = if matches!(knob, "film_mm" | "page_mm") { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t };
                let mut s = base.clone();
                set(&mut s, v);
                let weights = planner.film(s.film).weights[s.filter as usize];
                let (median, top) = tone::exposure_levels(&grid, w as usize, h as usize, weights);
                let auto = if s.auto_ev { tone::auto_ev(median, top, s.auto_ev_target) } else { 0.0 };
                let plan = planner.plan(&s, (w, h), auto, None, engine.gpu.max_dim);
                engine.render(&mut photo, &plan);
                let (_, _, rgba) = engine.read_display(&mut photo, &plan.view, 2);
                let y: Vec<f32> = rgba.as_chunks::<4>().0.iter().map(|p| p[1]).collect();
                let change = before.as_ref().map_or(0.0, |b| b.iter().zip(&y).map(|(a, b)| (a - b).abs() as f64).sum::<f64>() / y.len() as f64);
                let mut sorted = y.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
                let (rw, rh) = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
                let texture = (1..rh - 1).flat_map(|r| (1..rw - 1).map(move |c| (r, c))).map(|(r, c)| (y[r * rw + c] - 0.25 * (y[r * rw + c - 1] + y[r * rw + c + 1] + y[(r - 1) * rw + c] + y[(r + 1) * rw + c])).abs() as f64).sum::<f64>() / ((rw - 2) * (rh - 2)) as f64;
                let own = photo.percentiles().map(|p| p.map(|v| format!("{v:.4}")).join(" ")).unwrap_or_default();
                writeln!(out, "{knob},{name},{v:.5},{change:.6},{:.5},{:.5},{:.5},{texture:.6},{own}", percentile(&sorted, 0.05), percentile(&sorted, 0.5), percentile(&sorted, 0.95)).unwrap();
                before = Some(y);
            }
            engine.release(&mut photo);
        }
        eprintln!("{name}: {:.0} s", start.elapsed().as_secs_f64());
    }
}
