//! The studio window: library, views, controls, import and export.

mod controls;
mod shots;
mod tiers;
mod views;
mod workers;

use alexander_studio::engine::Engine;
use alexander_studio::export::{self, Format, Size};
use alexander_studio::gpu::Precision;
use alexander_studio::library::{Id, Library, Status};
use alexander_studio::load;
use alexander_studio::params::Settings;
use alexander_studio::plan::Planner;
use alexander_studio::provenance::Record;
use egui::{Key, Modifiers};
use std::collections::{HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use tiers::Tiers;
use workers::{Done, Job, Workers};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Grid,
    Survey,
    Single,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SizeChoice {
    Original,
    Grid,
    Custom,
}

struct ExportForm {
    dir: PathBuf,
    format: Format,
    size: SizeChoice,
    long: u32,
}

struct ExportRun {
    queue: VecDeque<Id>,
    form: ExportForm,
    total: usize,
    done: usize,
    encoding: usize,
    failed: Vec<String>,
    claimed: HashSet<PathBuf>,
    planner: Planner,
}

pub struct App {
    lib: Library,
    tiers: Tiers,
    workers: Workers,
    mode: Mode,
    panel: bool,
    thumb_pt: f32,
    before: bool,
    clipboard: Option<Settings>,
    /// An edit to the looks is under way (a slider held): one undo step for all of it.
    edit_open: bool,
    /// Single view: None fits the photo; otherwise screen pixels per grid pixel.
    zoom: Option<f32>,
    /// Single view: the point of the photo at the middle of the view, as fractions.
    center: [f32; 2],
    scroll_to_focus: bool,
    grid_cols: usize,
    grid_rows_seen: (usize, usize),
    grid_view_h: f32,
    opening: usize,
    pending: bool,
    export_form: Option<ExportForm>,
    export_run: Option<ExportRun>,
    export_errors: Vec<String>,
    last_dir: Option<PathBuf>,
    shots: Option<shots::Shots>,
    frame_times: VecDeque<f32>,
    last_fit: f32,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, paths: Vec<PathBuf>) -> App {
        let rs = cc.wgpu_render_state.as_ref().expect("the wgpu renderer");
        let engine = Engine::new(rs.device.clone(), rs.queue.clone(), Precision::Half);
        let tiers = Tiers::new(engine, rs.device.clone(), rs.renderer.clone());
        views::style(&cc.egui_ctx);
        let ctx = cc.egui_ctx.clone();
        let workers = Workers::new(move || ctx.request_repaint());
        let shots = shots::Shots::from_env(paths.clone());
        let mut app = App {
            lib: Library::new(),
            tiers,
            workers,
            mode: Mode::Grid,
            panel: true,
            thumb_pt: 168.0,
            before: false,
            clipboard: None,
            edit_open: false,
            zoom: None,
            center: [0.5, 0.5],
            scroll_to_focus: false,
            grid_cols: 1,
            grid_rows_seen: (0, 0),
            grid_view_h: 0.0,
            opening: 0,
            pending: false,
            export_form: None,
            export_run: None,
            export_errors: Vec::new(),
            last_dir: None,
            shots,
            frame_times: VecDeque::new(),
            last_fit: 1.0,
        };
        if app.shots.is_none() && !paths.is_empty() {
            app.import(paths);
        }
        app
    }

    // ---------- import ---------------------------------------------------------------------

    fn import(&mut self, paths: Vec<PathBuf>) {
        if !paths.is_empty() {
            self.opening += 1;
            self.workers.send(Job::Collect(paths));
        }
    }

    fn import_dialog(&mut self) {
        let mut d = rfd::FileDialog::new().add_filter("Photos", load::extensions());
        if let Some(dir) = &self.last_dir {
            d = d.set_directory(dir);
        }
        if let Some(files) = d.pick_files() {
            self.last_dir = files.first().and_then(|f| f.parent().map(|p| p.to_path_buf()));
            self.import(files);
        }
    }

    fn poll(&mut self) {
        while let Ok(done) = self.workers.rx.try_recv() {
            match done {
                Done::Collected(paths) => {
                    self.opening -= 1;
                    let mut first = None;
                    for p in paths {
                        if let Some(id) = self.lib.add(p.clone()) {
                            first.get_or_insert(id);
                            self.opening += 1;
                            self.workers.send(Job::Open { id, path: p });
                        }
                    }
                    if let (Some(id), true) = (first, self.lib.selected.is_empty()) {
                        self.lib.click(id);
                    }
                }
                Done::Opened { id, result } => {
                    self.opening -= 1;
                    self.opened(id, result);
                }
                Done::Full { id, long, result } => match result {
                    Ok(px) if self.lib.get(id).is_some() => self.tiers.full_arrived(id, long, px),
                    _ => {
                        self.tiers.loading.remove(&(id, long));
                    }
                },
                Done::Encoded { path, result } => {
                    if let Some(run) = &mut self.export_run {
                        run.encoding -= 1;
                        run.done += 1;
                        if let Err(e) = result {
                            run.failed.push(format!("{}: {e}", path.display()));
                        }
                    }
                }
            }
        }
    }

    fn opened(&mut self, id: Id, result: Result<Box<load::Opened>, String>) {
        let Some(it) = self.lib.get(id) else { return };
        match result {
            Err(e) => self.lib.get_mut(id).unwrap().status = Status::Failed(e),
            Ok(o) => {
                // an export of ours came back as its original: one entry per original
                if o.path != it.path {
                    if let Some(other) = self.lib.find_path(&o.path) {
                        if o.look.is_some() {
                            self.lib.checkpoint(&[other]);
                            self.lib.set_look(&[other], o.look);
                        }
                        let was_focus = self.lib.focus == Some(id);
                        self.forget(&[id].into());
                        if was_focus {
                            self.lib.click(other);
                        }
                        return;
                    }
                }
                let it = self.lib.get_mut(id).unwrap();
                it.name = o.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                it.path = o.path;
                it.original = o.original;
                it.proxy = Some(Arc::new(o.proxy));
                it.levels = Some(o.levels);
                if o.look.is_some() {
                    it.look = o.look;
                    it.gen += 1;
                }
                it.status = Status::Ready;
            }
        }
    }

    fn forget(&mut self, ids: &std::collections::BTreeSet<Id>) {
        for &id in ids {
            self.tiers.forget(id);
        }
        self.lib.remove(ids);
    }

    // ---------- looks --------------------------------------------------------------------

    fn focus_look(&self) -> Option<&Settings> {
        self.lib.focus.and_then(|f| self.lib.get(f)).and_then(|it| it.look.as_ref())
    }

    fn set_looks(&mut self, look: Option<Settings>) {
        let ids = self.lib.selection();
        self.lib.checkpoint(&ids);
        self.lib.set_look(&ids, look);
    }

    fn copy_look(&mut self, ctx: &egui::Context) {
        if let Some(l) = self.focus_look().cloned() {
            ctx.copy_text(serde_json::to_string_pretty(&l).unwrap_or_default());
            self.clipboard = Some(l);
        }
    }

    /// Paste a look: from the system clipboard when it holds one, else the last one copied.
    fn paste_look(&mut self, text: Option<&str>) {
        let look = text.and_then(|t| serde_json::from_str::<Settings>(t.trim()).ok()).filter(|_| text.is_some_and(|t| t.trim_start().starts_with('{'))).or_else(|| self.clipboard.clone());
        if let Some(l) = look {
            self.set_looks(Some(l));
        }
    }

    fn save_look(&mut self) {
        let Some(l) = self.focus_look().cloned() else { return };
        if let Some(p) = rfd::FileDialog::new().add_filter("Look", &["json"]).set_file_name("look.json").save_file() {
            if let Err(e) = std::fs::write(&p, serde_json::to_string_pretty(&l).unwrap_or_default()) {
                self.export_errors.push(format!("{}: {e}", p.display()));
            }
        }
    }

    fn load_look(&mut self) {
        if let Some(p) = rfd::FileDialog::new().add_filter("Look", &["json"]).pick_file() {
            match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Settings>(&t).map_err(|e| e.to_string())) {
                Ok(l) => self.set_looks(Some(l)),
                Err(e) => self.export_errors.push(format!("{}: {e}", p.display())),
            }
        }
    }

    fn do_action(&mut self, ctx: &egui::Context, a: controls::Action) {
        use controls::Action;
        match a {
            Action::Process(on) => {
                let ids = self.lib.selection();
                self.lib.checkpoint(&ids);
                match on {
                    Some(on) => self.lib.set_process(&ids, on),
                    None => self.lib.toggle_process(&ids),
                }
            }
            Action::CopyLook => self.copy_look(ctx),
            Action::PasteLook => self.paste_look(None),
            Action::Reset => self.set_looks(None),
            Action::SaveLook => self.save_look(),
            Action::LoadLook => self.load_look(),
            Action::CopyFlags => {
                if let Some(l) = self.focus_look() {
                    ctx.copy_text(l.cli_flags());
                }
            }
        }
    }

    // ---------- keys ---------------------------------------------------------------------

    fn keys(&mut self, ctx: &egui::Context) {
        if ctx.wants_keyboard_input() {
            return;
        }
        let cmd = Modifiers::COMMAND;
        let cmd_shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let shift = Modifiers::SHIFT;
        let none = Modifiers::NONE;
        // copy and paste come as events, not keys
        let (mut copy, mut paste) = (false, None);
        ctx.input(|i| {
            for e in &i.events {
                match e {
                    egui::Event::Copy if i.modifiers.shift => copy = true,
                    egui::Event::Paste(t) if i.modifiers.shift => paste = Some(t.clone()),
                    _ => {}
                }
            }
        });
        if copy {
            self.copy_look(ctx);
        }
        if let Some(t) = paste {
            self.paste_look(Some(&t));
        }
        let k = |m: Modifiers, key: Key| ctx.input_mut(|i| i.consume_key(m, key));
        if k(cmd_shift, Key::E) {
            self.open_export();
        }
        if k(cmd_shift, Key::R) {
            self.set_looks(None);
        }
        if k(cmd_shift, Key::V) && self.clipboard.is_some() {
            self.paste_look(None);
        }
        if k(cmd_shift, Key::Z) {
            self.lib.redo();
        }
        if k(cmd, Key::Z) {
            self.lib.undo();
        }
        if k(cmd, Key::O) {
            self.import_dialog();
        }
        if k(cmd, Key::A) {
            self.lib.select_all();
        }
        if k(cmd, Key::D) {
            self.lib.select_none();
        }
        if k(cmd, Key::Num0) {
            self.zoom = None;
        }
        if k(cmd, Key::Num1) {
            self.zoom_100();
        }
        let zoom_in = k(cmd, Key::Equals) || k(cmd, Key::Plus);
        let zoom_out = k(cmd, Key::Minus);
        if zoom_in || zoom_out {
            let f = if zoom_in { 1.5 } else { 1.0 / 1.5 };
            match self.mode {
                Mode::Grid => self.thumb_pt = (self.thumb_pt * f).clamp(80.0, 480.0),
                _ => self.zoom = Some(self.current_zoom() * f),
            }
        }
        if k(none, Key::G) {
            self.mode = Mode::Grid;
            self.scroll_to_focus = true;
        }
        if k(none, Key::N) {
            self.mode = Mode::Survey;
        }
        if (k(none, Key::E) || k(none, Key::Enter)) && self.lib.focus.is_some() {
            self.mode = Mode::Single;
            self.zoom = None;
        }
        if k(none, Key::Escape) && self.mode != Mode::Grid {
            self.mode = Mode::Grid;
            self.scroll_to_focus = true;
        }
        if k(none, Key::Tab) {
            self.panel = !self.panel;
        }
        if k(none, Key::Backslash) {
            self.before = !self.before;
        }
        if k(none, Key::V) && !self.lib.selected.is_empty() {
            self.do_action(ctx, controls::Action::Process(None));
        }
        if k(none, Key::Z) && self.mode == Mode::Single {
            if self.zoom.is_some() {
                self.zoom = None;
            } else {
                self.zoom_100();
            }
        }
        if k(none, Key::Delete) || k(none, Key::Backspace) {
            let ids = self.lib.selected.clone();
            self.forget(&ids);
        }
        let mut moved = false;
        for (key, dx, dy) in [(Key::ArrowLeft, -1, 0), (Key::ArrowRight, 1, 0), (Key::ArrowUp, 0, -1), (Key::ArrowDown, 0, 1)] {
            let ext = k(shift, key);
            if ext || k(none, key) {
                let delta = dx + dy * if self.mode == Mode::Grid { self.grid_cols as isize } else { 1 };
                match self.mode {
                    Mode::Survey => self.lib.step_in_selection(delta),
                    _ => self.lib.step(delta, ext && self.mode == Mode::Grid),
                }
                moved = true;
            }
        }
        if moved {
            self.scroll_to_focus = true;
            if self.mode == Mode::Single {
                self.zoom = None;
            }
        }
    }

    // ---------- export -------------------------------------------------------------------

    fn exportable(&self) -> usize {
        self.lib.selected.iter().filter(|&&id| self.lib.get(id).is_some_and(|it| it.look.is_some() && it.ready())).count()
    }

    fn open_export(&mut self) {
        if self.exportable() == 0 || self.export_run.is_some() {
            return;
        }
        let dir = self.last_dir.clone().or_else(|| self.lib.focus.and_then(|f| self.lib.get(f)).and_then(|it| it.path.parent().map(|p| p.to_path_buf()))).unwrap_or_default();
        self.export_form = Some(ExportForm { dir, format: Format::Jpeg, size: SizeChoice::Original, long: 3000 });
    }

    fn start_export(&mut self, form: ExportForm) {
        let queue: VecDeque<Id> = self.lib.selected.iter().copied().filter(|&id| self.lib.get(id).is_some_and(|it| it.look.is_some() && it.ready())).collect();
        let total = queue.len();
        self.export_run = Some(ExportRun { queue, form, total, done: 0, encoding: 0, failed: Vec::new(), claimed: HashSet::new(), planner: Planner::new() });
    }

    /// One export render a frame at most: it reads the picture back from the GPU, which waits.
    fn export_step(&mut self) {
        let Some(run) = &mut self.export_run else { return };
        if run.done >= run.total && run.encoding == 0 {
            let run = self.export_run.take().unwrap();
            self.export_errors.extend(run.failed);
            return;
        }
        if run.encoding >= 2 {
            return;
        }
        let Some(&id) = run.queue.front() else { return };
        let Some(item) = self.lib.get(id) else {
            run.queue.pop_front();
            run.done += 1;
            return;
        };
        match self.tiers.export_pixels(item, &mut run.planner) {
            Ok(None) => {
                run.queue.pop_front();
                run.done += 1;
            }
            Ok(Some((w, h, rgba))) => {
                run.queue.pop_front();
                let lib = &self.lib;
                let claimed = &run.claimed;
                let path = export::output_path(&run.form.dir, &item.path, run.form.format, &|p| claimed.contains(p) || lib.find_path(p).is_some());
                run.claimed.insert(path.clone());
                let size = match run.form.size {
                    SizeChoice::Original => Size::Original,
                    SizeChoice::Grid => Size::Grid,
                    SizeChoice::Custom => Size::LongEdge(run.form.long),
                };
                run.encoding += 1;
                let job = Job::Encode {
                    pic: Box::new(export::Picture { w, h, rgba }),
                    size,
                    original_long: item.original.0.max(item.original.1),
                    format: run.form.format,
                    record: Box::new(Record { source: item.path.clone(), look: item.look.clone().unwrap() }),
                    path,
                };
                self.workers.send(job);
            }
            Err(loads) => {
                for (id, long) in loads {
                    if let Some(it) = self.lib.get(id) {
                        self.tiers.loading.insert((id, long));
                        self.workers.send(Job::Full { id, path: it.path.clone(), long });
                    }
                }
            }
        }
    }

    // ---------- single view zoom ---------------------------------------------------------

    fn current_zoom(&self) -> f32 {
        self.zoom.unwrap_or(self.last_fit)
    }

    fn zoom_100(&mut self) {
        self.zoom = Some(1.0);
    }

    /// Nothing loading, rendering or exporting.
    fn idle(&self) -> bool {
        self.opening == 0 && self.workers.queued() == 0 && self.tiers.loading.is_empty() && !self.pending && self.export_run.is_none()
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let t0 = std::time::Instant::now();
        self.poll();
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.import(dropped);
        }
        if let Some(mut s) = self.shots.take() {
            s.step(self, ctx);
            self.shots = Some(s);
        }
        self.keys(ctx);
        let actions = self.layout(ctx);
        for a in actions {
            self.do_action(ctx, a);
        }
        self.export_step();
        let (loads, pending) = self.tiers.process(&self.lib, self.before);
        self.pending = pending;
        // while a knob is held the grid size may be passing through many values
        if !self.edit_open {
            for (id, long) in loads {
                if let Some(it) = self.lib.get(id) {
                    self.tiers.loading.insert((id, long));
                    self.workers.send(Job::Full { id, path: it.path.clone(), long });
                }
            }
        } else {
            self.pending = true;
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            self.edit_open = false;
        }
        if self.pending || self.export_run.is_some() || self.shots.is_some() {
            ctx.request_repaint();
        }
        if self.tiers.trace {
            self.frame_times.push_back(t0.elapsed().as_secs_f32() * 1000.0);
            if self.frame_times.len() == 60 {
                let mean = self.frame_times.iter().sum::<f32>() / 60.0;
                let max = self.frame_times.iter().cloned().fold(0.0, f32::max);
                let (p, f, t, c) = self.tiers.census();
                eprintln!("frames: mean {mean:.2} ms, max {max:.2} ms; library {}; on GPU: {p} proxies, {f} full; {t} thumbnails, {c} canvases", self.lib.len());
                self.frame_times.clear();
            }
        }
    }
}
