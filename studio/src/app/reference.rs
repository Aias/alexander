//! The reference: a picture to work against, such as a scanned page of the book, shown beside
//! the photo being edited. Its pictures are the ones in the References folder of the app's
//! data folder, by name; pictures added to it are copied there. It floats over the main window
//! or stands in a window of its own, where pictures can also be dropped on it.

use super::views::LABEL;
use super::workers::Job;
use super::{session, App, Dialog};
use egui::{Align, Button, Color32, CursorIcon, Layout, Pos2, Rect, RichText, Sense, TextureHandle, Ui, Vec2, ViewportBuilder, ViewportId};
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Reference {
    pub pictures: Vec<PathBuf>,
    /// The picture shown, a place in `pictures`.
    pub at: usize,
    pub shown: bool,
    /// In a window of its own rather than over the main window.
    pub apart: bool,
    /// Points to a picture pixel, and the picture point at the middle of the view; none
    /// while the picture fits the view.
    pub zoom: Option<(f32, Pos2)>,
    /// Pictures read, or why they could not be; none yet while one is being read.
    loaded: HashMap<PathBuf, Option<Result<TextureHandle, String>>>,
}

/// Longest edge a picture is read at, the most a texture holds.
pub const LONG: u32 = 8192;

fn folder() -> PathBuf {
    session::data_dir().join("References")
}

/// The pictures in the folder, by name.
fn listed() -> Vec<PathBuf> {
    let picture = |p: &PathBuf| p.extension().and_then(|e| e.to_str()).is_some_and(|e| alexander_studio::load::extensions().iter().any(|x| x.eq_ignore_ascii_case(e)));
    let mut pictures: Vec<PathBuf> = std::fs::read_dir(folder()).into_iter().flatten().flatten().map(|e| e.path()).filter(picture).collect();
    pictures.sort();
    pictures
}

impl Reference {
    pub fn new() -> Reference {
        Reference { pictures: listed(), at: 0, shown: false, apart: false, zoom: None, loaded: HashMap::new() }
    }

    fn show(&mut self, at: usize) {
        self.at = at;
        self.zoom = None;
    }

    fn current(&self) -> Option<&PathBuf> {
        self.pictures.get(self.at.min(self.pictures.len().saturating_sub(1)))
    }

    /// Copy pictures into the folder, in place of any of the same name, and show the first.
    pub fn add(&mut self, paths: Vec<PathBuf>) {
        let _ = std::fs::create_dir_all(folder());
        let mut first = None;
        for p in paths {
            let Some(to) = p.file_name().map(|n| folder().join(n)) else { continue };
            if to == p || std::fs::copy(&p, &to).is_ok() {
                self.loaded.remove(&to);
                first.get_or_insert(to);
            }
        }
        self.pictures = listed();
        if let Some(i) = first.and_then(|f| self.pictures.iter().position(|p| *p == f)) {
            self.show(i);
            self.shown = true;
        }
    }

    /// Move the picture shown to the Trash, or on other systems delete it.
    fn remove(&mut self) {
        let Some(p) = self.current().cloned() else { return };
        #[cfg(target_os = "macos")]
        let gone = objc2_foundation::NSURL::from_file_path(&p).is_some_and(|url| objc2_foundation::NSFileManager::defaultManager().trashItemAtURL_resultingItemURL_error(&url, None).is_ok());
        #[cfg(not(target_os = "macos"))]
        let gone = std::fs::remove_file(&p).is_ok();
        if gone {
            self.loaded.remove(&p);
            self.pictures = listed();
            self.show(self.at.min(self.pictures.len().saturating_sub(1)));
        }
    }

    pub fn arrived(&mut self, ctx: &egui::Context, path: PathBuf, result: Result<alexander_studio::load::Pixels, String>) {
        let texture = result.map(|px| ctx.load_texture(path.display().to_string(), egui::ColorImage::from_rgba_unmultiplied([px.w as usize, px.h as usize], &px.rgba), egui::TextureOptions::LINEAR));
        self.loaded.insert(path, Some(texture));
    }
}

impl App {
    /// The reference, over the main window or in its own, when it is shown.
    pub(super) fn reference_ui(&mut self, ctx: &egui::Context, area: egui::Rect) {
        if !self.reference.shown {
            return;
        }
        let n = self.reference.pictures.len();
        let at = self.reference.at.min(n.saturating_sub(1));
        let near: Vec<PathBuf> = (0..n.min(3)).map(|k| self.reference.pictures[(at + n + k - 1) % n].clone()).collect();
        self.reference.loaded.retain(|p, _| near.contains(p));
        for p in near {
            self.reference.loaded.entry(p.clone()).or_insert_with(|| {
                self.workers.send(Job::Reference(p));
                None
            });
        }
        if self.reference.apart {
            let builder = ViewportBuilder::default().with_title("Reference").with_inner_size([560.0, 440.0]).with_min_inner_size([240.0, 180.0]).with_drag_and_drop(true);
            ctx.show_viewport_immediate(ViewportId::from_hash_of("reference"), builder, |ui, _| {
                egui::CentralPanel::default().show(ui, |ui| self.reference_contents(ui));
                let dropped: Vec<PathBuf> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
                self.reference.add(dropped);
                if ui.input(|i| i.viewport().close_requested()) {
                    self.reference.shown = false;
                }
            });
        } else {
            let mut open = true;
            let corner = area.right_top() + egui::vec2(-336.0, 16.0);
            egui::Window::new("Reference").open(&mut open).resizable(true).collapsible(false).default_pos(corner).default_size([320.0, 260.0]).min_size([200.0, 150.0]).show(ctx, |ui| self.reference_contents(ui));
            self.reference.shown &= open;
        }
    }

    fn reference_contents(&mut self, ui: &mut Ui) {
        let n = self.reference.pictures.len();
        ui.horizontal(|ui| {
            if ui.add_enabled(n > 1, Button::new("‹")).clicked() {
                self.reference.show((self.reference.at + n - 1) % n);
            }
            if ui.add_enabled(n > 1, Button::new("›")).clicked() {
                self.reference.show((self.reference.at + 1) % n);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button(if self.reference.apart { "Pop In" } else { "Pop Out" }).clicked() {
                    self.reference.apart = !self.reference.apart;
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
        let r = ui.allocate_rect(area, Sense::click_and_drag());
        ui.painter().rect_filled(area, egui::CornerRadius::same(4), super::views::VIEW_BG);
        match self.reference.current().and_then(|p| self.reference.loaded.get(p)) {
            Some(Some(Ok(t))) => {
                let size = t.size_vec2();
                let (z, c) = zoomed(ui, &r, area, size, &mut self.reference.zoom);
                let shown = Rect::from_center_size(area.center() + (size / 2.0 - c.to_vec2()) * z, size * z);
                ui.painter_at(area).image(t.id(), shown, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
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
        if n == 0 {
            return;
        }
        r.context_menu(|ui| {
            #[cfg(target_os = "macos")]
            if let (true, Some(p)) = (ui.button("Show in Finder").clicked(), self.reference.current().cloned()) {
                std::thread::spawn(move || std::process::Command::new("/usr/bin/open").arg("-R").arg(p).status());
                ui.close();
            }
            if ui.button("Move to Trash").clicked() {
                self.reference.remove();
                ui.close();
            }
        });
    }
}

fn zoomed(ui: &Ui, r: &egui::Response, area: Rect, size: Vec2, zoom: &mut Option<(f32, Pos2)>) -> (f32, Pos2) {
    let fit = (area.width() / size.x).min(area.height() / size.y);
    let actual = 1.0 / ui.ctx().pixels_per_point();
    let (mut z, mut c) = zoom.unwrap_or((fit, (size / 2.0).to_pos2()));
    let mut on = zoom.is_some();
    if let Some(h) = r.hover_pos() {
        let (scroll, factor) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
        c -= scroll / z;
        if (factor - 1.0).abs() > 1e-4 {
            let g = c + (h - area.center()) / z;
            z = (z * factor).clamp(fit.min(actual) * 0.5, 16.0 * actual);
            c = g - (h - area.center()) / z;
            on = true;
        }
    }
    if r.dragged() {
        c -= r.drag_delta() / z;
    }
    if r.clicked() {
        on = !on;
        if let (true, Some(h)) = (on, r.interact_pointer_pos()) {
            let g = c + (h - area.center()) / z;
            z = actual;
            c = g - (h - area.center()) / z;
        }
    }
    if r.hovered() {
        ui.ctx().set_cursor_icon(if !on { CursorIcon::ZoomIn } else if r.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab });
    }
    if !on {
        *zoom = None;
        return (fit, (size / 2.0).to_pos2());
    }
    for i in 0..2 {
        let half = area.size()[i] / (2.0 * z);
        c[i] = if size[i] <= 2.0 * half { size[i] / 2.0 } else { c[i].clamp(half, size[i] - half) };
    }
    *zoom = Some((z, c));
    (z, c)
}
