//! Render one picture without a window, for checking the engine against twb2.py.
//!
//!     cargo run --release --example headless -- in.png out.png ['{"grain": 0.0, "house": 0.0}']
//!
//! The input is used at its own size (as `--detail-px 0` does); the output is a 16-bit
//! greyscale PNG of the display values. Settings are given as JSON; unnamed ones keep
//! their defaults.
use alexander_studio::engine::Engine;
use alexander_studio::gpu::Precision;
use alexander_studio::params::Settings;
use alexander_studio::plan::Planner;
use alexander_studio::tone;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: headless in.png out.png [settings as JSON]");
        std::process::exit(2);
    }
    let s: Settings = args.get(3).map(|j| serde_json::from_str(j).expect("settings")).unwrap_or_default();
    let precision = if std::env::var("STUDIO_FULL").is_ok() { Precision::Full } else { Precision::Half };
    let (mut engine, name) = Engine::headless(precision).expect("no usable GPU");
    let img = image::open(&args[1]).expect("input").into_rgba8();
    let (w, h) = img.dimensions();
    let mut planner = Planner::new();
    let weights = planner.film(s.film).weights[s.filter as usize];
    let (median, top) = tone::exposure_levels(img.as_raw(), w as usize, h as usize, weights);
    let plan = planner.plan(&s, (w, h), tone::auto_ev(median, top, s.auto_ev_target), None, engine.gpu.max_dim);
    let mut photo = engine.upload(img.as_raw(), w, h);
    let start = std::time::Instant::now();
    engine.render(&mut photo, &plan);
    let (_, _, rgba) = engine.read_display(&mut photo, &plan.view, 1);
    eprintln!("{name}: {w}x{h} in {:.0} ms (flags: {})", start.elapsed().as_secs_f64() * 1000.0, s.cli_flags());
    let grey: Vec<u16> = rgba.as_chunks::<4>().0.iter().map(|p| (p[1].clamp(0.0, 1.0) * 65535.0 + 0.5) as u16).collect();
    image::ImageBuffer::<image::Luma<u16>, _>::from_raw(w, h, grey).unwrap().save(&args[2]).expect("output");
}
