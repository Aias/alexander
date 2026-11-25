//! Laying out the window: the three views, the filmstrip and where the panels go.

use super::{App, Command, Compare, Mode};
use alexander_studio::engine::Canvas;
use alexander_studio::library::{Id, Item, Status};
use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Align2, Button, Color32, CornerRadius, CursorIcon, FontId, Margin, Pos2, Rect, Sense, Stroke, StrokeKind, Ui};

pub const VIEW_BG: Color32 = Color32::from_gray(22);
pub const BAR: Color32 = Color32::from_gray(29);
const CELL: Color32 = Color32::from_gray(36);
const CELL_HOVER: Color32 = Color32::from_gray(44);
const CELL_SELECTED: Color32 = Color32::from_gray(58);
const CELL_FOCUS: Color32 = Color32::from_gray(72);
const FOCUS_LINE: Color32 = Color32::from_gray(170);
/// Labels: quieter than values and controls, but read at better than 5:1 on the panel.
pub const LABEL: Color32 = Color32::from_gray(150);
pub const TEXT: Color32 = Color32::from_gray(222);

pub fn style(ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    if let Ok(sf) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert("SF".into(), std::sync::Arc::new(egui::FontData::from_owned(sf)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "SF".into());
        ctx.set_fonts(fonts);
    }
    let mut v = egui::Visuals::dark();
    v.panel_fill = Color32::from_gray(34);
    v.window_fill = Color32::from_gray(40);
    v.extreme_bg_color = Color32::from_gray(26);
    v.faint_bg_color = Color32::from_gray(38);
    v.selection.bg_fill = Color32::from_gray(96);
    v.selection.stroke = Stroke::new(1.0f32, Color32::from_gray(230));
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0f32, Color32::from_gray(44));
    v.widgets.inactive.weak_bg_fill = Color32::from_gray(50);
    v.widgets.inactive.bg_fill = Color32::from_gray(60);
    v.widgets.inactive.fg_stroke = Stroke::new(1.0f32, Color32::from_gray(205));
    v.widgets.hovered.weak_bg_fill = Color32::from_gray(64);
    v.widgets.hovered.bg_stroke = Stroke::NONE;
    v.widgets.active.weak_bg_fill = Color32::from_gray(78);
    v.widgets.active.bg_stroke = Stroke::NONE;
    v.widgets.open.weak_bg_fill = Color32::from_gray(64);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
        w.corner_radius = CornerRadius::same(4);
        w.expansion = 0.0;
    }
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(7);
    v.window_stroke = Stroke::new(1.0f32, Color32::from_gray(56));
    v.window_shadow = egui::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(110) };
    v.popup_shadow = egui::Shadow { offset: [0, 2], blur: 8, spread: 0, color: Color32::from_black_alpha(90) };
    v.hyperlink_color = Color32::from_gray(200);
    // egui's plain text is 4.4:1 on the dialogs' fill; a little brighter reads comfortably
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0f32, Color32::from_gray(176));
    ctx.set_visuals(v);
    ctx.style_mut(|s| {
        s.spacing.item_spacing = vec2(6.0, 5.0);
        s.spacing.button_padding = vec2(10.0, 3.0);
        s.spacing.interact_size.y = 22.0;
        s.spacing.menu_margin = Margin::same(5);
        s.text_styles.insert(egui::TextStyle::Body, FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Button, FontId::proportional(12.5));
        s.text_styles.insert(egui::TextStyle::Small, FontId::proportional(11.0));
        s.text_styles.insert(egui::TextStyle::Heading, FontId::proportional(15.0));
        s.interaction.tooltip_delay = 0.6;
    });
}

/// A row of mutually exclusive choices as one control, each with its key in a tooltip.
pub fn segmented<T: PartialEq + Copy>(ui: &mut Ui, value: &mut T, options: &[(T, &str, &str)]) -> egui::Response {
    egui::Frame::NONE.fill(VIEW_BG).corner_radius(CornerRadius::same(6)).inner_margin(2.0).show(ui, |ui| {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            ui.spacing_mut().button_padding.x = 4.0;
            let w = (ui.available_width() - 2.0 * (options.len() - 1) as f32) / options.len() as f32;
            for &(v, label, tip) in options {
                let r = ui.add_sized([w, ui.available_height()], egui::SelectableLabel::new(*value == v, label));
                let r = if tip.is_empty() { r } else { r.on_hover_text(tip) };
                if r.clicked() {
                    *value = v;
                }
            }
        });
    })
    .response
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

/// A dark label over a picture, with its corner `align` at `at`.
fn pill(ui: &Ui, at: Pos2, align: Align2, text: &str) {
    let g = ui.painter().layout_no_wrap(text.into(), FontId::proportional(11.5), Color32::from_gray(230));
    let pill = align.anchor_size(at, g.size() + vec2(16.0, 8.0));
    ui.painter().rect_filled(pill, CornerRadius::same(5), Color32::from_black_alpha(170));
    ui.painter().galley(pill.min + vec2(8.0, 4.0), g, Color32::WHITE);
}

/// A rectangle of `w` x `h` physical pixels centred in `area`, on pixel boundaries.
fn centred(area: Rect, w: u32, h: u32, ppp: f32) -> Rect {
    let a = px(area, ppp);
    let x = a[0] + ((a[2] - a[0]) - w as i32) / 2;
    let y = a[1] + ((a[3] - a[1]) - h as i32) / 2;
    from_px([x, y, x + w as i32, y + h as i32], ppp)
}

impl App {
    pub(super) fn layout(&mut self, ctx: &egui::Context) -> Vec<Command> {
        let mut actions = Vec::new();
        self.toolbar(ctx);
        let has_photos = !self.lib.is_empty();
        if has_photos && self.mode != Mode::Grid {
            egui::TopBottomPanel::bottom("filmstrip").exact_height(92.0).frame(egui::Frame::NONE.fill(BAR)).show(ctx, |ui| self.filmstrip(ui));
        }
        if has_photos && self.panel {
            let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(12, 10));
            egui::SidePanel::right("controls").resizable(true).default_width(340.0).width_range(320.0..=460.0).frame(frame).show(ctx, |ui| self.panel_ui(ui, &mut actions));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(VIEW_BG)).show(ctx, |ui| {
            if has_photos {
                egui::TopBottomPanel::bottom("status").exact_height(30.0).frame(egui::Frame::NONE.fill(BAR).inner_margin(Margin::symmetric(10, 0))).show_inside(ui, |ui| self.status_bar(ui));
                egui::CentralPanel::default().frame(egui::Frame::NONE).show_inside(ui, |ui| {
                    match self.mode {
                        Mode::Grid => self.grid(ui),
                        Mode::Survey => self.survey(ui),
                        Mode::Loupe => self.loupe(ui),
                    }
                    if self.before() {
                        pill(ui, ui.max_rect().min + vec2(12.0, 12.0), Align2::LEFT_TOP, "Before");
                    }
                });
            } else {
                self.empty(ui);
            }
            if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
                ui.painter().rect_stroke(ui.max_rect().shrink(3.0), CornerRadius::same(6), Stroke::new(2.0f32, Color32::from_gray(160)), StrokeKind::Inside);
            }
        });
        self.export_window(ctx);
        self.errors_window(ctx);
        self.reference_ui(ctx);
        actions
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

    /// The menu a right click on a photo opens. As in Finder, a photo outside the selection
    /// becomes the selection first, so the commands act on what is selected; the photo
    /// clicked becomes the focused one, whose look and picture are copied.
    fn photo_menu(&mut self, r: &egui::Response, id: Id) {
        if r.secondary_clicked() {
            if self.lib.selected().contains(&id) {
                self.lib.focus = Some(id);
            } else {
                self.lib.click(id);
            }
        }
        r.context_menu(|ui| {
            ui.set_min_width(230.0);
            let ids = self.lib.selection();
            let items: Vec<&Item> = ids.iter().filter_map(|&i| self.lib.get(i)).collect();
            let paths: Vec<String> = items.iter().map(|it| it.path.display().to_string()).collect();
            let off = items.iter().any(|it| it.look.is_none());
            let unstarred = items.iter().any(|it| !it.starred);
            if self.mode != Mode::Loupe {
                self.entry(ui, "Open in Loupe", "↩", Command::View(Mode::Loupe));
            }
            #[cfg(target_os = "macos")]
            self.entry(ui, "Show in Finder", "⌘R", Command::ShowInFinder);
            if ui.button(if paths.len() > 1 { "Copy File Paths" } else { "Copy File Path" }).clicked() {
                ui.ctx().copy_text(paths.join("\n"));
                ui.close_menu();
            }
            self.entry(ui, "Copy Image", "", Command::CopyImage);
            self.entry(ui, "Create Virtual Copy", "⌘'", Command::VirtualCopy);
            ui.separator();
            self.entry(ui, if unstarred { "Star" } else { "Remove Star" }, "S", Command::Star);
            self.entry(ui, if off { "Turn Film On" } else { "Turn Film Off" }, "V", Command::Process(None));
            self.entry(ui, "Copy Look", "⇧⌘C", Command::CopyLook);
            self.entry(ui, "Paste Look", "⇧⌘V", Command::PasteLook);
            self.entry(ui, "Reset Look", "⇧⌘R", Command::ResetLook);
            ui.separator();
            self.entry(ui, "Export…", "⇧⌘E", Command::Export);
            ui.separator();
            let remove = if ids.len() > 1 { format!("Remove {} Photos from Library", ids.len()) } else { "Remove from Library".to_string() };
            self.entry(ui, &remove, "⌫", Command::Remove);
        });
    }

    /// A menu entry that runs `c`, enabled when it can run.
    pub(super) fn entry(&mut self, ui: &mut Ui, text: &str, keys: &str, c: Command) {
        if ui.add_enabled(self.can(&c), Button::new(text).shortcut_text(keys)).clicked() {
            ui.close_menu();
            self.run(&ui.ctx().clone(), c);
        }
    }

    /// The canvas columns that show the photo itself in a split, for a canvas `w` pixels
    /// wide across the picture.
    fn split_source(&self, w: u32) -> [f32; 2] {
        if self.compare != Compare::Split {
            return Canvas::RENDER;
        }
        let x = self.split * w as f32;
        if self.split_swap {
            [x, Canvas::PHOTO[1]]
        } else {
            [Canvas::PHOTO[0], x]
        }
    }

    /// The split's divider over a picture shown at `img`, dragged to move it, with each side
    /// labelled when `named`.
    fn divider(&mut self, ui: &Ui, id: Id, img: Rect, named: bool) {
        if self.compare != Compare::Split {
            return;
        }
        let x = img.left() + self.split * img.width();
        let r = ui.interact(Rect::from_x_y_ranges(x - 6.0..=x + 6.0, img.y_range()), egui::Id::new(("divider", id)), Sense::drag());
        if let Some(p) = r.interact_pointer_pos().filter(|_| r.dragged()) {
            self.split = ((p.x - img.left()) / img.width()).clamp(0.0, 1.0);
        }
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        let p = ui.painter().with_clip_rect(img);
        p.vline(x, img.y_range(), Stroke::new(3.0f32, Color32::from_black_alpha(90)));
        p.vline(x, img.y_range(), Stroke::new(1.0f32, Color32::WHITE));
        p.circle(pos2(x, img.center().y), 7.0, Color32::from_black_alpha(170), Stroke::new(1.5f32, Color32::WHITE));
        if named {
            let (left, right) = if self.split_swap { ("After", "Before") } else { ("Before", "After") };
            pill(ui, img.left_top() + vec2(12.0, 12.0), Align2::LEFT_TOP, left);
            pill(ui, img.right_top() + vec2(-12.0, 12.0), Align2::RIGHT_TOP, right);
        }
    }

    /// A photo's cell in the grid or the filmstrip, with its name below when `named`.
    fn thumb_cell(&mut self, ui: &mut Ui, id: Id, cell: Rect, rank_base: u8, hovered: bool, named: bool) {
        let ppp = ui.ctx().pixels_per_point();
        let selected = self.lib.selected().contains(&id);
        let focused = self.lib.focus == Some(id);
        let fill = match (selected, focused, hovered) {
            (true, true, _) => CELL_FOCUS,
            (true, false, _) => CELL_SELECTED,
            (false, _, true) => CELL_HOVER,
            _ => CELL,
        };
        let p = ui.painter();
        p.rect_filled(cell, CornerRadius::same(4), fill);
        let name_h = if named { 22.0 } else { 0.0 };
        let body = Rect::from_min_max(cell.min, pos2(cell.max.x, cell.max.y - name_h));
        let inner = body.shrink(cell.width().min(cell.height()) * 0.07);
        let item = self.lib.get(id).unwrap();
        match &item.status {
            Status::Ready => {
                let b = px(inner, ppp);
                // in the loupe the large picture of the focused photo goes first
                let rank = if focused && self.mode == Mode::Grid { 0 } else { rank_base };
                let (tex, size) = self.tiers.thumb(item, [(b[2] - b[0]).max(1) as u32, (b[3] - b[1]).max(1) as u32], rank);
                if let Some(t) = tex {
                    let r = centred(inner, size[0], size[1], ppp);
                    p.image(t, r, uv(), Color32::WHITE);
                }
            }
            Status::Failed(_) => {
                p.text(inner.center(), Align2::CENTER_CENTER, "⚠", FontId::proportional(20.0), LABEL);
            }
            Status::Loading => egui::Spinner::new().size(16.0).color(LABEL).paint_at(ui, Rect::from_center_size(inner.center(), vec2(16.0, 16.0))),
        }
        if named {
            let colour = if selected { TEXT } else { LABEL };
            let y = cell.max.y - name_h / 2.0 - 4.0;
            let mark = self.film_mark.as_ref().filter(|_| item.look.is_some()).map(|t| (t.id(), t.aspect_ratio() * 11.0));
            let badge_w = mark.map_or(0.0, |(_, w)| w + 6.0);
            let mut job = LayoutJob::simple_singleline(item.title(), FontId::proportional(11.0), colour);
            job.wrap = TextWrapping::truncate_at_width(inner.width() - badge_w);
            let name = ui.fonts(|f| f.layout_job(job));
            ui.painter().galley(pos2(inner.min.x, y - name.size().y / 2.0), name, colour);
            if let Some((t, w)) = mark {
                ui.painter().image(t, Rect::from_min_size(pos2(inner.max.x - w, y - 5.5), vec2(w, 11.0)), uv(), LABEL);
            }
        }
        if focused && selected {
            ui.painter().rect_stroke(cell, CornerRadius::same(4), Stroke::new(1.0f32, FOCUS_LINE), StrokeKind::Inside);
        }
        self.star(ui, id, cell);
    }

    /// A photo's star in the corner of its cell: shown when it has one, and offered under
    /// the pointer when it has none. Clicking it stars this photo alone, or takes its star away.
    fn star(&mut self, ui: &Ui, id: Id, cell: Rect) {
        let starred = self.lib.get(id).is_some_and(|it| it.starred);
        if !starred && !ui.rect_contains_pointer(cell) {
            return;
        }
        let at = Rect::from_center_size(cell.min + vec2(12.0, 12.0), vec2(18.0, 18.0));
        let r = ui.interact(at, egui::Id::new(("star", id)), Sense::click()).on_hover_text("S");
        let colour = if starred || r.hovered() { Color32::WHITE } else { LABEL };
        ui.painter().text(at.center(), Align2::CENTER_CENTER, if starred { "★" } else { "☆" }, FontId::proportional(13.0), colour);
        if r.clicked() {
            self.lib.toggle_starred(&[id]);
        }
    }

    fn grid(&mut self, ui: &mut Ui) {
        let gap = 6.0;
        let cell = self.thumb_pt;
        let named = cell >= 120.0;
        let avail = ui.available_width() - 2.0 * gap;
        let cols = ((avail + gap) / (cell + gap)).floor().max(1.0) as usize;
        self.grid_cols = cols;
        let w = (avail - gap * (cols - 1) as f32) / cols as f32;
        let row_h = w;
        let n = self.lib.shown().len();
        let rows = n.div_ceil(cols);
        let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
        if self.scroll_to_focus {
            if let Some(i) = self.lib.focus.and_then(|f| self.lib.shown_position(f)) {
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
                    let id = self.lib.shown()[i];
                    let x = rect.min.x + gap + c as f32 * (w + gap);
                    let cell = Rect::from_min_size(pos2(x, rect.min.y), vec2(w, row_h));
                    let r = ui.interact(cell, egui::Id::new(("cell", id)), Sense::click());
                    self.clicked(ui, id, &r);
                    if r.double_clicked() {
                        self.lib.click(id);
                        self.mode = Mode::Loupe;
                        self.zoom = None;
                    }
                    let hovered = r.hovered();
                    self.photo_menu(&r, id);
                    if let Status::Failed(e) = &self.lib.get(id).unwrap().status {
                        r.on_hover_text(e.clone());
                    }
                    self.thumb_cell(ui, id, cell, 2, hovered, named);
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
        let area = ui.max_rect().shrink(12.0);
        let aspect = {
            let a: Vec<f32> = ids.iter().filter_map(|&id| self.lib.get(id)).filter(|it| it.original.1 > 0).map(|it| self.tiers.shown(it).1).map(|(w, h)| w as f32 / h as f32).collect();
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
                self.mode = Mode::Loupe;
                self.zoom = None;
            }
            self.photo_menu(&r, id);
            let Some(item) = self.lib.get(id) else { continue };
            if !item.ready() {
                continue;
            }
            let inner = cell.shrink(8.0);
            let b = px(inner, ppp);
            let (origin, (gw, gh), angle) = self.tiers.shown(item);
            let z = ((b[2] - b[0]) as f32 / gw as f32).min((b[3] - b[1]) as f32 / gh as f32);
            let (iw, ih) = (((gw as f32 * z).round() as u32).max(1), ((gh as f32 * z).round() as u32).max(1));
            let img = centred(inner, iw, ih, ppp);
            let focused = self.lib.focus == Some(id);
            let canvas = Canvas { origin, scale: gw as f32 / iw as f32, source: self.split_source(iw), clip: self.clipping, angle };
            let tex = self.tiers.canvas(item, canvas, [iw, ih], if focused { 0 } else { 1 }).or_else(|| self.tiers.thumb_tex(id));
            if let Some(t) = tex {
                ui.painter().image(t, img, uv(), Color32::WHITE);
            }
            self.divider(ui, id, img, false);
            if focused && ids.len() > 1 {
                ui.painter().rect_stroke(img.expand(4.0), CornerRadius::ZERO, Stroke::new(1.5f32, FOCUS_LINE), StrokeKind::Inside);
            }
        }
    }

    fn loupe(&mut self, ui: &mut Ui) {
        let ppp = ui.ctx().pixels_per_point();
        let rect = ui.max_rect();
        let resp = ui.interact(rect, egui::Id::new("loupe"), Sense::click_and_drag());
        let Some(id) = self.lib.focus else { return };
        self.photo_menu(&resp, id);
        if self.cropping.as_ref().is_some_and(|e| e.id == id) {
            self.crop_view(ui, rect);
            return;
        }
        let Some(item) = self.lib.get(id) else { return };
        if !item.ready() {
            return;
        }
        if resp.hovered() {
            ui.ctx().set_cursor_icon(match self.zoom {
                None => CursorIcon::ZoomIn,
                Some(_) if resp.dragged() => CursorIcon::Grabbing,
                Some(_) => CursorIcon::Grab,
            });
        }
        let vp = px(rect, ppp);
        let (vw, vh) = ((vp[2] - vp[0]) as f32, (vp[3] - vp[1]) as f32);
        // positions below are within the part shown; `origin` places it on the full grid
        let (origin, (gw, gh), angle) = self.tiers.shown(item);
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
        if resp.clicked() {
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
        let canvas = Canvas { origin: [origin[0] + c[0] + (x0 as f32 - vc[0]) / z, origin[1] + c[1] + (y0 as f32 - vc[1]) / z], scale: 1.0 / z, source: self.split_source((x1 - x0) as u32), clip: self.clipping, angle };
        if let Some(pos) = self.lib.shown_position(id) {
            for j in [pos + 1, pos.wrapping_sub(1)] {
                if let Some(next) = self.lib.shown().get(j).and_then(|&n| self.lib.get(n)) {
                    self.tiers.prefetch(next);
                }
            }
        }
        let Some(item) = self.lib.get(id) else { return };
        if let Some(t) = self.tiers.canvas(item, canvas, [(x1 - x0) as u32, (y1 - y0) as u32], 0) {
            ui.painter().image(t, shown, uv(), Color32::WHITE);
        } else if let Some(t) = self.tiers.thumb_tex(item.id) {
            // the photo's thumbnail (its shown part), placed where the photo is and cut to the view
            let full = Rect::from_min_max(pos2((vc[0] - c[0] * z) / ppp, (vc[1] - c[1] * z) / ppp), pos2((vc[0] + (gwf - c[0]) * z) / ppp, (vc[1] + (ghf - c[1]) * z) / ppp));
            ui.painter().with_clip_rect(rect).image(t, full, uv(), Color32::WHITE);
        }
        self.divider(ui, id, shown, true);
    }

    fn filmstrip(&mut self, ui: &mut Ui) {
        let h = ui.available_height();
        let gap = 6.0;
        let cell = vec2((h - 2.0 * gap) * 1.25, h - 2.0 * gap);
        let n = self.lib.shown().len();
        let scroll_to = if self.scroll_to_focus { self.lib.focus.and_then(|f| self.lib.shown_position(f)) } else { None };
        self.scroll_to_focus = false;
        egui::ScrollArea::horizontal().auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let total = n as f32 * (cell.x + gap) + gap;
            ui.set_min_size(vec2(total, h));
            let first = ((viewport.min.x - gap) / (cell.x + gap)).floor().max(0.0) as usize;
            let last = (((viewport.max.x) / (cell.x + gap)).ceil() as usize + 1).min(n);
            if let Some(i) = scroll_to {
                let r = Rect::from_min_size(origin + vec2(gap + i as f32 * (cell.x + gap), gap), cell);
                ui.scroll_to_rect(r, Some(egui::Align::Center));
            }
            for i in first..last {
                let id = self.lib.shown()[i];
                let r = Rect::from_min_size(origin + vec2(gap + i as f32 * (cell.x + gap), gap), cell);
                let resp = ui.interact(r, egui::Id::new(("strip", id)), Sense::click());
                self.clicked(ui, id, &resp);
                if resp.clicked() {
                    self.zoom = None;
                }
                self.photo_menu(&resp, id);
                self.thumb_cell(ui, id, r, 3, resp.hovered(), false);
            }
        });
    }
}
