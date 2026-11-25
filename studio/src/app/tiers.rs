//! Which photos are on the GPU and at what size, and keeping what is on screen current.
//!
//! Two tiers. Every photo has a small proxy in memory; proxies of photos near the screen are
//! put on the GPU and rendered for thumbnails. Photos shown large also get their full
//! simulation grid on the GPU. Both are kept in least-recently-used order under a byte
//! budget, so GPU memory does not grow with the library. Views ask during layout for what
//! they want to show; `process` then renders and draws it, most important first, within a
//! time budget, and the rest catches up over the next frames.

use alexander_studio::engine::{Canvas, Engine, Photo, Stage};
use alexander_studio::library::{Id, Item, Library};
use alexander_studio::load::{self, Pixels};
use alexander_studio::params::{Settings, View};
use alexander_studio::plan::{Plan, Planner, ViewPlan};
use egui::TextureId;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROXY_BUDGET: u64 = 256 << 20;
const FULL_BUDGET: u64 = 1200 << 20;
const THUMB_BUDGET: u64 = 192 << 20;
/// Time spent on renders other than the focused photo's in one frame.
const FRAME_BUDGET: Duration = Duration::from_millis(12);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tier {
    Proxy,
    Full,
}

struct Resident {
    photo: Photo,
    /// The look generation the plan was made for.
    gen: Option<u64>,
    plan: Option<Plan>,
    /// Counts the renders that changed the picture.
    version: u64,
    used: u64,
}

impl Resident {
    fn new(photo: Photo) -> Resident {
        Resident { photo, gen: None, plan: None, version: 0, used: 0 }
    }

    fn bytes(&self) -> u64 {
        let (w, h) = self.photo.size();
        let k = self.plan.as_ref().map_or(1, |p| p.optics.scale as u64);
        // the photo itself, five kept stages and the finished picture with its half-size copies
        w as u64 * h as u64 * (4 + 18 * k * k)
    }

    /// Fit to show: the photo as it is, or a render of its look (possibly an older one).
    fn showable(&self, item: &Item) -> bool {
        item.look.is_none() || self.photo.rendered()
    }

    fn stale(&self, item: &Item) -> bool {
        item.look.is_some() && self.gen != Some(item.gen)
    }
}

#[derive(Clone, PartialEq)]
struct DrawKey {
    tier: Tier,
    version: u64,
    gen: u64,
    canvas: [u32; 3],
    size: [u32; 2],
    source: bool,
}

struct Surface {
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    id: TextureId,
    size: [u32; 2],
    key: Option<DrawKey>,
    used: u64,
}

impl Surface {
    fn bytes(&self) -> u64 {
        self.size[0] as u64 * self.size[1] as u64 * 4
    }
}

#[derive(Clone, Copy)]
enum Want {
    Thumb,
    /// `canvas` is in the coordinates of the photo's full grid of `grid` pixels.
    Canvas { canvas: Canvas, grid: (u32, u32) },
}

#[derive(Clone, Copy)]
struct Request {
    id: Id,
    want: Want,
    size: [u32; 2],
    rank: u8,
}

pub struct Tiers {
    pub engine: Engine,
    device: wgpu::Device,
    renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
    planners: [Planner; 2],
    proxies: HashMap<Id, Resident>,
    fulls: HashMap<Id, Resident>,
    thumbs: HashMap<Id, Surface>,
    canvases: HashMap<Id, Surface>,
    requests: Vec<Request>,
    /// Full grids asked of the loaders and not yet arrived: photo and long edge.
    pub loading: HashSet<(Id, u32)>,
    frame: u64,
    /// The first stage the focused photo's last render had to redo, for the trace.
    pub last_focus_stage: Option<(Tier, Stage, f32)>,
    pub trace: bool,
}

pub fn default_view() -> ViewPlan {
    ViewPlan { mode: View::Print, solid: 0.0, scan_black: 0.0, scan_white: 1.0 }
}

impl Tiers {
    pub fn new(engine: Engine, device: wgpu::Device, renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>) -> Tiers {
        Tiers {
            engine,
            device,
            renderer,
            planners: [Planner::new(), Planner::new()],
            proxies: HashMap::new(),
            fulls: HashMap::new(),
            thumbs: HashMap::new(),
            canvases: HashMap::new(),
            requests: Vec::new(),
            loading: HashSet::new(),
            frame: 1,
            last_focus_stage: None,
            trace: std::env::var("STUDIO_TRACE").is_ok(),
        }
    }

    pub fn max_dim(&self) -> u32 {
        self.engine.gpu.max_dim
    }

    /// Long edge of the full grid a photo is shown and exported at.
    pub fn grid_long(&self, item: &Item) -> u32 {
        let detail = item.look.as_ref().map_or(Settings::default().detail_px, |l| l.detail_px);
        load::grid_long(detail, item.original, self.max_dim())
    }

    pub fn grid_size(&self, item: &Item) -> (u32, u32) {
        load::grid_size(item.original, self.grid_long(item))
    }

    // ---------- asked for during layout ---------------------------------------------------

    /// A thumbnail of `item` for a box of `max` physical pixels: its texture (drawn at
    /// some size, maybe an older look) and the size it should be shown at.
    pub fn thumb(&mut self, item: &Item, max: [u32; 2], rank: u8) -> (Option<TextureId>, [u32; 2]) {
        let Some(p) = &item.proxy else { return (None, [0, 0]) };
        let s = (max[0] as f32 / p.w as f32).min(max[1] as f32 / p.h as f32);
        let size = [((p.w as f32 * s).round() as u32).clamp(1, p.w.max(1) * 2), ((p.h as f32 * s).round() as u32).clamp(1, p.h.max(1) * 2)];
        self.requests.push(Request { id: item.id, want: Want::Thumb, size, rank });
        (self.thumbs.get(&item.id).filter(|t| t.key.is_some()).map(|t| t.id), size)
    }

    /// `item` drawn into a canvas of `size` physical pixels looking at its full grid as
    /// `canvas` says. Falls back to the proxy until the full grid is ready.
    pub fn canvas(&mut self, item: &Item, canvas: Canvas, size: [u32; 2], rank: u8) -> Option<TextureId> {
        if size[0] == 0 || size[1] == 0 || item.proxy.is_none() {
            return None;
        }
        let grid = self.grid_size(item);
        self.requests.push(Request { id: item.id, want: Want::Canvas { canvas, grid }, size, rank });
        self.canvases.get(&item.id).filter(|t| t.key.is_some()).map(|t| t.id)
    }

    // ---------- loading ------------------------------------------------------------------

    /// A full grid arrived from the loaders.
    pub fn full_arrived(&mut self, id: Id, long: u32, px: Pixels) {
        self.loading.remove(&(id, long));
        let photo = self.engine.upload(&px.rgba, px.w, px.h);
        if let Some(mut old) = self.fulls.insert(id, Resident { used: self.frame, ..Resident::new(photo) }) {
            self.engine.release(&mut old.photo);
        }
    }

    pub fn forget(&mut self, id: Id) {
        for map in [&mut self.proxies, &mut self.fulls] {
            if let Some(mut r) = map.remove(&id) {
                self.engine.release(&mut r.photo);
            }
        }
        for map in [&mut self.thumbs, &mut self.canvases] {
            if let Some(s) = map.remove(&id) {
                self.renderer.write().free_texture(&s.id);
            }
        }
        self.loading.retain(|(i, _)| *i != id);
    }

    // ---------- each frame -----------------------------------------------------------------

    /// Render and draw what the views asked for. Returns the full grids to load and
    /// whether work is left for later frames.
    pub fn process(&mut self, lib: &Library, before: bool) -> (Vec<(Id, u32)>, bool) {
        let start = Instant::now();
        let mut requests = std::mem::take(&mut self.requests);
        requests.sort_by_key(|r| r.rank);
        let mut to_load = Vec::new();
        let mut pending = false;
        let mut full_bytes = 0u64;
        let mut first = true;
        for r in &requests {
            let Some(item) = lib.get(r.id) else { continue };
            if item.proxy.is_none() {
                continue;
            }
            // the first request always renders; the rest share the frame's budget
            let may_render = first || start.elapsed() < FRAME_BUDGET;
            first = false;
            match r.want {
                Want::Thumb => {
                    if !self.ensure(Tier::Proxy, item, may_render, r.rank == 0) {
                        pending = true;
                    }
                    self.draw_thumb(item, r.size, before);
                }
                Want::Canvas { canvas, grid } => {
                    let long = grid.0.max(grid.1);
                    // a canvas no larger than the proxy is drawn from the proxy
                    let wants_full = r.size[0].max(r.size[1]) as f32 > 1.15 * item.proxy.as_ref().map_or(0, |p| p.w.max(p.h)) as f32 || canvas.scale < 1.0;
                    let mut tier = Tier::Proxy;
                    if wants_full {
                        let have = self.fulls.get(&item.id).map(|f| f.photo.size());
                        let fits = full_bytes + (long as u64 * long as u64 * 14) < FULL_BUDGET;
                        if have.is_some_and(|s| s.0.max(s.1) == long) || (have.is_some() && fits) {
                            if have.is_some_and(|s| s.0.max(s.1) != long) && !self.loading.contains(&(item.id, long)) {
                                to_load.push((item.id, long));
                            }
                            let ok = self.ensure(Tier::Full, item, may_render, r.rank == 0);
                            pending |= !ok;
                            let f = &self.fulls[&item.id];
                            full_bytes += f.bytes();
                            if f.showable(item) {
                                tier = Tier::Full;
                            }
                        } else if fits && !self.loading.contains(&(item.id, long)) {
                            to_load.push((item.id, long));
                            pending = true;
                        } else if fits {
                            pending = true;
                        }
                    }
                    if tier == Tier::Proxy {
                        pending |= !self.ensure(Tier::Proxy, item, may_render, r.rank == 0);
                    }
                    self.draw_canvas(item, tier, canvas, grid, r.size, before);
                }
            }
        }
        self.evict();
        self.frame += 1;
        (to_load, pending)
    }

    /// Put `item` on the GPU at `tier` (the proxy is uploaded on demand; a full grid must
    /// have arrived) and render it when its look changed. False when a render was owed and
    /// left for later.
    fn ensure(&mut self, tier: Tier, item: &Item, may_render: bool, focused: bool) -> bool {
        let frame = self.frame;
        if tier == Tier::Proxy && !self.proxies.contains_key(&item.id) {
            let p = item.proxy.as_ref().unwrap();
            let photo = self.engine.upload(&p.rgba, p.w, p.h);
            self.proxies.insert(item.id, Resident::new(photo));
        }
        let map = if tier == Tier::Proxy { &mut self.proxies } else { &mut self.fulls };
        let Some(res) = map.get_mut(&item.id) else { return false };
        res.used = frame;
        let Some(look) = &item.look else { return true };
        if !res.stale(item) {
            return true;
        }
        if !may_render {
            return false;
        }
        let planner = &mut self.planners[tier as usize];
        let plan = planner.plan(look, res.photo.size(), item.auto_ev(look), self.engine.gpu.max_dim);
        let t = Instant::now();
        if let Some(stage) = self.engine.render(&mut res.photo, &plan) {
            res.version += 1;
            if focused {
                self.last_focus_stage = Some((tier, stage, t.elapsed().as_secs_f32() * 1000.0));
                if self.trace {
                    eprintln!("render {:?} {} from {:?} in {:.1} ms", tier, item.name, stage, t.elapsed().as_secs_f32() * 1000.0);
                }
            }
        }
        res.plan = Some(plan);
        res.gen = Some(item.gen);
        true
    }

    fn surface(&self, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("view"),
            size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = texture.create_view(&Default::default());
        (texture, view)
    }

    /// Draw into the surface of `id` in `map`, replacing it when the size changed.
    #[allow(clippy::too_many_arguments)]
    fn draw_into(&mut self, thumbs: bool, id: Id, tier: Tier, key: DrawKey, canvas: Canvas, view: ViewPlan, filter: wgpu::FilterMode) {
        let frame = self.frame;
        let size = key.size;
        let map = if thumbs { &mut self.thumbs } else { &mut self.canvases };
        if let Some(s) = map.get_mut(&id) {
            s.used = frame;
            if s.key.as_ref() == Some(&key) {
                return;
            }
        }
        let resized = map.get(&id).is_none_or(|s| s.size != size);
        if resized {
            let (texture, tview) = self.surface(size);
            let map = if thumbs { &mut self.thumbs } else { &mut self.canvases };
            match map.get_mut(&id) {
                Some(s) => {
                    self.renderer.write().update_egui_texture_from_wgpu_texture(&self.device, &tview, filter, s.id);
                    s.texture.destroy();
                    s.texture = texture;
                    s.view = tview;
                    s.size = size;
                }
                None => {
                    let tid = self.renderer.write().register_native_texture(&self.device, &tview, filter);
                    map.insert(id, Surface { texture, view: tview, id: tid, size, key: None, used: frame });
                }
            }
        }
        let res = if tier == Tier::Proxy { self.proxies.get_mut(&id) } else { self.fulls.get_mut(&id) };
        let Some(res) = res else { return };
        let map = if thumbs { &mut self.thumbs } else { &mut self.canvases };
        let s = map.get_mut(&id).unwrap();
        self.engine.draw(&mut res.photo, &view, &canvas, &s.view, wgpu::TextureFormat::Rgba8UnormSrgb, false);
        s.key = Some(key);
    }

    fn view_of(res: &Resident, item: &Item) -> ViewPlan {
        match (&item.look, &res.plan) {
            (Some(_), Some(p)) => p.view.clone(),
            _ => default_view(),
        }
    }

    fn draw_thumb(&mut self, item: &Item, size: [u32; 2], before: bool) {
        let Some(res) = self.proxies.get(&item.id) else { return };
        if !res.showable(item) {
            return;
        }
        let (w, _) = res.photo.size();
        let source = before || item.look.is_none();
        let canvas = Canvas { origin: [0.0, 0.0], scale: w as f32 / size[0] as f32, show_source: source };
        let key = DrawKey { tier: Tier::Proxy, version: res.version, gen: item.gen, canvas: canvas_bits(&canvas), size, source };
        let view = Self::view_of(res, item);
        self.draw_into(true, item.id, Tier::Proxy, key, canvas, view, wgpu::FilterMode::Linear);
    }

    fn draw_canvas(&mut self, item: &Item, tier: Tier, canvas: Canvas, grid: (u32, u32), size: [u32; 2], before: bool) {
        let res = if tier == Tier::Proxy { self.proxies.get(&item.id) } else { self.fulls.get(&item.id) };
        let Some(res) = res else { return };
        if !res.showable(item) {
            // keep showing what was there, if anything
            if let Some(s) = self.canvases.get_mut(&item.id) {
                s.used = self.frame;
            }
            return;
        }
        let (pw, _) = res.photo.size();
        let k = pw as f32 / grid.0 as f32;
        let source = before || item.look.is_none();
        let c = Canvas { origin: [canvas.origin[0] * k, canvas.origin[1] * k], scale: canvas.scale * k, show_source: source };
        let key = DrawKey { tier, version: res.version, gen: item.gen, canvas: canvas_bits(&c), size, source };
        let view = Self::view_of(res, item);
        self.draw_into(false, item.id, tier, key, c, view, wgpu::FilterMode::Nearest);
    }

    /// Let go of what has not been needed lately, oldest first, down to the budgets.
    /// Nothing used this frame is let go of.
    fn evict(&mut self) {
        let frame = self.frame;
        for (map, budget) in [(&mut self.proxies, PROXY_BUDGET), (&mut self.fulls, FULL_BUDGET)] {
            let mut total: u64 = map.values().map(|r| r.bytes()).sum();
            if total <= budget {
                continue;
            }
            let mut old: Vec<(u64, Id)> = map.iter().filter(|(_, r)| r.used < frame).map(|(id, r)| (r.used, *id)).collect();
            old.sort();
            for (_, id) in old {
                if total <= budget {
                    break;
                }
                let mut r = map.remove(&id).unwrap();
                total -= r.bytes();
                self.engine.release(&mut r.photo);
            }
        }
        // canvases belong to the views on screen now
        let gone: Vec<Id> = self.canvases.iter().filter(|(_, s)| s.used + 1 < frame).map(|(id, _)| *id).collect();
        for id in gone {
            let s = self.canvases.remove(&id).unwrap();
            self.renderer.write().free_texture(&s.id);
        }
        let mut total: u64 = self.thumbs.values().map(|s| s.bytes()).sum();
        if total > THUMB_BUDGET {
            let mut old: Vec<(u64, Id)> = self.thumbs.iter().filter(|(_, s)| s.used + 1 < frame).map(|(id, s)| (s.used, *id)).collect();
            old.sort();
            for (_, id) in old {
                if total <= THUMB_BUDGET {
                    break;
                }
                let s = self.thumbs.remove(&id).unwrap();
                total -= s.bytes();
                self.renderer.write().free_texture(&s.id);
            }
        }
    }

    /// The finished picture of `item` at its full grid, as display values, for export.
    /// None until the full grid has arrived (it is then asked for).
    pub fn export_pixels(&mut self, item: &Item, planner: &mut Planner) -> Result<Option<(u32, u32, Vec<f32>)>, Vec<(Id, u32)>> {
        let long = self.grid_long(item);
        let Some(look) = &item.look else { return Ok(None) };
        match self.fulls.get_mut(&item.id) {
            Some(res) if res.photo.size().0.max(res.photo.size().1) == long => {
                res.used = self.frame;
                let plan = planner.plan(look, res.photo.size(), item.auto_ev(look), self.engine.gpu.max_dim);
                if self.engine.render(&mut res.photo, &plan).is_some() {
                    res.version += 1;
                }
                let view = plan.view.clone();
                res.plan = Some(plan);
                res.gen = Some(item.gen);
                Ok(Some(self.engine.read_display(&mut res.photo, &view)))
            }
            _ => {
                if self.loading.contains(&(item.id, long)) {
                    Err(Vec::new())
                } else {
                    Err(vec![(item.id, long)])
                }
            }
        }
    }

    /// Numbers for the trace: photos on the GPU at each tier and surfaces held.
    pub fn census(&self) -> (usize, usize, usize, usize) {
        (self.proxies.len(), self.fulls.len(), self.thumbs.len(), self.canvases.len())
    }
}

fn canvas_bits(c: &Canvas) -> [u32; 3] {
    [c.origin[0].to_bits(), c.origin[1].to_bits(), c.scale.to_bits()]
}
