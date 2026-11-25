//! The panel of knobs. It edits one `Settings`; the caller carries what changed over to
//! every selected photo.

use alexander_studio::film::{Dev, FilmKind, Filter};
use alexander_studio::params::{Repro, Settings, View};
use egui::{ComboBox, Label, RichText, Sense, Slider, Ui};

pub enum Action {
    /// None switches it over; otherwise on or off.
    Process(Option<bool>),
    CopyLook,
    PasteLook,
    Reset,
    SaveLook,
    LoadLook,
    CopyFlags,
}

const LABEL_W: f32 = 96.0;
const VALUE_W: f32 = 46.0;

fn label_cell(ui: &mut Ui, label: &str, sense: Sense) -> egui::Response {
    let size = egui::vec2(LABEL_W, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.set_min_size(size);
        ui.add(Label::new(RichText::new(label).weak()).sense(sense).truncate())
    })
    .inner
}

/// A labelled slider. Double-clicking the label or the slider restores the default.
#[allow(clippy::too_many_arguments)]
fn knob(ui: &mut Ui, label: &str, tip: &str, v: &mut f64, range: std::ops::RangeInclusive<f64>, default: f64, decimals: usize) {
    ui.horizontal(|ui| {
        let l = label_cell(ui, label, Sense::click());
        let l = if tip.is_empty() { l } else { l.on_hover_text(tip) };
        ui.spacing_mut().slider_width = (ui.available_width() - VALUE_W - ui.spacing().item_spacing.x).max(40.0);
        let r = ui.add(Slider::new(v, range).max_decimals(decimals).min_decimals(decimals).trailing_fill(true).clamping(egui::SliderClamping::Never));
        if l.double_clicked() || r.double_clicked() {
            *v = default;
        }
    });
}

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        let l = label_cell(ui, label, Sense::hover());
        if !tip.is_empty() {
            l.on_hover_text(tip);
        }
        add(ui);
    });
}

fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, v: &mut T, options: &[(T, &str)]) {
    let current = options.iter().find(|o| o.0 == *v).map_or("", |o| o.1);
    ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(current).show_ui(ui, |ui| {
        for (o, name) in options {
            ui.selectable_value(v, *o, *name);
        }
    });
}

fn section(ui: &mut Ui, name: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    egui::CollapsingHeader::new(RichText::new(name).strong()).default_open(open).show(ui, |ui| {
        ui.add_space(2.0);
        add(ui);
        ui.add_space(4.0);
    });
}

/// The panel's header: process on or off, and the less frequent commands.
pub fn header(ui: &mut Ui, processed: bool, can_paste: bool, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let mut on = processed;
        ui.selectable_value(&mut on, false, "Original").on_hover_text("V");
        ui.selectable_value(&mut on, true, "Film").on_hover_text("V");
        if on != processed {
            actions.push(Action::Process(Some(on)));
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.menu_button("…", |ui| {
                ui.set_min_width(200.0);
                let mut item = |ui: &mut Ui, text: &str, keys: &str, enabled: bool, a: Action| {
                    if ui.add_enabled(enabled, egui::Button::new(text).shortcut_text(keys)).clicked() {
                        actions.push(a);
                        ui.close_menu();
                    }
                };
                item(ui, "Copy Look", "⇧⌘C", processed, Action::CopyLook);
                item(ui, "Paste Look", "⇧⌘V", can_paste, Action::PasteLook);
                item(ui, "Reset to Original", "⇧⌘R", true, Action::Reset);
                ui.separator();
                item(ui, "Save Look…", "", processed, Action::SaveLook);
                item(ui, "Load Look…", "", true, Action::LoadLook);
                item(ui, "Copy as twb2.py Flags", "", processed, Action::CopyFlags);
            });
        });
    });
}

pub fn knobs(ui: &mut Ui, s: &mut Settings) {
    let d = Settings::default();
    section(ui, "Negative", true, |ui| {
        row(ui, "Film", "", |ui| combo(ui, "film", &mut s.film, &FilmKind::ALL.map(|k| (k, k.label()))));
        row(ui, "Filter", "", |ui| combo(ui, "filter", &mut s.filter, &[(Filter::None, "None"), (Filter::Yellow, "Yellow"), (Filter::Orange, "Orange"), (Filter::Red, "Red")]));
        knob(ui, "Film size", "Long edge of film the picture fills, mm. 36 is a full 35mm frame", &mut s.film_mm, 8.0..=72.0, d.film_mm, 1);
        knob(ui, "Exposure", "Stops", &mut s.ev, -3.0..=3.0, d.ev, 2);
        row(ui, "Auto exposure", "", |ui| {
            ui.checkbox(&mut s.auto_ev, "");
        });
        if s.auto_ev {
            knob(ui, "Auto target", "Display value the median is placed at", &mut s.auto_ev_target, 0.2..=0.7, d.auto_ev_target, 2);
        }
        let mut kind = match s.dev {
            None => 0,
            Some(Dev::Minutes(_)) => 1,
            Some(Dev::Norm1952) => 2,
            Some(Dev::Norm1956) => 3,
            Some(Dev::Norm1981) => 4,
        };
        let was = kind;
        row(ui, "Development", "", |ui| combo(ui, "dev", &mut kind, &[(0, "Datasheet"), (1, "Minutes"), (2, "1952 norm"), (3, "1956 norm"), (4, "1981 norm")]));
        if kind != was {
            s.dev = match kind {
                0 => None,
                1 => Some(Dev::Minutes(if s.film == FilmKind::DoubleX { 6.0 } else { 7.0 })),
                2 => Some(Dev::Norm1952),
                3 => Some(Dev::Norm1956),
                _ => Some(Dev::Norm1981),
            };
        }
        if let Some(Dev::Minutes(m)) = &mut s.dev {
            let def = if s.film == FilmKind::DoubleX { 6.0 } else { 7.0 };
            knob(ui, "Minutes", "Longer is more contrast", m, 3.0..=14.0, def, 1);
        }
        knob(ui, "Softness", "Blur on the film, µm", &mut s.lens_um, 0.0..=40.0, d.lens_um, 1);
        knob(ui, "Softness shape", "2 is a Gaussian; lower adds a long faint skirt", &mut s.lens_exponent, 0.6..=2.0, d.lens_exponent, 2);
        knob(ui, "Halation", "Glow around bright areas", &mut s.halation, 0.0..=0.2, d.halation, 3);
        knob(ui, "Halation width", "µm", &mut s.halation_um, 10.0..=200.0, d.halation_um, 0);
        knob(ui, "Grain", "Relative to the datasheet", &mut s.grain, 0.0..=3.0, d.grain, 2);
        knob(ui, "Grain in lights", "Grain kept in light tones", &mut s.grain_light, 0.0..=1.0, d.grain_light, 2);
        knob(ui, "Grain with dev.", "How grain grows with development", &mut s.grain_dev, 0.0..=2.5, d.grain_dev, 2);
        let mut edges = !s.no_edge;
        row(ui, "Edge effects", "", |ui| {
            ui.checkbox(&mut edges, "");
        });
        s.no_edge = !edges;
        if edges {
            knob(ui, "Edge strength", "", &mut s.edge, 0.0..=3.0, d.edge, 2);
            knob(ui, "Edge density", "0: tone difference alone. 1: Nelson", &mut s.edge_density, 0.0..=1.0, d.edge_density, 2);
        }
    });
    section(ui, "Print", true, |ui| {
        knob(ui, "Grade", "Paper grade", &mut s.grade, -1.0..=5.0, d.grade, 2);
        knob(ui, "Exposure", "Stops; more is darker", &mut s.print_ev, -2.5..=2.5, d.print_ev, 2);
        knob(ui, "Grey density", "Print density of a metered grey", &mut s.print_grey, 0.3..=1.3, d.print_grey, 3);
        knob(ui, "Enlarger blur", "µm on the film", &mut s.enlarger_um, 0.0..=15.0, d.enlarger_um, 1);
        knob(ui, "House tones", "0: straight print. 1: the book's tone range", &mut s.house, 0.0..=1.0, d.house, 2);
    });
    section(ui, "Reproduction", true, |ui| {
        row(ui, "Finish", "", |ui| combo(ui, "repro", &mut s.repro, &[(Repro::None, "Print"), (Repro::Copy, "Copied from a page"), (Repro::Book, "Book page")]));
        match s.repro {
            Repro::None => {}
            Repro::Copy => {
                knob(ui, "Copy blur", "Thousandths of the long edge", &mut s.copy_blur, 0.0..=2.0, d.copy_blur, 2);
                knob(ui, "Copy contrast", "", &mut s.copy_gain, 0.5..=2.0, d.copy_gain, 2);
                knob(ui, "Highlight drop", "Highlight dot lost at each printing", &mut s.copy_drop, 0.0..=0.3, d.copy_drop, 3);
                knob(ui, "Shadow fill", "Shadow tone that closes to solid ink", &mut s.copy_fill, 0.7..=1.0, d.copy_fill, 3);
                knob(ui, "Source ink", "Solid ink density on the source page", &mut s.copy_ink, 0.8..=2.5, d.copy_ink, 2);
            }
            Repro::Book => {
                let mut auto = s.page_mm.is_none();
                row(ui, "Page size auto", "From the file's size at the PPI below", |ui| {
                    ui.checkbox(&mut auto, "");
                });
                if auto {
                    s.page_mm = None;
                    knob(ui, "PPI", "", &mut s.ppi, 72.0..=600.0, d.ppi, 0);
                } else {
                    let mut mm = s.page_mm.unwrap_or(200.0);
                    knob(ui, "Page size", "Long edge on the page, mm", &mut mm, 30.0..=400.0, 200.0, 0);
                    s.page_mm = Some(mm);
                }
                knob(ui, "Screen", "Lines per inch", &mut s.lpi, 65.0..=200.0, d.lpi, 0);
                knob(ui, "Ink density", "1.20 uncoated, 1.85 coated", &mut s.ink_density, 0.8..=2.2, d.ink_density, 2);
                knob(ui, "Dot gain", "", &mut s.dot_gain, 0.0..=0.3, d.dot_gain, 3);
                knob(ui, "Smallest dot", "", &mut s.min_dot, 0.0..=0.1, d.min_dot, 3);
                knob(ui, "Camera blur", "Copy camera blur on the page, mm", &mut s.copy_blur_mm, 0.0..=0.2, d.copy_blur_mm, 3);
                knob(ui, "Paper scatter", "Light spread in the paper, mm", &mut s.paper_scatter_mm, 0.0..=0.2, d.paper_scatter_mm, 3);
            }
        }
    });
    if s.repro == Repro::Book {
        section(ui, "Viewing", true, |ui| {
            let mut v = s.view;
            row(ui, "View", "", |ui| combo(ui, "view", &mut v, &[(None, "Auto"), (Some(View::Scan), "Scan"), (Some(View::White), "Ink on white"), (Some(View::Page), "Ink on cream"), (Some(View::Print), "Print")]));
            s.view = v;
            if s.view_mode() == View::Scan {
                knob(ui, "Scan black", "Display value of solid ink", &mut s.scan_black, 0.0..=0.3, d.scan_black, 3);
                knob(ui, "Scan white", "Page value sent to white", &mut s.scan_white, 0.7..=1.0, d.scan_white, 3);
            }
        });
    }
    section(ui, "Simulation", false, |ui| {
        let mut grid = s.detail_px as f64;
        knob(ui, "Grid", "Long edge of the simulation grid, px. 0 is the photo's own size", &mut grid, 0.0..=8192.0, d.detail_px as f64, 0);
        s.detail_px = ((grid / 32.0).round() * 32.0).clamp(0.0, 16384.0) as u32;
        row(ui, "Seed", "Grain pattern", |ui| {
            ui.add(egui::DragValue::new(&mut s.seed).range(0..=u32::MAX));
        });
    });
}
