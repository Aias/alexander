//! Presets: looks kept under a name and given to the selected photos with a click. Four come
//! with the app, from the looks the README describes; the rest are the user's own, kept as
//! look files in the app's data folder. Resting the pointer on one shows it on the focused
//! photo without giving it.

use super::views::{LABEL, TEXT};
use super::{controls, session, App};
use alexander_studio::params::{Repro, Settings, View};
use egui::{vec2, Align2, Button, CornerRadius, FontId, Key, Sense, TextEdit, Ui};
use std::path::PathBuf;

pub struct Preset {
    pub name: String,
    pub look: Settings,
    /// The file of a preset of the user's own.
    pub file: Option<PathBuf>,
}

fn dir() -> PathBuf {
    session::data_dir().join("Presets")
}

/// The app's presets, then the user's own by name.
pub fn load() -> Vec<Preset> {
    let d = Settings::default();
    let builtin = [
        ("Default", d.clone()),
        ("Harder", Settings { grade: 4.25, print_ev: 0.0, ..d.clone() }),
        ("Copied from a Page", Settings { repro: Repro::Copy, ..d.clone() }),
        ("Book Page", Settings { repro: Repro::Book, view: Some(View::Page), ..d }),
    ];
    let mut own: Vec<Preset> = std::fs::read_dir(dir())
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let file = e.path();
            if file.extension()? != "json" {
                return None;
            }
            let look = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            Some(Preset { name: file.file_stem()?.to_string_lossy().into_owned(), look, file: Some(file) })
        })
        .collect();
    own.sort_by_key(|p| p.name.to_lowercase());
    builtin.into_iter().map(|(name, look)| Preset { name: name.into(), look, file: None }).chain(own).collect()
}

/// Keep `look` as the preset `name`, in place of one of the same name.
fn save(name: &str, look: &Settings) -> std::io::Result<()> {
    let name: String = name.trim().chars().map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c }).collect();
    std::fs::create_dir_all(dir())?;
    std::fs::write(dir().join(format!("{name}.json")), serde_json::to_string_pretty(look).unwrap_or_default())
}

impl App {
    pub(super) fn presets_ui(&mut self, ui: &mut Ui) {
        let focus = self.lib.focus.filter(|f| self.lib.selected().contains(f));
        let focus_look = focus.and_then(|f| self.lib.get(f)).and_then(|it| it.look.clone());
        controls::section(ui, "Presets", true, None, |ui| {
            let mut apply = None;
            let mut changed = false;
            for p in &self.presets {
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, CornerRadius::same(4), ui.visuals().widgets.hovered.weak_bg_fill);
                    self.preview_wanted = focus.map(|f| (f, p.look.clone()));
                }
                ui.painter().text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, &p.name, FontId::proportional(12.5), if r.hovered() { TEXT } else { LABEL });
                if r.clicked() {
                    apply = Some(p.look.clone());
                }
                let Some(file) = &p.file else { continue };
                r.context_menu(|ui| {
                    if ui.add_enabled(focus_look.is_some(), Button::new("Update with Current Look")).clicked() {
                        changed |= focus_look.as_ref().is_some_and(|l| save(&p.name, l).is_ok());
                        ui.close_menu();
                    }
                    #[cfg(target_os = "macos")]
                    if ui.button("Show in Finder").clicked() {
                        let file = file.clone();
                        std::thread::spawn(move || std::process::Command::new("/usr/bin/open").arg("-R").arg(file).status());
                        ui.close_menu();
                    }
                    if ui.button("Delete").clicked() {
                        changed |= std::fs::remove_file(file).is_ok();
                        ui.close_menu();
                    }
                });
            }
            ui.add_space(4.0);
            match &mut self.new_preset {
                None => {
                    if ui.add_enabled(focus_look.is_some(), Button::new("New Preset…")).clicked() {
                        self.new_preset = Some(String::new());
                    }
                }
                Some(name) => {
                    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    let mut done = escape;
                    ui.horizontal(|ui| {
                        let field = ui.add(TextEdit::singleline(name).hint_text("Name").desired_width(ui.available_width() - 70.0));
                        if !field.has_focus() && !enter && name.is_empty() {
                            field.request_focus();
                        }
                        let named = !name.trim().is_empty();
                        if (ui.add_enabled(named, Button::new("Save")).clicked() || enter && named) && focus_look.as_ref().is_some_and(|l| save(name, l).is_ok()) {
                            changed = true;
                            done = true;
                        }
                    });
                    if done {
                        self.new_preset = None;
                    }
                }
            }
            if let Some(look) = apply {
                self.set_looks(look);
            }
            if changed {
                self.presets = load();
            }
        });
    }
}
