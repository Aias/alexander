//! What can be done to the library and its photos, whether asked for by a key, a button,
//! a photo's menu or the menu bar.

use super::{App, Compare, Mode};
use alexander_studio::plan::Planner;
#[cfg(target_os = "macos")]
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum Command {
    Open,
    #[cfg(target_os = "macos")]
    OpenPhotos,
    #[cfg(target_os = "macos")]
    OpenRecent(PathBuf),
    #[cfg(target_os = "macos")]
    ClearRecent,
    Export,
    SaveLook,
    LoadLook,
    CopyFlags,
    Undo,
    Redo,
    SelectAll,
    SelectNone,
    CopyLook,
    PasteLook,
    ResetLook,
    /// None switches it over; otherwise on or off.
    Process(Option<bool>),
    Star,
    VirtualCopy,
    CopyImage,
    #[cfg(target_os = "macos")]
    ShowInFinder,
    Remove,
    View(Mode),
    Compare(Compare),
    Swap,
    Clipping,
    /// Closer, or with false further away: larger or smaller thumbnails in the grid.
    Zoom(bool),
    Fit,
    Actual,
    StarredOnly,
    Panel,
    Reference,
}

impl App {
    /// Whether the command can be carried out now, for menus to show it enabled.
    pub(super) fn can(&mut self, c: &Command) -> bool {
        let selected = !self.lib.selected().is_empty();
        let focus = self.lib.focus.and_then(|f| self.lib.get(f));
        match c {
            Command::Export => self.exportable() > 0 && self.export_run.is_none(),
            Command::SaveLook | Command::CopyFlags | Command::CopyLook | Command::ResetLook => focus.is_some_and(|it| it.look.is_some()),
            Command::CopyImage => focus.is_some_and(|it| it.ready() && it.look.is_some()) && self.copy_image.is_none(),
            Command::PasteLook => selected && self.clipboard.is_some(),
            Command::Process(_) | Command::Star | Command::VirtualCopy | Command::Remove | Command::LoadLook => selected,
            #[cfg(target_os = "macos")]
            Command::ShowInFinder => selected,
            Command::View(Mode::Loupe) => focus.is_some(),
            Command::Swap => self.compare == Compare::Split,
            Command::Zoom(_) => self.mode != Mode::Survey,
            Command::Fit | Command::Actual => self.mode == Mode::Loupe,
            _ => true,
        }
    }

    pub(super) fn run(&mut self, ctx: &egui::Context, c: Command) {
        if !self.can(&c) {
            return;
        }
        match c {
            Command::Open => self.import_dialog(),
            #[cfg(target_os = "macos")]
            Command::OpenPhotos => self.photos_dialog(),
            #[cfg(target_os = "macos")]
            Command::OpenRecent(folder) => self.import(vec![folder]),
            #[cfg(target_os = "macos")]
            Command::ClearRecent => self.recent.clear(),
            Command::Export => self.open_export(),
            Command::SaveLook => self.save_look(),
            Command::LoadLook => self.load_look(),
            Command::CopyFlags => {
                if let Some(l) = self.focus_look() {
                    ctx.copy_text(l.cli_flags());
                }
            }
            Command::Undo => self.step_history(false),
            Command::Redo => self.step_history(true),
            Command::SelectAll => self.lib.select_all(),
            Command::SelectNone => self.lib.select_none(),
            Command::CopyLook => self.copy_look(ctx),
            Command::PasteLook => self.paste_look(None),
            Command::ResetLook => {
                let ids = self.lib.selection();
                self.lib.checkpoint(&ids);
                self.lib.reset_looks(&ids);
            }
            Command::Process(on) => {
                let ids = self.lib.selection();
                self.lib.checkpoint(&ids);
                match on {
                    Some(on) => self.lib.set_process(&ids, on),
                    None => self.lib.toggle_process(&ids),
                }
            }
            Command::Star => self.lib.toggle_starred(&self.lib.selection()),
            Command::VirtualCopy => {
                self.lib.duplicate(&self.lib.selection());
            }
            Command::CopyImage => self.copy_image = self.lib.focus.map(|f| (f, Planner::new())),
            #[cfg(target_os = "macos")]
            Command::ShowInFinder => {
                let paths: Vec<PathBuf> = self.lib.selection().iter().filter_map(|&id| self.lib.get(id)).map(|it| it.path.clone()).collect();
                std::thread::spawn(move || std::process::Command::new("/usr/bin/open").arg("-R").args(paths).status());
            }
            Command::Remove => {
                let ids = self.lib.selected().clone();
                self.forget(&ids);
            }
            Command::View(m) => {
                self.mode = m;
                self.scroll_to_focus = true;
                self.zoom = None;
            }
            Command::Compare(c) => self.toggle_compare(c),
            Command::Swap => self.split_swap = !self.split_swap,
            Command::Clipping => self.clipping = !self.clipping,
            Command::Zoom(closer) => {
                let f = if closer { 1.5 } else { 1.0 / 1.5 };
                match self.mode {
                    Mode::Grid => self.thumb_pt = (self.thumb_pt * f).clamp(80.0, 480.0),
                    _ => self.zoom = Some(self.current_zoom() * f),
                }
            }
            Command::Fit => self.zoom = None,
            Command::Actual => self.zoom_100(),
            Command::StarredOnly => self.lib.set_starred_only(!self.lib.starred_only()),
            Command::Panel => self.panel = !self.panel,
            Command::Reference => self.reference.shown = !self.reference.shown,
        }
    }
}
