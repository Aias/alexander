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
use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROXY_BUDGET: u64 = 256 << 20;
const FULL_BUDGET: u64 = 1200 << 20;
const THUMB_BUDGET: u64 = 192 << 20;
/// Time spent on renders other than the focused photo's in one frame.
const FRAME_BUDGET: Duration = Duration::from_millis(12);
const HISTOGRAM_BINS: usize = 128;
/// Look generations of previews start here, above any a photo reaches by editing.
const PREVIEW_GEN: u64 = 1 << 62;

/// What a histogram counts: the photo, its look generation, whether the photo itself is
/// shown and whether the default look stands in for it.
type HistogramKey = (Id, u64, bool, bool);

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
    /// The frame this photo was last needed in, for a render or a draw.
    used: u64,
}

impl Resident {
    fn new(photo: Photo) -> Resident {
        Resident { photo, gen: None, plan: None, used: 0 }
    }

    fn bytes(&self) -> u64 {
        let (w, h) = self.photo.size();
        let k = self.plan.as_ref().map_or(1, |p| p.optics.scale as u64);
        // the photo itself, five kept stages, the finished picture with its half-size copies
        // and the default look standing in for the photo
        w as u64 * h as u64 * (4 + 18 * k * k + 4 * self.photo.has_before() as u64)
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
    /// Drawn from a render of the current look (or of no look), not an older one kept on
    /// screen while the new one is made.
    complete: bool,
    /// The default look stands in for the photo itself.
    before_default: bool,
    gen: u64,
    canvas: [u32; 7],
    size: [u32; 2],
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
    prefetch: Vec<(Id, u32)>,
    /// Full grids asked of the loaders and not yet arrived: photo and long edge.
    pub loading: HashSet<(Id, u32)>,
    frame: u64,
    /// The first stage the focused photo's last render had to redo, for the trace.
    pub last_focus_stage: Option<(Tier, Stage, f32)>,
    pub trace: bool,
    histogram: Option<(HistogramKey, Vec<f32>)>,
    /// A look shown on one photo without being given to it, and the generation it renders as.
    preview: Option<(Id, Settings, u64)>,
    previews: u64,
    /// Where a view shows the photo itself, a photo with a look shows the default look.
    before_default: bool,
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
            prefetch: Vec::new(),
            loading: HashSet::new(),
            frame: 1,
            last_focus_stage: None,
            trace: std::env::var("STUDIO_TRACE").is_ok(),
            histogram: None,
            preview: None,
            previews: 0,
            before_default: false,
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

    /// The part of the full grid a view shows: its top-left corner and size. The picture
    /// inside the border when the look strips it, otherwise the whole grid.
    pub fn shown(&self, item: &Item) -> ([f32; 2], (u32, u32), f32) {
        within(item, self.grid_size(item))
    }

    /// Show `look` on photo `id` in its renders without giving it to the photo, or stop.
    pub fn set_preview(&mut self, preview: Option<(Id, Settings)>) {
        if preview.as_ref().map(|(i, l)| (i, l)) != self.preview.as_ref().map(|(i, l, _)| (i, l)) {
            self.previews += 1;
            self.preview = preview.map(|(i, l)| (i, l, PREVIEW_GEN + self.previews));
        }
    }

    /// Show the default look where views show the photo itself, or stop and let go of its renders.
    pub fn set_before_default(&mut self, on: bool) {
        if self.before_default && !on {
            for r in self.proxies.values_mut().chain(self.fulls.values_mut()) {
                self.engine.drop_before(&mut r.photo);
            }
        }
        self.before_default = on;
    }

    /// Whether the default look stands in for `item` where a drawing shows the photo itself.
    fn shows_default(&self, item: &Item, photo_shown: bool) -> bool {
        self.before_default && photo_shown && item.look.is_some()
    }

    /// `item` as its renders show it: with the previewed look when it is the photo previewed.
    fn shown_item<'a>(&self, item: &'a Item) -> Cow<'a, Item> {
        match &self.preview {
            Some((id, look, gen)) if *id == item.id => Cow::Owned(Item { look: Some(look.clone()), gen: *gen, ..item.clone() }),
            _ => Cow::Borrowed(item),
        }
    }

    // ---------- asked for during layout ---------------------------------------------------

    /// A thumbnail of `item` for a box of `max` physical pixels, fitting inside it or, with
    /// `fill`, covering it: its texture (drawn at some size, maybe an older look) and the
    /// size it should be shown at.
    pub fn thumb(&mut self, item: &Item, max: [u32; 2], fill: bool, rank: u8) -> (Option<TextureId>, [u32; 2]) {
        let Some(p) = &item.proxy else { return (None, [0, 0]) };
        let (_, (pw, ph), _) = within(item, (p.w, p.h));
        let (sx, sy) = (max[0] as f32 / pw as f32, max[1] as f32 / ph as f32);
        let s = if fill { sx.max(sy) } else { sx.min(sy) };
        let size = [((pw as f32 * s).round() as u32).clamp(1, pw.max(1) * 2), ((ph as f32 * s).round() as u32).clamp(1, ph.max(1) * 2)];
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

    /// A thumbnail already drawn, to stand in while a larger picture is made.
    pub fn thumb_tex(&self, id: Id) -> Option<TextureId> {
        self.thumbs.get(&id).filter(|t| t.key.is_some()).map(|t| t.id)
    }

    /// Load the full grid of a photo likely to be shown next, without rendering it.
    pub fn prefetch(&mut self, item: &Item) {
        if item.ready() {
            let long = self.grid_long(item);
            self.prefetch.push((item.id, long));
        }
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
            let shown = self.shown_item(item);
            let item = &*shown;
            if item.proxy.is_none() {
                continue;
            }
            // the first request always renders; the rest share the frame's budget
            let may_render = first || start.elapsed() < FRAME_BUDGET;
            first = false;
            match r.want {
                Want::Thumb => {
                    // a thumbnail already showing the current look needs nothing from the
                    // GPU copy, which can then be let go of under the budget
                    let current = self.thumb_key(item, r.size, before);
                    if let Some(s) = self.thumbs.get_mut(&item.id).filter(|s| s.key == current) {
                        s.used = self.frame;
                        if r.rank == 0 {
                            self.touch(Tier::Proxy, item.id);
                        }
                        continue;
                    }
                    if !self.ensure(Tier::Proxy, item, may_render, r.rank == 0, self.shows_default(item, before)) {
                        pending = true;
                    }
                    self.draw_thumb(item, r.size, before);
                }
                Want::Canvas { canvas, grid } => {
                    let long = grid.0.max(grid.1);
                    // a canvas no larger than the proxy is drawn from the proxy
                    let wants_full = r.size[0].max(r.size[1]) as f32 > 1.15 * item.proxy.as_ref().map_or(0, |p| p.w.max(p.h)) as f32 || canvas.scale < 1.0;
                    let current = self.canvas_key(item, if wants_full { Tier::Full } else { Tier::Proxy }, canvas, grid, r.size, before);
                    if let Some(s) = self.canvases.get_mut(&item.id).filter(|s| s.key == current) {
                        s.used = self.frame;
                        // a full grid is read from disk again if let go, so the one on
                        // screen is kept, as is the proxy of the photo being edited
                        if let Some(f) = self.fulls.get_mut(&item.id).filter(|_| wants_full) {
                            f.used = self.frame;
                            full_bytes += f.bytes();
                        }
                        if r.rank == 0 {
                            self.touch(Tier::Proxy, item.id);
                        }
                        continue;
                    }
                    let mut tier = Tier::Proxy;
                    let default = self.shows_default(item, before || canvas.source != Canvas::RENDER);
                    if wants_full {
                        let have = self.fulls.get(&item.id).map(|f| f.photo.size());
                        let fits = full_bytes + (long as u64 * long as u64 * 14) < FULL_BUDGET;
                        if have.is_some_and(|s| s.0.max(s.1) == long) || (have.is_some() && fits) {
                            if have.is_some_and(|s| s.0.max(s.1) != long) && !self.loading.contains(&(item.id, long)) {
                                to_load.push((item.id, long));
                            }
                            let ok = self.ensure(Tier::Full, item, may_render, r.rank == 0, default);
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
                        pending |= !self.ensure(Tier::Proxy, item, may_render, r.rank == 0, default);
                    }
                    self.draw_canvas(item, tier, canvas, grid, r.size, before);
                }
            }
        }
        for (id, long) in std::mem::take(&mut self.prefetch) {
            let have = self.fulls.get(&id).is_some_and(|f| f.photo.size().0.max(f.photo.size().1) == long);
            let room = full_bytes + 2 * (long as u64 * long as u64 * 14) < FULL_BUDGET;
            if !have && room && !self.loading.contains(&(id, long)) && !to_load.contains(&(id, long)) {
                to_load.push((id, long));
            }
        }
        self.evict();
        self.frame += 1;
        (to_load, pending)
    }

    /// Put `item` on the GPU at `tier` (the proxy is uploaded on demand; a full grid must
    /// have arrived) and render it when its look changed, with a render of the default look
    /// kept first when `default`. False when a render was owed and left for later.
    fn ensure(&mut self, tier: Tier, item: &Item, may_render: bool, focused: bool, default: bool) -> bool {
        let frame = self.frame;
        if tier == Tier::Proxy && !self.proxies.contains_key(&item.id) {
            // uploads share the frame's budget with renders, so a screen of hundreds of
            // photos fills in over frames instead of holding all of them on the GPU at once
            if !may_render {
                return false;
            }
            let p = item.proxy.as_ref().unwrap();
            let photo = self.engine.upload(&p.rgba, p.w, p.h);
            self.proxies.insert(item.id, Resident::new(photo));
        }
        let map = if tier == Tier::Proxy { &mut self.proxies } else { &mut self.fulls };
        let Some(res) = map.get_mut(&item.id) else { return false };
        res.used = frame;
        let Some(look) = &item.look else { return true };
        let planner = &mut self.planners[tier as usize];
        if default && !res.photo.has_before() {
            if !may_render {
                return false;
            }
            let d = Settings::default();
            let plan = planner.plan(&d, res.photo.size(), item.auto_ev(&d), item.area(&d), self.engine.gpu.max_dim);
            self.engine.render(&mut res.photo, &plan);
            self.engine.keep_before(&mut res.photo, &plan.view);
            res.gen = None;
        }
        if !res.stale(item) {
            return true;
        }
        if !may_render {
            return false;
        }
        let plan = planner.plan(look, res.photo.size(), item.auto_ev(look), item.area(look), self.engine.gpu.max_dim);
        let t = Instant::now();
        if let Some(stage) = self.engine.render(&mut res.photo, &plan) {
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
            view_formats: &[wgpu::TextureFormat::Rgba8Unorm],
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
            let shown = texture.create_view(&wgpu::TextureViewDescriptor { format: Some(wgpu::TextureFormat::Rgba8Unorm), ..Default::default() });
            let map = if thumbs { &mut self.thumbs } else { &mut self.canvases };
            match map.get_mut(&id) {
                Some(s) => {
                    self.renderer.write().update_egui_texture_from_wgpu_texture(&self.device, &shown, filter, s.id);
                    s.texture.destroy();
                    s.texture = texture;
                    s.view = tview;
                    s.size = size;
                }
                None => {
                    let tid = self.renderer.write().register_native_texture(&self.device, &shown, filter);
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

    /// The key of a thumbnail of `item` drawn from a current render.
    fn thumb_key(&self, item: &Item, size: [u32; 2], before: bool) -> Option<DrawKey> {
        let p = item.proxy.as_ref()?;
        let source = before || item.look.is_none();
        let canvas = thumb_canvas(item, (p.w, p.h), size, source);
        Some(DrawKey { tier: Tier::Proxy, complete: true, before_default: self.shows_default(item, before), gen: item.gen, canvas: canvas_bits(&canvas), size })
    }

    /// The key of a canvas of `item` drawn from a current render at `tier`. A full grid
    /// counts only at the size the look asks for.
    #[allow(clippy::too_many_arguments)]
    fn canvas_key(&self, item: &Item, tier: Tier, canvas: Canvas, grid: (u32, u32), size: [u32; 2], before: bool) -> Option<DrawKey> {
        let pw = match tier {
            Tier::Proxy => item.proxy.as_ref()?.w,
            Tier::Full => grid.0,
        };
        let c = scaled(canvas, pw, grid, before || item.look.is_none());
        Some(DrawKey { tier, complete: true, before_default: self.shows_default(item, c.source != Canvas::RENDER), gen: item.gen, canvas: canvas_bits(&c), size })
    }

    /// Mark a photo on the GPU as needed this frame, so it is not let go of.
    fn touch(&mut self, tier: Tier, id: Id) {
        let map = if tier == Tier::Proxy { &mut self.proxies } else { &mut self.fulls };
        if let Some(r) = map.get_mut(&id) {
            r.used = self.frame;
        }
    }

    fn draw_thumb(&mut self, item: &Item, size: [u32; 2], before: bool) {
        let Some(res) = self.proxies.get(&item.id) else { return };
        if !res.showable(item) {
            return;
        }
        let source = before || item.look.is_none();
        let canvas = thumb_canvas(item, res.photo.size(), size, source);
        let default = self.shows_default(item, before);
        let key = DrawKey { tier: Tier::Proxy, complete: !res.stale(item) && (!default || res.photo.has_before()), before_default: default, gen: item.gen, canvas: canvas_bits(&canvas), size };
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
        let c = scaled(canvas, res.photo.size().0, grid, before || item.look.is_none());
        let default = self.shows_default(item, c.source != Canvas::RENDER);
        let key = DrawKey { tier, complete: !res.stale(item) && (!default || res.photo.has_before()), before_default: default, gen: item.gen, canvas: canvas_bits(&c), size };
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
    pub fn export_pixels(&mut self, item: &Item, planner: &mut Planner) -> Result<Option<Display>, Vec<(Id, u32)>> {
        let grid = self.grid_size(item);
        let long = self.grid_long(item);
        let Some(look) = &item.look else { return Ok(None) };
        match self.fulls.get_mut(&item.id) {
            Some(res) if res.photo.size().0.max(res.photo.size().1) == long => {
                res.used = self.frame;
                let plan = planner.plan(look, res.photo.size(), item.auto_ev(look), item.area(look), self.engine.gpu.max_dim);
                self.engine.render(&mut res.photo, &plan);
                let view = plan.view.clone();
                res.plan = Some(plan);
                res.gen = Some(item.gen);
                let (origin, (w, h), angle) = within(item, grid);
                let canvas = Canvas { origin, scale: 1.0, source: Canvas::RENDER, clip: false, angle };
                Ok(Some((w, h, self.engine.read_canvas(&mut res.photo, &view, &canvas, w, h))))
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

    /// How often each display value is shown, in `HISTOGRAM_BINS` bins from black to white:
    /// counted on the proxy's render of the look, or on the proxy itself, inside the crop.
    pub fn histogram(&mut self, item: &Item, before: bool) -> Option<&[f32]> {
        let shown = self.shown_item(item);
        let item = &*shown;
        let source = before || item.look.is_none();
        let default = self.shows_default(item, before);
        let key = (item.id, item.gen, source, default);
        if self.histogram.as_ref().is_none_or(|h| h.0 != key) {
            item.proxy.as_ref()?;
            self.ensure(Tier::Proxy, item, true, false, default);
            let res = self.proxies.get_mut(&item.id)?;
            let view = Self::view_of(res, item);
            let (origin, (w, h), angle) = within(item, res.photo.size());
            let canvas = Canvas { origin, scale: 2.0, source: if source { Canvas::PHOTO } else { Canvas::RENDER }, clip: false, angle };
            let values = self.engine.read_canvas(&mut res.photo, &view, &canvas, w.div_ceil(2), h.div_ceil(2));
            let mut bins = vec![0.0; HISTOGRAM_BINS];
            for v in values.chunks(4).filter(|v| v[3] > 0.0) {
                let luma = 0.2126 * v[0] + 0.7152 * v[1] + 0.0722 * v[2];
                bins[(luma.clamp(0.0, 1.0) * (HISTOGRAM_BINS - 1) as f32).round() as usize] += 1.0;
            }
            self.histogram = Some((key, bins));
        }
        self.histogram.as_ref().map(|h| h.1.as_slice())
    }

    /// Numbers for the trace: photos on the GPU at each tier and surfaces held.
    pub fn census(&self) -> (usize, usize, usize, usize) {
        (self.proxies.len(), self.fulls.len(), self.thumbs.len(), self.canvases.len())
    }
}

/// Width, height and display values, four floats a pixel.
pub type Display = (u32, u32, Vec<f32>);

/// `canvas`, given in the coordinates of a full grid of `grid`, for a photo `pw` wide;
/// showing the photo itself throughout when `photo`.
fn scaled(canvas: Canvas, pw: u32, grid: (u32, u32), photo: bool) -> Canvas {
    let k = pw as f32 / grid.0 as f32;
    let source = if photo { Canvas::PHOTO } else { canvas.source };
    Canvas { origin: [canvas.origin[0] * k, canvas.origin[1] * k], scale: canvas.scale * k, source, ..canvas }
}

/// The part of a `size` picture of `item` that is shown: the top-left corner and size of its
/// frame in the picture's pixels, and the angle in radians the picture turns under it.
fn within(item: &Item, size: (u32, u32)) -> ([f32; 2], (u32, u32), f32) {
    match item.crop() {
        Some(c) => {
            let at = |f: f64, n: u32| (f * n as f64).round().clamp(0.0, n as f64) as u32;
            let (x0, y0, x1, y1) = (at(c.left, size.0), at(c.top, size.1), at(c.right, size.0), at(c.bottom, size.1));
            ([x0 as f32, y0 as f32], ((x1 - x0).max(1), (y1 - y0).max(1)), c.angle.to_radians() as f32)
        }
        None => ([0.0, 0.0], size, 0.0),
    }
}

/// A thumbnail of `size` drawn from the proxy, of `proxy` pixels.
fn thumb_canvas(item: &Item, proxy: (u32, u32), size: [u32; 2], source: bool) -> Canvas {
    let (origin, (w, _), angle) = within(item, proxy);
    Canvas { origin, scale: w as f32 / size[0] as f32, source: if source { Canvas::PHOTO } else { Canvas::RENDER }, clip: false, angle }
}

fn canvas_bits(c: &Canvas) -> [u32; 7] {
    [c.origin[0].to_bits(), c.origin[1].to_bits(), c.scale.to_bits(), c.source[0].to_bits(), c.source[1].to_bits(), c.clip as u32, c.angle.to_bits()]
}
