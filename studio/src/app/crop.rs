//! Cropping one photo in the loupe: the whole photo shown turned by its angle under a level
//! frame, which keeps an aspect unless set free and never leaves the photo.

use super::views::{LABEL, TEXT};
use super::{controls, App, Mode};
use alexander_studio::engine::Canvas;
use alexander_studio::library::Id;
use alexander_studio::params::Crop;
use egui::{pos2, vec2, Button, Color32, CursorIcon, Rect, Sense, Stroke, StrokeKind, Ui};

/// The whole photo, level.
const WHOLE: Crop = Crop { left: 0.0, top: 0.0, right: 1.0, bottom: 1.0, angle: 0.0 };

/// Long side over short side for each aspect the frame can keep; None leaves it free.
const ASPECTS: [(&str, Option<f64>); 6] = [("Original", Some(0.0)), ("1:1", Some(1.0)), ("4:5", Some(1.25)), ("3:2", Some(1.5)), ("16:9", Some(16.0 / 9.0)), ("Free", None)];

#[derive(Clone)]
pub struct CropEdit {
    pub id: Id,
    crop: Crop,
    /// A place in `ASPECTS`.
    aspect: usize,
}

/// Whether the frame of `c` lies inside a `w` × `h` photo turned under it, as the canvas
/// pass turns it.
fn fits(c: &Crop, w: f64, h: f64) -> bool {
    let (s, k) = c.angle.to_radians().sin_cos();
    let (mx, my) = (w / 2.0, h / 2.0);
    [(c.left, c.top), (c.right, c.top), (c.left, c.bottom), (c.right, c.bottom)].iter().all(|&(fx, fy)| {
        let (dx, dy) = (fx * w - mx, fy * h - my);
        let (px, py) = (mx + k * dx + s * dy, my + k * dy - s * dx);
        (-1e-6..=w + 1e-6).contains(&px) && (-1e-6..=h + 1e-6).contains(&py)
    })
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
        self.cropping = Some(CropEdit { id: item.id, crop: item.crop().unwrap_or(WHOLE), aspect: 0 });
        self.mode = Mode::Loupe;
        self.zoom = None;
    }

    /// Give the photo the crop drawn, as one step undo takes back.
    pub(super) fn finish_crop(&mut self) {
        let Some(e) = self.cropping.take() else { return };
        let Some(mut look) = self.lib.get(e.id).and_then(|it| it.look.clone()) else { return };
        look.crop = (e.crop != WHOLE).then_some(e.crop);
        self.lib.checkpoint(&[e.id]);
        self.lib.set_look(&[e.id], Some(look));
    }

    /// The crop section of the panel: the button that starts cropping, or while cropping the
    /// frame's aspect, the photo's angle and the way out.
    pub(super) fn crop_ui(&mut self, ui: &mut Ui) {
        controls::section(ui, "Crop", true, None, |ui| {
            let can = self.can(&super::Command::Crop);
            let Some(mut e) = self.cropping.clone() else {
                let cropped = self.lib.focus.and_then(|f| self.lib.get(f)).and_then(|it| it.look.as_ref()).is_some_and(|l| l.crop.is_some());
                ui.horizontal(|ui| {
                    if ui.add_enabled(can, Button::new("Crop")).on_hover_text("R").clicked() {
                        self.start_crop();
                    }
                    if let (true, Some(id)) = (cropped && ui.add_enabled(can, Button::new("Clear")).clicked(), self.lib.focus) {
                        self.cropping = Some(CropEdit { id, crop: WHOLE, aspect: 0 });
                        self.finish_crop();
                    }
                });
                return;
            };
            let Some((w, h)) = self.lib.get(e.id).map(|it| self.tiers.grid_size(it)).map(|(w, h)| (w as f64, h as f64)) else { return };
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
            let before = e.crop.angle;
            controls::knob(ui, "Straighten", "Degrees the photo turns clockwise under the frame", &mut e.crop.angle, -45.0..=45.0, 0.0, 1);
            if e.crop.angle != before {
                e.crop = fit(e.crop, w, h);
            }
            ui.add_space(4.0);
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
        });
    }

    /// The loupe while cropping: the whole photo turned under the frame, and the frame with
    /// its handles.
    pub(super) fn crop_view(&mut self, ui: &mut Ui, rect: Rect) {
        let Some(mut e) = self.cropping.clone() else { return };
        let Some(item) = self.lib.get(e.id) else { return };
        let ppp = ui.ctx().pixels_per_point();
        let (gw, gh) = self.tiers.grid_size(item);
        let (w, h) = (gw as f32, gh as f32);
        let (s, k) = (e.crop.angle.to_radians() as f32).sin_cos();
        let (bw, bh) = (w * k.abs() + h * s.abs(), w * s.abs() + h * k.abs());
        let origin = [(w - bw) / 2.0, (h - bh) / 2.0];
        let area = rect.shrink(28.0);
        let z = (area.width() / bw).min(area.height() / bh);
        let img = Rect::from_center_size(area.center(), vec2(bw * z, bh * z));
        let size = [(img.width() * ppp).round().max(1.0) as u32, (img.height() * ppp).round().max(1.0) as u32];
        let canvas = Canvas { origin, scale: bw / size[0] as f32, source: Canvas::RENDER, clip: false, angle: e.crop.angle.to_radians() as f32 };
        if let Some(t) = self.tiers.canvas(item, canvas, size, 0) {
            ui.painter().image(t, img, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        let to_screen = |x: f64, y: f64| pos2(img.min.x + (x as f32 * w - origin[0]) * z, img.min.y + (y as f32 * h - origin[1]) * z);
        let frame = Rect::from_min_max(to_screen(e.crop.left, e.crop.top), to_screen(e.crop.right, e.crop.bottom));
        let p = ui.painter().with_clip_rect(rect);
        let shade = Color32::from_black_alpha(150);
        for r in [Rect::from_min_max(rect.min, pos2(rect.max.x, frame.min.y)), Rect::from_min_max(pos2(rect.min.x, frame.max.y), rect.max), Rect::from_min_max(pos2(rect.min.x, frame.min.y), pos2(frame.min.x, frame.max.y)), Rect::from_min_max(pos2(frame.max.x, frame.min.y), pos2(rect.max.x, frame.max.y))] {
            p.rect_filled(r, 0.0, shade);
        }
        for i in 1..3 {
            let t = i as f32 / 3.0;
            p.vline(frame.left() + frame.width() * t, frame.y_range(), Stroke::new(1.0f32, Color32::from_white_alpha(70)));
            p.hline(frame.x_range(), frame.top() + frame.height() * t, Stroke::new(1.0f32, Color32::from_white_alpha(70)));
        }
        p.rect_stroke(frame, 0.0, Stroke::new(1.0f32, TEXT), StrokeKind::Outside);
        let (fw, fh) = (w as f64, h as f64);
        let ratio = self.kept_ratio(&e, fw, fh);
        let mut moved = None;
        let inside = ui.interact(frame, egui::Id::new("crop-frame"), Sense::drag());
        if inside.hovered() || inside.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if inside.dragged() {
            moved = Some((0, 0, inside.drag_delta()));
        }
        for (sx, sy) in [(-1, -1), (1, -1), (-1, 1), (1, 1), (0, -1), (0, 1), (-1, 0), (1, 0)] {
            let at = pos2(frame.center().x + sx as f32 * frame.width() / 2.0, frame.center().y + sy as f32 * frame.height() / 2.0);
            let handle = Rect::from_center_size(at, vec2(16.0, 16.0));
            let r = ui.interact(handle, egui::Id::new(("crop-handle", sx, sy)), Sense::drag());
            p.rect_filled(Rect::from_center_size(at, vec2(7.0, 7.0)), 1.0, if r.hovered() || r.dragged() { Color32::WHITE } else { LABEL });
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
        let Some((sx, sy, d)) = moved else { return };
        let (dx, dy) = (d.x as f64 / z as f64, d.y as f64 / z as f64);
        let (mut x0, mut x1, mut y0, mut y1) = (e.crop.left * fw, e.crop.right * fw, e.crop.top * fh, e.crop.bottom * fh);
        if (sx, sy) == (0, 0) {
            (x0, x1, y0, y1) = (x0 + dx, x1 + dx, y0 + dy, y1 + dy);
        }
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
        if let (Some(r), true) = (ratio, (sx, sy) != (0, 0)) {
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
            self.cropping = Some(e);
        }
    }
}
