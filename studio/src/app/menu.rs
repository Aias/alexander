//! The menu bar, with every command and its key. Keys with ⌘ reach the menu before the
//! window, so the menu runs them; plain keys reach the window first, so the menu only shows
//! them and the window's own handling runs them, leaving them to text being typed.

use super::commands::Command;
use super::{App, Compare, Mode};
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{AboutMetadata, CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver};

/// An entry and the state its check mark shows, if it has one.
enum Entry {
    Plain(MenuItem),
    Checked(CheckMenuItem, fn(&App) -> bool),
}

pub struct MenuBar {
    _menu: Menu,
    commands: HashMap<MenuId, Command>,
    entries: Vec<(Entry, Command)>,
    recent: Submenu,
    /// The folders Open Recent lists.
    listed: Vec<PathBuf>,
    chosen: Receiver<MenuId>,
}

fn submenu(name: &str, items: &[&dyn IsMenuItem]) -> Submenu {
    let menu = Submenu::new(name, true);
    let _ = menu.append_items(items);
    menu
}

fn key(mods: Modifiers, code: Code) -> Option<Accelerator> {
    Some(Accelerator::new(mods, code))
}

const CMD: Modifiers = Modifiers::META;
const CMD_SHIFT: Modifiers = Modifiers::META.union(Modifiers::SHIFT);
const PLAIN: Modifiers = Modifiers::empty();

impl MenuBar {
    pub fn new(ctx: &egui::Context) -> MenuBar {
        let (tx, chosen) = channel();
        let ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(std::sync::Arc::new(move |e: MenuEvent| {
            let _ = tx.send(e.id);
            ctx.request_repaint();
        })));
        let mut b = MenuBar { _menu: Menu::new(), commands: HashMap::new(), entries: Vec::new(), recent: Submenu::new("Open Recent", true), listed: Vec::new(), chosen };
        let sep = PredefinedMenuItem::separator;
        let about = AboutMetadata { name: Some("Alexander".into()), version: Some(env!("CARGO_PKG_VERSION").into()), ..Default::default() };
        let app = submenu("Alexander", &[&PredefinedMenuItem::about(Some("About Alexander"), Some(about)), &sep(), &PredefinedMenuItem::services(None), &sep(), &PredefinedMenuItem::hide(Some("Hide Alexander")), &PredefinedMenuItem::hide_others(None), &PredefinedMenuItem::show_all(None), &sep(), &PredefinedMenuItem::quit(Some("Quit Alexander"))],
        );
        let file = submenu("File", &[
                &b.item("Open…", key(CMD, Code::KeyO), Command::Open),
                &b.item("Open from Photos…", key(CMD_SHIFT, Code::KeyO), Command::OpenPhotos),
                &b.recent.clone(),
                &sep(),
                &b.item("Export…", key(CMD_SHIFT, Code::KeyE), Command::Export),
                &sep(),
                &b.item("Save Look…", None, Command::SaveLook),
                &b.item("Load Look…", None, Command::LoadLook),
                &b.item("Copy as twb2.py Flags", None, Command::CopyFlags),
                &sep(),
                &PredefinedMenuItem::close_window(None),
            ],
        );
        let edit = submenu("Edit", &[
                &b.item("Undo", key(CMD, Code::KeyZ), Command::Undo),
                &b.item("Redo", key(CMD_SHIFT, Code::KeyZ), Command::Redo),
                &sep(),
                &b.item("Select All", key(CMD, Code::KeyA), Command::SelectAll),
                &b.item("Select None", key(CMD, Code::KeyD), Command::SelectNone),
                &sep(),
                &b.item("Copy Look", key(CMD_SHIFT, Code::KeyC), Command::CopyLook),
                &b.item("Paste Look", key(CMD_SHIFT, Code::KeyV), Command::PasteLook),
                &b.item("Reset Look", key(CMD_SHIFT, Code::KeyR), Command::ResetLook),
            ],
        );
        let photo = submenu("Photo", &[
                &b.check("Star", key(PLAIN, Code::KeyS), Command::Star, |a| a.lib.focus.and_then(|id| a.lib.get(id)).is_some_and(|it| it.starred)),
                &b.check("Film", key(PLAIN, Code::KeyV), Command::Process(None), |a| a.lib.focus.and_then(|id| a.lib.get(id)).is_some_and(|it| it.look.is_some())),
                &b.check("Crop", key(PLAIN, Code::KeyR), Command::Crop, |a| a.cropping.is_some()),
                &b.item("Create Virtual Copy", key(CMD, Code::Quote), Command::VirtualCopy),
                &b.item("Copy Image", None, Command::CopyImage),
                &b.item("Show in Finder", key(CMD, Code::KeyR), Command::ShowInFinder),
                &sep(),
                &b.item("Remove from Library", None, Command::Remove),
            ],
        );
        let view = submenu("View", &[
                &b.check("Grid", key(PLAIN, Code::KeyG), Command::View(Mode::Grid), |a| a.mode == Mode::Grid),
                &b.check("Survey", key(PLAIN, Code::KeyN), Command::View(Mode::Survey), |a| a.mode == Mode::Survey),
                &b.check("Loupe", key(PLAIN, Code::KeyE), Command::View(Mode::Loupe), |a| a.mode == Mode::Loupe),
                &sep(),
                &b.check("Before", key(PLAIN, Code::Backslash), Command::Compare(Compare::Before), |a| a.compare == Compare::Before),
                &b.check("Split", key(PLAIN, Code::KeyY), Command::Compare(Compare::Split), |a| a.compare == Compare::Split),
                &b.check("Swap Sides", None, Command::Swap, |a| a.split_swap),
                &b.check("Clipping", key(PLAIN, Code::KeyJ), Command::Clipping, |a| a.clipping),
                &sep(),
                &b.item("Zoom In", key(CMD, Code::Equal), Command::Zoom(true)),
                &b.item("Zoom Out", key(CMD, Code::Minus), Command::Zoom(false)),
                &b.item("Fit", key(CMD, Code::Digit0), Command::Fit),
                &b.item("100%", key(CMD, Code::Digit1), Command::Actual),
                &sep(),
                &b.check("Starred Only", None, Command::StarredOnly, |a| a.lib.starred_only()),
                &b.check("Panel", key(PLAIN, Code::Tab), Command::Panel, |a| a.panel),
                &b.check("Reference", key(Modifiers::SHIFT, Code::KeyR), Command::Reference, |a| a.reference.shown),
                &sep(),
                &PredefinedMenuItem::fullscreen(None),
            ],
        );
        let window = submenu("Window", &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::maximize(None), &sep(), &PredefinedMenuItem::bring_all_to_front(None)]);
        let submenus: [&dyn IsMenuItem; 6] = [&app, &file, &edit, &photo, &view, &window];
        for s in submenus {
            let _ = b._menu.append(s);
        }
        b._menu.init_for_nsapp();
        window.set_as_windows_menu_for_nsapp();
        b
    }

    fn item(&mut self, text: &str, key: Option<Accelerator>, c: Command) -> MenuItem {
        let item = MenuItem::new(text, true, key);
        self.commands.insert(item.id().clone(), c.clone());
        self.entries.push((Entry::Plain(item.clone()), c));
        item
    }

    fn check(&mut self, text: &str, key: Option<Accelerator>, c: Command, on: fn(&App) -> bool) -> CheckMenuItem {
        let item = CheckMenuItem::new(text, true, false, key);
        self.commands.insert(item.id().clone(), c.clone());
        self.entries.push((Entry::Checked(item.clone(), on), c));
        item
    }

    /// The commands chosen from the menu since the last frame.
    pub fn chosen(&self) -> Vec<Command> {
        self.chosen.try_iter().filter_map(|id| self.commands.get(&id).cloned()).collect()
    }

    /// Show what can be done now and what is on, and list the recent folders.
    pub fn sync(&mut self, app: &mut App) {
        for (entry, c) in &self.entries {
            let can = app.can(c);
            match entry {
                Entry::Plain(item) => item.set_enabled(can),
                Entry::Checked(item, on) => {
                    item.set_enabled(can);
                    item.set_checked(on(app));
                }
            }
        }
        if self.listed != app.recent {
            while self.recent.remove_at(0).is_some() {}
            self.commands.retain(|_, c| !matches!(c, Command::OpenRecent(_) | Command::ClearRecent));
            let home = std::env::home_dir().unwrap_or_default();
            for folder in &app.recent {
                let name = folder.strip_prefix(&home).map_or_else(|_| folder.display().to_string(), |p| format!("~/{}", p.display()));
                let item = MenuItem::new(name, true, None);
                self.commands.insert(item.id().clone(), Command::OpenRecent(folder.clone()));
                let _ = self.recent.append(&item);
            }
            let clear = MenuItem::new("Clear Menu", !app.recent.is_empty(), None);
            self.commands.insert(clear.id().clone(), Command::ClearRecent);
            let _ = self.recent.append_items(&[&PredefinedMenuItem::separator(), &clear]);
            self.listed = app.recent.clone();
        }
    }
}
