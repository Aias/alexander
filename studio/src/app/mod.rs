//! The studio window: library, views, controls, import and export.

mod chrome;
mod commands;
mod controls;
mod crop;
#[cfg(target_os = "macos")]
pub mod finder;
#[cfg(target_os = "macos")]
mod menu;
#[cfg(target_os = "macos")]
mod photos;
mod presets;
mod reference;
mod session;
mod shots;
#[cfg(target_os = "macos")]
mod symbols;
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
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Poll, Wake, Waker};
use commands::Command;
use tiers::Tiers;
use workers::{Done, Job, Workers};

/// How the views compare a photo's render with the photo itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Compare {
    /// The render alone.
    After,
    /// The photo itself alone.
    Before,
    /// The photo itself on one side of a divider and the render on the other.
    Split,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Mode {
    Grid,
    Survey,
    Detail,
}

/// What a file dialog's paths are for.
enum Dialog {
    Open,
    #[cfg(target_os = "macos")]
    Photos,
    ExportFolder,
    SaveLook,
    LoadLook,
    Reference,
}

/// The paths a file dialog gives when it closes.
type Paths = Pin<Box<dyn Future<Output = Vec<PathBuf>>>>;

/// Asks for a frame when a file dialog finishes, so its paths are taken up at once.
struct Repaint(egui::Context);

impl Wake for Repaint {
    fn wake(self: Arc<Self>) {
        self.0.request_repaint();
    }
}

/// Where photos picked from the Photos library are copied.
fn photos_cache() -> PathBuf {
    std::env::home_dir().unwrap_or_default().join("Library/Caches/studio.alexander.app/Photos")
}

fn paths(handles: impl IntoIterator<Item = rfd::FileHandle>) -> Vec<PathBuf> {
    handles.into_iter().map(|h| h.path().to_path_buf()).collect()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SizeChoice {
    Original,
    Grid,
    Custom,
}

struct ExportForm {
    dir: PathBuf,
    /// Whether `dir` is a folder, checked when it is chosen rather than every frame.
    dir_ok: bool,
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
    /// Grid and Survey: photos fill their cells, cut to them, rather than fit inside.
    fill: bool,
    compare: Compare,
    /// Split: the divider's place across the picture, from 0 to 1, and whether the photo
    /// itself is on the right.
    split: f32,
    split_swap: bool,
    /// Pure white and pure black are marked in Survey and Detail.
    clipping: bool,
    /// A photo whose finished picture is being read back for the clipboard.
    copy_image: Option<(Id, Planner)>,
    clipboard: Option<Settings>,
    /// Commands chosen from a photo's menu, run once the views are laid out, since they may
    /// take photos away from the views laying them out.
    chosen: Vec<Command>,
    /// An edit to the looks is under way (a slider held): one undo step for all of it.
    edit_open: bool,
    /// Detail: None fits the photo; otherwise screen pixels per grid pixel.
    zoom: Option<f32>,
    /// Detail: the point of the photo at the middle of the view, as fractions.
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
    /// Library revision and the count of exportable selected photos at it.
    exportable: (u64, usize),
    /// The mark on a photo with a look: the SF Symbol for film where there is one.
    film_mark: Option<egui::TextureHandle>,
    /// The rail's icons: Edit, Crop and Reference.
    rail_icons: [Option<egui::TextureHandle>; 3],
    presets: Vec<presets::Preset>,
    reference: reference::Reference,
    /// A crop being drawn on the focused photo in Detail.
    cropping: Option<crop::CropEdit>,
    /// A preset being named, while it is typed.
    new_preset: Option<presets::Draft>,
    /// The preset under the pointer this frame, to show on the focused photo.
    preview_wanted: Option<(Id, Settings)>,
    #[cfg(target_os = "macos")]
    menu: Option<menu::MenuBar>,
    /// Folders photos were opened from, latest first, for Open Recent.
    recent: Vec<PathBuf>,
    /// Whether the session is remembered: not in a scripted run, which starts afresh.
    remember: bool,
    /// What the session's file last recorded, and when it was written.
    saved: (session::Saved, std::time::Instant),
    /// The file dialog open, if any, and the paths it gives when it closes.
    dialog: Option<(Dialog, Paths)>,
    waker: Waker,
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
        let remember = shots.is_none();
        #[cfg(target_os = "macos")]
        finder::wake(&cc.egui_ctx);
        let mut app = App {
            lib: Library::new(),
            tiers,
            workers,
            mode: Mode::Grid,
            panel: true,
            thumb_pt: 168.0,
            fill: false,
            compare: Compare::After,
            split: 0.5,
            split_swap: false,
            clipping: false,
            copy_image: None,
            clipboard: None,
            chosen: Vec::new(),
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
            exportable: (u64::MAX, 0),
            #[cfg(target_os = "macos")]
            film_mark: symbols::symbol("film", 28).map(|image| cc.egui_ctx.load_texture("film", image, egui::TextureOptions::LINEAR)),
            #[cfg(not(target_os = "macos"))]
            film_mark: None,
            #[cfg(target_os = "macos")]
            rail_icons: ["slider.horizontal.3", "crop.rotate", "pip"].map(|name| symbols::symbol(name, 40).map(|image| cc.egui_ctx.load_texture(name, image, egui::TextureOptions::LINEAR))),
            #[cfg(not(target_os = "macos"))]
            rail_icons: [None, None, None],
            presets: presets::load(),
            reference: reference::Reference::new(),
            cropping: None,
            new_preset: None,
            preview_wanted: None,
            #[cfg(target_os = "macos")]
            menu: Some(menu::MenuBar::new(&cc.egui_ctx)),
            recent: Vec::new(),
            remember,
            saved: ((0, Mode::Grid, 0, 0, 0, [false; 3]), std::time::Instant::now()),
            dialog: None,
            waker: Waker::from(Arc::new(Repaint(cc.egui_ctx.clone()))),
        };
        if app.remember {
            app.restore_session();
        }
        if app.shots.is_none() && !paths.is_empty() {
            app.import(paths);
        }
        app
    }

    // ---------- import ---------------------------------------------------------------------

    fn import(&mut self, paths: Vec<PathBuf>) {
        self.remember_folders(&paths);
        if !paths.is_empty() {
            self.opening += 1;
            self.workers.send(Job::Collect(paths));
        }
    }

    fn import_dialog(&mut self) {
        let mut d = rfd::AsyncFileDialog::new().add_filter("Photos", load::extensions());
        if let Some(dir) = &self.last_dir {
            d = d.set_directory(dir);
        }
        self.ask(Dialog::Open, async move { paths(d.pick_files().await.into_iter().flatten()) });
    }

    #[cfg(target_os = "macos")]
    fn photos_dialog(&mut self) {
        self.ask(Dialog::Photos, photos::pick(photos_cache()));
    }

    /// Open a file dialog unless one is already open. rfd begins it as a sheet on the window
    /// when the future is first polled, and the sheet returns at once, so the window's
    /// events go on arriving between frames. A dialog run to completion inside a frame would
    /// deliver them inside that frame, and winit aborts when that happens.
    fn ask(&mut self, dialog: Dialog, paths: impl Future<Output = Vec<PathBuf>> + 'static) {
        if self.dialog.is_none() {
            self.dialog = Some((dialog, Box::pin(paths)));
        }
    }

    fn poll_dialog(&mut self) {
        let Some((_, future)) = &mut self.dialog else { return };
        let Poll::Ready(paths) = future.as_mut().poll(&mut std::task::Context::from_waker(&self.waker)) else { return };
        let Some((dialog, _)) = self.dialog.take() else { return };
        match dialog {
            Dialog::Open => {
                if let Some(dir) = paths.first().and_then(|f| f.parent()) {
                    self.last_dir = Some(dir.to_path_buf());
                }
                self.import(paths);
            }
            #[cfg(target_os = "macos")]
            Dialog::Photos => self.import(paths),
            Dialog::ExportFolder => {
                if let (Some(d), Some(form)) = (paths.into_iter().next(), &mut self.export_form) {
                    form.dir_ok = d.is_dir();
                    form.dir = d;
                }
            }
            Dialog::SaveLook => {
                let Some(look) = self.focus_look().cloned() else { return };
                for p in paths {
                    if let Err(e) = std::fs::write(&p, serde_json::to_string_pretty(&look).unwrap_or_default()) {
                        self.export_errors.push(format!("{}: {e}", p.display()));
                    }
                }
            }
            Dialog::Reference => self.reference.add(paths),
            Dialog::LoadLook => {
                for p in paths {
                    match std::fs::read_to_string(&p).map_err(|e| e.to_string()).and_then(|t| serde_json::from_str::<Settings>(&t).map_err(|e| e.to_string())) {
                        Ok(l) => self.set_looks(l),
                        Err(e) => self.export_errors.push(format!("{}: {e}", p.display())),
                    }
                }
            }
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.workers.rx.try_recv() {
            match done {
                Done::Collected(paths) => {
                    self.opening -= 1;
                    if self.lib.starred_only() {
                        self.lib.set_starred_only(false);
                    }
                    let mut first = None;
                    for p in paths {
                        if let Some(id) = self.lib.add(p.clone()) {
                            first.get_or_insert(id);
                            self.opening += 1;
                            self.workers.send(Job::Open { id, path: p });
                        }
                    }
                    if let (Some(id), true) = (first, self.lib.selected().is_empty()) {
                        self.lib.click(id);
                    }
                }
                Done::Opened { id, result } => {
                    self.opening -= 1;
                    self.opened(id, result);
                }
                Done::Full { id, long, result } => {
                    self.tiers.loading.remove(&(id, long));
                    let Some(item) = self.lib.get(id) else { continue };
                    match result {
                        // a grid asked for before `detail_px` changed is no longer wanted
                        Ok(px) if self.tiers.grid_long(item) == long => self.tiers.full_arrived(id, long, px),
                        Ok(_) => {}
                        // the file went away or changed since it was opened: asking again
                        // every frame would only read the disk to fail again
                        Err(e) => {
                            self.tiers.forget(id);
                            self.lib.get_mut(id).unwrap().status = Status::Failed(e);
                        }
                    }
                }
                Done::Reference { path, result } => self.reference.arrived(ctx, path, result),
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
                // an export of ours came back as its original: shown on the original or a copy of it
                if o.path != it.path {
                    if let Some(other) = self.lib.find_path(&o.path) {
                        let shown = o.look.map_or(other, |l| self.lib.give_look(other, l));
                        let was_focus = self.lib.focus == Some(id);
                        self.tiers.forget(id);
                        self.lib.take_out(&[id].into());
                        if was_focus {
                            self.lib.click(shown);
                        }
                        return;
                    }
                }
                self.lib.set_path(id, o.path);
                let it = self.lib.get_mut(id).unwrap();
                it.original = o.original;
                it.proxy = Some(Arc::new(o.proxy));
                it.levels = Some(o.levels);
                it.frame = o.frame;
                if o.look.is_some() {
                    it.look = o.look;
                    it.gen += 1;
                }
                it.status = Status::Ready;
            }
        }
    }

    /// Take photos out of the library, which undo can put back, and let go of them on the GPU.
    fn forget(&mut self, ids: &std::collections::BTreeSet<Id>) {
        for &id in ids {
            self.tiers.forget(id);
        }
        self.lib.remove(ids);
    }

    /// Undo or redo. Photos that come back are opened again; photos that go are let go of.
    fn step_history(&mut self, redo: bool) {
        let ids = if redo { self.lib.redo() } else { self.lib.undo() };
        for id in ids {
            match self.lib.get(id) {
                None => self.tiers.forget(id),
                Some(it) if it.status == Status::Loading => {
                    self.opening += 1;
                    self.workers.send(Job::Open { id, path: it.path.clone() });
                }
                Some(_) => {}
            }
        }
    }

    // ---------- looks --------------------------------------------------------------------

    fn focus_look(&self) -> Option<&Settings> {
        self.lib.focus.and_then(|f| self.lib.get(f)).and_then(|it| it.look.as_ref())
    }

    /// Give the selected photos `look`, each keeping its own crop.
    fn set_looks(&mut self, look: Settings) {
        let ids = self.lib.selection();
        self.lib.checkpoint(&ids);
        for id in ids {
            let crop = self.lib.get(id).and_then(|it| it.look.as_ref()).and_then(|l| l.crop);
            self.lib.set_look(&[id], Some(Settings { crop, ..look.clone() }));
        }
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
            self.set_looks(l);
        }
    }

    fn save_look(&mut self) {
        if self.focus_look().is_none() {
            return;
        }
        let d = rfd::AsyncFileDialog::new().add_filter("Look", &["json"]).set_file_name("look.json");
        self.ask(Dialog::SaveLook, async move { paths(d.save_file().await) });
    }

    fn load_look(&mut self) {
        let d = rfd::AsyncFileDialog::new().add_filter("Look", &["json"]);
        self.ask(Dialog::LoadLook, async move { paths(d.pick_file().await) });
    }

    // ---------- keys ---------------------------------------------------------------------

    fn keys(&mut self, ctx: &egui::Context) {
        // a dialog takes the keys: Return there means Export, not Detail, and
        // Delete must not take photos out of the library behind it
        if ctx.egui_wants_keyboard_input() || self.export_form.is_some() || !self.export_errors.is_empty() {
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
        if self.cropping.is_some() {
            if k(none, Key::Enter) {
                self.finish_crop();
            }
            if k(none, Key::Escape) {
                self.cropping = None;
            }
        }
        // shifted keys first: a key asked for without shift is also pressed with it
        let mut keyed = vec![
            (cmd_shift, Key::E, Command::Export),
            (cmd_shift, Key::R, Command::ResetLook),
            (cmd_shift, Key::V, Command::PasteLook),
            (cmd_shift, Key::Z, Command::Redo),
            (cmd, Key::Z, Command::Undo),
            (cmd, Key::Quote, Command::VirtualCopy),
            (cmd, Key::A, Command::SelectAll),
            (cmd, Key::D, Command::SelectNone),
            (cmd, Key::Num0, Command::Fit),
            (cmd, Key::Num1, Command::Actual),
            (cmd, Key::Equals, Command::Zoom(true)),
            (cmd, Key::Plus, Command::Zoom(true)),
            (cmd, Key::Minus, Command::Zoom(false)),
            (none, Key::G, Command::View(Mode::Grid)),
            (none, Key::N, Command::View(Mode::Survey)),
            (none, Key::E, Command::View(Mode::Detail)),
            (none, Key::Enter, Command::View(Mode::Detail)),
            (none, Key::Tab, Command::Panel),
            (shift, Key::R, Command::Reference),
            (none, Key::Backslash, Command::Compare(Compare::Before)),
            (none, Key::Y, Command::Compare(Compare::Split)),
            (none, Key::J, Command::Clipping),
            (none, Key::R, Command::Crop),
            (none, Key::S, Command::Star),
            (none, Key::V, Command::Process(None)),
            (none, Key::Delete, Command::Remove),
            (none, Key::Backspace, Command::Remove),
        ];
        #[cfg(target_os = "macos")]
        keyed.extend([(cmd_shift, Key::O, Command::OpenPhotos), (cmd, Key::R, Command::ShowInFinder)]);
        keyed.push((cmd, Key::O, Command::Open));
        for (m, key, c) in keyed {
            if k(m, key) {
                self.run(ctx, c);
            }
        }
        if k(none, Key::Escape) && self.mode != Mode::Grid {
            self.mode = Mode::Grid;
            self.scroll_to_focus = true;
        }
        if k(none, Key::Z) && self.mode == Mode::Detail {
            if self.zoom.is_some() {
                self.zoom = None;
            } else {
                self.zoom_100();
            }
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
            if self.mode == Mode::Detail {
                self.zoom = None;
            }
        }
    }

    // ---------- export -------------------------------------------------------------------

    /// Selected photos that can be exported. Counted again only when the library has
    /// changed, so a large selection costs nothing on frames that change nothing.
    fn exportable(&mut self) -> usize {
        let rev = self.lib.revision();
        if self.exportable.0 != rev {
            let n = self.lib.selected().iter().filter(|&&id| self.lib.get(id).is_some_and(|it| it.look.is_some() && it.ready())).count();
            self.exportable = (rev, n);
        }
        self.exportable.1
    }

    fn open_export(&mut self) {
        if self.exportable() == 0 || self.export_run.is_some() {
            return;
        }
        let dir = self
            .last_dir
            .clone()
            .or_else(|| self.lib.focus.and_then(|f| self.lib.get(f)).and_then(|it| it.path.parent().map(|p| p.to_path_buf())))
            .filter(|d| !d.starts_with(photos_cache()))
            .or_else(|| std::env::home_dir().map(|h| h.join("Pictures")))
            .unwrap_or_default();
        self.export_form = Some(ExportForm { dir_ok: dir.is_dir(), dir, format: Format::Jpeg, size: SizeChoice::Original, long: 3000 });
    }

    fn start_export(&mut self, form: ExportForm) {
        let queue: VecDeque<Id> = self.lib.selected().iter().copied().filter(|&id| self.lib.get(id).is_some_and(|it| it.look.is_some() && it.ready())).collect();
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
        if let Status::Failed(e) = &item.status {
            run.failed.push(format!("{}: {e}", item.path.display()));
            run.queue.pop_front();
            run.done += 1;
            return;
        }
        match self.tiers.export_pixels(item, &mut run.planner) {
            Ok(None) => {
                run.queue.pop_front();
                run.done += 1;
            }
            Ok(Some((w, h, rgba))) => {
                run.queue.pop_front();
                let pic = export::Picture { w, h, rgba };
                let original_long = export::cropped_long((w, h), self.tiers.grid_size(item), item.original.0.max(item.original.1));
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
                    pic: Box::new(pic),
                    size,
                    original_long,
                    format: run.form.format,
                    record: Box::new(Record { source: item.path.clone(), look: item.look.clone().unwrap() }),
                    path,
                };
                self.workers.send(job);
            }
            Err(loads) => self.load_fulls(loads),
        }
    }

    // ---------- detail zoom ---------------------------------------------------------------

    fn current_zoom(&self) -> f32 {
        self.zoom.unwrap_or(self.last_fit)
    }

    fn zoom_100(&mut self) {
        self.zoom = Some(1.0);
    }

    /// Nothing loading, rendering or exporting.
    fn load_fulls(&mut self, loads: Vec<(Id, u32)>) {
        for (id, long) in loads {
            if let Some(it) = self.lib.get(id) {
                self.tiers.loading.insert((id, long));
                self.workers.send(Job::Full { id, path: it.path.clone(), long });
            }
        }
    }

    /// Put the finished picture of a photo on the clipboard at the size of its simulation
    /// grid, cut to the picture inside its border when the look strips it, once the photo's
    /// full grid has loaded.
    fn copy_image_step(&mut self, ctx: &egui::Context) {
        let Some((id, planner)) = &mut self.copy_image else { return };
        let Some(item) = self.lib.get(*id) else {
            self.copy_image = None;
            return;
        };
        match self.tiers.export_pixels(item, planner) {
            Ok(Some((w, h, rgba))) => {
                let bytes: Vec<u8> = rgba.iter().map(|v| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect();
                ctx.copy_image(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &bytes));
                self.copy_image = None;
            }
            Ok(None) => self.copy_image = None,
            Err(loads) => self.load_fulls(loads),
        }
    }

    fn before(&self) -> bool {
        self.compare == Compare::Before
    }

    /// Switch a compare on, or off again when it is already on.
    fn toggle_compare(&mut self, c: Compare) {
        self.compare = if self.compare == c { Compare::After } else { c };
    }

    fn idle(&self) -> bool {
        self.opening == 0 && self.workers.queued() == 0 && self.tiers.loading.is_empty() && !self.pending && self.export_run.is_none()
    }
}

impl eframe::App for App {
    fn on_exit(&mut self) {
        if self.remember {
            let _ = self.save_session(true);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = &ui.ctx().clone();
        let t0 = std::time::Instant::now();
        self.poll(ctx);
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.import(dropped);
        }
        #[cfg(target_os = "macos")]
        {
            let opened = finder::arrived();
            if !opened.is_empty() {
                self.import(opened);
            }
        }
        if let Some(mut s) = self.shots.take() {
            s.step(self, ctx);
            self.shots = Some(s);
        }
        #[cfg(target_os = "macos")]
        let menu = self.menu.take();
        #[cfg(target_os = "macos")]
        if let Some(menu) = &menu {
            // a dialog keeps the menu's commands from the library behind it, and text being
            // typed keeps its own undo and select all
            let typing = ctx.egui_wants_keyboard_input();
            for c in menu.chosen() {
                let held = typing && matches!(c, Command::Undo | Command::Redo | Command::SelectAll | Command::SelectNone);
                if self.export_form.is_none() && self.export_errors.is_empty() && !held {
                    self.run(ctx, c);
                }
            }
        }
        self.keys(ctx);
        let actions = self.layout(ui);
        self.tiers.set_preview(self.preview_wanted.take());
        if self.cropping.as_ref().is_some_and(|e| self.lib.focus != Some(e.id) || self.mode != Mode::Detail) {
            self.finish_crop();
            if self.mode == Mode::Detail && self.can(&Command::Crop) {
                self.start_crop();
            }
        }
        #[cfg(target_os = "macos")]
        if let Some(mut menu) = menu {
            menu.sync(self);
            self.menu = Some(menu);
        }
        for c in actions {
            self.run(ctx, c);
        }
        self.poll_dialog();
        self.export_step();
        self.copy_image_step(ctx);
        if let Some(wait) = self.remember.then(|| self.save_session(false)).flatten() {
            ctx.request_repaint_after(wait);
        }
        let (loads, pending) = self.tiers.process(&self.lib, self.before());
        self.pending = pending;
        // while a knob is held the grid size may be passing through many values
        if !self.edit_open {
            self.load_fulls(loads);
        } else {
            self.pending = true;
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            self.edit_open = false;
        }
        if self.pending || self.export_run.is_some() || self.copy_image.is_some() || self.shots.is_some() {
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
