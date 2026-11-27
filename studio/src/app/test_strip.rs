//! The test strip, as a darkroom printer makes one: the focused photo in Detail in patches,
//! each under another value of a setting, to choose a value by eye. One setting steps through
//! every patch in reading order. Of two, the first steps across the strip and the second down
//! it. Each patch looks at the photo as Detail does, at its zoom and about its centre, so a
//! strip made at 100% shows grain and softness. The patch under the pointer shows its values,
//! and a click on it gives them to the selected photos. A double click gives them and puts
//! the strip away, as it applies a profile and closes the browser in Lightroom's Profile
//! Browser.

use super::controls::{fraction, value_at};
use super::theme::{SHADOW, STRONG};
use super::views::{from_px, pill, px, uv};
use super::App;
use alexander_studio::engine::Canvas;
use alexander_studio::library::{Id, Item};
use alexander_studio::params::Settings;
use egui::{vec2, Align2, Color32, CornerRadius, CursorIcon, Pos2, Rect, Stroke, StrokeKind, Ui};

/// Patches along each side of the strip.
const SIDE: usize = 5;

impl App {
    /// Detail with the test strip over the photo, where `resp` senses the view.
    pub(super) fn test_strip_view(&mut self, ui: &Ui, rect: Rect, resp: &egui::Response, id: Id) {
        let Some(item) = self.lib.get(id) else { return };
        let ppp = ui.ctx().pixels_per_point();
        let strip = self.test_strip.clone();
        let mut look = item.settings();
        // the photo as its patches render it: under the look its knobs show
        let item = Item { look: Some(look.clone()), ..item.clone() };
        let (origin, (gw, gh), angle) = self.tiers.shown(&item);
        let (gwf, ghf) = (gw as f32, gh as f32);
        // the strip covers the photo as Detail fits it, in patches of whole pixels a pixel apart
        let (vp, side) = (px(rect, ppp), SIDE as i32);
        let fit = ((vp[2] - vp[0]) as f32 / gwf).min((vp[3] - vp[1]) as f32 / ghf);
        let pitch = [(gwf * fit) as i32 / side, (ghf * fit) as i32 / side];
        let first = [(vp[0] + vp[2] - pitch[0] * side) / 2, (vp[1] + vp[3] - pitch[1] * side) / 2];
        let patch = |i: usize| {
            let (x, y) = (first[0] + (i % SIDE) as i32 * pitch[0], first[1] + (i / SIDE) as i32 * pitch[1]);
            [x + 1, y + 1, x + pitch[0] - 1, y + pitch[1] - 1]
        };
        let under = |at: Option<Pos2>| at.and_then(|at| (0..SIDE * SIDE).find(|&i| from_px(patch(i), ppp).contains(at)));
        let hovered = under(resp.hover_pos());
        let view = patch(hovered.unwrap_or(0));
        let Some((at, corner, z)) = self.detail_frame(ui, resp, view, (gwf, ghf), false) else { return };
        let last = if strip.len() == 1 { SIDE * SIDE - 1 } else { SIDE - 1 };
        let step = |j: usize, i: usize| if strip.len() == 1 { i } else if j == 0 { i % SIDE } else { i / SIDE };
        let value = |j: usize, i: usize| value_at(step(j, i) as f64 / last as f64, &strip[j].range, strip[j].log);
        let nearest: Vec<usize> = strip.iter().map(|k| (fraction(*(k.at)(&mut look), &k.range, k.log) * last as f64).round() as usize).collect();
        let looks: Vec<Settings> = (0..SIDE * SIDE)
            .map(|i| {
                let mut patched = look.clone();
                for (j, k) in strip.iter().enumerate() {
                    *(k.at)(&mut patched) = value(j, i);
                }
                patched
            })
            .collect();
        let size = [(at[2] - at[0]) as u32, (at[3] - at[1]) as u32];
        let canvas = Canvas { origin: [origin[0] + corner[0], origin[1] + corner[1]], scale: 1.0 / z, source: Canvas::RENDER, clip: self.clipping, angle };
        for (i, texture) in self.tiers.test_strip(&item, &looks, canvas, size).into_iter().enumerate() {
            let p = patch(i);
            let frame = from_px(p, ppp);
            if let Some(t) = texture {
                let (dx, dy) = (p[0] - view[0], p[1] - view[1]);
                ui.painter().image(t, from_px([at[0] + dx, at[1] + dy, at[2] + dx, at[3] + dy], ppp), uv(), Color32::WHITE);
            }
            if (0..strip.len()).all(|j| step(j, i) == nearest[j]) {
                for (edge, colour) in [(2.0f32, SHADOW), (0.0, STRONG)] {
                    ui.painter().rect_stroke(frame, CornerRadius::ZERO, Stroke::new(1.0 + edge, colour), StrokeKind::Inside);
                }
            }
        }
        if let Some(i) = hovered.filter(|_| !resp.dragged()) {
            ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            let values: Vec<String> = strip.iter().enumerate().map(|(j, k)| format!("{} {:.*}", k.label, k.decimals, value(j, i))).collect();
            pill(ui, from_px(patch(i), ppp).left_top() + vec2(6.0, 6.0), Align2::LEFT_TOP, &values.join("\n"));
        }
        if let Some(i) = under(resp.interact_pointer_pos()).filter(|_| resp.clicked()) {
            if looks[i] != look {
                let ids = self.lib.selection();
                self.lib.checkpoint(&ids);
                self.lib.apply_change(&ids, &look, &looks[i]);
            } else if resp.double_clicked() || resp.triple_clicked() {
                // a repeated click on the patch whose values are given. A click close after a
                // click on another patch gives its own values first and leaves the strip up
                self.test_strip.clear();
            }
        }
    }
}
