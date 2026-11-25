//! The knobs. Names and defaults follow `twb2.py`, so a look tuned here can be
//! reproduced on the command line (`Settings::cli_flags`).

use crate::film::{Dev, FilmKind, Filter};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Repro {
    None,
    Copy,
    Book,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    Print,
    Scan,
    White,
    Page,
}

/// What to do about a uniform border round the picture (see `frame`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    /// Find it, measure exposure and tones inside it, keep it in the picture.
    Keep,
    /// The same, then show and export only the picture inside it.
    Strip,
    /// Do not look for one.
    Off,
}

/// The part of the photo kept. The photo turns `angle` degrees clockwise about its centre
/// under a level frame, which keeps the fractions of its width from `left` to `right` and of
/// its height from `top` to `bottom`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Crop {
    pub left: f64,
    pub top: f64,
    pub right: f64,
    pub bottom: f64,
    pub angle: f64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub detail_px: u32,
    pub seed: u32,
    // negative
    pub film: FilmKind,
    pub frame: Frame,
    pub crop: Option<Crop>,
    pub film_mm: f64,
    pub filter: Filter,
    pub lens_um: f64,
    pub lens_exponent: f64,
    pub halation: f64,
    pub halation_um: f64,
    pub ev: f64,
    pub auto_ev: bool,
    pub auto_ev_target: f64,
    pub dev: Option<Dev>,
    pub grain: f64,
    pub grain_light: f64,
    pub grain_dev: f64,
    pub edge: f64,
    pub edge_density: f64,
    pub no_edge: bool,
    // print
    pub grade: f64,
    pub print_ev: f64,
    pub print_grey: f64,
    pub enlarger_um: f64,
    pub house: f64,
    // copied from a printed page
    pub copy_blur: f64,
    pub copy_gain: f64,
    pub copy_drop: f64,
    pub copy_fill: f64,
    pub copy_ink: f64,
    // book page
    pub repro: Repro,
    pub page_mm: Option<f64>,
    pub ppi: f64,
    pub lpi: f64,
    pub ink_density: f64,
    pub dot_gain: f64,
    pub min_dot: f64,
    pub copy_blur_mm: f64,
    pub paper_scatter_mm: f64,
    // viewing
    pub view: Option<View>,
    pub scan_black: f64,
    pub scan_white: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            detail_px: 2560,
            seed: 1,
            film: FilmKind::Trix1956,
            frame: Frame::Keep,
            crop: None,
            film_mm: 24.0,
            filter: Filter::None,
            lens_um: 10.0,
            lens_exponent: 1.15,
            halation: 0.0,
            halation_um: 65.0,
            ev: 0.0,
            auto_ev: true,
            auto_ev_target: 0.45,
            dev: None,
            grain: 1.0,
            grain_light: 0.6,
            grain_dev: 1.3,
            edge: 1.0,
            edge_density: 0.0,
            no_edge: false,
            grade: 3.0,
            print_ev: -0.5,
            print_grey: 0.745,
            enlarger_um: 2.3,
            house: 1.0,
            copy_blur: 0.35,
            copy_gain: 1.2,
            copy_drop: 0.05,
            copy_fill: 0.95,
            copy_ink: 1.6,
            repro: Repro::None,
            page_mm: None,
            ppi: 300.0,
            lpi: 133.0,
            ink_density: 1.20,
            dot_gain: 0.08,
            min_dot: 0.03,
            copy_blur_mm: 0.03,
            paper_scatter_mm: 0.06,
            view: None,
            scan_black: 0.07,
            scan_white: 0.97,
        }
    }
}

impl Settings {
    pub fn view_mode(&self) -> View {
        self.view.unwrap_or(if self.repro == Repro::Book { View::Scan } else { View::Print })
    }

    /// The `twb2.py` flags that reproduce these settings. Only values that differ from the
    /// command line's own defaults are listed.
    pub fn cli_flags(&self) -> String {
        let d = Settings::default();
        let mut f: Vec<String> = Vec::new();
        let mut num = |name: &str, v: f64, dv: f64| {
            if (v - dv).abs() > 1e-9 {
                f.push(format!("--{name} {}", trim(v)));
            }
        };
        num("detail-px", self.detail_px as f64, d.detail_px as f64);
        num("seed", self.seed as f64, d.seed as f64);
        num("film-mm", self.film_mm, d.film_mm);
        num("lens-um", self.lens_um, d.lens_um);
        num("lens-exponent", self.lens_exponent, d.lens_exponent);
        num("halation", self.halation, d.halation);
        num("halation-um", self.halation_um, d.halation_um);
        num("ev", self.ev, d.ev);
        num("grain", self.grain, d.grain);
        num("grain-light", self.grain_light, d.grain_light);
        num("grain-dev", self.grain_dev, d.grain_dev);
        num("edge", self.edge, d.edge);
        num("edge-density", self.edge_density, d.edge_density);
        num("grade", self.grade, d.grade);
        num("print-ev", self.print_ev, d.print_ev);
        num("print-grey", self.print_grey, d.print_grey);
        num("enlarger-um", self.enlarger_um, d.enlarger_um);
        num("house", self.house, d.house);
        num("copy-blur", self.copy_blur, d.copy_blur);
        num("copy-gain", self.copy_gain, d.copy_gain);
        num("copy-drop", self.copy_drop, d.copy_drop);
        num("copy-fill", self.copy_fill, d.copy_fill);
        num("copy-ink", self.copy_ink, d.copy_ink);
        num("ppi", self.ppi, d.ppi);
        num("lpi", self.lpi, d.lpi);
        num("ink-density", self.ink_density, d.ink_density);
        num("dot-gain", self.dot_gain, d.dot_gain);
        num("min-dot", self.min_dot, d.min_dot);
        num("copy-blur-mm", self.copy_blur_mm, d.copy_blur_mm);
        num("paper-scatter-mm", self.paper_scatter_mm, d.paper_scatter_mm);
        num("scan-black", self.scan_black, d.scan_black);
        num("scan-white", self.scan_white, d.scan_white);
        if self.film != d.film {
            f.push(format!("--film {}", self.film.cli()));
        }
        match self.frame {
            Frame::Keep => {}
            Frame::Strip => f.push("--frame strip".into()),
            Frame::Off => f.push("--frame off".into()),
        }
        if self.filter != d.filter {
            f.push(format!("--filter {}", self.filter.cli()));
        }
        if self.auto_ev {
            // off on the command line unless asked for
            if (self.auto_ev_target - 0.45).abs() > 1e-9 {
                f.push(format!("--auto-ev {}", trim(self.auto_ev_target)));
            } else {
                f.push("--auto-ev".into());
            }
        }
        if let Some(dev) = self.dev {
            f.push(format!("--dev {}", dev.cli()));
        }
        if self.no_edge {
            f.push("--no-edge".into());
        }
        match self.repro {
            Repro::None => {}
            Repro::Copy => f.push("--repro copy".into()),
            Repro::Book => f.push("--repro book".into()),
        }
        if let Some(mm) = self.page_mm {
            f.push(format!("--page-mm {}", trim(mm)));
        }
        if let Some(v) = self.view {
            f.push(format!(
                "--view {}",
                match v {
                    View::Print => "print",
                    View::Scan => "scan",
                    View::White => "white",
                    View::Page => "page",
                }
            ));
        }
        f.join(" ")
    }
}

fn trim(v: f64) -> String {
    let s = format!("{:.4}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s.is_empty() || s == "-" {
        "0".into()
    } else {
        s.into()
    }
}
