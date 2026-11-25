//! Whole renders on the GPU against the same renders from twb2.py (tests/make_fixtures.py).
//! Skipped, with a note, on a machine without a usable GPU.
use alexander_studio::engine::Engine;
use alexander_studio::film::{Dev, FilmKind, Filter};
use alexander_studio::gpu::Precision;
use alexander_studio::params::{Repro, Settings, View};
use alexander_studio::plan::Planner;
use alexander_studio::tone;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn base() -> Settings {
    Settings { grain: 0.0, detail_px: 0, film_mm: 3.6, auto_ev: true, ..Settings::default() }
}

fn cases() -> Vec<(&'static str, Settings)> {
    vec![
        ("default", base()),
        ("straight", Settings { house: 0.0, lens_exponent: 2.0, lens_um: 8.0, ..base() }),
        ("doublex", Settings { film: FilmKind::DoubleX, halation: 0.06, ..base() }),
        ("nelson_red", Settings { edge_density: 1.0, filter: Filter::Red, dev: Some(Dev::Norm1956), grade: 2.55, ..base() }),
        ("copy", Settings { repro: Repro::Copy, ..base() }),
        ("book", Settings { repro: Repro::Book, page_mm: Some(65.024), ..base() }),
        ("book_page", Settings { repro: Repro::Book, view: Some(View::Page), ..base() }),
    ]
}

/// Mean, 99.9th percentile and largest absolute difference.
fn differences(got: &[f32], want: &[f32]) -> (f64, f64, f64) {
    let mut d: Vec<f64> = got.iter().zip(want).map(|(a, b)| (*a as f64 - *b as f64).abs()).collect();
    d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (d.iter().sum::<f64>() / d.len() as f64, d[(d.len() as f64 * 0.999) as usize], *d.last().unwrap())
}

fn run(precision: Precision) {
    let Some((mut engine, name)) = Engine::headless(precision) else {
        eprintln!("no GPU here; render tests skipped");
        return;
    };
    eprintln!("rendering on {name}, {precision:?} precision");
    let scene = image::open(fixtures().join("scene.png")).unwrap().into_rgba8();
    let (w, h) = scene.dimensions();
    let mut photo = engine.upload(scene.as_raw(), w, h);
    let mut planner = Planner::new();
    let mut worst = 0.0f64;
    for (name, s) in cases() {
        let weights = planner.film(s.film).weights[s.filter as usize];
        let (median, top) = tone::exposure_levels(scene.as_raw(), w as usize, h as usize, weights);
        let auto = tone::auto_ev(median, top, s.auto_ev_target);
        let plan = planner.plan(&s, (w, h), auto, engine.gpu.max_dim);
        engine.render(&mut photo, &plan);
        let (_, _, rgba) = engine.read_display(&mut photo, &plan.view);
        let got: Vec<f32> = rgba.as_chunks::<4>().0.iter().map(|p| p[1]).collect();
        let want: Vec<f32> = image::open(fixtures().join(format!("render_{name}.png"))).unwrap().into_luma16().as_raw().iter().map(|&v| v as f32 / 65535.0).collect();
        assert_eq!(got.len(), want.len(), "{name}: size");
        let (mean, high, max) = differences(&got, &want);
        eprintln!("{name:12} auto {auto:+.3}  mean {mean:.5}  99.9% {high:.5}  max {max:.5}");
        worst = worst.max(mean);
        // a display step in 8 bits is 0.0039
        assert!(mean < 0.0015 && high < 0.012, "{name}: differs from twb2.py (mean {mean:.5}, 99.9% {high:.5}, max {max:.5})");
    }
    eprintln!("largest mean difference {worst:.5}");
}

#[test]
fn renders_match_python_half() {
    run(Precision::Half);
}

#[test]
fn renders_match_python_full() {
    run(Precision::Full);
}

/// Kept stages are reused: a change late in the chain redoes only what follows it, and
/// gives the same picture as rendering from scratch.
#[test]
fn stages_are_reused() {
    use alexander_studio::engine::Stage;
    let Some((mut engine, _)) = Engine::headless(Precision::Half) else {
        return;
    };
    let scene = image::open(fixtures().join("scene.png")).unwrap().into_rgba8();
    let (w, h) = scene.dimensions();
    let mut photo = engine.upload(scene.as_raw(), w, h);
    let mut planner = Planner::new();
    let mut plan = |s: &Settings| planner.plan(s, (w, h), -0.63, 8192);
    let a = Settings { detail_px: 0, film_mm: 3.6, ..Settings::default() };
    assert_eq!(engine.render(&mut photo, &plan(&a)), Some(Stage::Optics));
    assert_eq!(engine.render(&mut photo, &plan(&a)), None);
    for (s, stage) in [
        (Settings { grade: 2.0, ..a.clone() }, Stage::Paper),
        (Settings { grade: 2.0, grain: 1.5, ..a.clone() }, Stage::Grain),
        (Settings { grade: 2.0, grain: 1.5, house: 0.5, ..a.clone() }, Stage::Finish),
        (Settings { grade: 2.0, grain: 1.5, house: 0.5, ev: 0.3, ..a.clone() }, Stage::Develop),
        (Settings { grade: 2.0, grain: 1.5, house: 0.5, ev: 0.3, enlarger_um: 9.0, ..a.clone() }, Stage::Enlarge),
        (Settings { grade: 2.0, grain: 1.5, house: 0.5, ev: 0.3, enlarger_um: 9.0, lens_um: 14.0, ..a.clone() }, Stage::Optics),
    ] {
        let p = plan(&s);
        assert_eq!(engine.render(&mut photo, &p), Some(stage));
        let (_, _, kept) = engine.read_display(&mut photo, &p.view);
        let mut fresh = engine.upload(scene.as_raw(), w, h);
        engine.render(&mut fresh, &p);
        let (_, _, scratch) = engine.read_display(&mut fresh, &p.view);
        let (mean, _, max) = differences(&kept, &scratch);
        assert!(max < 1e-6, "reused stages differ from a fresh render at {stage:?}: mean {mean}, max {max}");
    }
}

/// Grain comes from a different random source than numpy's, so pictures cannot match pixel
/// for pixel. Its statistics on flat tones must: level, strength, skew, and how alike
/// neighbouring pixels are.
#[test]
fn grain_statistics_match_python() {
    let precision = if std::env::var("STUDIO_FULL").is_ok() { Precision::Full } else { Precision::Half };
    let Some((mut engine, _)) = Engine::headless(precision) else {
        return;
    };
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(fixtures().join("grain.json")).unwrap()).unwrap();
    let bands = image::open(fixtures().join("bands.png")).unwrap().into_rgba8();
    let (w, h) = bands.dimensions();
    let (bw, margin) = (fx["band_px"].as_u64().unwrap() as usize, fx["margin"].as_u64().unwrap() as usize);
    let mut photo = engine.upload(bands.as_raw(), w, h);
    let mut planner = Planner::new();
    let flat = Settings { house: 0.0, detail_px: 0, auto_ev: false, ..Settings::default() };
    let cases = [
        ("default", Settings { film_mm: 4.5, ..flat.clone() }),
        ("coarse", Settings { film_mm: 4.5, film: FilmKind::Trix, grain: 2.0, grain_light: 1.0, dev: Some(Dev::Minutes(9.0)), ..flat.clone() }),
        ("few_grains", Settings { film_mm: 1.2, grain_light: 1.0, ..flat.clone() }),
    ];
    let mut failures: Vec<String> = Vec::new();
    for (name, s) in cases {
        let plan = planner.plan(&s, (w, h), 0.0, engine.gpu.max_dim);
        engine.render(&mut photo, &plan);
        let (_, _, rgba) = engine.read_display(&mut photo, &plan.view);
        let at = |x: usize, y: usize| rgba[(y * w as usize + x) * 4 + 1] as f64;
        for (i, want) in fx["cases"][name]["bands"].as_array().unwrap().iter().enumerate() {
            let (x0, x1, y0, y1) = (i * bw + margin, (i + 1) * bw - margin, margin, h as usize - margin);
            let n = ((x1 - x0) * (y1 - y0)) as f64;
            let mean = (y0..y1).flat_map(|y| (x0..x1).map(move |x| (x, y))).map(|(x, y)| at(x, y)).sum::<f64>() / n;
            let (mut var, mut third, mut pair, mut pairs) = (0.0, 0.0, 0.0, 0.0);
            for y in y0..y1 {
                for x in x0..x1 {
                    let d = at(x, y) - mean;
                    var += d * d;
                    third += d * d * d;
                    if x + 1 < x1 {
                        pair += d * (at(x + 1, y) - mean);
                        pairs += 1.0;
                    }
                }
            }
            let std = (var / n).sqrt();
            let skew = third / n / std.powi(3).max(1e-12);
            let neighbour = pair / pairs / (var / n);
            let f = |k: &str| want[k].as_f64().unwrap();
            eprintln!("{name:10} band {i}: mean {mean:.4} ({:.4})  std {std:.4} ({:.4})  skew {skew:+.2} ({:+.2})  neighbour {neighbour:+.3} ({:+.3})", f("mean"), f("std"), f("skew"), f("neighbour"));
            // tolerances allow for the sampling error of both renders
            let se = 6.0 * f("std") / n.sqrt();
            for (what, got, want, tol) in [("mean", mean, f("mean"), 0.004 + se), ("std", std, f("std"), 0.05 * f("std") + 0.0008), ("skew", skew, f("skew"), 0.25), ("neighbour", neighbour, f("neighbour"), 0.06)] {
                if (got - want).abs() > tol {
                    failures.push(format!("{name} band {i}: {what} {got:.4} against {want:.4}"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "grain statistics differ from twb2.py:\n{}", failures.join("\n"));
}
