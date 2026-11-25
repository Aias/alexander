//! Laying out the window: toolbar, the three views, the filmstrip and the controls panel.

use super::{controls, App, Mode, SizeChoice};
use alexander_studio::engine::Canvas;
use alexander_studio::export::Format;
use alexander_studio::library::{Id, Status};
use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui};

const VIEW_BG: Color32 = Color32::from_gray(22);
const CELL: Color32 = Color32::from_gray(38);
const CELL_SELECTED: Color32 = Color32::from_gray(58);
const CELL_FOCUS: Color32 = Color32::from_gray(72);
const FOCUS_LINE: Color32 = Color32::from_gray(170);

pub fn style(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_gray(34);
    v.window_fill = Color32::from_gray(40);
    v.extreme_bg_color = Color32::from_gray(26);
    v.faint_bg_color = Color32::from_gray(38);
    v.selection.bg_fill = Color32::from_gray(96);
    v.selection.stroke = Stroke::new(1.0f32, Color32::from_gray(230));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, Color32::from_gray(48));
    v.widgets.inactive.weak_bg_fill = Color32::from_gray(48);
    v.widgets.inactive.bg_fill = Color32::from_gray(60);
    v.widgets.hovered.weak_bg_fill = Color32::from_gray(62);
    v.widgets.active.weak_bg_fill = Color32::from_gray(76);
    v.slider_trailing_fill = true;
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(90) };
    v.hyperlink_color = Color32::from_gray(200);
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 4.0);
        s.spacing.button_padding = vec2(8.0, 3.0);
        s.spacing.interact_size.y = 20.0;
        s.spacing.slider_rail_height = 3.0;
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(10.5));
        s.interaction.tooltip_delay = 0.6;
    });
}

fn px(r: Rect, ppp: f32) -> [i32; 4] {
    [(r.min.x * ppp).round() as i32, (r.min.y * ppp).round() as i32, (r.max.x * ppp).round() as i32, (r.max.y * ppp).round() as i32]
}

fn from_px(p: [i32; 4], ppp: f32) -> Rect {
    Rect::from_min_max(pos2(p[0] as f32 / ppp, p[1] as f32 / ppp), pos2(p[2] as f32 / ppp, p[3] as f32 / ppp))
}

fn uv() -> Rect {
    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))
}

/// A rectangle of `w` x `h` physical pixels centred in `area`, on pixel boundaries.
fn centred(area: Rect, w: u32, h: u32, ppp: f32) -> Rect {
    let a = px(area, ppp);
    let x = a[0] + ((a[2] - a[0]) - w as i32) / 2;
    let y = a[1] + ((a[3] - a[1]) - h as i32) / 2;
    from_px([x, y, x + w as i32, y + h as i32], ppp)
}

impl App {
    pub(super) fn layout(&mut self, ctx: &egui::Context) -> Vec<controls::Action> {
        let mut actions = Vec::new();
        self.toolbar(ctx);
        let has_photos = !self.lib.is_empty();
        if has_photos && self.panel && self.lib.focus.is_some() && !self.lib.selected.is_empty() {
            egui::SidePanel::right("controls").exact_width(292.0).resizable(false).show(ctx, |ui| {
                ui.add_space(6.0);
                self.panel_ui(ui, &mut actions);
            });
        }
        if has_photos && self.mode == Mode::Single {
            egui::TopBottomPanel::bottom("filmstrip").exact_height(88.0).frame(egui::Frame::NONE.fill(Color32::from_gray(30))).show(ctx, |ui| self.filmstrip(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(VIEW_BG)).show(ctx, |ui| {
            if !has_photos {
                self.empty(ui);
            } else {
                match self.mode {
                    Mode::Grid => self.grid(ui),
                    Mode::Survey => self.survey(ui),
                    Mode::Single => self.single(ui),
                }
                if self.before {
                    let r = ui.max_rect();
                    let g = ui.painter().layout_no_wrap("Before".into(), FontId::proportional(11.0), Color32::from_gray(220));
                    let pill = Rect::from_min_size(r.min + vec2(10.0, 10.0), g.size() + vec2(12.0, 6.0));
                    ui.painter().rect_filled(pill, CornerRadius::same(4), Color32::from_black_alpha(160));
                    ui.painter().galley(pill.min + vec2(6.0, 3.0), g, Color32::WHITE);
                }
            }
            if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
                ui.painter().rect_stroke(ui.max_rect().shrink(3.0), CornerRadius::same(4), Stroke::new(2.0f32, Color32::from_gray(160)), StrokeKind::Inside);
            }
        });
        self.export_window(ctx);
        self.errors_window(ctx);
        actions
    }

    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("toolbar").exact_height(34.0).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                if ui.button("Open…").on_hover_text("⌘O").clicked() {
                    self.import_dialog();
                }
                if self.lib.is_empty() {
                    return;
                }
                ui.add_space(12.0);
                let before = self.mode;
                egui::Frame::NONE.fill(Color32::from_gray(26)).corner_radius(CornerRadius::same(5)).inner_margin(2.0).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    ui.selectable_value(&mut self.mode, Mode::Grid, "All").on_hover_text("G");
                    ui.selectable_value(&mut self.mode, Mode::Survey, "Selected").on_hover_text("N");
                    ui.selectable_value(&mut self.mode, Mode::Single, "Single").on_hover_text("E");
                });
                if self.mode != before {
                    self.scroll_to_focus = true;
                    self.zoom = None;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let n = self.exportable();
                    if ui.add_enabled(n > 0 && self.export_run.is_none(), egui::Button::new("Export…")).on_hover_text("⇧⌘E").clicked() {
                        self.open_export();
                    }
                    if let Some(run) = &self.export_run {
                        let f = run.done as f32 / run.total.max(1) as f32;
                        ui.add(egui::ProgressBar::new(f).desired_width(120.0).desired_height(6.0));
                    }
                    ui.add_space(8.0);
                    ui.label(egui::RichText::new(format!("{} / {}", self.lib.selected.len(), self.lib.len())).weak().small());
                    if self.mode == Mode::Grid {
                        ui.add_space(8.0);
                        ui.spacing_mut().slider_width = 90.0;
                        ui.add(egui::Slider::new(&mut self.thumb_pt, 80.0..=480.0).show_value(false));
                    }
                });
            });
        });
    }

    fn panel_ui(&mut self, ui: &mut Ui, actions: &mut Vec<controls::Action>) {
        let Some(focus) = self.lib.focus.and_then(|f| self.lib.get(f)) else { return };
        let processed = focus.look.is_some();
        let mut s = focus.look.clone().or_else(|| focus.stash.clone()).unwrap_or_default();
        controls::header(ui, processed, self.clipboard.is_some(), actions);
        ui.separator();
        let before = s.clone();
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            controls::knobs(ui, &mut s);
        });
        if s != before {
            let ids = self.lib.selection();
            if !self.edit_open {
                self.lib.checkpoint(&ids);
                self.edit_open = true;
            }
            self.lib.apply_change(&ids, &before, &s);
        }
    }

    fn empty(&mut self, ui: &mut Ui) {
        let r = ui.max_rect();
        let b = ui.put(Rect::from_center_size(r.center(), vec2(160.0, 34.0)), egui::Button::new(egui::RichText::new("Open Photos…").size(14.0)));
        if b.clicked() {
            self.import_dialog();
        }
    }

    /// Selection by click with the usual modifiers.
    fn clicked(&mut self, ui: &Ui, id: Id, r: &egui::Response) {
        if r.clicked() {
            let m = ui.input(|i| i.modifiers);
            if m.command {
                self.lib.toggle(id);
            } else if m.shift {
                self.lib.extend(id);
            } else {
                self.lib.click(id);
            }
        }
    }

    fn thumb_cell(&mut self, ui: &mut Ui, id: Id, cell: Rect, rank_base: u8) {
        let ppp = ui.ctx().pixels_per_point();
        let selected = self.lib.selected.contains(&id);
        let focused = self.lib.focus == Some(id);
        let fill = if focused && selected {
            CELL_FOCUS
        } else if selected {
            CELL_SELECTED
        } else {
            CELL
        };
        let p = ui.painter();
        p.rect_filled(cell, CornerRadius::same(3), fill);
        let inner = cell.shrink(cell.width().min(cell.height()) * 0.07);
        let item = self.lib.get(id).unwrap();
        match &item.status {
            Status::Ready => {
                let b = px(inner, ppp);
                // in the single view the large picture of the focused photo goes first
                let rank = if focused && self.mode == Mode::Grid { 0 } else { rank_base };
                let (tex, size) = self.tiers.thumb(item, [(b[2] - b[0]).max(1) as u32, (b[3] - b[1]).max(1) as u32], rank);
                if let Some(t) = tex {
                    let r = centred(inner, size[0], size[1], ppp);
                    p.image(t, r, uv(), Color32::WHITE);
                }
            }
            Status::Failed(_) => {
                p.text(inner.center(), Align2::CENTER_CENTER, "!", FontId::proportional(16.0), Color32::from_gray(120));
            }
            Status::Loading => {}
        }
        if focused && selected {
            p.rect_stroke(cell, CornerRadius::same(3), Stroke::new(1.0f32, FOCUS_LINE), StrokeKind::Inside);
        }
    }

    fn grid(&mut self, ui: &mut Ui) {
        let gap = 4.0;
        let cell = self.thumb_pt;
        let avail = ui.available_width() - 2.0 * gap;
        let cols = ((avail + gap) / (cell + gap)).floor().max(1.0) as usize;
        self.grid_cols = cols;
        let w = (avail - gap * (cols - 1) as f32) / cols as f32;
        let row_h = w;
        let n = self.lib.len();
        let rows = n.div_ceil(cols);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if self.scroll_to_focus {
            if let Some(i) = self.lib.focus.and_then(|f| self.lib.position(f)) {
                let row = i / cols;
                let (a, b) = self.grid_rows_seen;
                let step = row_h + gap;
                if row <= a || row + 1 >= b {
                    area = area.vertical_scroll_offset((row as f32 * step - (self.grid_view_h - step) / 2.0).max(0.0));
                }
            }
            self.scroll_to_focus = false;
        }
        ui.add_space(gap);
        let out = area.show_rows(ui, row_h - ui.spacing().item_spacing.y + gap, rows, |ui, range| {
            self.grid_rows_seen = (range.start, range.end);
            for row in range {
                let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), row_h - ui.spacing().item_spacing.y + gap), Sense::hover());
                for c in 0..cols {
                    let i = row * cols + c;
                    if i >= n {
                        break;
                    }
                    let id = self.lib.at(i).id;
                    let x = rect.min.x + gap + c as f32 * (w + gap);
                    let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(w, row_h));
                    let r = ui.interact(cell, egui::Id::new(("cell", id)), Sense::click());
                    self.clicked(ui, id, &r);
                    if r.double_clicked() {
                        self.lib.click(id);
                        self.mode = Mode::Single;
                        self.zoom = None;
                    }
                    if let Status::Failed(e) = &self.lib.get(id).unwrap().status {
                        r.on_hover_text(e.clone());
                    }
                    self.thumb_cell(ui, id, cell, 2);
                }
            }
        });
        self.grid_view_h = out.inner_rect.height();
    }

    /// Fit `n` tiles of about `aspect` into `area` as large as they go: the column count.
    fn survey_cols(n: usize, area: egui::Vec2, aspect: f32) -> usize {
        let mut best = (0.0, 1);
        for c in 1..=n.max(1) {
            let r = n.div_ceil(c);
            let (cw, ch) = (area.x / c as f32, area.y / r as f32);
            let s = (cw / aspect).min(ch);
            let a = s * s * aspect;
            if a > best.0 {
                best = (a, c);
            }
        }
        best.1
    }

    fn survey(&mut self, ui: &mut Ui) {
        let ids = self.lib.selection();
        if ids.is_empty() {
            return;
        }
        let ppp = ui.ctx().pixels_per_point();
        let area = ui.max_rect().shrink(8.0);
        let aspect = {
            let a: Vec<f32> = ids.iter().filter_map(|&id| self.lib.get(id)).filter(|it| it.original.1 > 0).map(|it| it.original.0 as f32 / it.original.1 as f32).collect();
            if a.is_empty() {
                1.5
            } else {
                a.iter().sum::<f32>() / a.len() as f32
            }
        };
        let cols = Self::survey_cols(ids.len(), area.size(), aspect);
        let rows = ids.len().div_ceil(cols);
        let (cw, ch) = (area.width() / cols as f32, area.height() / rows as f32);
        for (k, &id) in ids.iter().enumerate() {
            let cell = Rect::from_min_size(area.min + vec2((k % cols) as f32 * cw, (k / cols) as f32 * ch), vec2(cw, ch));
            let r = ui.interact(cell, egui::Id::new(("tile", id)), Sense::click());
            if r.clicked() {
                if ui.input(|i| i.modifiers.command) {
                    self.lib.toggle(id);
                } else {
                    self.lib.focus = Some(id);
                }
            }
            if r.double_clicked() {
                self.lib.focus = Some(id);
                self.mode = Mode::Single;
                self.zoom = None;
            }
            let Some(item) = self.lib.get(id) else { continue };
            if !item.ready() {
                continue;
            }
            let inner = cell.shrink(6.0);
            let b = px(inner, ppp);
            let (gw, gh) = self.tiers.grid_size(item);
            let z = ((b[2] - b[0]) as f32 / gw as f32).min((b[3] - b[1]) as f32 / gh as f32);
            let (iw, ih) = (((gw as f32 * z).round() as u32).max(1), ((gh as f32 * z).round() as u32).max(1));
            let img = centred(inner, iw, ih, ppp);
            let focused = self.lib.focus == Some(id);
            let canvas = Canvas { origin: [0.0, 0.0], scale: gw as f32 / iw as f32, show_source: false };
            let tex = self.tiers.canvas(item, canvas, [iw, ih], if focused { 0 } else { 1 }).or_else(|| self.tiers.thumb_tex(id));
            if let Some(t) = tex {
                ui.painter().image(t, img, uv(), Color32::WHITE);
            }
            if focused && ids.len() > 1 {
                ui.painter().rect_stroke(img.expand(3.0), CornerRadius::ZERO, Stroke::new(1.0f32, FOCUS_LINE), StrokeKind::Inside);
            }
        }
    }

    fn single(&mut self, ui: &mut Ui) {
        let ppp = ui.ctx().pixels_per_point();
        let rect = ui.max_rect();
        let resp = ui.interact(rect, egui::Id::new("single"), Sense::click_and_drag());
        let Some(item) = self.lib.focus.and_then(|f| self.lib.get(f)) else { return };
        if !item.ready() {
            return;
        }
        let vp = px(rect, ppp);
        let (vw, vh) = ((vp[2] - vp[0]) as f32, (vp[3] - vp[1]) as f32);
        let (gw, gh) = self.tiers.grid_size(item);
        let (gwf, ghf) = (gw as f32, gh as f32);
        let fit = (vw / gwf).min(vh / ghf);
        self.last_fit = fit;
        let mut z = self.zoom.unwrap_or(fit);
        let vc = [vp[0] as f32 + vw / 2.0, vp[1] as f32 + vh / 2.0];
        let mut c = if self.zoom.is_none() { [gwf / 2.0, ghf / 2.0] } else { [self.center[0] * gwf, self.center[1] * ghf] };
        // zoom about the cursor: the photo point under it stays under it
        if let Some(hover) = resp.hover_pos() {
            let (scroll, pinch) = ui.input(|i| (i.smooth_scroll_delta.y, i.zoom_delta()));
            let factor = pinch * (scroll * 0.004).exp();
            if (factor - 1.0).abs() > 1e-4 {
                let hp = [hover.x * ppp, hover.y * ppp];
                let g = [c[0] + (hp[0] - vc[0]) / z, c[1] + (hp[1] - vc[1]) / z];
                let nz = (z * factor).clamp(fit.min(1.0) * 0.5, 16.0);
                c = [g[0] - (hp[0] - vc[0]) / nz, g[1] - (hp[1] - vc[1]) / nz];
                z = nz;
                self.zoom = Some(z);
            }
        }
        if resp.dragged() && self.zoom.is_some() {
            let d = resp.drag_delta();
            c = [c[0] - d.x * ppp / z, c[1] - d.y * ppp / z];
        }
        if resp.double_clicked() {
            if self.zoom.is_some() {
                self.zoom = None;
            } else if let Some(h) = resp.interact_pointer_pos() {
                let hp = [h.x * ppp, h.y * ppp];
                let g = [c[0] + (hp[0] - vc[0]) / z, c[1] + (hp[1] - vc[1]) / z];
                z = 1.0;
                c = [g[0] - (hp[0] - vc[0]), g[1] - (hp[1] - vc[1])];
                self.zoom = Some(z);
            }
        }
        if let Some(zz) = self.zoom {
            z = zz;
            // keep the photo covering the view where it can
            for (i, (len, view)) in [(gwf, vw), (ghf, vh)].into_iter().enumerate() {
                let half = view / (2.0 * z);
                c[i] = if len <= 2.0 * half { len / 2.0 } else { c[i].clamp(half, len - half) };
            }
            self.center = [c[0] / gwf, c[1] / ghf];
        } else {
            z = fit;
            c = [gwf / 2.0, ghf / 2.0];
        }
        let x0 = (vc[0] - c[0] * z).round().max(vp[0] as f32) as i32;
        let y0 = (vc[1] - c[1] * z).round().max(vp[1] as f32) as i32;
        let x1 = (vc[0] + (gwf - c[0]) * z).round().min(vp[2] as f32) as i32;
        let y1 = (vc[1] + (ghf - c[1]) * z).round().min(vp[3] as f32) as i32;
        if x1 <= x0 || y1 <= y0 {
            return;
        }
        let shown = from_px([x0, y0, x1, y1], ppp);
        let canvas = Canvas { origin: [c[0] + (x0 as f32 - vc[0]) / z, c[1] + (y0 as f32 - vc[1]) / z], scale: 1.0 / z, show_source: false };
        let pos = self.lib.position(item.id).unwrap_or(0);
        for j in [pos + 1, pos.wrapping_sub(1)] {
            if j < self.lib.len() {
                self.tiers.prefetch(self.lib.at(j));
            }
        }
        let item = self.lib.at(pos);
        if let Some(t) = self.tiers.canvas(item, canvas, [(x1 - x0) as u32, (y1 - y0) as u32], 0) {
            ui.painter().image(t, shown, uv(), Color32::WHITE);
        } else if let Some(t) = self.tiers.thumb_tex(item.id) {
            // the whole photo's thumbnail, placed where the photo is and cut to the view
            let full = Rect::from_min_max(pos2((vc[0] - c[0] * z) / ppp, (vc[1] - c[1] * z) / ppp), pos2((vc[0] + (gwf - c[0]) * z) / ppp, (vc[1] + (ghf - c[1]) * z) / ppp));
            ui.painter().with_clip_rect(rect).image(t, full, uv(), Color32::WHITE);
        }
    }

    fn filmstrip(&mut self, ui: &mut Ui) {
        let h = ui.available_height();
        let cell = vec2(h * 1.25, h - 8.0);
        let gap = 4.0;
        let n = self.lib.len();
        let scroll_to = if self.scroll_to_focus { self.lib.focus.and_then(|f| self.lib.position(f)) } else { None };
        self.scroll_to_focus = false;
        egui::ScrollArea::horizontal().auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let total = n as f32 * (cell.x + gap) + gap;
            ui.set_min_size(vec2(total, h));
            let first = ((viewport.min.x - gap) / (cell.x + gap)).floor().max(0.0) as usize;
            let last = (((viewport.max.x) / (cell.x + gap)).ceil() as usize + 1).min(n);
            if let Some(i) = scroll_to {
                let r = Rect::from_min_size(origin + vec2(gap + i as f32 * (cell.x + gap), 4.0), cell);
                ui.scroll_to_rect(r, Some(egui::Align::Center));
            }
            for i in first..last {
                let id = self.lib.at(i).id;
                let r = Rect::from_min_size(origin + vec2(gap + i as f32 * (cell.x + gap), 4.0), cell);
                let resp = ui.interact(r, egui::Id::new(("strip", id)), Sense::click());
                self.clicked(ui, id, &resp);
                if resp.clicked() {
                    self.zoom = None;
                }
                self.thumb_cell(ui, id, r, 3);
            }
        });
    }

    fn export_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.export_form.take() else { return };
        let mut open = true;
        let mut go = false;
        let mut cancel = false;
        let n = self.exportable();
        egui::Window::new("Export").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).open(&mut open).show(ctx, |ui| {
            egui::Grid::new("export").num_columns(2).spacing([10.0, 8.0]).show(ui, |ui| {
                ui.label("Folder");
                ui.horizontal(|ui| {
                    let name = form.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| form.dir.display().to_string());
                    ui.add(egui::Label::new(name).truncate()).on_hover_text(form.dir.display().to_string());
                    if ui.button("Choose…").clicked() {
                        if let Some(d) = rfd::FileDialog::new().set_directory(&form.dir).pick_folder() {
                            form.dir = d;
                        }
                    }
                });
                ui.end_row();
                ui.label("Format");
                egui::ComboBox::from_id_salt("format").selected_text(form.format.label()).show_ui(ui, |ui| {
                    for f in Format::ALL {
                        ui.selectable_value(&mut form.format, f, f.label());
                    }
                });
                ui.end_row();
                ui.label("Size");
                ui.horizontal(|ui| {
                    let label = match form.size {
                        SizeChoice::Original => "Original",
                        SizeChoice::Grid => "Simulation grid",
                        SizeChoice::Custom => "Long edge",
                    };
                    egui::ComboBox::from_id_salt("size").selected_text(label).show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.size, SizeChoice::Original, "Original");
                        ui.selectable_value(&mut form.size, SizeChoice::Grid, "Simulation grid");
                        ui.selectable_value(&mut form.size, SizeChoice::Custom, "Long edge");
                    });
                    if form.size == SizeChoice::Custom {
                        ui.add(egui::DragValue::new(&mut form.long).range(64..=30000).suffix(" px"));
                    }
                });
                ui.end_row();
            });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let label = if n == 1 { "Export".to_string() } else { format!("Export {n}") };
                    if ui.add_enabled(n > 0 && form.dir.is_dir(), egui::Button::new(label)).clicked() || ui.input(|i| i.key_pressed(egui::Key::Enter)) && n > 0 && form.dir.is_dir() {
                        go = true;
                    }
                    if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        cancel = true;
                    }
                });
            });
        });
        if go {
            self.last_dir = Some(form.dir.clone());
            self.start_export(form);
        } else if open && !cancel {
            self.export_form = Some(form);
        }
    }

    fn errors_window(&mut self, ctx: &egui::Context) {
        if self.export_errors.is_empty() {
            return;
        }
        let mut close = false;
        egui::Window::new("Could not save").collapsible(false).resizable(false).anchor(Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
            for e in self.export_errors.iter().take(12) {
                ui.label(e);
            }
            if ui.button("OK").clicked() {
                close = true;
            }
        });
        if close {
            self.export_errors.clear();
        }
    }
}
