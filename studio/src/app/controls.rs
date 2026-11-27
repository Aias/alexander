//! The controls panel: the histogram, the process switch, the knobs in sections and the
//! look commands. It edits one `Settings`; the change is carried over to every selected photo.
//! Each knob of the look carries the switch that puts its setting on the test strip.

use super::help::help;
use super::theme::{label_family, title_font, CORNER, FILL_1, FILL_2, FILL_3, LABEL, PANEL, PLAIN, QUIET, STRONG, WELL};
use super::views::segmented;
use super::{App, Command, Mode};
use alexander_studio::film::{Dev, FilmKind, Filter};
use alexander_studio::params::{Frame, Repro, Settings, View};
use egui::collapsing_header::CollapsingState;
use egui::{pos2, vec2, Align, Align2, Button, Color32, ComboBox, CornerRadius, CursorIcon, DragValue, EventFilter, Key, Layout, Margin, Modifiers, Rect, RichText, Sense, Shape, Stroke, TextStyle, TextWrapMode, Ui, WidgetText};
use std::ops::RangeInclusive;

const LABEL_W: f32 = 116.0;
const VALUE_W: f32 = 44.0;
const HELP_W: f32 = 250.0;
const HANDLE_R: f32 = 5.0;
const EDGE: Margin = Margin { left: 0, right: 12, top: 0, bottom: 0 };

impl App {
    pub(super) fn panel_ui(&mut self, ui: &mut Ui, actions: &mut Vec<Command>) {
        let focus = self.lib.focus.filter(|_| !self.lib.selected().is_empty()).and_then(|f| self.lib.get(f));
        let before = self.before();
        egui::Frame::NONE.inner_margin(EDGE).show(ui, |ui| histogram(ui, focus.and_then(|it| self.tiers.histogram(it, before)), &mut self.clipping));
        if self.cropping.is_some() {
            ui.add_space(10.0);
            egui::Frame::NONE.inner_margin(EDGE).show(ui, |ui| self.crop_ui(ui));
            return;
        }
        let processed = focus.is_some_and(|it| it.look.is_some());
        let bordered = focus.is_some_and(|it| it.frame.is_some());
        let mut s = focus.map(|it| it.settings()).unwrap_or_default();
        let enabled = focus.is_some();
        let can_paste = self.clipboard.is_some();
        ui.add_space(10.0);
        egui::Frame::NONE.inner_margin(EDGE).show(ui, |ui| ui.add_enabled_ui(enabled, |ui| header(ui, processed, actions)));
        ui.add_space(6.0);
        egui::Panel::bottom("look").frame(egui::Frame::NONE.inner_margin(Margin { top: 8, ..EDGE })).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| footer(ui, processed, can_paste, actions));
        });
        let before = s.clone();
        let tested = self.test_strip.len();
        egui::ScrollArea::vertical().auto_shrink([false, false]).content_margin(EDGE).show(ui, |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                self.presets_ui(ui);
                knobs(ui, &mut s, bordered, &mut self.test_strip);
            });
        });
        if self.test_strip.len() > tested && self.mode != Mode::Detail {
            actions.push(Command::View(Mode::Detail));
        }
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
/// of the clipped ends fills the box. A triangle in each top corner lights when shadows or
/// highlights clip, and a click on either switches the marking of clipped values.
fn histogram(ui: &mut Ui, bins: Option<&[f32]>, clipping: &mut bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 88.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CORNER, WELL);
    for i in 1..4 {
        p.vline(rect.left() + rect.width() * i as f32 / 4.0, rect.y_range(), Stroke::new(1.0f32, PANEL));
    }
    let Some(bins) = bins else { return };
    let top = bins[1..bins.len() - 1].iter().copied().fold(1.0, f32::max);
    let w = rect.width() / bins.len() as f32;
    let inner = rect.shrink2(vec2(0.0, 4.0));
    for (i, &c) in bins.iter().enumerate() {
        let h = (c / top).min(1.0) * inner.height();
        let x = rect.left() + i as f32 * w;
        p.rect_filled(Rect::from_min_max(pos2(x, inner.bottom() - h), pos2(x + w, inner.bottom())), CornerRadius::ZERO, LABEL);
    }
    let total: f32 = bins.iter().sum();
    for (corner, count, toward) in [(rect.left_top() + vec2(10.0, 10.0), bins[0], -1.0), (rect.right_top() + vec2(-10.0, 10.0), bins[bins.len() - 1], 1.0)] {
        let r = ui.interact(Rect::from_center_size(corner, vec2(16.0, 16.0)), ui.scope_id().with(("clip", corner.x as i32)), Sense::click()).on_hover_text("J");
        if r.clicked() {
            *clipping = !*clipping;
        }
        let lit = count > total * 0.001;
        let fill = if lit { Color32::WHITE } else { FILL_3 };
        let pts = [corner + vec2(-4.0 * toward, -4.0), corner + vec2(4.0 * toward, -4.0), corner + vec2(4.0 * toward, 4.0)];
        ui.painter().add(Shape::convex_polygon(pts.to_vec(), fill, Stroke::NONE));
        if *clipping {
            ui.painter().rect_stroke(Rect::from_center_size(corner, vec2(14.0, 14.0)), CORNER, Stroke::new(1.0f32, PLAIN), egui::StrokeKind::Inside);
        }
    }
}

/// The process switch and the commands for looks as files.
fn header(ui: &mut Ui, processed: bool, actions: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        let mut on = processed;
        ui.add_sized([ui.available_width() - 34.0, 26.0], |ui: &mut Ui| segmented(ui, &mut on, &[(false, "Original", "V"), (true, "Film", "V")]));
        if on != processed {
            actions.push(Command::Process(Some(on)));
        }
        let (r, _) = egui::containers::menu::MenuButton::from_button(Button::new("").min_size(vec2(26.0, 26.0))).ui(ui, |ui| {
            ui.set_min_width(200.0);
            let mut item = |ui: &mut Ui, text: &str, enabled: bool, a: Command| {
                if ui.add_enabled(enabled, Button::new(text)).clicked() {
                    actions.push(a);
                    ui.close();
                }
            };
            item(ui, "Save Look…", processed, Command::SaveLook);
            item(ui, "Load Look…", true, Command::LoadLook);
            item(ui, "Copy as twb2.py Flags", processed, Command::CopyFlags);
        });
        let colour = ui.style().interact(&r).fg_stroke.color;
        for dx in [-5.0, 0.0, 5.0] {
            ui.painter().circle_filled(r.rect.center() + vec2(dx, 0.0), 1.4, colour);
        }
    });
}

fn footer(ui: &mut Ui, processed: bool, can_paste: bool, actions: &mut Vec<Command>) {
    ui.horizontal(|ui| {
        let mut button = |ui: &mut Ui, text: &str, keys: &str, enabled: bool, a: Command| {
            if ui.add_enabled(enabled, Button::new(text).min_size(vec2(64.0, 24.0))).on_hover_text(keys).clicked() {
                actions.push(a);
            }
        };
        button(ui, "Copy", "⇧⌘C", processed, Command::CopyLook);
        button(ui, "Paste", "⇧⌘V", can_paste, Command::PasteLook);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| button(ui, "Reset", "⇧⌘R", processed, Command::ResetLook));
    });
}

/// How far along a slider over `range` the value `v` lies, from 0 to 1. With `log` the
/// slider spaces its values by ratio.
pub(super) fn fraction(v: f64, range: &RangeInclusive<f64>, log: bool) -> f64 {
    let (lo, hi) = (*range.start(), *range.end());
    (if log { (v / lo).ln() / (hi / lo).ln() } else { (v - lo) / (hi - lo) }).clamp(0.0, 1.0)
}

/// The value `t` of the way along that slider.
pub(super) fn value_at(t: f64, range: &RangeInclusive<f64>, log: bool) -> f64 {
    let (lo, hi) = (*range.start(), *range.end());
    if log { lo * (hi / lo).powf(t) } else { lo + (hi - lo) * t }
}

/// A slider without its value: a thin rail and a small handle. Dragging the handle moves
/// it from where it was taken; pressing the rail elsewhere puts it there.
pub fn slider(ui: &mut Ui, v: &mut f64, range: RangeInclusive<f64>, width: f32, log: bool) -> egui::Response {
    let t_of = |v: f64| fraction(v, &range, log);
    let v_of = |t: f64| value_at(t, &range, log);
    let (rect, mut r) = ui.allocate_exact_size(vec2(width.max(40.0), ui.spacing().interact_size.y), Sense::click_and_drag());
    let rail = rect.x_range().shrink(HANDLE_R + 1.0);
    let x_of = |v: f64| rail.min + rail.span() * t_of(v) as f32;
    if let Some(p) = r.interact_pointer_pos().filter(|_| r.is_pointer_button_down_on()) {
        let grip = ui.data_mut(|d| {
            *d.get_temp_mut_or_insert_with(r.id, || {
                let dx = x_of(*v) - p.x;
                if dx.abs() <= HANDLE_R + 3.0 { dx } else { 0.0 }
            })
        });
        let nv = v_of(((p.x + grip - rail.min) / rail.span()).clamp(0.0, 1.0) as f64);
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
            *v = v_of((t_of(*v) + steps / 100.0).clamp(0.0, 1.0));
            r.mark_changed();
        }
    }
    let c = pos2(x_of(*v), rect.center().y);
    let p = ui.painter();
    p.hline(rail, c.y, Stroke::new(2.0f32, FILL_2));
    let lit = r.hovered() || r.dragged() || r.has_focus();
    p.circle_filled(c, HANDLE_R + if lit { 1.0 } else { 0.0 }, if lit { Color32::WHITE } else { PLAIN });
    if r.has_focus() {
        p.circle_stroke(c, HANDLE_R + 3.5, Stroke::new(1.0f32, PLAIN));
    }
    r
}

/// A setting's name. When the setting is off its default the name reads as a link under
/// the pointer, and a click on it asks for the default back: the return value.
fn name(ui: &mut Ui, label: &str, tip: &str, changed: bool) -> bool {
    let (rect, r) = ui.allocate_exact_size(vec2(LABEL_W, ui.spacing().interact_size.y), if changed { Sense::click() } else { Sense::hover() });
    let help = help(label).filter(|_| r.contains_pointer() && ui.input(|i| i.modifiers.alt));
    let link = changed && r.hovered() && help.is_none();
    if link {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    let text = RichText::new(label).family(label_family()).color(if link { STRONG } else { LABEL });
    let galley = WidgetText::from(if link { text.underline() } else { text }).into_galley(ui, Some(TextWrapMode::Truncate), rect.width(), TextStyle::Body);
    ui.painter().galley(pos2(rect.left(), rect.center().y - galley.size().y / 2.0), galley, LABEL);
    let r = match help {
        Some(text) => {
            help_card(ui, label, text, rect);
            r
        }
        None if tip.is_empty() => r,
        None => r.on_hover_text(tip),
    };
    changed && r.clicked()
}

fn help_card(ui: &Ui, label: &str, text: &str, at: Rect) {
    ui.ctx().set_cursor_icon(CursorIcon::Help);
    egui::Area::new(egui::Id::unique("setting help"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::RIGHT_TOP)
        .fixed_pos(pos2(ui.clip_rect().left() - 8.0, at.top() - 4.0))
        .interactable(false)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).inner_margin(8).show(ui, |ui| {
                ui.set_width(HELP_W);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new(label).font(title_font()).color(STRONG));
                ui.label(text);
            });
        });
}

/// A labelled slider and its value. Clicking the name or double-clicking the slider
/// restores the default; the value can be dragged or typed, past the slider's ends too.
pub(super) fn knob(ui: &mut Ui, label: &str, tip: &str, v: &mut f64, range: RangeInclusive<f64>, default: f64, decimals: usize) {
    ui.horizontal(|ui| {
        if name(ui, label, tip, *v != default) {
            *v = default;
        }
        dial(ui, v, range, default, decimals, false);
    });
}
fn log_knob(ui: &mut Ui, label: &str, tip: &str, v: &mut f64, range: RangeInclusive<f64>, default: f64, decimals: usize) {
    ui.horizontal(|ui| {
        if name(ui, label, tip, *v != default) {
            *v = default;
        }
        dial(ui, v, range, default, decimals, true);
    });
}
fn dial(ui: &mut Ui, v: &mut f64, range: RangeInclusive<f64>, default: f64, decimals: usize, log: bool) {
    let speed = (range.end() - range.start()) / 300.0;
    let w = ui.available_width() - VALUE_W - ui.spacing().item_spacing.x;
    let r = slider(ui, v, range, w, log);
    ui.visuals_mut().widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
    ui.visuals_mut().widgets.inactive.fg_stroke.color = if *v == default { QUIET } else { STRONG };
    ui.spacing_mut().button_padding.x = 2.0;
    ui.add_sized([VALUE_W, ui.spacing().interact_size.y], DragValue::new(v).speed(speed).custom_formatter(|v, _| format!("{v:.decimals$}")));
    if r.double_clicked() {
        *v = default;
    }
}

/// A knob of the look: a setting's name and help, its place in the settings, the ends of its
/// slider, whether the slider spaces its values by ratio, and the decimals its values show.
#[derive(Clone)]
pub(super) struct Knob {
    pub label: &'static str,
    tip: &'static str,
    pub at: fn(&mut Settings) -> &mut f64,
    pub range: RangeInclusive<f64>,
    pub log: bool,
    pub decimals: usize,
}

impl Knob {
    pub(super) fn new(label: &'static str, tip: &'static str, at: fn(&mut Settings) -> &mut f64, range: RangeInclusive<f64>, decimals: usize) -> Knob {
        Knob { label, tip, at, range, log: false, decimals }
    }

    fn log(self) -> Knob {
        Knob { log: true, ..self }
    }

    /// The knob's row in the panel, as `knob` lays one out, with the test strip's switch.
    fn show(self, ui: &mut Ui, s: &mut Settings, strip: &mut Vec<Knob>) {
        let default = *(self.at)(&mut Settings::default());
        let row = ui.horizontal(|ui| {
            let v = (self.at)(s);
            if name(ui, self.label, self.tip, *v != default) {
                *v = default;
            }
            dial(ui, v, self.range.clone(), default, self.decimals, self.log);
        });
        strip_switch(ui, row.response.rect, strip, self);
    }
}

/// The switch at the end of a knob's name that puts its setting on the test strip or takes
/// it off, as T does for the row under the pointer. It shows while the pointer is on the
/// row or the setting is on the strip: three marks in a row for the setting that steps
/// across the strip, in a column for the one that steps down it. The first setting chosen
/// steps across, and each one chosen after it takes the place of the one stepping down.
fn strip_switch(ui: &mut Ui, row: Rect, strip: &mut Vec<Knob>, knob: Knob) {
    let place = strip.iter().position(|k| k.label == knob.label);
    let over = ui.is_enabled() && ui.rect_contains_pointer(row);
    if place.is_none() && !over {
        return;
    }
    let rect = Rect::from_center_size(pos2(row.left() + LABEL_W - 7.0, row.center().y), vec2(14.0, 14.0));
    let r = ui.interact(rect, egui::Id::unique(("test strip", knob.label)), Sense::click()).on_hover_text("T");
    let down = place.unwrap_or(strip.len().min(1)) == 1;
    let colour = if r.hovered() { Color32::WHITE } else if place.is_some() { STRONG } else { LABEL };
    for step in [-4.0, 0.0, 4.0] {
        let mark = rect.center() + if down { vec2(0.0, step) } else { vec2(step, 0.0) };
        ui.painter().rect_filled(Rect::from_center_size(mark, vec2(2.5, 2.5)), CornerRadius::ZERO, colour);
    }
    let keyed = over && !ui.ctx().egui_wants_keyboard_input() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::T));
    if r.clicked() || keyed {
        match place {
            Some(i) => {
                strip.remove(i);
            }
            None => {
                strip.truncate(1);
                strip.push(knob);
            }
        }
    }
}

/// A named row of controls; true when the name was clicked to restore the default.
pub(super) fn row(ui: &mut Ui, label: &str, tip: &str, changed: bool, add: impl FnOnce(&mut Ui)) -> bool {
    ui.horizontal(|ui| {
        let reset = name(ui, label, tip, changed);
        add(ui);
        reset
    })
    .inner
}

/// A setting of a few values as a row of toggles, each with its longer name, if any, as a tooltip.
fn toggles<T: PartialEq + Copy>(ui: &mut Ui, label: &str, tip: &str, v: &mut T, default: T, options: &[(T, &str, &str)]) {
    if row(ui, label, tip, *v != default, |ui| {
        ui.style_mut().override_text_style = Some(TextStyle::Small);
        ui.add_sized([ui.available_width(), ui.spacing().interact_size.y + 2.0], |ui: &mut Ui| segmented(ui, v, options));
    }) {
        *v = default;
    }
}

pub(super) fn combo<T: PartialEq + Copy>(ui: &mut Ui, id: &str, v: &mut T, options: &[(T, &str)]) {
    let current = options.iter().find(|o| o.0 == *v).map_or("", |o| o.1);
    ComboBox::from_id_salt(id).width(ui.available_width()).selected_text(current).show_ui(ui, |ui| {
        for (o, name) in options {
            ui.selectable_value(v, *o, *name);
        }
    });
}

fn check(ui: &mut Ui, label: &str, tip: &str, v: &mut bool, default: bool) {
    if row(ui, label, tip, *v != default, |ui| {
        ui.add(egui::Checkbox::without_text(v));
    }) {
        *v = default;
    }
}

/// A heading over a rule across the panel, brighter under the pointer when it can be clicked.
pub(super) fn title(ui: &mut Ui, name: &str, sense: Sense) -> (Rect, egui::Response) {
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 29.0), sense);
    ui.painter().hline(rect.x_range(), rect.top(), Stroke::new(1.0f32, FILL_1));
    let rect = rect.translate(vec2(0.0, ui.spacing().item_spacing.y / 2.0));
    ui.painter().text(rect.left_center(), Align2::LEFT_CENTER, name, title_font(), if r.hovered() && sense.senses_click() { Color32::WHITE } else { STRONG });
    (rect, r)
}

/// A section of knobs under a heading that opens and closes it. With `switch` the heading
/// carries a checkbox that turns the section's effect on and off.
pub(super) fn section(ui: &mut Ui, name: &str, open: bool, switch: Option<&mut bool>, add: impl FnOnce(&mut Ui)) {
    let mut state = CollapsingState::load_with_default_open(ui.ctx(), ui.make_persistent_id(name), open);
    let (rect, r) = title(ui, name, Sense::click());
    if r.clicked() {
        state.toggle(ui);
    }
    let p = ui.painter();
    let turn = egui::emath::Rot2::from_angle((state.openness(ui.ctx()) - 1.0) * std::f32::consts::FRAC_PI_2);
    let tip = pos2(rect.right() - 6.0, rect.center().y);
    p.add(Shape::convex_polygon([vec2(-3.5, -2.0), vec2(3.5, -2.0), vec2(0.0, 2.5)].map(|d| tip + turn * d).to_vec(), LABEL, Stroke::NONE));
    let on = switch.is_none_or(|on| {
        ui.put(Rect::from_center_size(pos2(rect.right() - 30.0, rect.center().y), vec2(18.0, 18.0)), egui::Checkbox::without_text(on));
        *on
    });
    state.show_body_unindented(ui, |ui| {
        ui.add_enabled_ui(on, add);
        ui.add_space(12.0);
    });
}

fn knobs(ui: &mut Ui, s: &mut Settings, bordered: bool, strip: &mut Vec<Knob>) {
    let d = Settings::default();
    section(ui, "Film", true, None, |ui| {
        let stock = |k: FilmKind| match k {
            FilmKind::Trix1956 => "Tri-X ’56",
            FilmKind::Trix => "Tri-X",
            FilmKind::DoubleX => "Double-X",
        };
        toggles(ui, "Stock", "", &mut s.film, d.film, &FilmKind::ALL.map(|k| (k, stock(k), k.label())));
        toggles(ui, "Filter", "", &mut s.filter, d.filter, &[(Filter::None, "None", ""), (Filter::Yellow, "Yellow", ""), (Filter::Orange, "Orange", ""), (Filter::Red, "Red", "")]);
        Knob::new("Size", "Long edge of film the picture fills, mm. 36 is a full 35mm frame", |s| &mut s.film_mm, 8.0..=72.0, 1).log().show(ui, s, strip);
        let mut grid = s.detail_px as f64;
        knob(ui, "Grid", "Long edge of the simulation grid, px. With Size, it sets how large the grain is in the frame. 0 is the photo's own size", &mut grid, 0.0..=8192.0, d.detail_px as f64, 0);
        s.detail_px = ((grid / 32.0).round() * 32.0).clamp(0.0, 16384.0) as u32;
        let mut kind = match s.dev {
            None => 0,
            Some(Dev::Minutes(_)) => 1,
            Some(Dev::Norm1952) => 2,
            Some(Dev::Norm1956) => 3,
            Some(Dev::Norm1981) => 4,
        };
        let was = kind;
        if row(ui, "Development", "", s.dev != d.dev, |ui| combo(ui, "dev", &mut kind, &[(0, "Datasheet"), (1, "Minutes"), (2, "1952 norm"), (3, "1956 norm"), (4, "1981 norm")])) {
            s.dev = d.dev;
        } else if kind != was {
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
            let range = s.film.dev_range();
            *m = m.clamp(*range.start(), *range.end());
            knob(ui, "Minutes", "Longer is more contrast", m, range, def, 1);
        }
    });
    section(ui, "Exposure", true, None, |ui| {
        let auto = Knob::new("Auto", "Display value the median is placed at", |s| &mut s.auto_ev_target, 0.2..=0.7, 2);
        let row = ui.horizontal(|ui| {
            if name(ui, auto.label, auto.tip, s.auto_ev != d.auto_ev || s.auto_ev_target != d.auto_ev_target) {
                (s.auto_ev, s.auto_ev_target) = (d.auto_ev, d.auto_ev_target);
            }
            ui.add(egui::Checkbox::without_text(&mut s.auto_ev));
            ui.add_enabled_ui(s.auto_ev, |ui| dial(ui, &mut s.auto_ev_target, auto.range.clone(), d.auto_ev_target, auto.decimals, false));
        });
        strip_switch(ui, row.response.rect, strip, auto);
        Knob::new("Compensation", "Stops", |s| &mut s.ev, -3.0..=3.0, 2).show(ui, s, strip);
        if bordered {
            let border = "A uniform border round the picture, such as a print's paper or letterbox bars. Keep: exposure and tones are measured inside it. Strip: the same, and only the picture inside is shown and exported. Off: not looked for";
            toggles(ui, "Border", border, &mut s.frame, d.frame, &[(Frame::Keep, "Keep", ""), (Frame::Strip, "Strip", ""), (Frame::Off, "Off", "")]);
        }
    });
    section(ui, "Lens", true, None, |ui| {
        Knob::new("Softness", "Blur on the film, µm", |s| &mut s.lens_um, 0.0..=40.0, 1).show(ui, s, strip);
        Knob::new("Falloff", "How the softness falls off: 2 is a Gaussian, lower adds a long faint skirt", |s| &mut s.lens_exponent, 0.6..=2.0, 2).show(ui, s, strip);
        Knob::new("Halation", "Glow around bright areas", |s| &mut s.halation, 0.0..=0.2, 2).show(ui, s, strip);
        Knob::new("Radius", "How far the halation spreads, µm", |s| &mut s.halation_um, 10.0..=200.0, 0).show(ui, s, strip);
    });
    section(ui, "Grain", true, None, |ui| {
        Knob::new("Amount", "Relative to the datasheet", |s| &mut s.grain, 0.0..=3.0, 2).show(ui, s, strip);
        Knob::new("In lights", "Grain kept in light tones", |s| &mut s.grain_light, 0.0..=1.0, 2).show(ui, s, strip);
        Knob::new("Growth", "How grain grows with development", |s| &mut s.grain_dev, 0.0..=2.5, 2).show(ui, s, strip);
    });
    let mut edges = !s.no_edge;
    section(ui, "Edge Effects", true, Some(&mut edges), |ui| {
        Knob::new("Strength", "", |s| &mut s.edge, 0.0..=3.0, 2).show(ui, s, strip);
        Knob::new("Density", "0: tone difference alone. 1: Nelson", |s| &mut s.edge_density, 0.0..=1.0, 2).show(ui, s, strip);
    });
    s.no_edge = !edges;
    section(ui, "Print", true, None, |ui| {
        Knob::new("Grade", "Paper grade", |s| &mut s.grade, -1.0..=5.0, 2).show(ui, s, strip);
        Knob::new("Exposure", "Stops; more is darker", |s| &mut s.print_ev, -2.5..=2.5, 2).show(ui, s, strip);
        Knob::new("Grey density", "Print density of a metered grey", |s| &mut s.print_grey, 0.3..=1.3, 2).show(ui, s, strip);
        Knob::new("Enlarger blur", "µm on the film", |s| &mut s.enlarger_um, 0.0..=15.0, 1).show(ui, s, strip);
        Knob::new("House tones", "0: straight print. 1: the book's tone range", |s| &mut s.house, 0.0..=1.0, 2).show(ui, s, strip);
    });
    section(ui, "Reproduction", true, None, |ui| {
        toggles(ui, "Finish", "", &mut s.repro, d.repro, &[(Repro::None, "Print", ""), (Repro::Copy, "Copy", "Copied from a page"), (Repro::Book, "Book", "Book page")]);
        match s.repro {
            Repro::None => {}
            Repro::Copy => {
                Knob::new("Copy blur", "Thousandths of the long edge", |s| &mut s.copy_blur, 0.0..=2.0, 2).show(ui, s, strip);
                Knob::new("Copy contrast", "", |s| &mut s.copy_gain, 0.5..=2.0, 2).show(ui, s, strip);
                Knob::new("Highlight drop", "Highlight dot lost at each printing", |s| &mut s.copy_drop, 0.0..=0.3, 2).show(ui, s, strip);
                Knob::new("Shadow fill", "Shadow tone that closes to solid ink", |s| &mut s.copy_fill, 0.7..=1.0, 2).show(ui, s, strip);
                Knob::new("Source ink", "Solid ink density on the source page", |s| &mut s.copy_ink, 0.8..=2.5, 2).show(ui, s, strip);
            }
            Repro::Book => {
                let mut auto = s.page_mm.is_none();
                check(ui, "Auto size", "Page size from the file's size at the PPI below", &mut auto, d.page_mm.is_none());
                if auto {
                    s.page_mm = None;
                    Knob::new("PPI", "", |s| &mut s.ppi, 72.0..=600.0, 0).show(ui, s, strip);
                } else {
                    let mut mm = s.page_mm.unwrap_or(200.0);
                    log_knob(ui, "Page size", "Long edge on the page, mm", &mut mm, 30.0..=400.0, 200.0, 0);
                    s.page_mm = Some(mm);
                }
                Knob::new("Screen", "Lines per inch", |s| &mut s.lpi, 65.0..=200.0, 0).show(ui, s, strip);
                Knob::new("Ink density", "1.20 uncoated, 1.85 coated", |s| &mut s.ink_density, 0.8..=2.2, 2).show(ui, s, strip);
                Knob::new("Dot gain", "", |s| &mut s.dot_gain, 0.0..=0.3, 2).show(ui, s, strip);
                Knob::new("Smallest dot", "", |s| &mut s.min_dot, 0.0..=0.1, 2).show(ui, s, strip);
                Knob::new("Camera blur", "Copy camera blur on the page, mm", |s| &mut s.copy_blur_mm, 0.0..=0.2, 2).show(ui, s, strip);
                Knob::new("Paper scatter", "Light spread in the paper, mm", |s| &mut s.paper_scatter_mm, 0.0..=0.2, 2).show(ui, s, strip);
                let mut v = s.view;
                if row(ui, "View", "", v != d.view, |ui| combo(ui, "view", &mut v, &[(None, "Auto"), (Some(View::Scan), "Scan"), (Some(View::White), "Ink on white"), (Some(View::Page), "Ink on cream"), (Some(View::Print), "Print")])) {
                    v = d.view;
                }
                s.view = v;
                if s.view_mode() == View::Scan {
                    Knob::new("Scan black", "Display value of solid ink", |s| &mut s.scan_black, 0.0..=0.3, 2).show(ui, s, strip);
                    Knob::new("Scan white", "Page value sent to white", |s| &mut s.scan_white, 0.7..=1.0, 2).show(ui, s, strip);
                }
            }
        }
    });
}
