//! The menus of Files (the shell's menu bar and the right-click menu) and what their entries do. The ids are the ones the
//! desktop's icons use (`memrium_de::files::cmd`), plus a few of the window's own.

use crate::{FilesApp, Place};
use chebrium::app::Host;
use chebrium::appmenu::{Item, Menu};
use chebrium::popmenu::PopMenu;
use memrium_de::files::{cmd, FilesPrefs, ViewMode, PRESETS};
use memrium_de::fsops::{self, SortBy};
use memrium_de::menu::MenuItem;
use std::path::PathBuf;

pub const NEW_WINDOW: u32 = 500;
pub const FIND: u32 = 501;
pub const LOCATION: u32 = 502;
pub const ZOOM_IN: u32 = 503;
pub const ZOOM_OUT: u32 = 504;
pub const CLOSE: u32 = 505;
pub const OPEN_IN_NEW: u32 = 506;

/// The shell's menu entry as one of the toolkit's.
pub fn item_of(m: &MenuItem) -> Item {
    if m.separator {
        return Item::sep();
    }
    let mut it = if m.sub.is_empty() { Item::new(m.id, m.label.clone()) } else { Item::parent(m.label.clone(), m.sub.iter().map(item_of).collect()) };
    it.key = m.shortcut.clone();
    it.enabled = m.enabled;
    it.checked = m.checked;
    it.danger = m.danger;
    if let Some(i) = m.icon {
        it.icon = i.into();
    }
    it
}

fn mi(id: u32, label: &str, icon: &'static str) -> Item {
    Item::new(id, label).icon(icon)
}

/// The "View" entries: how the folder is shown right now (mode, sorting, hidden files, zoom, filter).
fn view_items(a: &FilesApp) -> Vec<Item> {
    let p = &a.prefs;
    vec![
        Item::parent("View mode", vec![Item::new(cmd::VIEW_ICONS, "Icons").key("Ctrl+1").checked(p.view == ViewMode::Icons), Item::new(cmd::VIEW_DETAILS, "Details").key("Ctrl+2").checked(p.view == ViewMode::Details), Item::new(cmd::VIEW_COMPACT, "Compact").key("Ctrl+3").checked(p.view == ViewMode::Compact)]),
        Item::parent(
            "Sort by",
            vec![
                Item::new(cmd::SORT_NAME, "Name").checked(p.sort == SortBy::Name),
                Item::new(cmd::SORT_SIZE, "Size").checked(p.sort == SortBy::Size),
                Item::new(cmd::SORT_DATE, "Date modified").checked(p.sort == SortBy::Modified),
                Item::new(cmd::SORT_KIND, "Type").checked(p.sort == SortBy::Kind),
                Item::sep(),
                Item::new(cmd::SORT_DESC, "Descending").checked(p.sort_desc),
                Item::new(cmd::DIRS_FIRST, "Folders first").checked(p.dirs_first),
            ],
        ),
        Item::new(cmd::HIDDEN, "Show hidden files").key("Ctrl+H").checked(p.show_hidden),
        Item::sep(),
        Item::new(ZOOM_IN, "Zoom In").key("Ctrl++").enabled(p.view == ViewMode::Icons),
        Item::new(ZOOM_OUT, "Zoom Out").key("Ctrl+-").enabled(p.view == ViewMode::Icons),
        Item::sep(),
        Item::new(FIND, "Filter…").key("Ctrl+F"),
        Item::new(cmd::REFRESH, "Refresh").key("F5"),
    ]
}

/// The file manager's own settings: the look of the window (kept between runs), with the presets.
fn settings_items(a: &FilesApp) -> Vec<Item> {
    let p = &a.prefs;
    vec![
        Item::parent("Preset", PRESETS.iter().enumerate().map(|(i, n)| Item::new(cmd::PRESET_BASE + i as u32, *n).checked(p.preset == *n)).collect()),
        Item::sep(),
        Item::parent("Icon size", vec![Item::new(cmd::ICON_SMALL, "Small").checked(p.icon_size < 56.0), Item::new(cmd::ICON_MEDIUM, "Medium").checked((56.0..80.0).contains(&p.icon_size)), Item::new(cmd::ICON_LARGE, "Large").checked(p.icon_size >= 80.0)]),
        Item::parent("Columns", vec![Item::new(cmd::COL_KIND, "Type").checked(p.col_kind), Item::new(cmd::COL_SIZE, "Size").checked(p.col_size), Item::new(cmd::COL_DATE, "Date modified").checked(p.col_date)]),
        Item::sep(),
        Item::new(cmd::SIDEBAR, "Sidebar").checked(p.sidebar),
        Item::new(cmd::STATUSBAR, "Status bar").checked(p.status_bar),
        Item::new(cmd::LABELS, "Labelled toolbar").checked(p.toolbar_labels),
        Item::new(cmd::PATH_CRUMBS, "Breadcrumb path").checked(p.path_style == memrium_de::files::PathStyle::Breadcrumb),
        Item::new(cmd::SEL_ROUNDED, "Rounded selection").checked(!p.selection_row),
    ]
}

/// The menus of the shell's bar for this window.
pub fn bar_menus(a: &FilesApp) -> Vec<Menu> {
    if a.chooser.is_some() {
        return vec![Menu::new("View", view_items(a)), Menu::new("Settings", settings_items(a))];
    }
    let one = a.sel.len() == 1;
    let some = !a.sel.is_empty();
    let go_place = |ps: &[Place], base: u32| -> Vec<Item> { ps.iter().enumerate().map(|(i, p)| mi(base + i as u32, p.name, p.icon)).collect() };
    let mut go = vec![mi(cmd::G_BACK, "Back", "arrow-left").key("Alt+←").enabled(a.can_back()), mi(cmd::G_FWD, "Forward", "arrow-right").key("Alt+→").enabled(a.can_forward()), mi(cmd::G_UP, "Enclosing Folder", "arrow-up").key("Alt+↑").enabled(a.can_up()), Item::sep()];
    go.extend(go_place(&a.places, 600));
    go.push(Item::sep());
    go.extend(go_place(&a.system, 700));
    go.push(Item::sep());
    go.push(mi(LOCATION, "Go to Folder…", "folder-open").key("Ctrl+L"));
    go.push(mi(cmd::CONNECT_SERVER, "Connect to Server…", "server"));
    vec![
        Menu::new(
            "File",
            vec![
                mi(NEW_WINDOW, "New Window", "folder").key("Ctrl+N"),
                mi(cmd::NEW_FOLDER, "New Folder", "folder-plus").key("Ctrl+Shift+N").enabled(!a.computer()),
                mi(cmd::NEW_FILE, "New Text File", "file-plus").enabled(!a.computer()),
                Item::sep(),
                Item::new(cmd::OPEN, "Open").key("Enter").enabled(one),
                Item::new(cmd::RENAME, "Rename").key("F2").enabled(one),
                Item::new(cmd::TRASH, "Move to Trash").key("Delete").enabled(some && !a.in_trash()),
                Item::new(cmd::PROPERTIES, "Properties").key("Ctrl+I").enabled(one),
                Item::sep(),
                Item::new(CLOSE, "Close Window").key("Ctrl+W"),
            ],
        ),
        Menu::new(
            "Edit",
            vec![
                Item::new(cmd::UNDO, "Undo").key("Ctrl+Z").enabled(a.fm.can_undo()),
                Item::sep(),
                Item::new(cmd::CUT, "Cut").key("Ctrl+X").enabled(some),
                Item::new(cmd::COPY, "Copy").key("Ctrl+C").enabled(some),
                Item::new(cmd::PASTE, "Paste").key("Ctrl+V").enabled(!a.computer() && a.fm.can_paste()),
                Item::new(cmd::COPY_PATH, "Copy Path").enabled(one),
                Item::sep(),
                Item::new(cmd::SELECT_ALL, "Select All").key("Ctrl+A"),
            ],
        ),
        Menu::new("View", view_items(a)),
        Menu::new("Go", go),
        Menu::new("Settings", settings_items(a)),
    ]
}

impl FilesApp {
    /// The menu of what is under the pointer: an entry (or the selection), the empty part of the list, or a place.
    pub fn context_menu(&mut self, at: [f32; 2]) {
        if self.props.is_some() {
            return;
        }
        let items = if let Some(i) = self.entry_at(at) {
            let path = self.at(i).path.clone();
            if !self.sel.contains(&path) {
                self.select_only(i);
            }
            self.entry_menu()
        } else if chebrium::ui::inside(at, self.list_rect) {
            self.sel.clear();
            self.background_menu()
        } else if let Some((path, _)) = self.place_rects.iter().find(|(_, r)| chebrium::ui::inside(at, *r)).cloned() {
            let items = self.place_menu(path.clone());
            self.menu_place = Some(path);
            items
        } else {
            return;
        };
        if self.chooser.is_some() && items.is_empty() {
            return;
        }
        self.menu = Some(PopMenu::new(at, items));
    }

    fn place_menu(&self, path: PathBuf) -> Vec<Item> {
        let mut v = vec![mi(OPEN_IN_NEW, "Open in New Window", "folder")];
        if path == fsops::trash_path() {
            v.push(Item::sep());
            v.push(mi(cmd::EMPTY_TRASH, "Empty Trash", "trash-2").danger());
        }
        v
    }

    fn background_menu(&self) -> Vec<Item> {
        let c = self.computer();
        let mut v = Vec::new();
        if c {
            v.push(mi(cmd::CONNECT_SERVER, "Connect to Server…", "server"));
            v.push(Item::sep());
        } else {
            v.push(Item::parent("Create New", vec![mi(cmd::NEW_FOLDER, "Folder", "folder-plus").key("Ctrl+Shift+N"), mi(cmd::NEW_FILE, "Text file", "file-text")]).icon("file-plus"));
            v.push(mi(cmd::PASTE, "Paste", "clipboard-paste").key("Ctrl+V").enabled(self.fm.can_paste()));
            v.push(mi(cmd::UNDO, "Undo", "undo-2").key("Ctrl+Z").enabled(self.fm.can_undo()));
            v.push(Item::sep());
        }
        v.push(Item::new(cmd::SELECT_ALL, "Select all").key("Ctrl+A"));
        v.push(mi(cmd::REFRESH, "Refresh", "refresh-cw").key("F5"));
        if self.in_trash() {
            v.push(mi(cmd::EMPTY_TRASH, "Empty Trash", "trash-2").danger());
        }
        v.push(Item::sep());
        v.extend(view_items(self));
        v.push(Item::parent("Settings", settings_items(self)).icon("settings"));
        if !c {
            v.push(Item::sep());
            v.push(mi(cmd::TERMINAL, "Open Terminal here", "terminal"));
        }
        v
    }

    fn entry_menu(&self) -> Vec<Item> {
        let sel = self.selected_entries();
        let one = sel.len() == 1;
        let in_trash = self.in_trash();
        let paths: Vec<PathBuf> = sel.iter().map(|e| e.path.clone()).collect();
        let mut v = vec![mi(cmd::OPEN, "Open", "external-link").key("Enter").enabled(one), Item::sep()];
        v.push(mi(cmd::CUT, "Cut", "scissors").key("Ctrl+X"));
        v.push(mi(cmd::COPY, "Copy", "copy").key("Ctrl+C"));
        v.push(mi(cmd::PASTE, "Paste", "clipboard-paste").key("Ctrl+V").enabled(self.fm.can_paste() && one && sel[0].is_dir));
        v.push(Item::sep());
        v.push(mi(cmd::RENAME, "Rename…", "pencil").key("F2").enabled(one));
        if in_trash {
            v.push(mi(cmd::DELETE_FOREVER, "Delete permanently", "trash-2").danger());
        } else {
            v.push(mi(cmd::TRASH, "Move to Trash", "trash-2").key("Delete").danger());
        }
        if let Some(m) = fsops::quick_edit_menu(&paths, cmd::CONVERT_BASE) {
            v.push(item_of(&m));
        }
        if one && memrium_de::archive::is_archive(&sel[0].path) {
            v.push(mi(cmd::EXTRACT_HERE, "Extract here", "file-archive"));
        }
        if one && self.computer() && fsops::is_network_path(&sel[0].path) {
            v.push(Item::sep());
            v.push(mi(cmd::DISCONNECT, "Disconnect", "x"));
        }
        if !in_trash {
            v.push(mi(cmd::COMPRESS, "Compress…", "file-archive"));
        }
        v.push(Item::sep());
        v.push(Item::new(cmd::COPY_PATH, "Copy path").enabled(one));
        v.push(mi(cmd::PROPERTIES, "Properties", "info").key("Ctrl+I").enabled(one));
        v
    }

    fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected_entries().iter().map(|e| e.path.clone()).collect()
    }

    /// Start one of our programs on the same desktop.
    fn own(&self, program: &str, args: Vec<String>) {
        fsops::spawn_own(program, &args, &crate::socket());
    }

    /// Do what menu entry or key `id` says.
    pub fn command(&mut self, id: u32, host: &mut Host) {
        let mut prefs = true;
        let paths = self.selected_paths();
        let one = paths.first().cloned().filter(|_| paths.len() == 1);
        match id {
            cmd::OPEN => {
                if let Some(i) = one.and_then(|p| self.shown.iter().position(|&k| self.entries_path(k) == p)) {
                    self.open(i);
                }
            }
            cmd::CUT | cmd::COPY if !paths.is_empty() => self.fm.set_clip(paths, id == cmd::CUT),
            cmd::PASTE if !self.computer() => {
                let dir = match self.selected_entries().as_slice() {
                    [e] if e.is_dir => e.path.clone(),
                    _ => self.cwd.clone(),
                };
                let made = self.fm.paste_into(&dir);
                self.changed();
                if dir == self.cwd && !made.is_empty() {
                    self.sel = made;
                }
            }
            cmd::RENAME if one.is_some() => self.want_rename = true,
            cmd::TRASH if !paths.is_empty() => {
                self.fm.trash_all(&paths);
                self.sel.clear();
                self.cursor = None;
                self.changed();
            }
            cmd::DELETE_FOREVER => {
                for p in &paths {
                    if let Err(e) = fsops::delete_forever(p) {
                        self.fm.note = chebrium::i18n::trf("Could not delete: {}", &[e.to_string()]);
                    }
                }
                self.sel.clear();
                self.fm.gen += 1;
            }
            cmd::EMPTY_TRASH => {
                fsops::empty_trash();
                self.fm.gen += 1;
            }
            cmd::NEW_FOLDER | cmd::NEW_FILE if !self.computer() => {
                let dir = self.cwd.clone();
                let made = if id == cmd::NEW_FOLDER { self.fm.new_folder(&dir) } else { self.fm.new_file(&dir) };
                if let Some(p) = made {
                    self.changed();
                    self.select_path(&p);
                    self.want_rename = true;
                }
            }
            cmd::UNDO => {
                self.fm.undo_last();
                self.changed();
            }
            cmd::REFRESH => {
                self.reload();
                self.icons.forget();
            }
            cmd::SELECT_ALL => self.select_all(),
            cmd::RENAME_CONVERT | cmd::RENAME_ONLY => self.finish_convert(id == cmd::RENAME_CONVERT),
            n if (cmd::CONVERT_BASE..cmd::CONVERT_BASE + fsops::CONVERT_TO.len() as u32).contains(&n) => {
                self.fm.note = fsops::convert_all(&paths, fsops::CONVERT_TO[(n - cmd::CONVERT_BASE) as usize].1);
                self.changed();
            }
            cmd::ROTATE_LEFT | cmd::ROTATE_RIGHT => {
                self.fm.note = fsops::rotate_all(&paths, id == cmd::ROTATE_RIGHT);
                self.icons.forget();
                self.changed();
            }
            cmd::CONVERT_MORE => self.own("memrium_convert", paths.iter().map(|p| p.to_string_lossy().to_string()).collect()),
            cmd::EXTRACT_HERE | cmd::COMPRESS => {
                let mut args = vec![if id == cmd::EXTRACT_HERE { "--extract" } else { "--compress" }.to_string()];
                args.extend(paths.iter().map(|p| p.to_string_lossy().to_string()));
                self.own("memrium_archive", args);
            }
            cmd::CONNECT_SERVER => self.want_location = Some("smb://".into()),
            cmd::DISCONNECT => {
                if let Some(p) = one {
                    self.fm.note = match fsops::disconnect(&p) {
                        Ok(()) => chebrium::i18n::trf("Disconnected {}", &[p.display().to_string()]),
                        Err(e) => chebrium::i18n::trf("Could not disconnect: {}", &[e]),
                    };
                    self.reload();
                }
            }
            cmd::TERMINAL => {
                let mut c = std::process::Command::new(fsops::own_program("memrium_term"));
                c.current_dir(&self.cwd).env("WAYLAND_DISPLAY", crate::socket()).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
                if let Err(e) = c.spawn() {
                    self.fm.note = chebrium::i18n::trf("No terminal: {}", &[e.to_string()]);
                }
            }
            cmd::COPY_PATH => {
                if let Some(p) = one {
                    host.copy = Some(p.to_string_lossy().to_string());
                    self.fm.note = chebrium::i18n::trf("Path copied: {}", &[p.display().to_string()]);
                }
            }
            cmd::PROPERTIES => self.props = one.and_then(|p| self.entry_of(&p)),
            cmd::G_BACK => self.go_back(),
            cmd::G_FWD => self.go_forward(),
            cmd::G_UP => self.go_up(),
            NEW_WINDOW => self.own("memrium_files", vec![self.cwd.to_string_lossy().to_string()]),
            OPEN_IN_NEW => {
                let p = self.menu_place.take().unwrap_or_else(|| self.cwd.clone());
                self.own("memrium_files", vec![p.to_string_lossy().to_string()]);
            }
            CLOSE => host.quit = true,
            FIND => {
                self.filter_open = true;
                self.want_filter = true;
            }
            LOCATION => self.want_location = Some(self.cwd.to_string_lossy().to_string()),
            ZOOM_IN => self.prefs.icon_size = (self.prefs.icon_size + 8.0).min(128.0),
            ZOOM_OUT => self.prefs.icon_size = (self.prefs.icon_size - 8.0).max(40.0),
            n if (600..600 + self.places.len() as u32).contains(&n) => {
                self.go(self.places[(n - 600) as usize].path.clone());
                prefs = false;
            }
            n if (700..700 + self.system.len() as u32).contains(&n) => {
                self.go(self.system[(n - 700) as usize].path.clone());
                prefs = false;
            }
            cmd::VIEW_ICONS => self.prefs.view = ViewMode::Icons,
            cmd::VIEW_DETAILS => self.prefs.view = ViewMode::Details,
            cmd::VIEW_COMPACT => self.prefs.view = ViewMode::Compact,
            cmd::SORT_NAME | cmd::SORT_SIZE | cmd::SORT_DATE | cmd::SORT_KIND => {
                self.sort_by(match id {
                    cmd::SORT_NAME => SortBy::Name,
                    cmd::SORT_SIZE => SortBy::Size,
                    cmd::SORT_DATE => SortBy::Modified,
                    _ => SortBy::Kind,
                });
            }
            cmd::SORT_DESC => {
                self.prefs.sort_desc = !self.prefs.sort_desc;
                self.reload();
            }
            cmd::DIRS_FIRST => {
                self.prefs.dirs_first = !self.prefs.dirs_first;
                self.reload();
            }
            cmd::HIDDEN => {
                self.prefs.show_hidden = !self.prefs.show_hidden;
                self.reload();
            }
            cmd::SIDEBAR => self.prefs.sidebar = !self.prefs.sidebar,
            cmd::STATUSBAR => self.prefs.status_bar = !self.prefs.status_bar,
            cmd::LABELS => self.prefs.toolbar_labels = !self.prefs.toolbar_labels,
            cmd::PATH_CRUMBS => {
                use memrium_de::files::PathStyle::*;
                self.prefs.path_style = if self.prefs.path_style == Breadcrumb { Address } else { Breadcrumb };
            }
            cmd::SEL_ROUNDED => self.prefs.selection_row = !self.prefs.selection_row,
            cmd::COL_SIZE => self.prefs.col_size = !self.prefs.col_size,
            cmd::COL_DATE => self.prefs.col_date = !self.prefs.col_date,
            cmd::COL_KIND => self.prefs.col_kind = !self.prefs.col_kind,
            cmd::ICON_SMALL => self.prefs.icon_size = 48.0,
            cmd::ICON_MEDIUM => self.prefs.icon_size = 64.0,
            cmd::ICON_LARGE => self.prefs.icon_size = 96.0,
            n if (cmd::PRESET_BASE..cmd::PRESET_BASE + PRESETS.len() as u32).contains(&n) => {
                let hidden = self.prefs.show_hidden;
                self.prefs = FilesPrefs::preset(PRESETS[(n - cmd::PRESET_BASE) as usize]);
                self.prefs.show_hidden = hidden;
                self.reload();
            }
            _ => prefs = false,
        }
        if prefs {
            self.save_prefs();
        }
        host.redraw = true;
    }

    fn entries_path(&self, k: usize) -> PathBuf {
        self.entries[k].path.clone()
    }

    fn entry_of(&self, p: &PathBuf) -> Option<memrium_de::fsops::Entry> {
        self.entries.iter().find(|e| &e.path == p).cloned()
    }

    fn take_convert(&mut self) -> Option<(PathBuf, String)> {
        self.pending_convert.take()
    }

    fn finish_convert(&mut self, convert: bool) {
        let Some((from, name)) = self.take_convert() else { return };
        if convert {
            self.fm.note = match fsops::convert_image_to(&from, &name) {
                Ok(to) => {
                    self.changed();
                    self.select_path(&to);
                    chebrium::i18n::trf("Converted to {}; the original is kept", &[name])
                }
                Err(e) => chebrium::i18n::trf("Could not convert: {}", &[e]),
            };
        } else if let Some(to) = self.fm.rename(&from, &name) {
            self.changed();
            self.select_path(&to);
        }
    }

    pub fn sort_by(&mut self, by: SortBy) {
        if self.prefs.sort == by {
            self.prefs.sort_desc = !self.prefs.sort_desc;
        } else {
            self.prefs.sort = by;
            self.prefs.sort_desc = false;
        }
        self.reload();
    }
}
