//! Presets: looks kept under a name and given to the selected photos with a click. Four come
//! with the app, from the looks the README describes; the rest are the user's own, kept as
//! look files in the app's data folder, each with a description if it was given one.
//! Resting the pointer on one shows it on the focused photo without giving it.

use super::theme::{body_font, CORNER, LABEL, STRONG};
use super::{controls, session, App};
use alexander_studio::params::{Repro, Settings};
use egui::{vec2, Align2, Button, Key, Sense, TextEdit, Ui};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub struct Preset {
    pub name: String,
    pub description: String,
    pub look: Settings,
    /// The file of a preset of the user's own.
    pub file: Option<PathBuf>,
}

#[derive(Serialize, Deserialize)]
struct Stored {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    description: String,
    #[serde(flatten)]
    look: Settings,
}

#[derive(Default)]
pub struct Draft {
    name: String,
    description: String,
    editing: Option<(PathBuf, Settings)>,
}

fn dir() -> PathBuf {
    session::data_dir().join("Presets")
}

/// The app's presets, then the user's own by name.
pub fn load() -> Vec<Preset> {
    let d = Settings::default();
    let builtin = [
        ("Default", "A darkroom print: Tri-X printed on grade 3 paper, its tones brought to the range the book's pages share", d.clone()),
        ("Harder", "More contrast than the default: grade 4.25 paper and a fuller print exposure", Settings { grade: 4.25, print_ev: 0.0, ..d.clone() }),
        ("Copied from a Page", "The print printed in a book, photographed from the page and printed again, as the book's own pictures were made", Settings { repro: Repro::Copy, ..d.clone() }),
        ("Book Page", "The 1979 press on uncoated paper: weak ink, compressed tones, dot gain and softness, seen as a scan of the page", Settings { repro: Repro::Book, ..d }),
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
            let Stored { description, look } = serde_json::from_slice(&std::fs::read(&file).ok()?).ok()?;
            Some(Preset { name: file.file_stem()?.to_string_lossy().into_owned(), description, look, file: Some(file) })
        })
        .collect();
    own.sort_by_key(|p| p.name.to_lowercase());
    builtin.into_iter().map(|(name, description, look)| Preset { name: name.into(), description: description.into(), look, file: None }).chain(own).collect()
}

fn file(name: &str) -> PathBuf {
    let name: String = name.trim().chars().map(|c| if matches!(c, '/' | ':' | '\\') { '-' } else { c }).collect();
    dir().join(format!("{name}.json"))
}

/// Keep `look` as the preset in `file`, in place of what is there.
fn save(file: &Path, description: &str, look: &Settings) -> std::io::Result<()> {
    std::fs::create_dir_all(dir())?;
    let stored = Stored { description: description.trim().into(), look: look.clone() };
    std::fs::write(file, serde_json::to_string_pretty(&stored).unwrap_or_default())
}

impl App {
    pub(super) fn presets_ui(&mut self, ui: &mut Ui) {
        let focus = self.lib.focus.filter(|f| self.lib.selected().contains(f));
        let focus_look = focus.and_then(|f| self.lib.get(f)).and_then(|it| it.look.clone());
        controls::section(ui, "Presets", false, None, |ui| {
            let mut apply = None;
            let mut changed = false;
            for p in &self.presets {
                let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click());
                if r.hovered() {
                    ui.painter().rect_filled(rect, CORNER, ui.visuals().widgets.hovered.weak_bg_fill);
                    self.preview_wanted = focus.map(|f| (f, p.look.clone()));
                }
                ui.painter().text(rect.left_center() + vec2(6.0, 0.0), Align2::LEFT_CENTER, &p.name, body_font(), if r.hovered() { STRONG } else { LABEL });
                let r = if p.description.is_empty() { r } else { r.on_hover_text(&p.description) };
                if r.clicked() {
                    apply = Some(p.look.clone());
                }
                let Some(file) = &p.file else { continue };
                r.context_menu(|ui| {
                    if ui.add_enabled(focus_look.is_some(), Button::new("Update with Current Look")).clicked() {
                        changed |= focus_look.as_ref().is_some_and(|l| save(file, &p.description, l).is_ok());
                        ui.close();
                    }
                    if ui.button("Rename and Describe…").clicked() {
                        self.new_preset = Some(Draft { name: p.name.clone(), description: p.description.clone(), editing: Some((file.clone(), p.look.clone())) });
                        ui.close();
                    }
                    #[cfg(target_os = "macos")]
                    if ui.button("Show in Finder").clicked() {
                        let file = file.clone();
                        std::thread::spawn(move || std::process::Command::new("/usr/bin/open").arg("-R").arg(file).status());
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        changed |= std::fs::remove_file(file).is_ok();
                        ui.close();
                    }
                });
            }
            ui.add_space(4.0);
            match &mut self.new_preset {
                None => {
                    if ui.add_enabled(focus_look.is_some(), Button::new("New Preset…")).clicked() {
                        self.new_preset = Some(Draft::default());
                    }
                }
                Some(draft) => {
                    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
                    let mut done = escape;
                    let field = ui.add(TextEdit::singleline(&mut draft.name).hint_text("Name").desired_width(f32::INFINITY));
                    if !field.has_focus() && !enter && draft.name.is_empty() {
                        field.request_focus();
                    }
                    ui.horizontal(|ui| {
                        ui.add(TextEdit::singleline(&mut draft.description).hint_text("Description").desired_width(ui.available_width() - 70.0));
                        let named = !draft.name.trim().is_empty();
                        let look = draft.editing.as_ref().map(|(_, l)| l).or(focus_look.as_ref());
                        if ui.add_enabled(named, Button::new("Save")).clicked() || enter && named {
                            let to = file(&draft.name);
                            if let Some((old, _)) = &draft.editing {
                                let _ = std::fs::rename(old, &to);
                            }
                            if look.is_some_and(|l| save(&to, &draft.description, l).is_ok()) {
                                changed = true;
                                done = true;
                            }
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
