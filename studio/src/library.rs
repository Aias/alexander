//! The photos the user has opened, which of them are selected, and the look each one has.
//! Pure bookkeeping: no pixels and no GPU, so it can be tested on its own.

use crate::film::{FilmKind, Filter};
use crate::params::Settings;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Ids grow with every photo added and the library is never reordered, so sorting ids
/// gives library order.
pub type Id = u64;

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Loading,
    Ready,
    Failed(String),
}

/// A small copy of the photo, 8-bit sRGB with four bytes a pixel.
pub struct Proxy {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Median and 98th percentile of the photo as each film and filter sees it, for automatic
/// exposure. Indexed by film (in `FilmKind::ALL` order) and filter.
pub type Levels = [[(f64, f64); 4]; 3];

pub fn film_index(k: FilmKind) -> usize {
    FilmKind::ALL.iter().position(|&f| f == k).unwrap_or(0)
}

pub struct Item {
    pub id: Id,
    pub path: PathBuf,
    pub name: String,
    pub status: Status,
    /// Pixel size of the file once turned upright.
    pub original: (u32, u32),
    pub proxy: Option<Arc<Proxy>>,
    pub levels: Option<Levels>,
    /// None shows the photo as it is.
    pub look: Option<Settings>,
    /// The look set aside when the process is switched off, given back when it is switched on.
    pub stash: Option<Settings>,
    /// Bumped whenever `look` changes, so renders can tell they are stale without comparing settings.
    pub gen: u64,
}

impl Item {
    pub fn auto_ev(&self, s: &Settings) -> f64 {
        match &self.levels {
            Some(l) => {
                let (median, top) = l[film_index(s.film)][s.filter as usize];
                crate::tone::auto_ev(median, top, s.auto_ev_target)
            }
            None => 0.0,
        }
    }

    pub fn ready(&self) -> bool {
        self.status == Status::Ready
    }
}

/// The looks of some photos before a change, for undo.
pub type Snapshot = Vec<(Id, Option<Settings>, Option<Settings>)>;

#[derive(Default)]
pub struct Library {
    items: Vec<Item>,
    index: HashMap<Id, usize>,
    next_id: Id,
    pub selected: BTreeSet<Id>,
    pub focus: Option<Id>,
    anchor: Option<Id>,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Library {
    pub fn new() -> Library {
        Library { next_id: 1, ..Default::default() }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn at(&self, i: usize) -> &Item {
        &self.items[i]
    }

    pub fn position(&self, id: Id) -> Option<usize> {
        self.index.get(&id).copied()
    }

    pub fn get(&self, id: Id) -> Option<&Item> {
        self.index.get(&id).map(|&i| &self.items[i])
    }

    pub fn get_mut(&mut self, id: Id) -> Option<&mut Item> {
        self.index.get(&id).map(|&i| &mut self.items[i])
    }

    pub fn find_path(&self, path: &Path) -> Option<Id> {
        self.items.iter().find(|it| it.path == path).map(|it| it.id)
    }

    /// Add a photo unless the same file is already in the library. Returns the new id.
    pub fn add(&mut self, path: PathBuf) -> Option<Id> {
        if self.find_path(&path).is_some() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.index.insert(id, self.items.len());
        self.items.push(Item { id, path, name, status: Status::Loading, original: (0, 0), proxy: None, levels: None, look: None, stash: None, gen: 0 });
        Some(id)
    }

    /// Take photos out of the library (never off the disk). Focus moves to the photo after
    /// the first one removed.
    pub fn remove(&mut self, ids: &BTreeSet<Id>) {
        if ids.is_empty() {
            return;
        }
        let first = ids.iter().filter_map(|id| self.position(*id)).min();
        self.items.retain(|it| !ids.contains(&it.id));
        self.reindex();
        self.selected.retain(|id| !ids.contains(id));
        for stack in [&mut self.undo, &mut self.redo] {
            for snap in stack.iter_mut() {
                snap.retain(|(id, _, _)| !ids.contains(id));
            }
            stack.retain(|s| !s.is_empty());
        }
        if self.focus.is_some_and(|f| ids.contains(&f)) {
            self.focus = None;
            if self.selected.is_empty() {
                if let Some(i) = first {
                    if !self.items.is_empty() {
                        let id = self.items[i.min(self.items.len() - 1)].id;
                        self.selected.insert(id);
                        self.focus = Some(id);
                    }
                }
            } else {
                self.focus = self.selected.iter().next().copied();
            }
        }
        if self.anchor.is_some_and(|a| ids.contains(&a)) {
            self.anchor = self.focus;
        }
    }

    fn reindex(&mut self) {
        self.index = self.items.iter().enumerate().map(|(i, it)| (it.id, i)).collect();
    }

    // ---------- selection ----------------------------------------------------------------

    /// A plain click: this photo alone.
    pub fn click(&mut self, id: Id) {
        if self.position(id).is_none() {
            return;
        }
        self.selected.clear();
        self.selected.insert(id);
        self.focus = Some(id);
        self.anchor = Some(id);
    }

    /// Command-click: add or take away one photo.
    pub fn toggle(&mut self, id: Id) {
        if self.position(id).is_none() {
            return;
        }
        if self.selected.remove(&id) {
            if self.focus == Some(id) {
                self.focus = self.nearest_selected(id);
            }
        } else {
            self.selected.insert(id);
            self.focus = Some(id);
        }
        self.anchor = Some(id);
    }

    /// Shift-click: everything from the anchor to this photo.
    pub fn extend(&mut self, id: Id) {
        let Some(to) = self.position(id) else { return };
        let from = self.anchor.and_then(|a| self.position(a)).unwrap_or(to);
        let (a, b) = (from.min(to), from.max(to));
        self.selected = self.items[a..=b].iter().map(|it| it.id).collect();
        self.focus = Some(id);
    }

    pub fn select_all(&mut self) {
        self.selected = self.items.iter().map(|it| it.id).collect();
        if self.focus.is_none() {
            self.focus = self.items.first().map(|it| it.id);
            self.anchor = self.focus;
        }
    }

    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    fn nearest_selected(&self, id: Id) -> Option<Id> {
        self.selected.range(id..).next().or_else(|| self.selected.range(..id).next_back()).copied()
    }

    /// Move the focus `delta` places through the library. With `extend` the selection
    /// grows from the anchor as with shift-click; otherwise the new photo is selected alone.
    pub fn step(&mut self, delta: isize, extend: bool) {
        if self.items.is_empty() {
            return;
        }
        let cur = self.focus.and_then(|f| self.position(f));
        let next = match cur {
            None => 0,
            Some(i) => (i as isize + delta).clamp(0, self.items.len() as isize - 1) as usize,
        };
        let id = self.items[next].id;
        if extend {
            if self.anchor.is_none() {
                self.anchor = self.focus.or(Some(id));
            }
            self.extend(id);
        } else {
            self.click(id);
        }
    }

    /// Move the focus within the selection, leaving the selection as it is.
    pub fn step_in_selection(&mut self, delta: isize) {
        let ids: Vec<Id> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        let i = self.focus.and_then(|f| ids.iter().position(|&x| x == f)).unwrap_or(0);
        let j = (i as isize + delta).clamp(0, ids.len() as isize - 1) as usize;
        self.focus = Some(ids[j]);
    }

    /// Selected photos in library order.
    pub fn selection(&self) -> Vec<Id> {
        self.selected.iter().copied().collect()
    }

    // ---------- looks ----------------------------------------------------------------------

    fn snapshot(&self, ids: &[Id]) -> Snapshot {
        ids.iter().filter_map(|&id| self.get(id).map(|it| (id, it.look.clone(), it.stash.clone()))).collect()
    }

    /// Remember the looks of `ids` so the change about to be made can be undone.
    pub fn checkpoint(&mut self, ids: &[Id]) {
        let snap = self.snapshot(ids);
        if !snap.is_empty() {
            self.undo.push(snap);
            self.redo.clear();
            if self.undo.len() > 200 {
                self.undo.remove(0);
            }
        }
    }

    fn restore(&mut self, snap: Snapshot) -> Snapshot {
        let ids: Vec<Id> = snap.iter().map(|s| s.0).collect();
        let inverse = self.snapshot(&ids);
        for (id, look, stash) in snap {
            if let Some(it) = self.get_mut(id) {
                it.look = look;
                it.stash = stash;
                it.gen += 1;
            }
        }
        inverse
    }

    /// Returns the photos whose looks changed.
    pub fn undo(&mut self) -> Vec<Id> {
        let Some(s) = self.undo.pop() else { return Vec::new() };
        let ids = s.iter().map(|x| x.0).collect();
        let inv = self.restore(s);
        self.redo.push(inv);
        ids
    }

    pub fn redo(&mut self) -> Vec<Id> {
        let Some(s) = self.redo.pop() else { return Vec::new() };
        let ids = s.iter().map(|x| x.0).collect();
        let inv = self.restore(s);
        self.undo.push(inv);
        ids
    }

    pub fn set_look(&mut self, ids: &[Id], look: Option<Settings>) {
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                if it.look != look {
                    it.look = look.clone();
                    it.gen += 1;
                }
                if look.is_none() {
                    it.stash = None;
                }
            }
        }
    }

    /// Carry the knobs that differ between `before` and `after` over to every photo in
    /// `ids`, leaving their other knobs alone. A photo with no look starts from the defaults.
    pub fn apply_change(&mut self, ids: &[Id], before: &Settings, after: &Settings) {
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                let mut look = it.look.take().or_else(|| it.stash.take()).unwrap_or_default();
                sync_changed(before, after, &mut look);
                it.look = Some(look);
                it.gen += 1;
            }
        }
    }

    /// Switch the process on for all of `ids` when any is off, otherwise off for all.
    /// A look switched off is kept and comes back when switched on again.
    pub fn toggle_process(&mut self, ids: &[Id]) {
        let any_off = ids.iter().any(|&id| self.get(id).is_some_and(|it| it.look.is_none()));
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                if any_off {
                    if it.look.is_none() {
                        it.look = Some(it.stash.take().unwrap_or_default());
                        it.gen += 1;
                    }
                } else if let Some(l) = it.look.take() {
                    it.stash = Some(l);
                    it.gen += 1;
                }
            }
        }
    }
}

macro_rules! sync_fields {
    ($before:ident, $after:ident, $target:ident; $($f:ident),* $(,)?) => {
        $( if $before.$f != $after.$f { $target.$f = $after.$f.clone(); } )*
    };
}

/// Copy onto `target` each knob that differs between `before` and `after`.
pub fn sync_changed(before: &Settings, after: &Settings, target: &mut Settings) {
    // keep this list in step with `Settings`: tests/library check every field
    sync_fields!(before, after, target;
        detail_px, seed, film, film_mm, filter, lens_um, lens_exponent, halation, halation_um,
        ev, auto_ev, auto_ev_target, dev, grain, grain_light, grain_dev, edge, edge_density, no_edge,
        grade, print_ev, print_grey, enlarger_um, house,
        copy_blur, copy_gain, copy_drop, copy_fill, copy_ink,
        repro, page_mm, ppi, lpi, ink_density, dot_gain, min_dot, copy_blur_mm, paper_scatter_mm,
        view, scan_black, scan_white);
}

/// The filter a look's colour response comes from, as an index for `Levels`.
pub fn levels_key(s: &Settings) -> (usize, usize) {
    (film_index(s.film), s.filter as usize)
}

pub fn all_weightings() -> impl Iterator<Item = (FilmKind, Filter)> {
    FilmKind::ALL.into_iter().flat_map(|f| Filter::ALL.into_iter().map(move |x| (f, x)))
}
