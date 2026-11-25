//! A scripted run for checking the app without a person at it: with `STUDIO_SHOTS=<dir>`
//! set, the app opens the photos given on the command line, walks through its views,
//! saves a screenshot of each into the folder, exports the selection there and quits.

use super::{App, ExportForm, Mode, SizeChoice};
use alexander_studio::export::Format;
use alexander_studio::params::Settings;
use std::path::PathBuf;

pub struct Shots {
    dir: PathBuf,
    paths: Vec<PathBuf>,
    step: usize,
    frames: u32,
    idle: u32,
    waiting_shot: Option<String>,
}

impl Shots {
    pub fn from_env(paths: Vec<PathBuf>) -> Option<Shots> {
        let dir = PathBuf::from(std::env::var_os("STUDIO_SHOTS")?);
        std::fs::create_dir_all(&dir).ok()?;
        Some(Shots { dir, paths, step: 0, frames: 0, idle: 0, waiting_shot: None })
    }

    fn shot(&mut self, ctx: &egui::Context, name: &str) {
        ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        self.waiting_shot = Some(name.to_string());
    }

    pub fn step(&mut self, app: &mut App, ctx: &egui::Context) {
        self.frames += 1;
        if let Some(name) = self.waiting_shot.clone() {
            let img = ctx.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Screenshot { image, .. } => Some(image.clone()),
                    _ => None,
                })
            });
            if let Some(img) = img {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_array()).collect();
                let path = self.dir.join(format!("{name}.png"));
                if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, bytes) {
                    let _ = buf.save(&path);
                    eprintln!("shot {}", path.display());
                }
                self.waiting_shot = None;
                self.step += 1;
                self.idle = 0;
            }
            return;
        }
        // a few quiet frames in a row: everything asked for has been drawn
        if app.idle() {
            self.idle += 1;
        } else {
            self.idle = 0;
        }
        let settled = self.idle >= 4;
        match self.step {
            0 if self.frames > 5 => self.shot(ctx, "1-empty"),
            1 => {
                app.import(std::mem::take(&mut self.paths));
                self.step += 1;
            }
            2 if settled && !app.lib.is_empty() => {
                // a few photos selected, three of them given the process
                let ids: Vec<u64> = app.lib.items().iter().map(|it| it.id).collect();
                app.lib.click(ids[0]);
                for &id in ids.iter().take(4).skip(1) {
                    app.lib.toggle(id);
                }
                app.lib.focus = Some(ids[0]);
                let processed: Vec<u64> = ids.iter().take(3).copied().collect();
                app.lib.set_look(&processed, Some(Settings::default()));
                app.mode = Mode::Grid;
                self.step += 1;
            }
            3 if settled => self.shot(ctx, "2-grid"),
            4 => {
                app.mode = Mode::Survey;
                self.step += 1;
            }
            5 if settled => self.shot(ctx, "3-selected"),
            6 => {
                app.mode = Mode::Single;
                app.panel = true;
                app.zoom = None;
                self.step += 1;
            }
            7 if settled => self.shot(ctx, "4-single"),
            8 => {
                app.zoom = Some(1.0);
                app.center = [0.45, 0.5];
                self.step += 1;
            }
            9 if settled => self.shot(ctx, "5-single-100"),
            10 => {
                // a late-stage knob: only the print and what follows should be redone
                let id = app.lib.focus.unwrap();
                let before = app.lib.get(id).unwrap().look.clone().unwrap();
                let after = Settings { grade: before.grade + 1.0, ..before.clone() };
                app.lib.apply_change(&[id], &before, &after);
                app.zoom = None;
                self.step += 1;
            }
            11 if settled => {
                eprintln!("grade change on the focused photo redid from: {:?}", app.tiers.last_focus_stage);
                self.shot(ctx, "6-grade")
            }
            12 => {
                let dir = self.dir.join("export");
                let _ = std::fs::create_dir_all(&dir);
                app.start_export(ExportForm { dir, dir_ok: true, format: Format::Png8, size: SizeChoice::Original, long: 0 });
                self.step += 1;
            }
            13 if settled => {
                eprintln!("export finished; errors: {:?}", app.export_errors);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                self.step += 1;
            }
            _ => {}
        }
    }
}
