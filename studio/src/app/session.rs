//! The library as the last session left it: its photos with their looks and stars, what was
//! selected, and how the window showed them. Written down at most once a second while it
//! changes and when the app quits, and read back when the app starts.

use super::{App, Mode};
use alexander_studio::library::Id;
use alexander_studio::params::Settings;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Serialize, Deserialize)]
struct Photo {
    path: PathBuf,
    copy: u32,
    look: Option<Settings>,
    stash: Option<Settings>,
    starred: bool,
}

#[derive(Serialize, Deserialize)]
struct Session {
    photos: Vec<Photo>,
    /// Places in `photos`.
    selected: Vec<usize>,
    focus: Option<usize>,
    mode: Mode,
    thumb_pt: f32,
    #[serde(default)]
    fill: bool,
    #[serde(default)]
    before_default: bool,
    starred_only: bool,
    last_dir: Option<PathBuf>,
    recent: Vec<PathBuf>,
    reference_at: usize,
    reference_shown: bool,
    reference_apart: bool,
}

/// Where the app keeps what it remembers.
pub fn data_dir() -> PathBuf {
    let home = std::env::home_dir().unwrap_or_default();
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/studio.alexander.app")
    } else {
        std::env::var_os("XDG_DATA_HOME").map_or_else(|| home.join(".local/share"), PathBuf::from).join("alexander-studio")
    }
}

fn file() -> PathBuf {
    data_dir().join("library.json")
}

/// What a session's file records, to tell whether it needs writing again.
pub type Saved = (u64, Mode, u32, usize, usize, [bool; 4]);

impl App {
    fn saved_key(&self) -> Saved {
        let r = &self.reference;
        (self.lib.revision(), self.mode, self.thumb_pt.to_bits(), self.recent.len(), r.at, [r.shown, r.apart, self.fill, self.before_default])
    }

    /// Open the library as the last session left it.
    pub(super) fn restore_session(&mut self) {
        let Some(s) = std::fs::read(file()).ok().and_then(|b| serde_json::from_slice::<Session>(&b).ok()) else { return };
        let ids: Vec<Id> = s.photos.into_iter().map(|p| self.lib.restore(p.path, p.copy, p.look, p.stash, p.starred)).collect();
        for &id in &ids {
            if let Some(it) = self.lib.get(id) {
                let path = it.path.clone();
                self.open(id, path);
            }
        }
        self.lib.set_starred_only(s.starred_only);
        let selected: Vec<Id> = s.selected.iter().filter_map(|&i| ids.get(i).copied()).collect();
        self.lib.set_selection(&selected, s.focus.and_then(|i| ids.get(i).copied()));
        self.mode = s.mode;
        self.thumb_pt = s.thumb_pt;
        self.fill = s.fill;
        self.before_default = s.before_default;
        self.last_dir = s.last_dir;
        self.recent = s.recent;
        self.reference.at = s.reference_at;
        self.reference.shown = s.reference_shown;
        self.reference.apart = s.reference_apart;
        self.saved = (self.saved_key(), Instant::now());
    }

    /// Write the session down when it has changed: at once when `now`, otherwise at most
    /// once a second, so a held slider does not write every frame. Returns how long a write
    /// still waits, for a frame to be asked for then.
    pub(super) fn save_session(&mut self, now: bool) -> Option<Duration> {
        let key = self.saved_key();
        if key == self.saved.0 {
            return None;
        }
        let wait = Duration::from_secs(1).saturating_sub(self.saved.1.elapsed());
        if !now && !wait.is_zero() {
            return Some(wait);
        }
        let place = |id: Id| self.lib.position(id);
        let s = Session {
            photos: self.lib.items().iter().map(|it| Photo { path: it.path.clone(), copy: it.copy, look: it.look.clone(), stash: it.stash.clone(), starred: it.starred }).collect(),
            selected: self.lib.selection().into_iter().filter_map(place).collect(),
            focus: self.lib.focus.and_then(place),
            mode: self.mode,
            thumb_pt: self.thumb_pt,
            fill: self.fill,
            before_default: self.before_default,
            starred_only: self.lib.starred_only(),
            last_dir: self.last_dir.clone(),
            recent: self.recent.clone(),
            reference_at: self.reference.at,
            reference_shown: self.reference.shown,
            reference_apart: self.reference.apart,
        };
        let (dir, tmp) = (data_dir(), file().with_extension("json.tmp"));
        let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&tmp, serde_json::to_vec(&s).unwrap_or_default())).and_then(|_| std::fs::rename(&tmp, file()));
        if let Err(e) = written {
            eprintln!("could not remember the library in {}: {e}", dir.display());
        }
        self.saved = (key, Instant::now());
        None
    }

    /// Put the folders of photos opened from disk at the top of Open Recent.
    pub(super) fn remember_folders(&mut self, paths: &[PathBuf]) {
        let cache = super::photos_cache();
        for p in paths.iter().rev().filter(|p| !p.starts_with(&cache)) {
            let folder = if p.is_dir() { p.clone() } else { p.parent().map(|d| d.to_path_buf()).unwrap_or_default() };
            self.recent.retain(|r| *r != folder);
            self.recent.insert(0, folder);
        }
        self.recent.truncate(10);
    }
}
