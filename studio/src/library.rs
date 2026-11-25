//! The photos the user has opened, which of them are selected, and the look each one has.
//! Pure bookkeeping: no pixels and no GPU, so it can be tested on its own.

use crate::film::{FilmKind, Filter};
use crate::frame::Area;
use crate::params::{Frame, Settings};
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
    /// The picture inside a uniform border, when the photo has one, and its levels.
    pub frame: Option<(Area, Levels)>,
    /// None shows the photo as it is.
    pub look: Option<Settings>,
    /// The look set aside when the process is switched off, given back when it is switched on.
    pub stash: Option<Settings>,
    /// Bumped whenever `look` changes, so renders can tell they are stale without comparing settings.
    pub gen: u64,
    pub starred: bool,
}

impl Item {
    pub fn auto_ev(&self, s: &Settings) -> f64 {
        let levels = match (&self.frame, s.frame) {
            (Some((_, inside)), Frame::Keep | Frame::Strip) => Some(inside),
            _ => self.levels.as_ref(),
        };
        match levels {
            Some(l) => {
                let (median, top) = l[film_index(s.film)][s.filter as usize];
                crate::tone::auto_ev(median, top, s.auto_ev_target)
            }
            None => 0.0,
        }
    }

    /// The picture area the look measures inside, if any.
    pub fn area(&self, s: &Settings) -> Option<Area> {
        match (&self.frame, s.frame) {
            (Some((a, _)), Frame::Keep | Frame::Strip) => Some(*a),
            _ => None,
        }
    }

    /// The part of the photo shown and exported: the picture area when the look strips the
    /// border, otherwise None for the whole.
    pub fn crop(&self) -> Option<Area> {
        let look = self.look.as_ref()?;
        if look.frame == Frame::Strip {
            self.area(look)
        } else {
            None
        }
    }

    pub fn ready(&self) -> bool {
        self.status == Status::Ready
    }
}

/// The looks of some photos before a change, for undo.
pub type Snapshot = Vec<(Id, Option<Settings>, Option<Settings>)>;

/// A change undo and redo take back.
enum Step {
    /// The looks of some photos before a change.
    Looks(Snapshot),
    /// Photos taken out of the library, each with its place in it.
    Removed(Vec<(usize, Item)>),
    /// Photos put back into the library.
    Restored(Vec<Id>),
}

#[derive(Default)]
pub struct Library {
    items: Vec<Item>,
    index: HashMap<Id, usize>,
    by_path: HashMap<PathBuf, Id>,
    /// Bumped by every change that can alter what the views show or count, so the app
    /// can keep figures over the selection without walking it each frame.
    rev: u64,
    next_id: Id,
    selected: BTreeSet<Id>,
    pub focus: Option<Id>,
    anchor: Option<Id>,
    starred_only: bool,
    /// The photos the views show, in library order: every photo, or only the starred ones.
    shown: Vec<Id>,
    shown_index: HashMap<Id, usize>,
    undo: Vec<Step>,
    redo: Vec<Step>,
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
        self.rev += 1;
        self.index.get(&id).map(|&i| &mut self.items[i])
    }

    pub fn find_path(&self, path: &Path) -> Option<Id> {
        self.by_path.get(path).copied()
    }

    /// Point a photo at another file (an export found to stand for its original).
    pub fn set_path(&mut self, id: Id, path: PathBuf) {
        let Some(&i) = self.index.get(&id) else { return };
        self.rev += 1;
        let it = &mut self.items[i];
        self.by_path.remove(&it.path);
        it.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        it.path = path.clone();
        self.by_path.insert(path, id);
    }

    /// Changes with every edit to the library, its selection or its looks.
    pub fn revision(&self) -> u64 {
        self.rev
    }

    /// Add a photo unless the same file is already in the library. Returns the new id.
    pub fn add(&mut self, path: PathBuf) -> Option<Id> {
        if self.find_path(&path).is_some() {
            return None;
        }
        let id = self.next_id;
        self.next_id += 1;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.rev += 1;
        self.by_path.insert(path.clone(), id);
        self.index.insert(id, self.items.len());
        self.items.push(Item { id, path, name, status: Status::Loading, original: (0, 0), proxy: None, levels: None, frame: None, look: None, stash: None, gen: 0, starred: false });
        if !self.starred_only {
            self.shown_index.insert(id, self.shown.len());
            self.shown.push(id);
        }
        Some(id)
    }

    /// Take photos out of the library (never off the disk) so that undo can put them back.
    pub fn remove(&mut self, ids: &BTreeSet<Id>) {
        let taken = self.take_out(ids);
        if !taken.is_empty() {
            self.push_undo(Step::Removed(taken));
        }
    }

    /// Take photos out of the library, giving each with the place it had. Their pixels are
    /// let go of, so a photo put back is opened again. Focus moves to the photo after the
    /// first one taken out.
    pub fn take_out(&mut self, ids: &BTreeSet<Id>) -> Vec<(usize, Item)> {
        if ids.is_empty() {
            return Vec::new();
        }
        let first = ids.iter().filter_map(|id| self.shown_position(*id)).min();
        self.rev += 1;
        let mut taken = Vec::new();
        for (i, mut it) in std::mem::take(&mut self.items).into_iter().enumerate() {
            if ids.contains(&it.id) {
                it.proxy = None;
                it.status = Status::Loading;
                taken.push((i, it));
            } else {
                self.items.push(it);
            }
        }
        self.by_path.retain(|_, id| !ids.contains(id));
        self.reindex();
        self.selected.retain(|id| !ids.contains(id));
        if self.focus.is_some_and(|f| ids.contains(&f)) {
            self.focus = None;
            if self.selected.is_empty() {
                if let Some(i) = first {
                    if let Some(&id) = self.shown.get(i.min(self.shown.len().saturating_sub(1))) {
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
        taken
    }

    /// Put photos taken out back at their places, selected.
    fn put_back(&mut self, items: Vec<(usize, Item)>) {
        self.rev += 1;
        self.selected.clear();
        for (i, it) in items {
            self.by_path.insert(it.path.clone(), it.id);
            self.selected.insert(it.id);
            self.items.insert(i.min(self.items.len()), it);
        }
        self.reindex();
        self.focus = self.selected.iter().next().copied();
        self.anchor = self.focus;
        self.drop_hidden();
    }

    fn reindex(&mut self) {
        self.index = self.items.iter().enumerate().map(|(i, it)| (it.id, i)).collect();
        self.shown = self.items.iter().filter(|it| it.starred || !self.starred_only).map(|it| it.id).collect();
        self.shown_index = self.shown.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    }

    /// Take the photos the views do not show out of the selection, the focus and the anchor.
    fn drop_hidden(&mut self) {
        self.selected.retain(|id| self.shown_index.contains_key(id));
        if self.focus.is_some_and(|f| !self.shown_index.contains_key(&f)) {
            self.focus = self.selected.iter().next().copied();
        }
        if self.anchor.is_some_and(|a| !self.shown_index.contains_key(&a)) {
            self.anchor = self.focus;
        }
    }

    /// The photos the views show, in library order.
    pub fn shown(&self) -> &[Id] {
        &self.shown
    }

    pub fn shown_position(&self, id: Id) -> Option<usize> {
        self.shown_index.get(&id).copied()
    }

    pub fn starred_only(&self) -> bool {
        self.starred_only
    }

    /// Show only the starred photos, or every photo. Photos hidden leave the selection.
    pub fn set_starred_only(&mut self, on: bool) {
        self.rev += 1;
        self.starred_only = on;
        self.reindex();
        self.drop_hidden();
    }

    /// Star the photos, or take their stars away when every one of them has a star.
    pub fn toggle_starred(&mut self, ids: &[Id]) {
        let on = ids.iter().any(|&id| self.get(id).is_some_and(|it| !it.starred));
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                it.starred = on;
            }
        }
        self.reindex();
        self.drop_hidden();
    }

    // ---------- selection ----------------------------------------------------------------

    /// A plain click: this photo alone.
    pub fn click(&mut self, id: Id) {
        self.rev += 1;
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
        self.rev += 1;
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
        self.rev += 1;
        let Some(to) = self.shown_position(id) else { return };
        let from = self.anchor.and_then(|a| self.shown_position(a)).unwrap_or(to);
        let (a, b) = (from.min(to), from.max(to));
        self.selected = self.shown[a..=b].iter().copied().collect();
        self.focus = Some(id);
    }

    pub fn select_all(&mut self) {
        self.rev += 1;
        self.selected = self.shown.iter().copied().collect();
        if self.focus.is_none() {
            self.focus = self.shown.first().copied();
            self.anchor = self.focus;
        }
    }

    pub fn select_none(&mut self) {
        self.rev += 1;
        self.selected.clear();
    }

    fn nearest_selected(&self, id: Id) -> Option<Id> {
        self.selected.range(id..).next().or_else(|| self.selected.range(..id).next_back()).copied()
    }

    /// Move the focus `delta` places through the photos shown. With `extend` the selection
    /// grows from the anchor as with shift-click; otherwise the new photo is selected alone.
    pub fn step(&mut self, delta: isize, extend: bool) {
        if self.shown.is_empty() {
            return;
        }
        let cur = self.focus.and_then(|f| self.shown_position(f));
        let next = match cur {
            None => 0,
            Some(i) => (i as isize + delta).clamp(0, self.shown.len() as isize - 1) as usize,
        };
        let id = self.shown[next];
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
        self.rev += 1;
        let ids: Vec<Id> = self.selected.iter().copied().collect();
        if ids.is_empty() {
            return;
        }
        let i = self.focus.and_then(|f| ids.iter().position(|&x| x == f)).unwrap_or(0);
        let j = (i as isize + delta).clamp(0, ids.len() as isize - 1) as usize;
        self.focus = Some(ids[j]);
    }

    pub fn selected(&self) -> &BTreeSet<Id> {
        &self.selected
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
            self.push_undo(Step::Looks(snap));
        }
    }

    fn push_undo(&mut self, step: Step) {
        self.undo.push(step);
        self.redo.clear();
        if self.undo.len() > 200 {
            self.undo.remove(0);
        }
    }

    fn restore(&mut self, snap: Snapshot) -> Snapshot {
        self.rev += 1;
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

    /// Take a step back: returns the step that makes it again and the photos it touched.
    fn apply(&mut self, step: Step) -> (Step, Vec<Id>) {
        match step {
            Step::Looks(s) => {
                let ids = s.iter().map(|x| x.0).collect();
                (Step::Looks(self.restore(s)), ids)
            }
            Step::Removed(items) => {
                let ids: Vec<Id> = items.iter().map(|(_, it)| it.id).collect();
                self.put_back(items);
                (Step::Restored(ids.clone()), ids)
            }
            Step::Restored(ids) => (Step::Removed(self.take_out(&ids.iter().copied().collect())), ids),
        }
    }

    /// Returns the photos whose looks changed or that came back or went.
    pub fn undo(&mut self) -> Vec<Id> {
        let Some(s) = self.undo.pop() else { return Vec::new() };
        let (inv, ids) = self.apply(s);
        self.redo.push(inv);
        ids
    }

    pub fn redo(&mut self) -> Vec<Id> {
        let Some(s) = self.redo.pop() else { return Vec::new() };
        let (inv, ids) = self.apply(s);
        self.undo.push(inv);
        ids
    }

    pub fn set_look(&mut self, ids: &[Id], look: Option<Settings>) {
        self.rev += 1;
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

    /// Put the film settings of photos back to the defaults and leave the process on or off.
    /// A photo shown as it is starts from the defaults when the process is switched on again.
    pub fn reset_looks(&mut self, ids: &[Id]) {
        self.rev += 1;
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                if it.look.as_ref().is_some_and(|l| *l != Settings::default()) {
                    it.look = Some(Settings::default());
                    it.gen += 1;
                }
                it.stash = None;
            }
        }
    }

    /// Carry the knobs that differ between `before` and `after` over to every photo in
    /// `ids`, leaving their other knobs alone. A photo with no look starts from the defaults.
    pub fn apply_change(&mut self, ids: &[Id], before: &Settings, after: &Settings) {
        self.rev += 1;
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
    pub fn toggle_process(&mut self, ids: &[Id]) {
        let any_off = ids.iter().any(|&id| self.get(id).is_some_and(|it| it.look.is_none()));
        self.set_process(ids, any_off);
    }

    /// Switch the process on or off. A look switched off is kept and comes back when it is
    /// switched on again; a photo never processed starts from the defaults.
    pub fn set_process(&mut self, ids: &[Id], on: bool) {
        self.rev += 1;
        for &id in ids {
            if let Some(it) = self.get_mut(id) {
                if on && it.look.is_none() {
                    it.look = Some(it.stash.take().unwrap_or_default());
                    it.gen += 1;
                } else if !on {
                    if let Some(l) = it.look.take() {
                        it.stash = Some(l);
                        it.gen += 1;
                    }
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
        detail_px, seed, film, frame, film_mm, filter, lens_um, lens_exponent, halation, halation_um,
        ev, auto_ev, auto_ev_target, dev, grain, grain_light, grain_dev, edge, edge_density, no_edge,
        grade, print_ev, print_grey, enlarger_um, house,
        copy_blur, copy_gain, copy_drop, copy_fill, copy_ink,
        repro, page_mm, ppi, lpi, ink_density, dot_gain, min_dot, copy_blur_mm, paper_scatter_mm,
        view, scan_black, scan_white);
}

pub fn all_weightings() -> impl Iterator<Item = (FilmKind, Filter)> {
    FilmKind::ALL.into_iter().flat_map(|f| Filter::ALL.into_iter().map(move |x| (f, x)))
}
