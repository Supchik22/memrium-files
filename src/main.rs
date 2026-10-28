//! Memrium Files: a file manager built on Chebrium. A standalone Wayland client (software-drawn, redraws only
//! while something moves) that shares its file operations, trash and undo with the desktop through `fsops`.

mod chooser;

use chebrium::app::{self, App, Config, Decorations, Host};
use chebrium::chrome;
use chebrium::appmenu::{Item, Menu};
use chebrium::draw::Align;
use chebrium::input::{Key, KeyMods};
use chebrium::spring::Spring;
use chebrium::ui::{inside, Ui};
use chooser::{Chooser, Mode};
use memrium_de::fsops::{self, Entry, Fm, SortBy};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

const SIDE_W: f32 = 200.0;
const TOP_H: f32 = 52.0;
const HEAD_H: f32 = 28.0;
const ROW_H: f32 = 32.0;
const STATUS_H: f32 = 28.0;
/// the bar of the file dialog (name, filter, buttons) above the status line
const BAR_H: f32 = 56.0;
const SCROLL_KEY: u64 = 90_000;

struct Place {
    name: String,
    icon: &'static str,
    path: PathBuf,
}

fn places() -> Vec<Place> {
    let home = fsops::home();
    let mut out = vec![Place { name: "Home".into(), icon: "house", path: home.clone() }];
    for (name, icon, dir) in [("Desktop", "monitor", "Desktop"), ("Documents", "file-text", "Documents"), ("Downloads", "download", "Downloads"), ("Pictures", "image", "Pictures"), ("Music", "music", "Music"), ("Videos", "video", "Videos")] {
        let path = home.join(dir);
        if path.is_dir() {
            out.push(Place { name: name.into(), icon, path });
        }
    }
    out.push(Place { name: "Trash".into(), icon: "trash-2", path: fsops::trash_path() });
    out.push(Place { name: "This computer".into(), icon: "hard-drive", path: PathBuf::from(fsops::COMPUTER) });
    out
}

/// The icons of `.desktop` launchers, found in the installed icon themes (the index is built on a thread the first time
/// a launcher is listed; a picture is made once per icon).
#[derive(Default)]
struct AppIcons {
    index: Arc<OnceLock<memrium_de::apps::IconIndex>>,
    started: bool,
    pics: HashMap<String, Option<Arc<chebrium::picture::Pic>>>,
}

impl AppIcons {
    /// The picture for `icon`, or None while the themes are still being read (`wait` says so) or when there is none.
    fn get(&mut self, icon: &str, wait: &mut bool) -> Option<Arc<chebrium::picture::Pic>> {
        if let Some(p) = self.pics.get(icon) {
            return p.clone();
        }
        if !self.started {
            self.started = true;
            let index = self.index.clone();
            std::thread::spawn(move || {
                let _ = index.set(memrium_de::apps::IconIndex::build());
            });
        }
        let Some(index) = self.index.get() else {
            *wait = true;
            return None;
        };
        let pic = index.load(icon, 64).map(|i| Arc::new(chebrium::picture::Pic::from_rgba(i.width(), i.height(), i.as_raw())));
        self.pics.insert(icon.to_string(), pic.clone());
        pic
    }
}

struct FilesApp {
    app_icons: AppIcons,
    cwd: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    entries: Vec<Entry>,
    sel: Vec<PathBuf>,
    /// the row the keyboard works on
    cursor: Option<usize>,
    sort: SortBy,
    desc: bool,
    hidden: bool,
    fm: Fm,
    gen: u64,
    places: Vec<Place>,
    /// where the list wants to be scrolled to (the drawn position eases towards it)
    scroll: f32,
    /// bring the cursor row into view on the next frame
    reveal: bool,
    renaming: Option<PathBuf>,
    want_rename: bool,
    rename_buf: String,
    error: String,
    title: String,
    /// the file dialog the portal asked for (None: the ordinary file manager)
    chooser: Option<Chooser>,
    /// the answer was given: close the window on the next chance
    done: bool,
    /// the sidebar's one highlight: x, y, w, h and opacity
    side_hl: [Spring; 5],
    side_hl_init: bool,
}

impl FilesApp {
    fn new(start: PathBuf, chooser: Option<Chooser>) -> FilesApp {
        let mut app = FilesApp {
            cwd: start,
            back: Vec::new(),
            forward: Vec::new(),
            entries: Vec::new(),
            sel: Vec::new(),
            cursor: None,
            sort: SortBy::Name,
            desc: false,
            hidden: false,
            fm: Fm::default(),
            gen: 0,
            places: places(),
            scroll: 0.0,
            reveal: false,
            renaming: None,
            want_rename: false,
            rename_buf: String::new(),
            error: String::new(),
            title: String::new(),
            app_icons: AppIcons::default(),
            chooser,
            done: false,
            side_hl: [Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2400.0, 1.0)],
            side_hl_init: false,
        };
        app.reload();
        app
    }

    fn reload(&mut self) {
        self.entries = if fsops::is_computer(&self.cwd) {
            self.error.clear();
            fsops::drives()
        } else {
            match fsops::list_dir(&self.cwd, self.hidden, self.sort, self.desc, true) {
                Ok(mut e) => {
                    self.error.clear();
                    if let Some(c) = &self.chooser {
                        e.retain(|x| c.shows(x));
                    }
                    e
                }
                Err(e) => {
                    self.error = format!("{}: {e}", self.cwd.display());
                    Vec::new()
                }
            }
        };
        let entries = &self.entries;
        self.sel.retain(|p| entries.iter().any(|e| &e.path == p));
        self.cursor = self.cursor.filter(|&i| i < self.entries.len());
    }

    fn go(&mut self, path: PathBuf) {
        if path == self.cwd {
            return;
        }
        self.back.push(std::mem::replace(&mut self.cwd, path));
        self.forward.clear();
        self.arrived();
    }

    fn arrived(&mut self) {
        self.sel.clear();
        self.cursor = None;
        self.scroll = 0.0;
        self.renaming = None;
        self.fm.note.clear();
        if let Some(c) = &mut self.chooser {
            c.replace = false;
        }
        self.reload();
    }

    fn go_back(&mut self) {
        if let Some(p) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.cwd, p));
            self.arrived();
        }
    }

    fn go_forward(&mut self) {
        if let Some(p) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.cwd, p));
            self.arrived();
        }
    }

    fn go_up(&mut self) {
        if fsops::is_computer(&self.cwd) {
            return;
        }
        if let Some(parent) = self.cwd.parent() {
            let from = self.cwd.clone();
            self.go(parent.to_path_buf());
            // land on the folder we came out of
            self.select_path(&from);
        }
    }

    fn select_path(&mut self, path: &Path) {
        if let Some(i) = self.entries.iter().position(|e| e.path == path) {
            self.select_only(i);
            self.reveal = true;
        }
    }

    fn select_only(&mut self, i: usize) {
        self.sel = vec![self.entries[i].path.clone()];
        self.cursor = Some(i);
        // saving over an existing file: its name goes into the field
        let e = &self.entries[i];
        if let Some(c) = self.chooser.as_mut().filter(|c| c.mode == Mode::Save && !e.is_dir) {
            c.name = e.name.clone();
            c.replace = false;
        }
    }

    /// Whether several rows may be selected (not in a dialog that wants one file).
    fn multi(&self) -> bool {
        self.chooser.as_ref().map_or(true, |c| c.multiple && c.mode != Mode::Save)
    }

    /// Whether the dialog's main button can be pressed.
    fn can_accept(&self) -> bool {
        let Some(c) = &self.chooser else { return false };
        let computer = fsops::is_computer(&self.cwd);
        match c.mode {
            Mode::Open => self.sel.iter().any(|p| self.entries.iter().any(|e| &e.path == p && !e.is_dir)),
            Mode::Dir => !computer || !self.sel.is_empty(),
            Mode::Save => !computer && !c.name.trim().is_empty() && !c.name.contains('/'),
        }
    }

    /// The main button of the dialog: gives the answer (saving over a file asks once more first).
    fn accept(&mut self) {
        if !self.can_accept() {
            return;
        }
        let cwd = self.cwd.clone();
        let sel_dirs: Vec<PathBuf> = self.entries.iter().filter(|e| e.is_dir && self.sel.contains(&e.path)).map(|e| e.path.clone()).collect();
        let sel_files: Vec<PathBuf> = self.entries.iter().filter(|e| !e.is_dir && self.sel.contains(&e.path)).map(|e| e.path.clone()).collect();
        let Some(c) = self.chooser.as_mut() else { return };
        let paths = match c.mode {
            Mode::Open => sel_files,
            Mode::Dir if sel_dirs.is_empty() => vec![cwd],
            Mode::Dir => sel_dirs,
            Mode::Save => {
                let path = cwd.join(c.name.trim());
                if path.exists() && !c.replace {
                    c.replace = true;
                    return;
                }
                vec![path]
            }
        };
        c.emit(&paths);
        self.done = true;
    }

    fn toggle(&mut self, i: usize) {
        if !self.multi() {
            return self.select_only(i);
        }
        let p = self.entries[i].path.clone();
        match self.sel.iter().position(|q| *q == p) {
            Some(at) => {
                self.sel.remove(at);
            }
            None => self.sel.push(p),
        }
        self.cursor = Some(i);
    }

    fn open(&mut self, i: usize) {
        let e = self.entries[i].clone();
        if e.is_dir {
            self.go(e.path);
        } else if self.chooser.is_some() {
            self.select_only(i);
            self.accept();
        } else {
            fsops::open_external(&e.path, &std::env::var("WAYLAND_DISPLAY").unwrap_or_default());
        }
    }

    fn move_cursor(&mut self, delta: isize) {
        if self.entries.is_empty() {
            return;
        }
        let next = match self.cursor {
            Some(c) => (c as isize + delta).clamp(0, self.entries.len() as isize - 1) as usize,
            None if delta >= 0 => 0,
            None => self.entries.len() - 1,
        };
        self.select_only(next);
        self.reveal = true;
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.sel.clone()
    }

    fn trash_selected(&mut self) {
        let paths = self.selected_paths();
        if paths.is_empty() {
            return;
        }
        self.fm.trash_all(&paths);
        self.sel.clear();
        self.cursor = None;
    }

    fn new_folder(&mut self) {
        if fsops::is_computer(&self.cwd) {
            return;
        }
        if let Some(p) = self.fm.new_folder(&self.cwd.clone()) {
            self.gen = self.fm.gen;
            self.reload();
            self.select_path(&p);
            self.want_rename = true;
        }
    }

    fn paste(&mut self) {
        let made = self.fm.paste_into(&self.cwd.clone());
        self.gen = self.fm.gen;
        self.reload();
        if !made.is_empty() {
            self.sel = made;
        }
    }

    fn sort_by(&mut self, by: SortBy) {
        if self.sort == by {
            self.desc = !self.desc;
        } else {
            self.sort = by;
            self.desc = false;
        }
        self.reload();
    }

    fn shown_path(&self) -> String {
        if fsops::is_computer(&self.cwd) {
            return "This computer".into();
        }
        let home = fsops::home();
        match self.cwd.strip_prefix(&home) {
            Ok(rest) if rest.as_os_str().is_empty() => "~".into(),
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => self.cwd.display().to_string(),
        }
    }

    fn status(&self) -> String {
        if !self.error.is_empty() {
            return self.error.clone();
        }
        if !self.fm.note.is_empty() {
            return self.fm.note.clone();
        }
        if self.sel.is_empty() {
            return chebrium::i18n::trf("Items: {}", &[self.entries.len().to_string()]);
        }
        let size: u64 = self.entries.iter().filter(|e| !e.is_dir && self.sel.contains(&e.path)).map(|e| e.size).sum();
        if size > 0 {
            chebrium::i18n::trf("{} selected, {}", &[self.sel.len().to_string(), fsops::human_size(size)])
        } else {
            chebrium::i18n::trf("{} selected", &[self.sel.len().to_string()])
        }
    }
}

/// `text`, cut short with an ellipsis so it is no wider than `max`.
fn fit(ui: &mut Ui<'_>, text: &str, size: f32, bold: bool, max: f32) -> String {
    if ui.measure_raw(text, size, bold) <= max {
        return text.to_string();
    }
    let mut s = text.to_string();
    while !s.is_empty() && ui.measure_raw(&format!("{s}…"), size, bold) > max {
        s.pop();
    }
    format!("{s}…")
}

fn icon_button(ui: &mut Ui<'_>, cx: f32, cy: f32, icon: &str, enabled: bool) -> bool {
    let r = [cx - 15.0, cy - 15.0, 30.0, 30.0];
    let hov = enabled && ui.hit(r);
    let key = ui.id() * 64;
    let wash = ui.ease(key, if hov { 1.0 } else { 0.0 }, 22.0);
    ui.fill(r, 15.0, ui.ink, 0.10 * wash);
    ui.glyph(cx, cy, 17.0, icon, ui.ink, if enabled { 0.9 } else { 0.28 });
    ui.click && hov
}

/// The entries of the menu bar that do what a key does: (id, key, ctrl, alt).
const MENU_KEYS: [(u32, Key, bool, bool); 14] = [
    (1, Key::Char('n'), true, false),
    (2, Key::Enter, false, false),
    (3, Key::F2, false, false),
    (4, Key::Delete, false, false),
    (5, Key::Char('w'), true, false),
    (10, Key::Char('z'), true, false),
    (11, Key::Char('x'), true, false),
    (12, Key::Char('c'), true, false),
    (13, Key::Char('v'), true, false),
    (14, Key::Char('a'), true, false),
    (20, Key::F5, false, false),
    (30, Key::Left, false, true),
    (31, Key::Right, false, true),
    (32, Key::Up, false, true),
];
const MENU_SORT: u32 = 40;
const MENU_HIDDEN: u32 = 21;
const MENU_GO: u32 = 50;

impl App for FilesApp {
    fn menus(&self) -> Vec<Menu> {
        let sort = |i: u32, label: &str, by: SortBy| Item::new(MENU_SORT + i, label).checked(self.sort == by);
        vec![
            Menu::new("File", vec![Item::new(1, "New Folder").key("Ctrl+N"), Item::new(2, "Open").key("Enter"), Item::new(3, "Rename").key("F2"), Item::new(4, "Move to Trash").key("Delete"), Item::sep(), Item::new(5, "Close Window").key("Ctrl+W")]),
            Menu::new("Edit", vec![Item::new(10, "Undo").key("Ctrl+Z"), Item::sep(), Item::new(11, "Cut").key("Ctrl+X"), Item::new(12, "Copy").key("Ctrl+C"), Item::new(13, "Paste").key("Ctrl+V"), Item::sep(), Item::new(14, "Select All").key("Ctrl+A")]),
            Menu::new(
                "View",
                vec![
                    Item::new(MENU_HIDDEN, "Show Hidden Files").key("Ctrl+H").checked(self.hidden),
                    Item::new(20, "Refresh").key("F5"),
                    Item::sep(),
                    Item::parent("Sort by", vec![sort(0, "Name", SortBy::Name), sort(1, "Size", SortBy::Size), sort(2, "Date modified", SortBy::Modified), sort(3, "Type", SortBy::Kind)]),
                ],
            ),
            Menu::new("Go", vec![Item::new(30, "Back").key("Alt+←"), Item::new(31, "Forward").key("Alt+→"), Item::new(32, "Enclosing Folder").key("Alt+↑"), Item::sep(), Item::new(MENU_GO, "Home"), Item::new(MENU_GO + 1, "Desktop"), Item::new(MENU_GO + 2, "Documents"), Item::new(MENU_GO + 3, "Downloads")]),
        ]
    }

    fn menu_action(&mut self, id: u32, host: &mut Host) {
        host.redraw = true;
        if let Some((_, key, ctrl, alt)) = MENU_KEYS.iter().find(|k| k.0 == id) {
            self.key(*key, KeyMods { ctrl: *ctrl, alt: *alt, shift: false }, host);
            return;
        }
        match id {
            MENU_HIDDEN => {
                self.hidden = !self.hidden;
                self.reload();
            }
            i if (MENU_SORT..MENU_SORT + 4).contains(&i) => self.sort_by([SortBy::Name, SortBy::Size, SortBy::Modified, SortBy::Kind][(i - MENU_SORT) as usize]),
            i if (MENU_GO..MENU_GO + 4).contains(&i) => {
                let h = fsops::home();
                let p = match i - MENU_GO {
                    0 => h,
                    1 => h.join("Desktop"),
                    2 => h.join("Documents"),
                    _ => h.join("Downloads"),
                };
                if p.is_dir() {
                    self.go(p);
                }
            }
            _ => {}
        }
    }

    fn key(&mut self, key: Key, mods: KeyMods, host: &mut Host) {
        host.redraw = true;
        self.key_inner(key, mods, host);
        host.quit |= self.done;
    }

    fn draw(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        chrome::resize_borders(ui, host, size);
        self.draw_inner(ui, size, host);
        host.quit |= self.done;
    }
}

impl FilesApp {
    fn key_inner(&mut self, key: Key, mods: KeyMods, host: &mut Host) {
        match key {
            Key::Left if mods.alt => self.go_back(),
            Key::Right if mods.alt => self.go_forward(),
            Key::Up if mods.alt => self.go_up(),
            Key::Down if mods.ctrl => {
                if let Some(i) = self.cursor {
                    self.open(i);
                }
            }
            Key::Down => self.move_cursor(1),
            Key::Up => self.move_cursor(-1),
            Key::PageDown => self.move_cursor(10),
            Key::PageUp => self.move_cursor(-10),
            Key::Home => self.move_cursor(isize::MIN / 2),
            Key::End => self.move_cursor(isize::MAX / 2),
            Key::Enter => {
                if let Some(i) = self.cursor {
                    self.open(i);
                }
            }
            Key::Backspace => self.go_up(),
            Key::Delete => self.trash_selected(),
            Key::F2 => self.want_rename = self.sel.len() == 1,
            Key::F5 => self.reload(),
            Key::Esc if self.chooser.is_some() => host.quit = true,
            Key::Esc => self.sel.clear(),
            Key::Char(c) if mods.ctrl => match c {
                'a' if self.multi() => self.sel = self.entries.iter().map(|e| e.path.clone()).collect(),
                'c' => self.fm.set_clip(self.selected_paths(), false),
                'x' => self.fm.set_clip(self.selected_paths(), true),
                'v' => self.paste(),
                'z' => {
                    self.fm.undo_last();
                }
                'h' => {
                    self.hidden = !self.hidden;
                    self.reload();
                }
                'n' => self.new_folder(),
                'q' | 'w' => host.quit = true,
                _ => {}
            },
            _ => {}
        }
    }

    fn draw_inner(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        let [w, h] = size;
        if self.fm.gen != self.gen {
            self.gen = self.fm.gen;
            self.reload();
        }
        let accent = ui.accent;
        let ink = ui.ink;
        let dark = ink[0] > 0.5;
        let paper = if dark { [0.0; 3] } else { [1.0; 3] };
        // the content is a sheet of paper next to the sidebar's floating glass
        ui.fill([SIDE_W, 0.0, w - SIDE_W, h], 0.0, paper, if dark { 0.32 } else { 0.78 });
        // the file dialog adds a bar for the name, the filter and the buttons below the list
        let foot = if self.chooser.is_some() { BAR_H } else { 0.0 };
        let list = [SIDE_W, TOP_H + HEAD_H, w - SIDE_W, (h - TOP_H - HEAD_H - STATUS_H - foot).max(ROW_H)];

        // keep the scroll range sensible, honour the wheel and bring the keyboard cursor into view
        let max_scroll = (self.entries.len() as f32 * ROW_H + 8.0 - list[3]).max(0.0);
        if ui.scroll != 0.0 && inside(ui.mouse, list) {
            self.scroll += ui.scroll;
        }
        if self.reveal {
            self.reveal = false;
            if let Some(c) = self.cursor {
                let (top, bottom) = (c as f32 * ROW_H, (c + 1) as f32 * ROW_H + 8.0);
                if top < self.scroll {
                    self.scroll = top;
                } else if bottom > self.scroll + list[3] {
                    self.scroll = bottom - list[3];
                }
            }
        }
        self.scroll = self.scroll.clamp(0.0, max_scroll);
        let scroll = ui.ease(SCROLL_KEY, self.scroll, 18.0);

        // F2 or a new folder: start renaming the selected row
        if self.want_rename {
            self.want_rename = false;
            if let [p] = self.sel.as_slice() {
                if let Some(e) = self.entries.iter().find(|e| &e.path == p) {
                    self.rename_buf = e.name.clone();
                    self.renaming = Some(e.path.clone());
                    ui.begin_edit("rename", &e.name);
                }
            }
        }

        // ---- the list (drawn first; the bars below cover whatever sticks out)
        let computer = fsops::is_computer(&self.cwd);
        let n = self.entries.len();
        let first = (scroll / ROW_H).floor().max(0.0) as usize;
        let last = (((scroll + list[3]) / ROW_H).ceil() as usize + 1).min(n);
        let (mut open, mut clicked_row) = (None, false);
        let mut renamed = None;
        ui.push_clip(list);
        for i in first..last {
            let e = self.entries[i].clone();
            let r = [list[0] + 8.0, list[1] - scroll + 4.0 + i as f32 * ROW_H, list[2] - 16.0, ROW_H - 2.0];
            let hov = ui.hit(r) && inside(ui.mouse, list);
            let selected = self.sel.contains(&e.path);
            let wash = ui.ease(30_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
            if selected {
                ui.fill(r, 8.0, accent, 0.28);
            } else if wash > 0.01 {
                ui.fill(r, 8.0, ink, 0.07 * wash);
            }
            if self.cursor == Some(i) && selected {
                ui.fill([r[0], r[1] + 6.0, 3.0, r[3] - 12.0], 1.5, accent, 1.0);
            }
            let cy = r[1] + r[3] / 2.0;
            let mut wait = false;
            let pic = e.app.as_ref().filter(|(_, i)| !i.is_empty()).and_then(|(_, i)| self.app_icons.get(i, &mut wait));
            match pic {
                Some(pic) => ui.picture([r[0] + 12.0, cy - 11.0, 22.0, 22.0], &pic, [0.0, 0.0, pic.w as f32, pic.h as f32], 1.0),
                None => ui.glyph(r[0] + 22.0, cy, 18.0, e.kind.icon(), e.kind.tint(accent), 1.0),
            }
            // the themes are still being read: look again shortly
            host.redraw |= wait;
            let right = r[0] + r[2] - 16.0;
            let (mod_x, name_max) = if computer { (right, right - r[0] - 60.0) } else { (right - 110.0 - 150.0, right - 110.0 - 150.0 - r[0] - 56.0) };
            if self.renaming.as_ref() == Some(&e.path) {
                let mut buf = self.rename_buf.clone();
                if ui.text_field([r[0] + 40.0, r[1] + 1.0, name_max.min(360.0), r[3] - 2.0], "rename", &mut buf) {
                    renamed = Some((e.path.clone(), buf.clone()));
                }
                self.rename_buf = buf;
            } else {
                let name = fit(ui, e.title(), 13.5, false, name_max);
                ui.text_raw(r[0] + 44.0, cy + 4.8, &name, 13.5, false, ink, if e.hidden { 0.55 } else { 0.95 }, Align::Left);
            }
            if !computer {
                ui.text_raw(mod_x, cy + 4.5, &fsops::format_time(e.modified), 12.0, false, ink, 0.55, Align::Left);
                if !e.is_dir {
                    ui.text_raw(right, cy + 4.5, &fsops::human_size(e.size), 12.0, false, ink, 0.55, Align::Right);
                }
            }
            if ui.click && hov {
                clicked_row = true;
                if ui.mods.ctrl && self.multi() {
                    self.toggle(i);
                } else if !(selected && self.sel.len() > 1) || ui.double {
                    self.select_only(i);
                }
                if ui.double && !ui.mods.ctrl {
                    open = Some(i);
                }
            }
        }
        ui.pop_clip();
        if ui.click && inside(ui.mouse, list) && !clicked_row && self.renaming.is_none() {
            self.sel.clear();
        }
        if let Some(i) = open {
            self.open(i);
        }
        if let Some((path, name)) = renamed {
            if let Some(to) = self.fm.rename(&path, &name) {
                self.gen = self.fm.gen;
                self.reload();
                self.select_path(&to);
            }
            self.renaming = None;
        } else if self.renaming.is_some() && !ui.editing() {
            // Esc, or the field lost the pointer without a change
            self.renaming = None;
        }
        if n == 0 {
            let msg = if self.error.is_empty() { "This folder is empty" } else { "Cannot open this folder" };
            ui.text(list[0] + list[2] / 2.0, list[1] + 60.0, msg, 14.0, false, ink, 0.45, Align::Center);
        }

        // ---- column header
        ui.fill([SIDE_W + 8.0, TOP_H + HEAD_H - 1.0, w - SIDE_W - 16.0, 1.0], 0.0, ink, 0.10);
        let right = w - 8.0 - 16.0;
        let mut sort_to = None;
        let head = |ui: &mut Ui<'_>, label: &str, x: f32, align: Align, by: SortBy, width: f32, sort: SortBy, desc: bool| {
            let rx = if align == Align::Right { x - width } else { x };
            let hov = ui.hit([rx - 4.0, TOP_H, width + 8.0, HEAD_H]);
            ui.text(x, TOP_H + 18.5, label, 11.5, true, ink, if sort == by { 0.9 } else if hov { 0.8 } else { 0.5 }, align);
            if sort == by {
                let ax = if align == Align::Right { x - ui.measure(label, 11.5, true) - 10.0 } else { x + ui.measure(label, 11.5, true) + 10.0 };
                ui.glyph(ax, TOP_H + 14.0, 12.0, if desc { "chevron-down" } else { "chevron-up" }, ink, 0.8);
            }
            ui.click && hov
        };
        if head(ui, "Name", SIDE_W + 52.0, Align::Left, SortBy::Name, 80.0, self.sort, self.desc) {
            sort_to = Some(SortBy::Name);
        }
        if !computer {
            if head(ui, "Modified", right - 110.0 - 150.0 + 8.0 + 8.0, Align::Left, SortBy::Modified, 100.0, self.sort, self.desc) {
                sort_to = Some(SortBy::Modified);
            }
            if head(ui, "Size", right + 8.0, Align::Right, SortBy::Size, 60.0, self.sort, self.desc) {
                sort_to = Some(SortBy::Size);
            }
        }
        if let Some(by) = sort_to {
            self.sort_by(by);
        }

        let title = if let Some(c) = &self.chooser {
            c.title.clone()
        } else if computer { "This computer".to_string() } else { self.cwd.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "Files".into()) };
        if title != self.title {
            host.title = Some(title.clone());
            self.title = title;
        }

        // ---- toolbar in the title row: a back/forward pill, the folder's name, a pill of actions. The window's own
        // buttons sit over the sidebar's corner, so the row is one piece with the window's top edge.
        let cy = TOP_H / 2.0;
        let pill_x = SIDE_W + 14.0;
        ui.fill([pill_x, cy - 16.0, 68.0, 32.0], 16.0, ink, 0.065);
        if icon_button(ui, pill_x + 18.0, cy, "chevron-left", !self.back.is_empty()) {
            self.go_back();
        }
        if icon_button(ui, pill_x + 50.0, cy, "chevron-right", !self.forward.is_empty()) {
            self.go_forward();
        }
        let actions_w = 5.0 * 32.0 + 8.0;
        let actions_x = w - actions_w - 14.0;
        ui.fill([actions_x, cy - 16.0, actions_w, 32.0], 16.0, ink, 0.065);
        let mut ax = actions_x + 20.0;
        if icon_button(ui, ax, cy, "arrow-up", !computer && self.cwd.parent().is_some()) {
            self.go_up();
        }
        ax += 32.0;
        if icon_button(ui, ax, cy, "refresh-cw", true) {
            self.reload();
        }
        ax += 32.0;
        if icon_button(ui, ax, cy, "folder-plus", !computer) {
            self.new_folder();
        }
        ax += 32.0;
        if icon_button(ui, ax, cy, "trash-2", !self.sel.is_empty()) {
            self.trash_selected();
        }
        ax += 32.0;
        if icon_button(ui, ax, cy, if self.hidden { "eye" } else { "eye-off" }, true) {
            self.hidden = !self.hidden;
            self.reload();
        }
        let title_x = pill_x + 68.0 + 16.0;
        let title_w = (actions_x - 12.0 - title_x).max(60.0);
        let name = fit(ui, &self.title, 15.0, true, title_w);
        ui.text_raw(title_x, cy - 1.5, &name, 15.0, true, ink, 0.95, Align::Left);
        let path = fit(ui, &self.shown_path(), 11.0, false, title_w);
        ui.text_raw(title_x, cy + 13.0, &path, 11.0, false, ink, 0.5, Align::Left);
        // a press on the bare toolbar moves the window, twice maximises it
        let on_widget = ui.hit([pill_x, 0.0, 68.0, TOP_H]) || ui.hit([actions_x, 0.0, actions_w, TOP_H]);
        chrome::integrated(ui, host, size, TOP_H, on_widget);

        // ---- status bar
        let mut status = self.status();
        if let Some(c) = self.chooser.as_ref().filter(|c| c.replace) {
            status = chebrium::i18n::trf("“{}” already exists. Replace it?", &[c.name.trim().to_string()]);
        }
        ui.text_raw(SIDE_W + 18.0, h - foot - 10.0, &status, 12.0, false, ink, if foot > 0.0 && self.chooser.as_ref().is_some_and(|c| c.replace) { 0.9 } else { 0.55 }, Align::Left);

        // ---- the file dialog's bar: [name] [filter] [Cancel] [Open]
        if self.chooser.is_some() {
            let can = self.can_accept();
            let bar = [SIDE_W, h - foot, w - SIDE_W, foot];
            ui.fill([bar[0], bar[1], bar[2], 1.0], 0.0, ink, 0.10);
            let by = bar[1] + (foot - 32.0) / 2.0;
            let mut x = w - 16.0;
            let (mut cancel, mut ok, mut cycle) = (false, false, false);
            let (label, filter_name, many) = {
                let c = self.chooser.as_ref().unwrap();
                let label = if c.replace { "Replace".to_string() } else { c.accept.clone() };
                (label, c.filters.get(c.filter).map(|f| f.name.clone()), c.filters.len() > 1)
            };
            x -= 110.0;
            if can {
                ok = ui.button([x, by, 110.0, 32.0], &label, true);
            } else {
                ui.fill([x, by, 110.0, 32.0], 8.0, ink, 0.06);
                ui.text(x + 55.0, by + 20.5, &label, 12.5, true, ink, 0.3, Align::Center);
            }
            x -= 8.0 + 90.0;
            cancel |= ui.button([x, by, 90.0, 32.0], "Cancel", false);
            if let Some(name) = filter_name {
                x -= 8.0 + 190.0;
                let shown = fit(ui, &name, 12.5, false, 150.0);
                let r = [x, by, 190.0, 32.0];
                let hov = many && ui.hit(r);
                ui.fill(r, 8.0, ink, if hov { 0.14 } else { 0.08 });
                ui.text_raw(r[0] + 12.0, by + 20.5, &shown, 12.5, false, ink, 0.95, Align::Left);
                if many {
                    ui.glyph(r[0] + r[2] - 16.0, by + 16.0, 14.0, "chevrons-up-down", ink, 0.6);
                }
                cycle = ui.click && hov;
            }
            let mut name_changed = false;
            if self.chooser.as_ref().is_some_and(|c| c.mode == Mode::Save) {
                let c = self.chooser.as_mut().unwrap();
                if c.focus_name {
                    c.focus_name = false;
                    ui.begin_edit("choose-name", &c.name);
                }
                let mut buf = c.name.clone();
                name_changed = ui.text_field([bar[0] + 16.0, by, (x - 12.0 - bar[0] - 16.0).max(120.0), 32.0], "choose-name", &mut buf);
                if name_changed {
                    c.name = buf;
                    c.replace = false;
                }
            }
            if cancel {
                host.quit = true;
            }
            if cycle {
                if let Some(c) = self.chooser.as_mut() {
                    c.filter = (c.filter + 1) % c.filters.len();
                }
                self.reload();
            }
            if ok {
                self.accept();
            }
            host.redraw |= name_changed || cycle;
        }

        // ---- sidebar
        let panel = [8.0, 8.0, SIDE_W - 16.0, h - 16.0];
        ui.fill(panel, 18.0, paper, if dark { 0.06 } else { 0.42 });
        ui.fill([panel[0], panel[1], panel[2], 1.0], 0.0, [1.0; 3], if dark { 0.05 } else { 0.5 });
        // one highlight glides to the place the window is in (the way the shell's menus do)
        let row = |i: usize| [16.0, 56.0 + i as f32 * 36.0, SIDE_W - 32.0, 32.0];
        match self.places.iter().position(|p| self.cwd == p.path) {
            Some(i) => {
                for (s, v) in self.side_hl.iter_mut().zip(row(i)) {
                    s.target = v;
                }
                self.side_hl[4].target = 1.0;
                if !self.side_hl_init {
                    self.side_hl_init = true;
                    for s in self.side_hl.iter_mut() {
                        s.snap();
                    }
                    self.side_hl[4].x = 0.0;
                }
            }
            None => self.side_hl[4].target = 0.0,
        }
        let dt = ui.dt();
        for s in self.side_hl.iter_mut() {
            s.update(dt);
        }
        host.redraw |= self.side_hl.iter().any(|s| !s.at_rest());
        let a = self.side_hl[4].x.clamp(0.0, 1.0);
        let [hx, hy, hw, hh] = [self.side_hl[0].x, self.side_hl[1].x, self.side_hl[2].x.max(0.0), self.side_hl[3].x.max(0.0)];
        ui.fill([hx, hy, hw, hh], 10.0, ink, 0.09 * a);
        let mut go = None;
        for (i, p) in self.places.iter().enumerate() {
            let r = row(i);
            let here = self.cwd == p.path;
            let hov = ui.hit(r);
            let wash = ui.ease(40_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
            ui.fill(r, 10.0, ink, 0.07 * wash * if here { 0.0 } else { 1.0 });
            ui.glyph(r[0] + 18.0, r[1] + r[3] / 2.0, 16.0, p.icon, accent, 1.0);
            ui.text(r[0] + 40.0, r[1] + r[3] / 2.0 + 4.8, &p.name, 13.5, here, ink, 0.95, Align::Left);
            if ui.click && hov {
                go = Some(p.path.clone());
            }
        }
        if let Some(p) = go {
            self.go(p);
        }

    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (chooser, start) = Chooser::from_args(&args);
    let start = start.unwrap_or_else(fsops::home);
    let title = chooser.as_ref().map_or("Memrium Files".to_string(), |c| c.title.clone());
    let (w, h) = if chooser.is_some() { (880, 580) } else { (960, 620) };
    app::run(Config::new(&title, "memrium-files").size(w, h).min_size(560, 360).decorations(Decorations::Client), FilesApp::new(start, chooser));
}
