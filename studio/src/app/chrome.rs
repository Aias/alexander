//! The window's chrome: the toolbar, the status bar under the view, and the dialogs.

use super::views::{segmented, LABEL};
use super::{controls, paths, App, Compare, Dialog, Mode, SizeChoice};
use alexander_studio::export::Format;
use egui::{vec2, Align, Align2, Button, FontId, Key, Layout, Margin, Rect, RichText, Ui};

const DIALOG_BUTTON: egui::Vec2 = vec2(84.0, 26.0);

fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    egui::Frame::window(&ctx.style()).inner_margin(Margin::same(18))
}

/// Buttons along the right edge of a dialog, first given rightmost.
fn dialog_buttons(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(14.0);
    ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), add));
}

impl App {
    pub(super) fn toolbar(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(10, 0));
        egui::TopBottomPanel::top("toolbar").exact_height(40.0).frame(frame).show(ctx, |ui| {
            let bar = ui.max_rect();
            ui.horizontal_centered(|ui| {
                if ui.button("Open…").on_hover_text("⌘O").clicked() {
                    self.import_dialog();
                }
                #[cfg(target_os = "macos")]
                if ui.button("Open from Photos…").on_hover_text("⇧⌘O").clicked() {
                    self.photos_dialog();
                }
                if self.lib.is_empty() {
                    return;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let n = self.exportable();
                    if ui.add_enabled(n > 0 && self.export_run.is_none(), Button::new("Export…")).on_hover_text("⇧⌘E").clicked() {
                        self.open_export();
                    }
                    if let Some(run) = &self.export_run {
                        ui.add(egui::ProgressBar::new(run.done as f32 / run.total.max(1) as f32).desired_width(110.0).desired_height(5.0));
                        ui.label(RichText::new(format!("Exporting {} of {}", (run.done + 1).min(run.total), run.total)).color(LABEL));
                    }
                });
            });
            if !self.lib.is_empty() {
                let before = self.mode;
                let views = [(Mode::Grid, "Grid", "G"), (Mode::Survey, "Survey", "N"), (Mode::Loupe, "Loupe", "E")];
                ui.put(Rect::from_center_size(bar.center(), vec2(228.0, 26.0)), |ui: &mut Ui| segmented(ui, &mut self.mode, &views));
                if self.mode != before {
                    self.scroll_to_focus = true;
                    self.zoom = None;
                }
            }
        });
    }

    /// Under the view: what is open and selected, and the view's own settings.
    pub(super) fn status_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            match self.mode {
                Mode::Grid => {
                    let mut size = self.thumb_pt as f64;
                    controls::slider(ui, &mut size, 80.0..=480.0, 110.0).on_hover_text("⌘+ / ⌘−");
                    self.thumb_pt = size as f32;
                }
                Mode::Loupe => {
                    if self.zoom != Some(1.0) {
                        ui.label(RichText::new(format!("{:.0}%", self.current_zoom() * 100.0)).color(LABEL));
                    }
                    if ui.selectable_label(self.zoom == Some(1.0), "100%").on_hover_text("⌘1").clicked() {
                        self.zoom_100();
                    }
                    if ui.selectable_label(self.zoom.is_none(), "Fit").on_hover_text("⌘0").clicked() {
                        self.zoom = None;
                    }
                }
                Mode::Survey => {}
            }
            ui.add_space(8.0);
            if self.compare == Compare::Split && ui.selectable_label(self.split_swap, "Swap").clicked() {
                self.split_swap = !self.split_swap;
            }
            for (c, label, keys) in [(Compare::Split, "Split", "Y"), (Compare::Before, "Before", "\\")] {
                if ui.selectable_label(self.compare == c, label).on_hover_text(keys).clicked() {
                    self.toggle_compare(c);
                }
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let mut starred_only = self.lib.starred_only();
                if ui.toggle_value(&mut starred_only, "★").on_hover_text("Starred photos only").clicked() {
                    self.lib.set_starred_only(starred_only);
                }
                if self.opening > 0 || self.copy_image.is_some() {
                    ui.add(egui::Spinner::new().size(12.0).color(LABEL));
                }
                let n = self.lib.len();
                let mut info = format!("{n} {}", if n == 1 { "photo" } else { "photos" });
                if starred_only {
                    info = format!("{} of {info}", self.lib.shown().len());
                }
                let k = self.lib.selected().len();
                if k > 0 {
                    info += &format!(" / {k} selected");
                }
                if let Some(it) = self.lib.focus.and_then(|f| self.lib.get(f)) {
                    info += &format!(" / {}", it.name);
                }
                ui.add(egui::Label::new(RichText::new(info).color(LABEL)).truncate());
            });
        });
    }

    pub(super) fn empty(&mut self, ui: &mut Ui) {
        let c = ui.max_rect().center();
        let big = |text: &str| Button::new(RichText::new(text).size(14.0)).min_size(vec2(168.0, 34.0));
        let row = Rect::from_center_size(c, vec2(if cfg!(target_os = "macos") { 346.0 } else { 168.0 }, 34.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(row).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if ui.add(big("Open…")).clicked() {
                self.import_dialog();
            }
            #[cfg(target_os = "macos")]
            if ui.add(big("Open from Photos…")).clicked() {
                self.photos_dialog();
            }
        });
        ui.painter().text(c + vec2(0.0, 36.0), Align2::CENTER_CENTER, "or drop photos and folders here", FontId::proportional(12.0), LABEL);
    }

    pub(super) fn export_window(&mut self, ctx: &egui::Context) {
        let Some(mut form) = self.export_form.take() else { return };
        let n = self.exportable();
        let ready = n > 0 && form.dir_ok;
        let (mut go, mut cancel, mut choose) = (false, false, None);
        let modal = egui::Modal::new(egui::Id::new("export")).frame(dialog_frame(ctx)).show(ctx, |ui| {
            ui.set_width(340.0);
            ui.heading(if n == 1 { "Export 1 Photo".to_string() } else { format!("Export {n} Photos") });
            ui.add_space(12.0);
            egui::Grid::new("export").num_columns(2).spacing([12.0, 10.0]).show(ui, |ui| {
                ui.label("Folder");
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Choose…").clicked() {
                            choose = Some(rfd::AsyncFileDialog::new().set_directory(&form.dir));
                        }
                        let name = form.dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| form.dir.display().to_string());
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| ui.add(egui::Label::new(name).truncate()).on_hover_text(form.dir.display().to_string()));
                    });
                });
                ui.end_row();
                ui.label("Format");
                egui::ComboBox::from_id_salt("format").width(ui.available_width()).selected_text(form.format.label()).show_ui(ui, |ui| {
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
                    let w = if form.size == SizeChoice::Custom { ui.available_width() - 86.0 } else { ui.available_width() };
                    egui::ComboBox::from_id_salt("size").width(w).selected_text(label).show_ui(ui, |ui| {
                        ui.selectable_value(&mut form.size, SizeChoice::Original, "Original");
                        ui.selectable_value(&mut form.size, SizeChoice::Grid, "Simulation grid");
                        ui.selectable_value(&mut form.size, SizeChoice::Custom, "Long edge");
                    });
                    if form.size == SizeChoice::Custom {
                        ui.add_sized([80.0, ui.spacing().interact_size.y], egui::DragValue::new(&mut form.long).range(64..=30000).suffix(" px"));
                    }
                });
                ui.end_row();
            });
            dialog_buttons(ui, |ui| {
                go = ui.add_enabled(ready, Button::new("Export").min_size(DIALOG_BUTTON).fill(egui::Color32::from_gray(92))).clicked();
                cancel = ui.add(Button::new("Cancel").min_size(DIALOG_BUTTON)).clicked();
            });
        });
        if let Some(d) = choose {
            self.ask(Dialog::ExportFolder, async move { paths(d.pick_folder().await) });
        }
        go |= ready && ctx.input(|i| i.key_pressed(Key::Enter));
        if go {
            self.last_dir = Some(form.dir.clone());
            self.start_export(form);
        } else if !cancel && !modal.should_close() {
            self.export_form = Some(form);
        }
    }

    pub(super) fn errors_window(&mut self, ctx: &egui::Context) {
        if self.export_errors.is_empty() {
            return;
        }
        let mut close = false;
        let modal = egui::Modal::new(egui::Id::new("errors")).frame(dialog_frame(ctx)).show(ctx, |ui| {
            ui.set_max_width(480.0);
            for e in self.export_errors.iter().take(12) {
                ui.label(e);
            }
            if self.export_errors.len() > 12 {
                ui.label(RichText::new(format!("and {} more", self.export_errors.len() - 12)).color(LABEL));
            }
            dialog_buttons(ui, |ui| close = ui.add(Button::new("OK").min_size(DIALOG_BUTTON)).clicked());
        });
        if close || modal.should_close() || ctx.input(|i| i.key_pressed(Key::Enter)) {
            self.export_errors.clear();
        }
    }
}
