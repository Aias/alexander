//! Cropping one photo in Detail: the photo shown turned by its angle under a level frame
//! held at the middle of the view, which keeps an aspect unless set free and never leaves
//! the photo. Dragging inside the frame slides the photo under it, a pinch sizes the photo
//! under it, and dragging just outside a corner of the frame turns the photo under it.

use super::theme::{LABEL, SCRIM, SHADOW, STRONG};
use super::views::{from_px, px, uv};
use super::{controls, App, Mode};
use alexander_studio::engine::Canvas;
use alexander_studio::library::Id;
use alexander_studio::params::Crop;
use egui::emath::GuiRounding;
use egui::{pos2, vec2, Color32, CursorIcon, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2};

/// The whole photo, level.
const WHOLE: Crop = Crop { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0, angle: 0.0 };

/// Long side over short side for each aspect the frame can keep; None leaves it free.
const ASPECTS: [(&str, Option<f64>); 6] = [("Original", Some(0.0)), ("1:1", Some(1.0)), ("4:5", Some(1.25)), ("3:2", Some(1.5)), ("16:9", Some(16.0 / 9.0)), ("Free", None)];

/// The frame's handles, each by the sides it moves: -1 the left or top, 1 the right or bottom.
const HANDLES: [(i32, i32); 8] = [(-1, -1), (1, -1), (-1, 1), (1, 1), (0, -1), (0, 1), (-1, 0), (1, 0)];

#[derive(Clone)]
pub struct CropEdit {
    pub id: Id,
    crop: Crop,
    /// A place in `ASPECTS`.
    aspect: usize,
    /// The photo point at the middle of the view, in grid pixels, and the points a grid
    /// pixel spans there: held while a handle is dragged, then easing to the frame. None
    /// when the view follows the frame.
    view: Option<(Pos2, f32)>,
}

impl CropEdit {
    /// Turn the photo `degrees` further under the frame, shrinking the frame to stay on a
    /// `grid` photo.
    pub fn turn(&mut self, degrees: f64, grid: (u32, u32)) {
        self.crop = turned(self.crop, (self.crop.angle + degrees).clamp(-45.0, 45.0), grid.0 as f64, grid.1 as f64);
    }
}

/// The point of a `w` × `h` photo under the middle of the frame of `c`, in the photo's own
/// pixels, and half the width and height the frame spans there, the photo turned under it
/// as the canvas pass turns it.
fn placed(c: &Crop, w: f64, h: f64) -> ([f64; 2], [f64; 2]) {
    let (s, k) = c.angle.to_radians().sin_cos();
    let (dx, dy) = ((c.left + c.right - 1.0) * w / 2.0, (c.top + c.bottom - 1.0) * h / 2.0);
    let (hw, hh) = ((c.right - c.left) * w / 2.0, (c.bottom - c.top) * h / 2.0);
    ([w / 2.0 + k * dx + s * dy, h / 2.0 + k * dy - s * dx], [k.abs() * hw + s.abs() * hh, s.abs() * hw + k.abs() * hh])
}

/// The frame reaching `half` of the photo's width and height either side of its middle,
/// which lies over the point `p` of a `w` × `h` photo turned `angle` degrees under it.
fn over(p: [f64; 2], half: [f64; 2], angle: f64, w: f64, h: f64) -> Crop {
    let (s, k) = angle.to_radians().sin_cos();
    let (dx, dy) = (p[0] - w / 2.0, p[1] - h / 2.0);
    let (cx, cy) = (0.5 + (k * dx - s * dy) / w, 0.5 + (s * dx + k * dy) / h);
    Crop { left: cx - half[0], top: cy - half[1], right: cx + half[0], bottom: cy + half[1], angle }
}

/// Whether the frame of `c` lies inside a `w` × `h` photo turned under it.
fn fits(c: &Crop, w: f64, h: f64) -> bool {
    let (p, e) = placed(c, w, h);
    p[0] - e[0] >= -1e-6 && p[0] + e[0] <= w + 1e-6 && p[1] - e[1] >= -1e-6 && p[1] + e[1] <= h + 1e-6
}

/// `c` made to fit: shrunk about its centre, or about the photo's when its centre is outside.
fn fit(c: Crop, w: f64, h: f64) -> Crop {
    if fits(&c, w, h) {
        return c;
    }
    let (hw, hh) = ((c.right - c.left) / 2.0, (c.bottom - c.top) / 2.0);
    let at = |s: f64, cx: f64, cy: f64| Crop { left: cx - hw * s, right: cx + hw * s, top: cy - hh * s, bottom: cy + hh * s, angle: c.angle };
    let centre = ((c.left + c.right) / 2.0, (c.top + c.bottom) / 2.0);
    let (cx, cy) = if fits(&at(0.0, centre.0, centre.1), w, h) { centre } else { (0.5, 0.5) };
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..32 {
        let mid = (lo + hi) / 2.0;
        if fits(&at(mid, cx, cy), w, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    at(lo, cx, cy)
}

/// `c` put on the photo: its frame shrunk until the photo has room for it, then moved the
/// least that brings it inside.
fn onto(c: Crop, w: f64, h: f64) -> Crop {
    if fits(&c, w, h) {
        return c;
    }
    let (p, e) = placed(&c, w, h);
    let s = (w / 2.0 / e[0]).min(h / 2.0 / e[1]).min(1.0);
    let p = [p[0].max(s * e[0]).min(w - s * e[0]), p[1].max(s * e[1]).min(h - s * e[1])];
    over(p, [(c.right - c.left) / 2.0 * s, (c.bottom - c.top) / 2.0 * s], c.angle, w, h)
}

/// `c` with the photo turned to `angle` degrees about the point under the middle of the
/// frame, the frame shrunk to stay on the photo.
fn turned(c: Crop, angle: f64, w: f64, h: f64) -> Crop {
    fit(over(placed(&c, w, h).0, [(c.right - c.left) / 2.0, (c.bottom - c.top) / 2.0], angle, w, h), w, h)
}

/// `c` with the photo slid `by` of its pixels under the frame, as far as it covers the frame.
fn slid(c: Crop, by: Vec2, w: f64, h: f64) -> Crop {
    let (dx, dy) = (by.x as f64 / w, by.y as f64 / h);
    onto(Crop { left: c.left - dx, right: c.right - dx, top: c.top - dy, bottom: c.bottom - dy, ..c }, w, h)
}

impl App {
    /// The ratio of width to height the frame keeps, turned the way the frame is, if any.
    fn kept_ratio(&self, e: &CropEdit, w: f64, h: f64) -> Option<f64> {
        let long = ASPECTS[e.aspect].1?;
        let long = if long == 0.0 { w.max(h) / w.min(h) } else { long };
        let (fw, fh) = ((e.crop.right - e.crop.left) * w, (e.crop.bottom - e.crop.top) * h);
        Some(if fw >= fh { long } else { 1.0 / long })
    }

    pub(super) fn start_crop(&mut self) {
        let Some(item) = self.lib.focus.and_then(|f| self.lib.get(f)) else { return };
        self.cropping = Some(CropEdit { id: item.id, crop: item.crop().unwrap_or(WHOLE), aspect: 0, view: None });
        self.mode = Mode::Detail;
        self.zoom = None;
        self.panel = true;
    }

    /// Give the photo the crop drawn, as one step undo takes back.
    pub(super) fn finish_crop(&mut self) {
        let Some(e) = self.cropping.take() else { return };
        let Some(mut look) = self.lib.get(e.id).and_then(|it| it.look.clone()) else { return };
        look.crop = (e.crop != WHOLE).then_some(e.crop);
        self.lib.checkpoint(&[e.id]);
        self.lib.set_look(&[e.id], Some(look));
    }

    /// Slide photo `id` `by` grid pixels under its crop, the whole of a drag as one step
    /// undo takes back.
    pub(super) fn slide(&mut self, id: Id, by: Vec2) {
        let Some(item) = self.lib.get(id) else { return };
        let (Some(crop), Some(mut look)) = (item.crop(), item.look.clone()) else { return };
        let (w, h) = self.tiers.grid_size(item);
        let next = slid(crop, by, w as f64, h as f64);
        if next == crop {
            return;
        }
        look.crop = Some(next);
        if !self.edit_open {
            self.lib.checkpoint(&[id]);
            self.edit_open = true;
        }
        self.lib.set_look(&[id], Some(look));
    }

    /// The panel while cropping: the frame's aspect, the photo's angle and the way out.
    pub(super) fn crop_ui(&mut self, ui: &mut Ui) {
        let Some(mut e) = self.cropping.clone() else { return };
        let Some((w, h)) = self.lib.get(e.id).map(|it| self.tiers.grid_size(it)).map(|(w, h)| (w as f64, h as f64)) else { return };
        controls::title(ui, "Crop", Sense::hover());
        let options: Vec<(usize, &str)> = ASPECTS.iter().enumerate().map(|(i, a)| (i, a.0)).collect();
        let was = e.aspect;
        if controls::row(ui, "Aspect", "", e.aspect != 0, |ui| controls::combo(ui, "aspect", &mut e.aspect, &options)) {
            e.aspect = 0;
        }
        if e.aspect != was {
            if let Some(r) = self.kept_ratio(&e, w, h) {
                let (cx, cy, fw) = ((e.crop.left + e.crop.right) / 2.0, (e.crop.top + e.crop.bottom) / 2.0, (e.crop.right - e.crop.left) * w);
                let fh = fw / r / h;
                e.crop = fit(Crop { top: cy - fh / 2.0, bottom: cy + fh / 2.0, left: cx - fw / w / 2.0, right: cx + fw / w / 2.0, ..e.crop }, w, h);
            }
        }
        let mut angle = e.crop.angle;
        controls::knob(ui, "Straighten", "Degrees the photo turns clockwise under the frame", &mut angle, -45.0..=45.0, 0.0, 1);
        if angle != e.crop.angle {
            e.crop = turned(e.crop, angle, w, h);
        }
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button("Reset").clicked() {
                e.crop = WHOLE;
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Done").on_hover_text("Return").clicked() {
                    self.cropping = Some(e.clone());
                    self.finish_crop();
                } else if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    self.cropping = None;
                } else {
                    self.cropping = Some(e.clone());
                }
            });
        });
    }

    /// Detail while cropping: the frame with its handles at the middle of the view, as large
    /// as fits, and the photo turned under it.
    pub(super) fn crop_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(mut e) = self.cropping.clone() else { return };
        let Some(item) = self.lib.get(e.id) else { return };
        let ppp = ui.ctx().pixels_per_point();
        let (gw, gh) = self.tiers.grid_size(item);
        let (w, h) = (gw as f32, gh as f32);
        let (fw, fh) = (gw as f64, gh as f64);
        let area = rect.shrink(28.0);
        let span = vec2((e.crop.right - e.crop.left) as f32 * w, (e.crop.bottom - e.crop.top) as f32 * h);
        let rest = (pos2((e.crop.left + e.crop.right) as f32 * w / 2.0, (e.crop.top + e.crop.bottom) as f32 * h / 2.0), (area.width() / span.x).min(area.height() / span.y));
        let handle = |sx: i32, sy: i32| egui::Id::unique(("crop-handle", sx, sy));
        if HANDLES.iter().any(|&(sx, sy)| ui.ctx().is_being_dragged(handle(sx, sy))) {
            e.view.get_or_insert(rest);
        } else if let Some((mid, z)) = e.view {
            let k = 1.0 - (-ui.input(|i| i.stable_dt) / 0.06).exp();
            let settled = ((rest.0 - mid) * z).length() < 0.5 && (rest.1 / z - 1.0).abs() < 1e-3;
            e.view = (!settled).then_some((mid + (rest.0 - mid) * k, z + (rest.1 - z) * k));
            ui.ctx().request_repaint();
        }
        let (mid, z) = e.view.unwrap_or(rest);
        let view = px(rect, ppp);
        let shown = from_px(view, ppp);
        let corner = mid + (shown.min - area.center()) / z;
        let canvas = Canvas { origin: [corner.x, corner.y], scale: 1.0 / (z * ppp), source: Canvas::RENDER, clip: false, angle: e.crop.angle.to_radians() as f32 };
        if let Some(t) = self.tiers.canvas(item, canvas, [(view[2] - view[0]).max(1) as u32, (view[3] - view[1]).max(1) as u32], 0) {
            ui.painter().image(t, shown, uv(), Color32::WHITE);
        }
        let to_screen = |x: f64, y: f64| area.center() + (pos2(x as f32 * w, y as f32 * h) - mid) * z;
        let frame = Rect::from_min_max(to_screen(e.crop.left, e.crop.top), to_screen(e.crop.right, e.crop.bottom)).round_to_pixels(ppp);
        let p = ui.painter().with_clip_rect(rect);
        for r in [Rect::from_min_max(rect.min, pos2(rect.max.x, frame.min.y)), Rect::from_min_max(pos2(rect.min.x, frame.max.y), rect.max), Rect::from_min_max(pos2(rect.min.x, frame.min.y), pos2(frame.min.x, frame.max.y)), Rect::from_min_max(pos2(frame.max.x, frame.min.y), pos2(rect.max.x, frame.max.y))] {
            p.rect_filled(r, 0.0, SCRIM);
        }
        for i in 1..3 {
            let t = i as f32 / 3.0;
            p.vline(frame.left() + frame.width() * t, frame.y_range(), Stroke::new(1.0f32, Color32::from_white_alpha(70)));
            p.hline(frame.x_range(), frame.top() + frame.height() * t, Stroke::new(1.0f32, Color32::from_white_alpha(70)));
        }
        // a dark edge under the line keeps it a line on a light picture, where it would merge with the picture and leave the handles looking off it
        for (edge, colour) in [(2.0f32, SHADOW), (0.0, STRONG)] {
            p.rect_stroke(frame, 0.0, Stroke::new(1.0 + edge, colour), StrokeKind::Middle);
        }
        let ratio = self.kept_ratio(&e, fw, fh);
        let corners = [frame.left_top(), frame.right_top(), frame.left_bottom(), frame.right_bottom()];
        let near = |p: Pos2| !frame.contains(p) && corners.iter().any(|c| c.distance(p) < 36.0);
        let turn = ui.interact(rect, egui::Id::unique("crop-turn"), Sense::drag());
        let turning = egui::Id::unique("crop-turning");
        if turn.drag_started() {
            let at = ui.input(|i| i.pointer.press_origin()).is_some_and(near);
            ui.data_mut(|d| d.insert_temp(turning, at));
        }
        let turning = turn.dragged() && ui.data(|d| d.get_temp(turning)).unwrap_or(false);
        if let Some(at) = turn.hover_pos().filter(|&p| turning || near(p)) {
            turn_cursor(ui.ctx(), at, at - frame.center());
        }
        if let Some(at) = turn.interact_pointer_pos().filter(|_| turning) {
            let c = frame.center();
            let a = (at - c).angle() - (at - turn.drag_delta() - c).angle();
            let a = (a + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
            e.turn(a.to_degrees() as f64, (gw, gh));
            self.cropping = Some(e);
            return;
        }
        let pinch = ui.input(|i| i.zoom_delta()) as f64;
        if let Some(at) = ui.input(|i| i.pointer.hover_pos()).filter(|_| (pinch - 1.0).abs() > 1e-4 && ui.rect_contains_pointer(rect)) {
            let under = mid + (at - area.center()) / z;
            let (x, y) = ((under.x / w) as f64, (under.y / h) as f64);
            let next = Crop { left: x + (e.crop.left - x) / pinch, top: y + (e.crop.top - y) / pinch, right: x + (e.crop.right - x) / pinch, bottom: y + (e.crop.bottom - y) / pinch, ..e.crop };
            if next.right - next.left > 0.02 && next.bottom - next.top > 0.02 {
                e.crop = onto(next, fw, fh);
            }
        }
        let inside = ui.interact(frame, egui::Id::unique("crop-frame"), Sense::drag());
        if inside.hovered() || inside.dragged() {
            ui.ctx().set_cursor_icon(if inside.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab });
        }
        if inside.dragged() {
            e.crop = slid(e.crop, inside.drag_delta() / z, fw, fh);
        }
        let mut moved = None;
        for (sx, sy) in HANDLES {
            let at = pos2(frame.center().x + sx as f32 * frame.width() / 2.0, frame.center().y + sy as f32 * frame.height() / 2.0);
            let r = ui.interact(Rect::from_center_size(at, vec2(16.0, 16.0)), handle(sx, sy), Sense::drag());
            p.rect_filled(Rect::from_center_size(at, vec2(7.0, 7.0)).round_to_pixels(ppp), 0.0, if r.hovered() || r.dragged() { Color32::WHITE } else { LABEL });
            if r.hovered() || r.dragged() {
                ui.ctx().set_cursor_icon(match (sx, sy) {
                    (0, _) => CursorIcon::ResizeVertical,
                    (_, 0) => CursorIcon::ResizeHorizontal,
                    (a, b) if a == b => CursorIcon::ResizeNwSe,
                    _ => CursorIcon::ResizeNeSw,
                });
            }
            if r.dragged() {
                moved = Some((sx, sy, r.drag_delta()));
            }
        }
        let Some((sx, sy, d)) = moved else {
            self.cropping = Some(e);
            return;
        };
        let (dx, dy) = (d.x as f64 / z as f64, d.y as f64 / z as f64);
        let (mut x0, mut x1, mut y0, mut y1) = (e.crop.left * fw, e.crop.right * fw, e.crop.top * fh, e.crop.bottom * fh);
        match sx {
            -1 => x0 += dx,
            1 => x1 += dx,
            _ => {}
        }
        match sy {
            -1 => y0 += dy,
            1 => y1 += dy,
            _ => {}
        }
        if let Some(r) = ratio {
            if sx != 0 && sy != 0 {
                let height = (x1 - x0) / r;
                if sy < 0 {
                    y0 = y1 - height;
                } else {
                    y1 = y0 + height;
                }
            } else if sx != 0 {
                let (cy, height) = ((y0 + y1) / 2.0, (x1 - x0) / r);
                (y0, y1) = (cy - height / 2.0, cy + height / 2.0);
            } else {
                let (cx, width) = ((x0 + x1) / 2.0, (y1 - y0) * r);
                (x0, x1) = (cx - width / 2.0, cx + width / 2.0);
            }
        }
        let next = Crop { left: x0 / fw, right: x1 / fw, top: y0 / fh, bottom: y1 / fh, angle: e.crop.angle };
        if x1 - x0 > 0.02 * fw && y1 - y0 > 0.02 * fh && fits(&next, fw, fh) {
            e.crop = next;
        }
        self.cropping = Some(e);
    }
}

/// A curved arrow with a head at each end in place of the pointer at `at`, bowed toward
/// `out`, for turning the photo.
fn turn_cursor(ctx: &egui::Context, at: Pos2, out: Vec2) {
    ctx.set_cursor_icon(CursorIcon::None);
    let p = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("turn-cursor")));
    let (a0, c) = (out.angle(), at - out.normalized() * 7.0);
    let arc: Vec<Pos2> = (0..=10).map(|i| c + Vec2::angled(a0 - 0.8 + 0.16 * i as f32) * 9.0).collect();
    let heads = [(arc[0], -1.0), (arc[10], 1.0)].map(|(tip, turn)| {
        let along = Vec2::angled(a0 + turn * 0.8 + turn * std::f32::consts::FRAC_PI_2);
        let side = along.rot90() * 3.5;
        vec![tip + along * 4.0, tip - along * 1.5 + side, tip - along * 1.5 - side]
    });
    for (width, colour) in [(3.5f32, SCRIM), (1.5, Color32::WHITE)] {
        p.add(Shape::line(arc.clone(), Stroke::new(width, colour)));
    }
    for head in heads {
        p.add(Shape::convex_polygon(head, Color32::WHITE, Stroke::new(1.0f32, SCRIM)));
    }
}
