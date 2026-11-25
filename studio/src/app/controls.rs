//! The controls panel: the histogram, the process switch, the knobs in sections and the
//! look commands. It edits one `Settings`; the change is carried over to every selected photo.

use super::views::{segmented, LABEL, TEXT, VIEW_BG};
use super::App;
use alexander_studio::film::{Dev, FilmKind, Filter};
use alexander_studio::params::{Frame, Repro, Settings, View};
use egui::collapsing_header::CollapsingState;
use egui::{pos2, vec2, Align, Align2, Button, Color32, ComboBox, CornerRadius, DragValue, EventFilter, FontId, Key, Label, Layout, Rect, RichText, Sense, Shape, Stroke, Ui};
use std::ops::RangeInclusive;

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

const LABEL_W: f32 = 104.0;
const VALUE_W: f32 = 44.0;
const HANDLE_R: f32 = 5.0;
const RAIL: Color32 = Color32::from_gray(66);
const HANDLE: Color32 = Color32::from_gray(205);
const RULE: Color32 = Color32::from_gray(44);
const HISTOGRAM: Color32 = Color32::from_gray(150);

impl App {
    pub(super) fn panel_ui(&mut self, ui: &mut Ui, actions: &mut Vec<Action>) {
        let focus = self.lib.focus.filter(|_| !self.lib.selected().is_empty()).and_then(|f| self.lib.get(f));
        histogram(ui, focus.and_then(|it| self.tiers.histogram(it, self.before)));
        let processed = focus.is_some_and(|it| it.look.is_some());
        let mut s = focus.and_then(|it| it.look.clone().or_else(|| it.stash.clone())).unwrap_or_default();
        let enabled = focus.is_some();
        let can_paste = self.clipboard.is_some();
        ui.add_space(10.0);
        ui.add_enabled_ui(enabled, |ui| header(ui, processed, actions));
        ui.add_space(6.0);
        egui::TopBottomPanel::bottom("look").frame(egui::Frame::NONE.inner_margin(egui::Margin { top: 8, ..Default::default() })).show_inside(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| footer(ui, processed, can_paste, actions));
        });
        let before = s.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| knobs(ui, &mut s));
        });
        if enabled && s != before {
            let ids = self.lib.selection();
            if !self.edit_open {
                self.lib.checkpoint(&ids);
                self.edit_open = true;
            }
            self.lib.apply_change(&ids, &before, &s);
        }
    }
}

/// Counts of the display values from black to white, scaled so that the busiest tone short
/// of the clipped ends fills the box.
fn histogram(ui: &mut Ui, bins: Option<&[f32]>) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 88.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(4), VIEW_BG);
    for i in 1..4 {
        p.vline(rect.left() + rect.width() * i as f32 / 4.0, rect.y_range(), Stroke::new(1.0f32, Color32::from_gray(32)));
    }
    let Some(bins) = bins else { return };
    let top = bins[1..bins.len() - 1].iter().copied().fold(1.0, f32::max);
    let w = rect.width() / bins.len() as f32;
    let inner = rect.shrink2(vec2(0.0, 4.0));
    for (i, &c) in bins.iter().enumerate() {
        let h = (c / top).min(1.0) * inner.height();
        let x = rect.left() + i as f32 * w;
        p.rect_filled(Rect::from_min_max(pos2(x, inner.bottom() - h), pos2(x + w, inner.bottom())), CornerRadius::ZERO, HISTOGRAM);
    }
}

/// The process switch and the commands for looks as files.
fn header(ui: &mut Ui, processed: bool, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let mut on = processed;
        ui.add_sized([ui.available_width() - 34.0, 26.0], |ui: &mut Ui| segmented(ui, &mut on, &[(false, "Original", "V"), (true, "Film", "V")]));
        if on != processed {
            actions.push(Action::Process(Some(on)));
        }
        ui.spacing_mut().interact_size.y = 26.0;
        ui.menu_button("…", |ui| {
            ui.set_min_width(200.0);
            let mut item = |ui: &mut Ui, text: &str, enabled: bool, a: Action| {
                if ui.add_enabled(enabled, Button::new(text)).clicked() {
                    actions.push(a);
                    ui.close_menu();
                }
            };
            item(ui, "Save Look…", processed, Action::SaveLook);
            item(ui, "Load Look…", true, Action::LoadLook);
            item(ui, "Copy as twb2.py Flags", processed, Action::CopyFlags);
        });
    });
}

fn footer(ui: &mut Ui, processed: bool, can_paste: bool, actions: &mut Vec<Action>) {
    ui.horizontal(|ui| {
        let mut button = |ui: &mut Ui, text: &str, keys: &str, enabled: bool, a: Action| {
            if ui.add_enabled(enabled, Button::new(text).min_size(vec2(64.0, 24.0))).on_hover_text(keys).clicked() {
                actions.push(a);
            }
        };
        button(ui, "Copy", "⇧⌘C", processed, Action::CopyLook);
        button(ui, "Paste", "⇧⌘V", can_paste, Action::PasteLook);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| button(ui, "Reset", "⇧⌘R", true, Action::Reset));
    });
}

/// A slider without its value: a thin rail and a small handle. Dragging the handle moves
/// it from where it was taken; pressing the rail elsewhere puts it there.
pub fn slider(ui: &mut Ui, v: &mut f64, range: RangeInclusive<f64>, width: f32) -> egui::Response {
    let (lo, hi) = (*range.start(), *range.end());
    let (rect, mut r) = ui.allocate_exact_size(vec2(width.max(40.0), ui.spacing().interact_size.y), Sense::click_and_drag());
    let rail = rect.x_range().shrink(HANDLE_R + 1.0);
    let x_of = |v: f64| rail.min + rail.span() * ((v - lo) / (hi - lo)).clamp(0.0, 1.0) as f32;
    if let Some(p) = r.interact_pointer_pos().filter(|_| r.is_pointer_button_down_on()) {
        let grip = ui.data_mut(|d| {
            *d.get_temp_mut_or_insert_with(r.id, || {
                let dx = x_of(*v) - p.x;
                if dx.abs() <= HANDLE_R + 3.0 { dx } else { 0.0 }
            })
        });
        let nv = lo + (hi - lo) * ((p.x + grip - rail.min) / rail.span()).clamp(0.0, 1.0) as f64;
        if nv != *v {
            *v = nv;
            r.mark_changed();
        }
    } else {
        ui.data_mut(|d| d.remove::<f32>(r.id));
    }
    if r.has_focus() {
        ui.memory_mut(|m| m.set_focus_lock_filter(r.id, EventFilter { horizontal_arrows: true, ..Default::default() }));
        let steps = ui.input(|i| i.num_presses(Key::ArrowRight) as f64 - i.num_presses(Key::ArrowLeft) as f64);
        if steps != 0.0 {
            *v = (*v + steps * (hi - lo) / 100.0).clamp(lo, hi);
            r.mark_changed();
        }
    }
    let c = pos2(x_of(*v), rect.center().y);
    let p = ui.painter();
    p.hline(rail, c.y, Stroke::new(2.0f32, RAIL));
    let lit = r.hovered() || r.dragged() || r.has_focus();
    p.circle_filled(c, HANDLE_R + if lit { 1.0 } else { 0.0 }, if lit { Color32::WHITE } else { HANDLE });
    if r.has_focus() {
        p.circle_stroke(c, HANDLE_R + 3.5, Stroke::new(1.0f32, Color32::from_gray(200)));
    }
    r
}

fn label_cell(ui: &mut Ui, label: &str, sense: Sense) -> egui::Response {
    let size = vec2(LABEL_W, ui.spacing().interact_size.y);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        ui.add(Label::new(RichText::new(label).color(LABEL)).sense(sense).truncate())
    })
    .inner
}

/// A labelled slider and its value. Double-clicking the label or the slider restores the
/// default; the value can be dragged or typed, past the slider's ends too.
fn knob(ui: &mut Ui, label: &str, tip: &str, v: &mut f64, range: RangeInclusive<f64>, default: f64, decimals: usize) {
    ui.horizontal(|ui| {
        let l = label_cell(ui, label, Sense::click());
        let l = if tip.is_empty() { l } else { l.on_hover_text(tip) };
        let speed = (range.end() - range.start()) / 300.0;
        let w = ui.available_width() - VALUE_W - ui.spacing().item_spacing.x;
        let r = slider(ui, v, range, w);
        ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
        ui.spacing_mut().button_padding.x = 2.0;
        ui.add_sized([VALUE_W, ui.spacing().interact_size.y], DragValue::new(v).speed(speed).fixed_decimals(decimals));
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

fn check(ui: &mut Ui, label: &str, tip: &str, v: &mut bool) {
    row(ui, label, tip, |ui| {
        ui.add(egui::Checkbox::without_text(v));
    });
}

/// A section of knobs under a heading that opens and closes it. With `switch` the heading
/// carries a checkbox that turns the section's effect on and off.
fn section(ui: &mut Ui, name: &str, open: bool, switch: Option<&mut bool>, add: impl FnOnce(&mut Ui)) {
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(name), open);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    if r.clicked() {
        state.toggle(ui);
    }
    let p = ui.painter();
    p.hline(rect.x_range(), rect.top(), Stroke::new(1.0f32, RULE));
    p.text(rect.left_center(), Align2::LEFT_CENTER, name, FontId::proportional(12.5), if r.hovered() { Color32::WHITE } else { TEXT });
    let turn = egui::emath::Rot2::from_angle((state.openness(ui.ctx()) - 1.0) * std::f32::consts::FRAC_PI_2);
    let tip = pos2(rect.right() - 6.0, rect.center().y);
    p.add(Shape::convex_polygon([vec2(-3.5, -2.0), vec2(3.5, -2.0), vec2(0.0, 2.5)].map(|d| tip + turn * d).to_vec(), LABEL, Stroke::NONE));
    let on = switch.is_none_or(|on| {
        ui.put(Rect::from_center_size(pos2(rect.right() - 30.0, rect.center().y), vec2(18.0, 18.0)), egui::Checkbox::without_text(on));
        *on
    });
    state.show_body_unindented(ui, |ui| {
        ui.add_enabled_ui(on, add);
        ui.add_space(8.0);
    });
}

fn knobs(ui: &mut Ui, s: &mut Settings) {
    let d = Settings::default();
    section(ui, "Film", true, None, |ui| {
        row(ui, "Stock", "", |ui| combo(ui, "film", &mut s.film, &FilmKind::ALL.map(|k| (k, k.label()))));
        row(ui, "Filter", "", |ui| combo(ui, "filter", &mut s.filter, &[(Filter::None, "None"), (Filter::Yellow, "Yellow"), (Filter::Orange, "Orange"), (Filter::Red, "Red")]));
        knob(ui, "Size", "Long edge of film the picture fills, mm. 36 is a full 35mm frame", &mut s.film_mm, 8.0..=72.0, d.film_mm, 1);
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
    });
    section(ui, "Exposure", true, None, |ui| {
        check(ui, "Auto", "", &mut s.auto_ev);
        if s.auto_ev {
            knob(ui, "Auto target", "Display value the median is placed at", &mut s.auto_ev_target, 0.2..=0.7, d.auto_ev_target, 2);
        }
        knob(ui, "Exposure", "Stops", &mut s.ev, -3.0..=3.0, d.ev, 2);
        row(ui, "Border", "A uniform border round the picture, such as a print's paper or letterbox bars. Keep: exposure and tones are measured inside it. Strip: the same, and only the picture inside is shown and exported. Off: not looked for", |ui| {
            combo(ui, "frame", &mut s.frame, &[(Frame::Keep, "Keep"), (Frame::Strip, "Strip"), (Frame::Off, "Off")])
        });
    });
    section(ui, "Lens", true, None, |ui| {
        knob(ui, "Softness", "Blur on the film, µm", &mut s.lens_um, 0.0..=40.0, d.lens_um, 1);
        knob(ui, "Softness shape", "2 is a Gaussian; lower adds a long faint skirt", &mut s.lens_exponent, 0.6..=2.0, d.lens_exponent, 2);
        knob(ui, "Halation", "Glow around bright areas", &mut s.halation, 0.0..=0.2, d.halation, 3);
        knob(ui, "Halation width", "µm", &mut s.halation_um, 10.0..=200.0, d.halation_um, 0);
    });
    section(ui, "Grain", true, None, |ui| {
        knob(ui, "Amount", "Relative to the datasheet", &mut s.grain, 0.0..=3.0, d.grain, 2);
        knob(ui, "In lights", "Grain kept in light tones", &mut s.grain_light, 0.0..=1.0, d.grain_light, 2);
        knob(ui, "With development", "How grain grows with development", &mut s.grain_dev, 0.0..=2.5, d.grain_dev, 2);
    });
    let mut edges = !s.no_edge;
    section(ui, "Edge Effects", true, Some(&mut edges), |ui| {
        knob(ui, "Strength", "", &mut s.edge, 0.0..=3.0, d.edge, 2);
        knob(ui, "Density", "0: tone difference alone. 1: Nelson", &mut s.edge_density, 0.0..=1.0, d.edge_density, 2);
    });
    s.no_edge = !edges;
    section(ui, "Print", true, None, |ui| {
        knob(ui, "Grade", "Paper grade", &mut s.grade, -1.0..=5.0, d.grade, 2);
        knob(ui, "Exposure", "Stops; more is darker", &mut s.print_ev, -2.5..=2.5, d.print_ev, 2);
        knob(ui, "Grey density", "Print density of a metered grey", &mut s.print_grey, 0.3..=1.3, d.print_grey, 3);
        knob(ui, "Enlarger blur", "µm on the film", &mut s.enlarger_um, 0.0..=15.0, d.enlarger_um, 1);
        knob(ui, "House tones", "0: straight print. 1: the book's tone range", &mut s.house, 0.0..=1.0, d.house, 2);
    });
    section(ui, "Reproduction", true, None, |ui| {
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
                check(ui, "Page size auto", "From the file's size at the PPI below", &mut auto);
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
        section(ui, "Viewing", true, None, |ui| {
            let mut v = s.view;
            row(ui, "View", "", |ui| combo(ui, "view", &mut v, &[(None, "Auto"), (Some(View::Scan), "Scan"), (Some(View::White), "Ink on white"), (Some(View::Page), "Ink on cream"), (Some(View::Print), "Print")]));
            s.view = v;
            if s.view_mode() == View::Scan {
                knob(ui, "Scan black", "Display value of solid ink", &mut s.scan_black, 0.0..=0.3, d.scan_black, 3);
                knob(ui, "Scan white", "Page value sent to white", &mut s.scan_white, 0.7..=1.0, d.scan_white, 3);
            }
        });
    }
    section(ui, "Simulation", false, None, |ui| {
        let mut grid = s.detail_px as f64;
        knob(ui, "Grid", "Long edge of the simulation grid, px. 0 is the photo's own size", &mut grid, 0.0..=8192.0, d.detail_px as f64, 0);
        s.detail_px = ((grid / 32.0).round() * 32.0).clamp(0.0, 16384.0) as u32;
        row(ui, "Seed", "Grain pattern", |ui| {
            ui.add(DragValue::new(&mut s.seed).range(0..=u32::MAX));
        });
    });
}
