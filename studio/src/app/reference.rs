//! The reference: a picture to work against, such as a scanned page of the book, shown beside
//! the photo being edited. It goes through the pictures added to it, and floats over the main
//! window or stands in a window of its own, where pictures can also be dropped on it.

use super::views::LABEL;
use super::workers::Job;
use super::{App, Dialog};
use egui::{Align, Button, Color32, Layout, RichText, TextureHandle, Ui, ViewportBuilder, ViewportId};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Default)]
pub struct Reference {
    pub pictures: Vec<PathBuf>,
    /// The picture shown, a place in `pictures`.
    pub at: usize,
    pub shown: bool,
    /// In a window of its own rather than over the main window.
    pub apart: bool,
    /// Pictures read, or why they could not be; none yet while one is being read.
    loaded: HashMap<PathBuf, Option<Result<TextureHandle, String>>>,
}

/// Long edge pictures are read at: sharp in a large window, small on the GPU.
pub const LONG: u32 = 2048;

impl Reference {
    fn current(&self) -> Option<&PathBuf> {
        self.pictures.get(self.at.min(self.pictures.len().saturating_sub(1)))
    }

    /// Add pictures not yet among them and show the first of them.
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        let mut first = None;
        for p in paths {
            let i = self.pictures.iter().position(|q| *q == p).unwrap_or_else(|| {
                self.pictures.push(p);
                self.pictures.len() - 1
            });
            first.get_or_insert(i);
        }
        if let Some(i) = first {
            self.at = i;
            self.shown = true;
        }
    }

    pub fn arrived(&mut self, ctx: &egui::Context, path: PathBuf, result: Result<alexander_studio::load::Pixels, String>) {
        let texture = result.map(|px| ctx.load_texture(path.display().to_string(), egui::ColorImage::from_rgba_unmultiplied([px.w as usize, px.h as usize], &px.rgba), egui::TextureOptions::LINEAR));
        self.loaded.insert(path, Some(texture));
    }
}

impl App {
    /// The reference, over the main window or in its own, when it is shown.
    pub(super) fn reference_ui(&mut self, ctx: &egui::Context) {
        if !self.reference.shown {
            return;
        }
        if let Some(p) = self.reference.current().cloned() {
            self.reference.loaded.entry(p.clone()).or_insert_with(|| {
                self.workers.send(Job::Reference(p));
                None
            });
        }
        if self.reference.apart {
            let builder = ViewportBuilder::default().with_title("Reference").with_inner_size([560.0, 440.0]).with_min_inner_size([240.0, 180.0]).with_drag_and_drop(true);
            ctx.show_viewport_immediate(ViewportId::from_hash_of("reference"), builder, |ctx, _| {
                egui::CentralPanel::default().show(ctx, |ui| self.reference_contents(ui));
                let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
                self.reference.add(dropped);
                if ctx.input(|i| i.viewport().close_requested()) {
                    self.reference.shown = false;
                }
            });
        } else {
            let mut open = true;
            let corner = ctx.available_rect().right_top() + egui::vec2(-336.0, 16.0);
            egui::Window::new("Reference").open(&mut open).resizable(true).collapsible(false).default_pos(corner).default_size([320.0, 260.0]).min_size([200.0, 150.0]).show(ctx, |ui| self.reference_contents(ui));
            self.reference.shown &= open;
        }
    }

    fn reference_contents(&mut self, ui: &mut Ui) {
        let n = self.reference.pictures.len();
        ui.horizontal(|ui| {
            if ui.add_enabled(n > 1, Button::new("‹")).clicked() {
                self.reference.at = (self.reference.at + n - 1) % n;
            }
            if ui.add_enabled(n > 1, Button::new("›")).clicked() {
                self.reference.at = (self.reference.at + 1) % n;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(if self.reference.apart { "Pop In" } else { "Pop Out" }).clicked() {
                    self.reference.apart = !self.reference.apart;
                }
                if ui.add_enabled(n > 0, Button::new("Remove")).clicked() {
                    self.reference.pictures.remove(self.reference.at.min(n - 1));
                    self.reference.at = self.reference.at.min(self.reference.pictures.len().saturating_sub(1));
                }
                if ui.button("Add…").clicked() {
                    let d = rfd::AsyncFileDialog::new().add_filter("Pictures", alexander_studio::load::extensions());
                    self.ask(Dialog::Reference, async move { super::paths(d.pick_files().await.into_iter().flatten()) });
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    if let Some(p) = self.reference.current() {
                        let name = p.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
                        ui.add(egui::Label::new(RichText::new(format!("{} of {n} · {name}", self.reference.at.min(n - 1) + 1)).color(LABEL)).truncate());
                    }
                });
            });
        });
        let area = ui.available_rect_before_wrap();
        ui.painter().rect_filled(area, egui::CornerRadius::same(4), super::views::VIEW_BG);
        match self.reference.current().and_then(|p| self.reference.loaded.get(p)) {
            Some(Some(Ok(t))) => {
                let s = (area.width() / t.size_vec2().x).min(area.height() / t.size_vec2().y);
                ui.painter().image(t.id(), egui::Rect::from_center_size(area.center(), t.size_vec2() * s), egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
            }
            Some(Some(Err(e))) => {
                ui.put(area.shrink(12.0), egui::Label::new(RichText::new(e.as_str()).color(LABEL)));
            }
            Some(None) => {
                ui.put(egui::Rect::from_center_size(area.center(), egui::vec2(16.0, 16.0)), egui::Spinner::new().color(LABEL));
            }
            None if self.reference.apart => {
                ui.put(area.shrink(12.0), egui::Label::new(RichText::new("Drop pictures here").color(LABEL)));
            }
            None => {}
        }
        ui.allocate_rect(area, egui::Sense::hover());
    }
}
