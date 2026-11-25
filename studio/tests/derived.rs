//! The CPU-side maths against values dumped from twb2.py (tests/make_fixtures.py).
use alexander_studio::film::{contrast_index, curve_gamma, curve_x, interp, Dev, Film, FilmKind};
use alexander_studio::plan::soften_error;
use alexander_studio::tone::{compress_tau, lin_to_srgb, HOUSE_LUT_N};
use alexander_studio::tone::{house_lut, Paper};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("fixtures/derived.json")).unwrap()
}

fn close(name: &str, got: f64, want: f64, tol: f64) {
    assert!((got - want).abs() <= tol, "{name}: got {got}, want {want}");
}

#[test]
fn film_curves_and_development() {
    let fx = fixture();
    for kind in FilmKind::ALL {
        let film = Film::load(kind);
        let f = &fx["films"][kind.cli()];
        close("logh_grey", film.logh_grey(), f["logh_grey"].as_f64().unwrap(), 1e-9);
        close("granularity ci", film.granularity_contrast_index(), f["granularity_ci"].as_f64().unwrap(), 1e-6);
        for (key, dev) in [("None", None), ("9.0", Some(Dev::Minutes(9.0))), ("norm-1952", Some(Dev::Norm1952)), ("norm-1956", Some(Dev::Norm1956)), ("norm-1981", Some(Dev::Norm1981))] {
            let d = &f["devs"][key];
            let t = film.resolve_dev(dev);
            close(&format!("{} {key} minutes", kind.cli()), t, d["minutes"].as_f64().unwrap(), 0.011);
            let t = d["minutes"].as_f64().unwrap();
            let (cx, cy) = (curve_x(), film.curve(t));
            for (i, want) in d["curve_every_25"].as_array().unwrap().iter().enumerate() {
                close("curve", cy[i * 25], want.as_f64().unwrap(), 1e-9);
            }
            let base = cy.iter().cloned().fold(f64::INFINITY, f64::min);
            close("ref", interp(film.logh_grey(), &cx, &cy) - base, d["ref"].as_f64().unwrap(), 1e-9);
            close("contrast index", contrast_index(&cx, &cy), d["contrast_index"].as_f64().unwrap(), 1e-9);
            close("gamma", curve_gamma(&cx, &cy), d["gamma"].as_f64().unwrap(), 1e-9);
            close("development grain", film.development_grain(t, 1.3), d["development_grain"].as_f64().unwrap(), 1e-9);
            close("grain area", Film::grain_area_um2(film.rms_granularity), d["grain_area"].as_f64().unwrap(), 1e-12);
        }
    }
}

#[test]
fn paper_and_print() {
    let fx = fixture();
    for grade in ["3.0", "2.55", "0.9", "4.25"] {
        let p = Paper::new(grade.parse().unwrap());
        let f = &fx["paper"][grade];
        for (i, want) in f["curve_every_30"].as_array().unwrap().iter().enumerate() {
            close("paper curve", p.curve[i * 30], want.as_f64().unwrap(), 1e-9);
        }
        close("dmin", p.dmin, f["dmin"].as_f64().unwrap(), 1e-12);
        close("range", p.range, f["range"].as_f64().unwrap(), 1e-12);
        close("offset", p.offset(0.681, 0.745, -0.5), f["offset_ref0681"].as_f64().unwrap(), 1e-9);
    }
    let range = Paper::new(3.0).range;
    close("tau 1.6", compress_tau(range, 1.6).unwrap(), fx["misc"]["tau_2.1_1.6"].as_f64().unwrap(), 1e-9);
    close("tau 1.2", compress_tau(range, 1.2).unwrap(), fx["misc"]["tau_2.1_1.2"].as_f64().unwrap(), 1e-9);
}

#[test]
fn house_curve() {
    let fx = fixture();
    for case in fx["misc"]["house"].as_array().unwrap() {
        let own: Vec<f64> = case["own"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
        let lut = house_lut(&[own[0], own[1], own[2], own[3], own[4]], case["strength"].as_f64().unwrap());
        for (i, want) in case["out"].as_array().unwrap().iter().enumerate() {
            let v = i as f64 / 40.0;
            let pos = v * (HOUSE_LUT_N - 1) as f64;
            let (a, t) = (pos.floor() as usize, pos - pos.floor());
            let lin = lut[a] as f64 * (1.0 - t) + lut[(a + 1).min(HOUSE_LUT_N - 1)] as f64 * t;
            close("house", lin_to_srgb(lin), want.as_f64().unwrap(), 2e-4);
        }
    }
}

#[test]
fn long_tailed_blur_as_gaussians() {
    // The default film and softness at the default cell size, and a few others. Below 0.4
    // cycles per pixel the mixture follows the long-tailed blur closely. Right at the grid's
    // limit a sampled Gaussian cannot fall as far as the true curve when the blur is under a
    // pixel wide, so the tolerance over the whole range is looser.
    for (lens, n, emul, ne) in [(10.0 / 9.375, 1.15, 5.147 / 9.375, 2.0), (8.0 / 9.375, 1.15, 3.87 / 9.375, 2.0), (10.0 / 9.375, 1.15, 5.93 / 9.375, 1.108), (2.0, 1.0, 0.4, 2.0), (0.5, 1.15, 0.2, 2.0), (4.0, 1.15, 1.5, 2.0)] {
        let (low, all) = (soften_error(lens, n, emul, ne, 0.4), soften_error(lens, n, emul, ne, 0.5));
        assert!(low < 0.02 && all < 0.06, "mixture error {low} / {all} for lens {lens} n {n} emulsion {emul} n {ne}");
    }
    let e = soften_error(10.0 / 9.375, 1.15, 5.147 / 9.375, 2.0, 0.4);
    assert!(e < 0.006, "mixture error {e} at the defaults");
}
