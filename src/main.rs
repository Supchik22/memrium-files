//! Memrium Files: the desktop's file manager, built on Chebrium. A standalone Wayland client (software-drawn, redraws only
//! while something moves) that shares its file operations, trash and clipboard with the desktop through `fsops` and the
//! Wayland clipboard. It is also the desktop's file dialog (`--choose`, see `chooser`).
//!
//! Three views (details, compact, icons with thumbnails), the icon theme's file-type icons, context menus, a filter, a
//! location field (also for `smb://` and other servers), presets of the look (Dolphin, Icons, Details, Minimal) kept in
//! `~/.config/memrium/files.toml`, and drag and drop both ways.
//!
//! The modules: `view` draws, `actions` holds the menus and what their entries do, `icons` makes the pictures, `watch`
//! notices changes made by others.

mod actions;
mod chooser;
mod icons;
mod view;
mod watch;

use chebrium::app::{self, App, Config, Decorations, Host, Modifiers, PointerEvt, PointerKind, Waker};
use chebrium::appmenu::Menu;
use chebrium::input::{Key, KeyMods};
use chebrium::popmenu::PopMenu;
use chebrium::spring::Spring;
use chebrium::ui::Ui;
use chooser::{Chooser, Mode};
use memrium_de::files::{cmd, FilesPrefs, ViewMode};
use memrium_de::fsops::{self, Entry, Fm};
use std::path::{Path, PathBuf};
use std::time::Instant;

pub(crate) struct Place {
    pub name: &'static str,
    pub icon: &'static str,
    pub path: PathBuf,
}

/// The places of the sidebar: the home folders, then the system ones.
fn places() -> (Vec<Place>, Vec<Place>) {
    let home = fsops::home();
    let mut user = vec![Place { name: "Home", icon: "house", path: home.clone() }];
    for (name, icon, dir) in [("Desktop", "monitor", "Desktop"), ("Documents", "file-text", "Documents"), ("Downloads", "download", "Downloads"), ("Pictures", "image", "Pictures"), ("Music", "music", "Music"), ("Videos", "video", "Videos")] {
        let path = home.join(dir);
        if path.is_dir() {
            user.push(Place { name, icon, path });
        }
    }
    let system = vec![Place { name: "Computer", icon: "laptop", path: PathBuf::from(fsops::COMPUTER) }, Place { name: "Root", icon: "hard-drive", path: PathBuf::from("/") }, Place { name: "Trash", icon: "trash-2", path: fsops::trash_path() }];
    (user, system)
}

/// A press on a row that may turn into a drag of the selected files out of the window.
struct Press {
    at: [f32; 2],
    paths: Vec<PathBuf>,
}

pub(crate) struct FilesApp {
    pub prefs: FilesPrefs,
    pub cwd: PathBuf,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    /// everything in the folder, and the indices of what the filter lets through (what is shown)
    pub(crate) entries: Vec<Entry>,
    pub shown: Vec<usize>,
    pub sel: Vec<PathBuf>,
    /// where a Shift range starts
    anchor: Option<PathBuf>,
    /// the keyboard's row (an index into `shown`)
    pub cursor: Option<usize>,
    pub fm: Fm,
    gen: u64,
    pub places: Vec<Place>,
    pub system: Vec<Place>,
    /// mounted disks under the places, with how full they are
    pub devices: Vec<fsops::Mount>,
    /// where the list wants to be scrolled to (the drawn position eases towards it)
    pub scroll: f32,
    /// bring the cursor row into view on the next frame
    pub reveal: bool,
    pub renaming: Option<PathBuf>,
    pub want_rename: bool,
    pub rename_buf: String,
    /// the filter: what narrows the list, whether its field is shown, and that it wants the keyboard
    pub filter: String,
    pub filter_open: bool,
    pub want_filter: bool,
    /// the location field is being edited: the text to start it with on the next frame
    pub want_location: Option<String>,
    pub locating: bool,
    typeahead: (String, Instant),
    pub error: String,
    pub title: String,
    /// the context menu that is open
    pub menu: Option<PopMenu>,
    /// a picture renamed to another format's ending, waiting for the answer: (the file, the name typed)
    pending_convert: Option<(PathBuf, String)>,
    /// a server being connected to: its address and the answer when it comes
    mounting: Option<(String, std::sync::mpsc::Receiver<Result<PathBuf, String>>)>,
    /// the Properties card of this entry is open
    pub props: Option<Entry>,
    press: Option<Press>,
    /// the entries' rectangles as last drawn (index into `shown`), and the sidebar's places, to find what is under a
    /// point (a right click, a drop)
    pub rects: Vec<(usize, [f32; 4])>,
    pub place_rects: Vec<(PathBuf, [f32; 4])>,
    /// how many entries a row of the grid holds (for the arrow keys)
    pub cols: usize,
    /// the list's viewport as last drawn
    pub list_rect: [f32; 4],
    /// a text field has the keyboard (the window's own shortcuts wait)
    pub editing: bool,
    pub icons: icons::Icons,
    watch: watch::Watch,
    waker: Option<Waker>,
    /// the file dialog the portal asked for (None: the ordinary file manager)
    pub chooser: Option<Chooser>,
    /// the answer was given: close the window on the next chance
    pub done: bool,
    /// the sidebar's one highlight: x, y, w, h and opacity
    pub side_hl: [Spring; 5],
    pub side_hl_init: bool,
    /// where the pointer was at the last right press (a menu opened from the keyboard goes there too)
    pub last_pointer: [f32; 2],
    /// the place a sidebar menu was opened for
    menu_place: Option<PathBuf>,
}

impl FilesApp {
    fn new(start: PathBuf, chooser: Option<Chooser>) -> FilesApp {
        let (places, system) = places();
        let mut app = FilesApp {
            prefs: FilesPrefs::load(),
            cwd: start,
            back: Vec::new(),
            forward: Vec::new(),
            entries: Vec::new(),
            shown: Vec::new(),
            sel: Vec::new(),
            anchor: None,
            cursor: None,
            fm: Fm::default(),
            gen: 0,
            places,
            system,
            devices: fsops::mounts(),
            scroll: 0.0,
            reveal: false,
            renaming: None,
            want_rename: false,
            rename_buf: String::new(),
            filter: String::new(),
            filter_open: false,
            want_filter: false,
            want_location: None,
            locating: false,
            typeahead: (String::new(), Instant::now()),
            error: String::new(),
            title: String::new(),
            menu: None,
            pending_convert: None,
            mounting: None,
            props: None,
            press: None,
            rects: Vec::new(),
            place_rects: Vec::new(),
            cols: 1,
            list_rect: [0.0; 4],
            editing: false,
            icons: icons::Icons::new(),
            watch: watch::Watch::default(),
            waker: None,
            chooser,
            done: false,
            side_hl: [Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2200.0, 1.0), Spring::new(0.0, 2400.0, 1.0)],
            side_hl_init: false,
            last_pointer: [200.0, 120.0],
            menu_place: None,
        };
        app.cwd = app.cwd.canonicalize().unwrap_or(app.cwd.clone());
        app.reload();
        app
    }

    // ---------------------------------------------------------------- the folder

    pub fn reload(&mut self) {
        self.entries = if fsops::is_computer(&self.cwd) {
            self.error.clear();
            fsops::drives()
        } else {
            let p = &self.prefs;
            match fsops::list_dir(&self.cwd, p.show_hidden, p.sort, p.sort_desc, p.dirs_first) {
                Ok(mut e) => {
                    self.error.clear();
                    if let Some(c) = &self.chooser {
                        e.retain(|x| c.shows(x));
                    }
                    e
                }
                Err(e) => {
                    self.error = if e.kind() == std::io::ErrorKind::PermissionDenied { "Permission denied".into() } else { e.to_string() };
                    Vec::new()
                }
            }
        };
        let entries = &self.entries;
        self.sel.retain(|p| entries.iter().any(|e| &e.path == p));
        self.apply_filter();
        self.watch.follow((!fsops::is_computer(&self.cwd)).then(|| self.cwd.clone()));
    }

    pub fn apply_filter(&mut self) {
        let f = self.filter.to_lowercase();
        self.shown = (0..self.entries.len()).filter(|&i| f.is_empty() || self.entries[i].title().to_lowercase().contains(&f) || self.entries[i].name.to_lowercase().contains(&f)).collect();
        self.cursor = self.cursor.filter(|&i| i < self.shown.len());
    }

    /// The entry shown at position `i`.
    pub fn at(&self, i: usize) -> &Entry {
        &self.entries[self.shown[i]]
    }

    pub fn shown_len(&self) -> usize {
        self.shown.len()
    }

    pub fn selected_entries(&self) -> Vec<&Entry> {
        self.shown.iter().map(|&i| &self.entries[i]).filter(|e| self.sel.contains(&e.path)).collect()
    }

    pub fn in_trash(&self) -> bool {
        self.cwd.starts_with(fsops::trash_path())
    }

    pub fn computer(&self) -> bool {
        fsops::is_computer(&self.cwd)
    }

    // ---------------------------------------------------------------- moving around

    pub fn go(&mut self, path: PathBuf) {
        let path = if fsops::is_computer(&path) { path } else { path.canonicalize().unwrap_or(path) };
        if path == self.cwd {
            return;
        }
        if path == fsops::trash_path() {
            let _ = std::fs::create_dir_all(&path);
        }
        self.back.push(std::mem::replace(&mut self.cwd, path));
        self.forward.clear();
        self.arrived();
    }

    fn arrived(&mut self) {
        self.sel.clear();
        self.anchor = None;
        self.cursor = None;
        self.scroll = 0.0;
        self.renaming = None;
        self.filter.clear();
        self.filter_open = false;
        self.locating = false;
        self.fm.note.clear();
        self.props = None;
        if let Some(c) = &mut self.chooser {
            c.replace = false;
        }
        self.reload();
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    pub fn go_back(&mut self) {
        if let Some(p) = self.back.pop() {
            self.forward.push(std::mem::replace(&mut self.cwd, p));
            self.arrived();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(p) = self.forward.pop() {
            self.back.push(std::mem::replace(&mut self.cwd, p));
            self.arrived();
        }
    }

    pub fn can_up(&self) -> bool {
        !self.computer() && self.cwd.parent().is_some()
    }

    pub fn go_up(&mut self) {
        if !self.can_up() {
            return;
        }
        if let Some(parent) = self.cwd.parent() {
            let from = self.cwd.clone();
            self.go(parent.to_path_buf());
            // land on the folder we came out of
            self.select_path(&from);
        }
    }

    /// Connect to a server in the background; its folder opens when it is there.
    pub fn connect(&mut self, url: String) {
        let (tx, rx) = std::sync::mpsc::channel();
        let u = url.clone();
        let waker = self.waker.clone();
        std::thread::spawn(move || {
            let _ = tx.send(fsops::mount_server(&u));
            if let Some(w) = waker {
                w.wake();
            }
        });
        self.go(PathBuf::from(fsops::COMPUTER));
        self.fm.note = chebrium::i18n::trf("Connecting to {}…", &[url.clone()]);
        self.mounting = Some((url, rx));
    }

    fn poll_mount(&mut self) {
        let Some((url, rx)) = &self.mounting else { return };
        match rx.try_recv() {
            Ok(Ok(path)) => {
                self.mounting = None;
                self.fm.note.clear();
                self.go(path);
            }
            Ok(Err(e)) => {
                self.fm.note = chebrium::i18n::trf("Could not connect to {}: {}", &[url.clone(), e]);
                self.mounting = None;
                self.reload();
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            Err(_) => self.mounting = None,
        }
    }

    /// What was typed in the location field: a folder, a file (its folder opens with it selected) or a server.
    pub fn go_typed(&mut self, typed: &str) {
        let typed = typed.trim().trim_matches(|c| c == '"' || c == '\'');
        if typed.is_empty() {
            return;
        }
        if let Some(url) = fsops::server_url(typed) {
            return self.connect(url);
        }
        let path = expand_path(typed, &self.cwd);
        if path.is_dir() {
            self.go(path);
        } else if path.is_file() {
            if let Some(dir) = path.parent() {
                self.go(dir.to_path_buf());
                self.select_path(&path.canonicalize().unwrap_or(path));
            }
        } else {
            self.fm.note = chebrium::i18n::trf("No such folder: {}", &[path.display().to_string()]);
        }
    }

    // ---------------------------------------------------------------- selection

    pub fn select_path(&mut self, path: &Path) {
        if let Some(i) = self.shown.iter().position(|&k| self.entries[k].path == path) {
            self.select_only(i);
            self.reveal = true;
        }
    }

    pub fn select_only(&mut self, i: usize) {
        let e = self.at(i).clone();
        self.sel = vec![e.path.clone()];
        self.anchor = Some(e.path.clone());
        self.cursor = Some(i);
        // saving over an existing file: its name goes into the field
        if let Some(c) = self.chooser.as_mut().filter(|c| c.mode == Mode::Save && !e.is_dir) {
            c.name = e.name.clone();
            c.replace = false;
        }
    }

    /// Whether several entries may be selected (not in a dialog that wants one file).
    pub fn multi(&self) -> bool {
        self.chooser.as_ref().map_or(true, |c| c.multiple && c.mode != Mode::Save)
    }

    pub fn toggle(&mut self, i: usize) {
        if !self.multi() {
            return self.select_only(i);
        }
        let p = self.at(i).path.clone();
        match self.sel.iter().position(|q| *q == p) {
            Some(at) => {
                self.sel.remove(at);
            }
            None => self.sel.push(p.clone()),
        }
        self.anchor = Some(p);
        self.cursor = Some(i);
    }

    /// Select from the anchor to entry `i` (Shift with a click or an arrow).
    pub fn select_to(&mut self, i: usize) {
        if !self.multi() {
            return self.select_only(i);
        }
        let a = self.anchor.as_ref().and_then(|a| self.shown.iter().position(|&k| self.entries[k].path == *a)).unwrap_or(i);
        let (lo, hi) = (a.min(i), a.max(i));
        self.sel = (lo..=hi).map(|k| self.at(k).path.clone()).collect();
        self.cursor = Some(i);
    }

    pub fn select_all(&mut self) {
        if self.multi() {
            self.sel = self.shown.iter().map(|&i| self.entries[i].path.clone()).collect();
        }
    }

    fn move_cursor(&mut self, delta: isize, extend: bool) {
        let n = self.shown.len();
        if n == 0 {
            return;
        }
        let next = match self.cursor {
            Some(c) => (c as isize + delta).clamp(0, n as isize - 1) as usize,
            None if delta >= 0 => 0,
            None => n - 1,
        };
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.at(self.cursor.unwrap_or(next)).path.clone());
            }
            self.select_to(next);
        } else {
            self.select_only(next);
        }
        self.reveal = true;
    }

    /// The entry under a point of the window (index into `shown`), as last drawn.
    pub fn entry_at(&self, p: [f32; 2]) -> Option<usize> {
        if !chebrium::ui::inside(p, self.list_rect) {
            return None;
        }
        self.rects.iter().find(|(_, r)| chebrium::ui::inside(p, *r)).map(|(i, _)| *i)
    }

    /// Where files dropped at `p` go: a place of the sidebar, a folder under it, or the folder shown.
    pub fn drop_target(&self, p: [f32; 2]) -> Option<PathBuf> {
        if let Some((path, _)) = self.place_rects.iter().find(|(_, r)| chebrium::ui::inside(p, *r)) {
            return (!fsops::is_computer(path)).then(|| path.clone());
        }
        if let Some(i) = self.entry_at(p) {
            let e = self.at(i);
            if e.is_dir {
                return Some(e.path.clone());
            }
        }
        (!self.computer()).then(|| self.cwd.clone())
    }

    // ---------------------------------------------------------------- opening

    pub fn open(&mut self, i: usize) {
        let e = self.at(i).clone();
        if e.is_dir {
            self.go(e.path);
        } else if self.chooser.is_some() {
            self.select_only(i);
            self.accept();
        } else {
            fsops::open_external(&e.path, &socket());
        }
    }

    /// Whether the dialog's main button can be pressed.
    pub fn can_accept(&self) -> bool {
        let Some(c) = &self.chooser else { return false };
        let computer = self.computer();
        match c.mode {
            Mode::Open => self.selected_entries().iter().any(|e| !e.is_dir),
            Mode::Dir => !computer || !self.sel.is_empty(),
            Mode::Save => !computer && !c.name.trim().is_empty() && !c.name.contains('/'),
        }
    }

    /// The main button of the dialog: gives the answer (saving over a file asks once more first).
    pub fn accept(&mut self) {
        if !self.can_accept() {
            return;
        }
        let cwd = self.cwd.clone();
        let sel = self.selected_entries();
        let sel_dirs: Vec<PathBuf> = sel.iter().filter(|e| e.is_dir).map(|e| e.path.clone()).collect();
        let sel_files: Vec<PathBuf> = sel.iter().filter(|e| !e.is_dir).map(|e| e.path.clone()).collect();
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

    // ---------------------------------------------------------------- changes

    /// Files changed on disk through `fm`: show the folder as it is now.
    pub fn changed(&mut self) {
        self.gen = self.fm.gen;
        self.reload();
    }

    /// The preferences changed: keep them (not from a file dialog, which only borrows the look).
    pub fn save_prefs(&mut self) {
        if self.chooser.is_none() {
            self.prefs.save();
        }
    }

    /// The name of the entry being renamed was given: rename it, or ask first when it looks like a conversion.
    pub fn commit_rename(&mut self, path: PathBuf, name: String) {
        let name = name.trim().to_string();
        self.renaming = None;
        if name.is_empty() {
            return;
        }
        if let Some(label) = fsops::rename_converts_to(&path, &name) {
            let items = fsops::rename_prompt(&path, &name, label).iter().map(actions::item_of).collect();
            self.menu = Some(PopMenu::new(self.last_pointer, items));
            self.pending_convert = Some((path, name));
            return;
        }
        if let Some(to) = self.fm.rename(&path, &name) {
            self.changed();
            self.select_path(&to);
        }
    }

    /// Files dropped on the window at `at`: moved there when they are on the same disk, else copied (Ctrl copies always).
    fn drop_files(&mut self, files: Vec<PathBuf>, at: Option<[f32; 2]>, copy: bool) {
        use std::os::unix::fs::MetadataExt;
        let Some(dir) = at.and_then(|p| self.drop_target(p)).or_else(|| (!self.computer()).then(|| self.cwd.clone())) else { return };
        if dir.starts_with(fsops::trash_path()) {
            // into the Trash means "throw away"
            self.fm.trash_all(&files);
            return self.changed();
        }
        let dev = |p: &Path| std::fs::metadata(p).map(|m| m.dev()).ok();
        let here = dev(&dir);
        let (same, other): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|f| !copy && dev(f) == here && here.is_some());
        let n = self.fm.drop_into(&same, &dir, false) + self.fm.drop_into(&other, &dir, true);
        if n > 0 {
            let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into());
            self.fm.note = chebrium::i18n::trf("{} item(s) put in {}", &[n.to_string(), name]);
        }
        self.changed();
    }

    // ---------------------------------------------------------------- keys

    fn key_inner(&mut self, key: Key, mods: KeyMods, host: &mut Host) {
        if let Some(m) = &mut self.menu {
            m.key(key);
            return;
        }
        if self.props.is_some() && matches!(key, Key::Esc | Key::Enter) {
            self.props = None;
            return;
        }
        let grid = self.prefs.view != ViewMode::Details;
        let cols = if grid { self.cols.max(1) as isize } else { 1 };
        let shift = mods.shift;
        match key {
            Key::Left if mods.alt => self.go_back(),
            Key::Right if mods.alt => self.go_forward(),
            Key::Up if mods.alt => self.go_up(),
            Key::Home if mods.alt => self.go(fsops::home()),
            Key::Down if mods.ctrl => {
                if let Some(i) = self.cursor {
                    self.open(i);
                }
            }
            Key::Down => self.move_cursor(cols, shift),
            Key::Up => self.move_cursor(-cols, shift),
            Key::Right if grid => self.move_cursor(1, shift),
            Key::Left if grid => self.move_cursor(-1, shift),
            Key::Right => {
                if let Some(i) = self.cursor.filter(|&i| self.at(i).is_dir) {
                    self.open(i);
                }
            }
            Key::Left => self.go_up(),
            Key::PageDown => self.move_cursor(10 * cols, shift),
            Key::PageUp => self.move_cursor(-10 * cols, shift),
            Key::Home => self.move_cursor(isize::MIN / 2, shift),
            Key::End => self.move_cursor(isize::MAX / 2, shift),
            Key::Enter => {
                if let Some(i) = self.cursor {
                    self.open(i);
                }
            }
            Key::Backspace => self.go_up(),
            Key::Delete if shift || self.in_trash() => self.command(cmd::DELETE_FOREVER, host),
            Key::Delete => self.command(cmd::TRASH, host),
            Key::F2 => self.command(cmd::RENAME, host),
            Key::F5 => self.command(cmd::REFRESH, host),
            Key::Esc if !self.filter.is_empty() || self.filter_open => {
                self.filter.clear();
                self.filter_open = false;
                self.apply_filter();
            }
            Key::Esc if self.chooser.is_some() => host.quit = true,
            Key::Esc => {
                self.sel.clear();
                self.fm.note.clear();
            }
            Key::Char(c) if mods.ctrl => match c {
                'a' => self.command(cmd::SELECT_ALL, host),
                'c' => self.command(cmd::COPY, host),
                'x' => self.command(cmd::CUT, host),
                'v' => self.command(cmd::PASTE, host),
                'z' => self.command(cmd::UNDO, host),
                'h' => self.command(cmd::HIDDEN, host),
                'n' if shift => self.command(cmd::NEW_FOLDER, host),
                'n' => self.command(actions::NEW_WINDOW, host),
                'f' => self.command(actions::FIND, host),
                'l' => self.command(actions::LOCATION, host),
                '1' => self.command(cmd::VIEW_ICONS, host),
                '2' => self.command(cmd::VIEW_DETAILS, host),
                '3' => self.command(cmd::VIEW_COMPACT, host),
                '=' | '+' => self.command(actions::ZOOM_IN, host),
                '-' => self.command(actions::ZOOM_OUT, host),
                'i' => self.command(cmd::PROPERTIES, host),
                'q' | 'w' => host.quit = true,
                _ => {}
            },
            Key::Char(c) if !mods.alt && !c.is_control() && c != ' ' => {
                // type-ahead: jump to the first entry whose name starts with what was typed
                if self.typeahead.1.elapsed().as_millis() > 900 {
                    self.typeahead.0.clear();
                }
                self.typeahead.0.push(c);
                self.typeahead.1 = Instant::now();
                let q = self.typeahead.0.to_lowercase();
                if let Some(i) = (0..self.shown.len()).find(|&i| self.at(i).title().to_lowercase().starts_with(&q)) {
                    self.select_only(i);
                    self.reveal = true;
                }
            }
            _ => {}
        }
    }
}

/// What was typed in the location field as a path: `~` is home, a relative path starts from the folder shown.
fn expand_path(typed: &str, cwd: &Path) -> PathBuf {
    let typed = typed.strip_prefix("file://").unwrap_or(typed);
    if typed == "~" {
        fsops::home()
    } else if let Some(rest) = typed.strip_prefix("~/") {
        fsops::home().join(rest)
    } else {
        cwd.join(typed)
    }
}

/// The desktop the window is on (programs started from here go there too).
pub(crate) fn socket() -> String {
    std::env::var("WAYLAND_DISPLAY").unwrap_or_default()
}

impl App for FilesApp {
    fn start(&mut self, waker: Waker) {
        self.icons.start(waker.clone());
        self.watch.start(waker.clone());
        self.waker = Some(waker);
    }

    fn tick(&mut self, _host: &mut Host) {
        self.poll_mount();
        let ch = self.watch.take();
        if ch.prefs {
            let p = FilesPrefs::load();
            if p != self.prefs {
                let relist = (p.show_hidden, p.sort, p.sort_desc, p.dirs_first) != (self.prefs.show_hidden, self.prefs.sort, self.prefs.sort_desc, self.prefs.dirs_first);
                self.prefs = p;
                if relist {
                    self.reload();
                }
            }
        }
        // another program changed the folder (not while a name is being typed over one of its entries)
        if ch.dir && self.renaming.is_none() {
            self.reload();
            self.icons.forget();
        }
        if ch.mounts {
            self.devices = fsops::mounts();
            if self.computer() {
                self.reload();
            }
        }
        if self.fm.gen != self.gen {
            self.changed();
        }
    }

    fn menus(&self) -> Vec<Menu> {
        actions::bar_menus(self)
    }

    fn menu_action(&mut self, id: u32, host: &mut Host) {
        host.redraw = true;
        self.command(id, host);
        host.quit |= self.done;
    }

    fn key(&mut self, key: Key, mods: KeyMods, host: &mut Host) {
        host.redraw = true;
        self.key_inner(key, mods, host);
        host.quit |= self.done;
    }

    fn raw_pointer(&mut self, event: &PointerEvt, _mods: &Modifiers, host: &mut Host) -> bool {
        // the right button opens the menu of what is under it
        if let PointerKind::Press { button: 0x111, .. } = event.kind {
            self.last_pointer = event.pos;
            self.context_menu(event.pos);
            host.redraw = true;
            return true;
        }
        false
    }

    fn dropped(&mut self, files: Vec<PathBuf>, _text: String, host: &mut Host) {
        if files.is_empty() || self.chooser.is_some() {
            return;
        }
        let at = host.drag_over;
        self.drop_files(files, at, false);
        host.redraw = true;
    }

    fn draw(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        if self.fm.gen != self.gen {
            self.changed();
        }
        self.draw_window(ui, size, host);
        // a press on a row and a move: the selected files go out of the window as a drag
        if let Some(p) = &self.press {
            if !ui.held {
                self.press = None;
            } else if !host.dragging && (ui.mouse[0] - p.at[0]).hypot(ui.mouse[1] - p.at[1]) > 6.0 {
                let paths = self.press.take().map(|p| p.paths).unwrap_or_default();
                self.start_drag(paths, host);
            }
        }
        host.quit |= self.done;
    }
}

impl FilesApp {
    /// A press on a selected entry: dragging from here takes the selection along.
    pub fn pressed(&mut self, at: [f32; 2]) {
        if self.chooser.is_some() || self.computer() {
            return;
        }
        let paths: Vec<PathBuf> = self.selected_entries().iter().map(|e| e.path.clone()).collect();
        if !paths.is_empty() {
            self.press = Some(Press { at, paths });
        }
    }

    fn start_drag(&mut self, paths: Vec<PathBuf>, host: &mut Host) {
        let list: String = paths.iter().map(|p| fsops::uri_of(p) + "\r\n").collect();
        let bytes = std::sync::Arc::new(list.into_bytes());
        let make: std::sync::Arc<dyn Fn() -> Vec<u8> + Send + Sync> = std::sync::Arc::new(move || (*bytes).clone());
        // the first file's icon follows the pointer
        let icon = paths.first().and_then(|p| self.entries.iter().find(|e| &e.path == p).cloned()).and_then(|e| self.icons.get(&e, true)).map(|i| {
            let side = (48.0 * host.scale.max(1.0)).round() as u32;
            let small = image::imageops::resize(&*i.rgba, side, side, image::imageops::FilterType::Triangle);
            chebrium::app::DragIcon { rgba: small.into_raw(), w: side, h: side, scale: host.scale.max(1.0).round() as i32, hot: [24, 24] }
        });
        host.start_drag = Some(chebrium::app::DragOut { offers: vec![("text/uri-list".into(), make)], icon });
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (chooser, start) = Chooser::from_args(&args);
    // a file given instead of a folder (a "Show in folder" from another program): its folder opens with it selected
    let file = if chooser.is_none() { args.iter().filter(|a| !a.starts_with("--")).map(PathBuf::from).find(|p| p.is_file()) } else { None };
    let computer = chooser.is_none() && args.iter().any(|a| a == fsops::COMPUTER);
    let start = if computer { Some(PathBuf::from(fsops::COMPUTER)) } else { start };
    let start = start.or_else(|| file.as_ref().and_then(|f| f.parent().map(|d| d.to_path_buf()))).unwrap_or_else(fsops::home);
    let title = chooser.as_ref().map_or("Files".to_string(), |c| c.title.clone());
    let (w, h) = if chooser.is_some() { (880, 580) } else { (980, 620) };
    app::run(Config::new(&title, "memrium-files").size(w, h).min_size(560, 360).decorations(Decorations::Client), {
        let mut app = FilesApp::new(start, chooser);
        if let Some(f) = &file {
            app.select_path(&f.canonicalize().unwrap_or(f.clone()));
        }
        app
    });
}

#[cfg(test)]
mod tests;
