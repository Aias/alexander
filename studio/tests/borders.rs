//! Borders round the picture: the port finds the same picture area as twb2.py in each
//! synthetic photo of tests/make_fixtures.py (and none where it finds none), and a scene in
//! a mat renders as twb2.py renders it, with the border kept and stripped.
use alexander_studio::engine::Engine;
use alexander_studio::export::{cropped_long, Picture};
use alexander_studio::frame;
use alexander_studio::gpu::Precision;
use alexander_studio::params::{Frame, Settings};
use alexander_studio::plan::Planner;
use alexander_studio::tone;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn expected() -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join("borders.json")).unwrap()).unwrap()
}

/// Both tools apply one rule in double precision. They differ only in how 8-bit values
/// become linear light (float32 arrays in numpy, a table here) and in the order of sums in
/// the smoothing, which can move a band's end by a pixel where a value sits on the
/// tolerance, and the rounded-up inner edge by one more. Two pixels allow for both.
const PIXELS: i64 = 2;

#[test]
fn finds_the_same_areas_as_python() {
    let fx = expected();
    let mut failures = Vec::new();
    for (name, want) in fx["found"].as_object().unwrap() {
        let file = if name == "scene_framed" { "scene_framed.png".to_string() } else { format!("{name}.png") };
        let img = image::open(fixtures().join("borders").join(file)).unwrap().into_rgba8();
        let (w, h) = img.dimensions();
        let got = frame::find(img.as_raw(), w as usize, h as usize).map(|a| a.pixels(w, h));
        eprintln!("{name:18} python {want}  rust {got:?}");
        match (got, want.as_array()) {
            (None, None) => {}
            (Some(g), Some(wv)) => {
                for k in 0..4 {
                    if (g[k] as i64 - wv[k].as_i64().unwrap()).abs() > PIXELS {
                        failures.push(format!("{name}: {g:?} against {want}"));
                        break;
                    }
                }
            }
            _ => failures.push(format!("{name}: {got:?} against {want}")),
        }
    }
    assert!(failures.is_empty(), "picture areas differ from twb2.py:\n{}", failures.join("\n"));
}

#[test]
fn crop_keeps_resolution() {
    // a 2560-px grid of a 1873-px original: the picture area keeps the original's scale
    assert_eq!(cropped_long((1651, 2275), (1950, 2560), 1873), 1664);
    assert_eq!(cropped_long((300, 200), (400, 300), 400), 300);
    let pic = Picture { w: 4, h: 3, rgba: (0..48).map(|v| v as f32).collect() };
    let a = frame::Area { left: 0.25, top: 1.0 / 3.0, right: 0.75, bottom: 1.0 };
    let c = pic.crop(&a);
    assert_eq!((c.w, c.h), (2, 2));
    assert_eq!(c.rgba[..4], [20.0, 21.0, 22.0, 23.0]);
}

fn differences(got: &[f32], want: &[f32]) -> (f64, f64) {
    let mut d: Vec<f64> = got.iter().zip(want).map(|(a, b)| (*a as f64 - *b as f64).abs()).collect();
    d.sort_by(|a, b| a.partial_cmp(b).unwrap());
    (d.iter().sum::<f64>() / d.len() as f64, d[(d.len() as f64 * 0.999) as usize])
}

#[test]
fn framed_render_matches_python() {
    let Some((mut engine, _)) = Engine::headless(Precision::Half) else {
        eprintln!("no GPU here; render test skipped");
        return;
    };
    let fx = expected();
    let img = image::open(fixtures().join("borders/scene_framed.png")).unwrap().into_rgba8();
    let (w, h) = img.dimensions();
    let area = frame::find(img.as_raw(), w as usize, h as usize).expect("the mat is found");
    let s = Settings { grain: 0.0, detail_px: 0, film_mm: fx["film_mm"].as_f64().unwrap(), auto_ev: true, ..Settings::default() };
    let mut planner = Planner::new();
    let weights = planner.film(s.film).weights[s.filter as usize];
    let (median, top) = tone::exposure_levels_in(img.as_raw(), w as usize, h as usize, weights, area.pixels(w, h));
    let auto = tone::auto_ev(median, top, s.auto_ev_target);
    let mut photo = engine.upload(img.as_raw(), w, h);
    let mut render = |s: &Settings| {
        let plan = planner.plan(s, (w, h), auto, Some(area), engine.gpu.max_dim);
        engine.render(&mut photo, &plan);
        let (pw, ph, rgba) = engine.read_display(&mut photo, &plan.view);
        Picture { w: pw, h: ph, rgba }
    };
    let keep = render(&s);
    let off = render(&Settings { frame: Frame::Off, ..s.clone() });
    let load = |name: &str| -> (u32, u32, Vec<f32>) {
        let im = image::open(fixtures().join(name)).unwrap().into_luma16();
        let (iw, ih) = im.dimensions();
        (iw, ih, im.as_raw().iter().map(|&v| v as f32 / 65535.0).collect())
    };
    let green = |p: &Picture| -> Vec<f32> { p.rgba.as_chunks::<4>().0.iter().map(|q| q[1]).collect() };
    // kept: the whole sheet, measured inside the mat
    let (_, _, want) = load("render_framed_keep.png");
    let (mean, high) = differences(&green(&keep), &want);
    eprintln!("keep   mean {mean:.5}  99.9% {high:.5}");
    assert!(mean < 0.0015 && high < 0.012, "framed render differs from twb2.py (mean {mean:.5}, 99.9% {high:.5})");
    // measuring inside the mat changes the house tones; without it they are as before
    let (mean_off, _) = differences(&green(&off), &want);
    assert!(mean_off > mean, "measuring inside the border made no difference");
    // stripped: the same pixels, cut to the picture area
    let strip = keep.crop(&area);
    let (sw, sh, want) = load("render_framed_strip.png");
    assert_eq!((strip.w, strip.h), (sw, sh), "stripped size");
    let (mean, high) = differences(&green(&strip), &want);
    eprintln!("strip  mean {mean:.5}  99.9% {high:.5}");
    assert!(mean < 0.0015 && high < 0.012, "stripped render differs from twb2.py (mean {mean:.5}, 99.9% {high:.5})");
    let [x0, y0, x1, _] = area.pixels(w, h);
    for y in 0..strip.h {
        let a = &keep.rgba[((y + y0) * w + x0) as usize * 4..((y + y0) * w + x1) as usize * 4];
        let b = &strip.rgba[(y * strip.w) as usize * 4..((y + 1) * strip.w) as usize * 4];
        assert_eq!(a, b, "stripping changed a pixel in row {y}");
    }
}

/// The picture area is part of what the finish depends on: changing it redoes the finish
/// alone, with the house tones measured afresh, and stripping the border redoes nothing.
#[test]
fn area_reuses_stages() {
    use alexander_studio::engine::Stage;
    let Some((mut engine, _)) = Engine::headless(Precision::Half) else {
        return;
    };
    let img = image::open(fixtures().join("borders/scene_framed.png")).unwrap().into_rgba8();
    let (w, h) = img.dimensions();
    let area = frame::find(img.as_raw(), w as usize, h as usize).unwrap();
    let mut planner = Planner::new();
    let mut photo = engine.upload(img.as_raw(), w, h);
    let keep = Settings { detail_px: 0, film_mm: 3.6, ..Settings::default() };
    let strip = Settings { frame: Frame::Strip, ..keep.clone() };
    let off = Settings { frame: Frame::Off, ..keep.clone() };
    let mut plan = |s: &Settings| planner.plan(s, (w, h), -0.6, Some(area), 8192);
    let (p_off, p_keep, p_strip) = (plan(&off), plan(&keep), plan(&strip));
    assert_eq!(engine.render(&mut photo, &p_off), Some(Stage::Optics));
    assert_eq!(engine.render(&mut photo, &p_keep), Some(Stage::Finish));
    assert_eq!(engine.render(&mut photo, &p_strip), None);
    let (_, _, kept) = engine.read_display(&mut photo, &p_keep.view);
    let mut fresh = engine.upload(img.as_raw(), w, h);
    engine.render(&mut fresh, &p_keep);
    let (_, _, scratch) = engine.read_display(&mut fresh, &p_keep.view);
    assert!(kept.iter().zip(&scratch).all(|(a, b)| (a - b).abs() < 1e-6), "a reused render differs from a fresh one");
    engine.render(&mut photo, &p_off);
    let (_, _, back) = engine.read_display(&mut photo, &p_off.view);
    assert!(back.iter().zip(&kept).any(|(a, b)| (a - b).abs() > 1e-3), "the house tones were not measured afresh");
}

#[test]
fn border_choice_reaches_the_command_line() {
    assert!(!Settings::default().cli_flags().contains("--frame"));
    assert!(Settings { frame: Frame::Strip, ..Settings::default() }.cli_flags().contains("--frame strip"));
    assert!(Settings { frame: Frame::Off, ..Settings::default() }.cli_flags().contains("--frame off"));
    let old: Settings = serde_json::from_str(r#"{"grade": 2.0}"#).unwrap();
    assert_eq!(old.frame, Frame::Keep, "a look saved before the choice existed");
}
