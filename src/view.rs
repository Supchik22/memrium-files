//! Drawing the window. Two looks share the list: the glass one (a floating glass sidebar beside a paper sheet with the
//! toolbar row) and the Dolphin one (`dolphin.rs`: toolbar with a location bar, a flat Places panel, a status bar with
//! zoom and free space, the Information panel). Here: the layout, the list of a folder in its three views with the rubber
//! band, the column header, the tabs, the glass toolbar/sidebar/status, the file dialog's bar, and the cards over the
//! window (progress of copies, a name that is taken, Open With, Properties, Quick Look, the context menu).

use crate::chooser::Mode;
use crate::{place_title, FilesApp};
use chebrium::app::Host;
use chebrium::chrome;
use chebrium::draw::Align;
use chebrium::popmenu::{self, Outcome};
use chebrium::ui::{inside, Cursor, Ui};
use chebrium::widgets::{self, fit, round_button, Round};
use memrium_de::files::{cmd, FilesStyle, PathStyle, ViewMode};
use memrium_de::fsops::{self, SortBy};
use std::path::PathBuf;

const SIDE_W: f32 = 210.0;
pub const TOP_H: f32 = 52.0;
pub const HEAD_H: f32 = 28.0;
const ACT_H: f32 = 40.0;
pub const STATUS_H: f32 = 28.0;
const BAR_H: f32 = 56.0;
pub const TABS_H: f32 = 34.0;

fn icon_button(ui: &mut Ui<'_>, cx: f32, cy: f32, icon: &str, enabled: bool) -> bool {
    round_button(ui, cx, cy, icon, Round::ink(30.0).glyph(17.0).enabled(enabled))
}

/// A thin frame around `r` (four hairlines).
pub fn outline(ui: &mut Ui<'_>, r: [f32; 4], rgb: [f32; 3], alpha: f32) {
    ui.fill([r[0] + 2.0, r[1], r[2] - 4.0, 1.0], 0.0, rgb, alpha);
    ui.fill([r[0] + 2.0, r[1] + r[3] - 1.0, r[2] - 4.0, 1.0], 0.0, rgb, alpha);
    ui.fill([r[0], r[1] + 2.0, 1.0, r[3] - 4.0], 0.0, rgb, alpha);
    ui.fill([r[0] + r[2] - 1.0, r[1] + 2.0, 1.0, r[3] - 4.0], 0.0, rgb, alpha);
}

/// Where the parts of the window go this frame.
#[derive(Clone, Copy, Default)]
pub struct Lay {
    pub w: f32,
    pub h: f32,
    /// the sidebar / Places panel (zero wide when hidden)
    pub side: [f32; 4],
    /// the toolbar row's height (the labelled action row of the glass look included)
    pub top: f32,
    pub tabs: [f32; 4],
    /// the Dolphin search bar under the tabs
    pub find: [f32; 4],
    /// the list's viewport (under the column header in the details view)
    pub list: [f32; 4],
    /// the column header (zero tall when not the details view)
    pub head: [f32; 4],
    pub status: [f32; 4],
    pub info: [f32; 4],
    /// the file dialog's bar at the bottom
    pub foot: [f32; 4],
}

impl FilesApp {
    pub fn dolphin(&self) -> bool {
        self.prefs.style == FilesStyle::Dolphin
    }

    fn layout(&self, size: [f32; 2]) -> Lay {
        let [w, h] = size;
        let dialog = self.chooser.is_some();
        let dol = self.dolphin();
        let details = self.prefs.view == ViewMode::Details;
        let foot_h = if dialog { BAR_H } else { 0.0 };
        let status_h = if self.prefs.status_bar || dialog { if dol { crate::dolphin::STATUS_H } else { STATUS_H } } else { 0.0 };
        let tabs_h = if self.tabs.len() > 1 { TABS_H } else { 0.0 };
        let mut l = Lay { w, h, ..Default::default() };
        if dol {
            let top = crate::dolphin::TOOL_H;
            l.top = top;
            l.tabs = [0.0, top, w, tabs_h];
            let below = top + tabs_h;
            let side_w = if self.prefs.sidebar { self.prefs.sidebar_w.clamp(160.0, 320.0) } else { 0.0 };
            l.side = [0.0, below, side_w, h - below - foot_h];
            let info_w = if self.prefs.info_panel && !dialog && w - side_w > 560.0 { crate::dolphin::INFO_W } else { 0.0 };
            let find_h = if self.filter_open || !self.filter.is_empty() { crate::dolphin::FIND_H } else { 0.0 };
            let (x0, x1) = (side_w, w - info_w);
            l.find = [x0, below, x1 - x0, find_h];
            let head_h = if details { HEAD_H } else { 0.0 };
            l.head = [x0, below + find_h, x1 - x0, head_h];
            let list_y = below + find_h + head_h;
            l.status = [x0, h - foot_h - status_h, x1 - x0, status_h];
            l.list = [x0, list_y, x1 - x0, (l.status[1] - list_y).max(60.0)];
            l.info = [x1, below, info_w, h - below - foot_h];
            l.foot = [side_w, h - foot_h, w - side_w, foot_h];
        } else {
            let side_w = if self.prefs.sidebar { SIDE_W.min(self.prefs.sidebar_w.max(170.0) + 10.0) } else { 0.0 };
            let act = if !dialog && self.prefs.toolbar_labels { ACT_H } else { 0.0 };
            l.top = TOP_H + act;
            l.side = [0.0, 0.0, side_w, h];
            l.tabs = [side_w, l.top, w - side_w, tabs_h];
            let head_h = if details { HEAD_H } else { 0.0 };
            l.head = [side_w, l.top + tabs_h, w - side_w, head_h];
            let list_y = l.top + tabs_h + head_h;
            l.status = [side_w, h - foot_h - status_h, w - side_w, status_h];
            l.list = [side_w, list_y, w - side_w, (l.status[1] - list_y).max(60.0)];
            l.foot = [side_w, h - foot_h, w - side_w, foot_h];
        }
        l
    }

    pub fn draw_window(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        chrome::resize_borders(ui, host, size);
        let [w, h] = size;
        let (ink, dark) = (ui.ink, ui.dark);
        let paper = if dark { [0.0; 3] } else { [1.0; 3] };
        let dialog = self.chooser.is_some();
        let dol = self.dolphin();
        // an open menu or card is over everything: the window under it does not see the pointer
        let held = (self.menu.is_some()).then(|| popmenu::shield(ui));
        let modal = self.props.is_some() || self.ask.is_some() || self.picker.is_some() || self.quick;
        let veil = modal.then(|| (ui.mouse, ui.click, ui.double, ui.scroll));
        if veil.is_some() {
            ui.mouse = [-1000.0; 2];
            ui.click = false;
            ui.double = false;
            ui.scroll = 0.0;
        }
        self.icons.begin_frame();
        let l = self.layout(size);
        self.list_rect = l.list;
        self.side_rect = l.side;
        if dol {
            crate::dolphin::backgrounds(ui, &l);
        } else {
            ui.fill([l.side[2], 0.0, w - l.side[2], h], 0.0, paper, if dark { 0.32 } else { 0.78 });
        }

        // what files being dragged over the window would land on
        let drop_hot = host.drag_over.and_then(|p| self.drop_target(p));

        // F2 or a new folder: start renaming the selected entry
        if std::mem::take(&mut self.want_rename) {
            if let [p] = self.sel.as_slice() {
                if let Some(e) = self.entries.iter().find(|e| &e.path == p) {
                    self.rename_buf = e.name.clone();
                    self.renaming = Some(e.path.clone());
                    ui.begin_edit("rename", &e.name);
                }
            }
        }

        self.draw_entries(ui, &l, drop_hot.as_ref(), host);
        if l.head[3] > 0.0 {
            self.draw_header(ui, &l);
        }

        // the title of the window follows the folder
        let title = if let Some(c) = &self.chooser { c.title.clone() } else { place_title(&self.cwd) };
        if title != self.title {
            host.title = Some(title.clone());
            self.title = title;
        }
        if dol {
            self.dolphin_toolbar(ui, &l, host);
            if l.find[3] > 0.0 {
                self.dolphin_find(ui, &l);
            }
        } else {
            self.glass_toolbar(ui, &l, host);
        }
        if l.tabs[3] > 0.0 {
            self.draw_tabs(ui, l.tabs, host);
        } else {
            self.tab_rects.clear();
        }

        // ---- status line
        if l.status[3] > 0.0 {
            if dol {
                self.dolphin_status(ui, &l, host);
            } else {
                self.glass_status(ui, &l, host);
            }
        }
        // ---- the file dialog's bar: [name] [filter] [Cancel] [Open]
        if dialog {
            self.dialog_bar(ui, l.foot, host);
        }
        // ---- sidebar
        if self.prefs.sidebar {
            if dol {
                self.dolphin_places(ui, l.side, drop_hot.as_ref(), host);
            } else {
                self.draw_sidebar(ui, h, l.side[2], drop_hot.as_ref(), host);
            }
        } else {
            self.place_rects.clear();
        }
        if l.info[2] > 0.0 {
            self.info_panel(ui, l.info, host);
        }
        // pictures and file-type icons asked for are made now
        self.icons.end_frame();
        host.redraw |= self.icons.waiting;

        // ---- copies and moves going on
        self.draw_jobs(ui, l.list, host);

        // ---- the cards over the window
        if let Some((mouse, click, double, scroll)) = veil {
            ui.mouse = mouse;
            ui.click = click;
            ui.double = double;
            ui.scroll = scroll;
        }
        let _ = ink;
        if self.quick {
            self.draw_quick(ui, size, host);
        } else if self.ask.is_some() {
            self.draw_ask(ui, size, host);
        } else if self.picker.is_some() {
            self.draw_picker(ui, size, host);
        } else if let Some(e) = self.props.clone() {
            ui.fill([0.0, 0.0, w, h], 0.0, [0.0; 3], 0.18);
            let mime = if e.is_dir { String::new() } else { memrium_de::mimeapps::describe(&memrium_de::mimeapps::mime_of(&e.path)) };
            let lines = vec![
                ("Name", e.name.clone()),
                ("Type", if mime.is_empty() { chebrium::i18n::tr(e.kind.label()).into_owned() } else { mime }),
                ("Size", if e.is_dir { chebrium::i18n::tr("Folder").into_owned() } else { format!("{} ({} B)", fsops::human_size(e.size), e.size) }),
                ("Modified", fsops::format_time(e.modified)),
                ("Location", e.path.parent().map(|p| p.display().to_string()).unwrap_or_default()),
            ];
            let cw = 440.0f32.min(w - 40.0);
            let x = (w - cw) / 2.0;
            let y = (h / 2.0 - 100.0).max(60.0);
            let ch = widgets::facts_panel(ui, x, y, cw, &lines);
            if ui.click && !inside(ui.mouse, [x, y, cw, ch]) {
                self.props = None;
            }
            host.redraw = true;
        }

        // ---- the context menu, over everything
        if let (Some(m), Some(held)) = (self.menu.as_mut(), held) {
            let out = m.draw(ui, held, size);
            host.redraw |= m.animating();
            match out {
                Outcome::Open => {}
                Outcome::Chosen(id) => {
                    self.menu = None;
                    self.command(id, host);
                }
                Outcome::Closed => {
                    self.menu = None;
                    self.pending_convert = None;
                    self.menu_place = None;
                }
            }
            host.redraw = true;
        }
    }

    /// Columns of the details view from the right edge of the list: Size (right aligned), Modified, Type (or where an
    /// entry is, for search results and Recent); the name gets what is left. (size_x, date_x, kind_x, edge)
    fn columns(&self, list: [f32; 4]) -> (Option<f32>, Option<f32>, Option<f32>, f32) {
        let mut edge = list[0] + list[2] - 16.0 - widgets::BAR_ZONE;
        let wide = list[2] > 520.0 && !self.computer();
        let size_x = (wide && self.prefs.col_size).then(|| {
            let x = edge;
            edge -= 84.0;
            x
        });
        let date_x = (wide && self.prefs.col_date).then(|| {
            edge -= 150.0;
            edge
        });
        let kind_x = (wide && (self.prefs.col_kind || self.mixed())).then(|| {
            edge -= if self.mixed() { 190.0 } else { 120.0 };
            edge
        });
        (size_x, date_x, kind_x, edge)
    }

    /// The cell of entry `i` in content coordinates (y from the top of the list's content).
    fn cell(&self, i: usize, list: [f32; 4], cols: usize, cw: f32, ch: f32) -> [f32; 4] {
        let details = self.prefs.view == ViewMode::Details;
        [list[0] + 8.0 + (i % cols) as f32 * cw, 4.0 + (i / cols) as f32 * ch, cw, if details { ch - 2.0 } else { ch }]
    }

    fn draw_entries(&mut self, ui: &mut Ui<'_>, l: &Lay, drop_hot: Option<&PathBuf>, host: &mut Host) {
        let list = l.list;
        let (accent, ink) = (ui.accent, ui.ink);
        let dol = self.dolphin();
        let n = self.shown_len();
        let mut want_open = None;
        let mut renamed = None;
        let mut press_at = None;
        let details = self.prefs.view == ViewMode::Details;
        let usable_w = list[2] - 16.0 - widgets::BAR_ZONE;
        let (cell_w, cell_h) = match self.prefs.view {
            ViewMode::Details => (usable_w, self.prefs.row_h),
            ViewMode::Icons => (self.prefs.icon_size + 44.0, self.prefs.icon_size + 50.0),
            ViewMode::Compact => (236.0, self.prefs.row_h - 2.0),
        };
        let cols = if details { 1 } else { ((usable_w / cell_w).floor() as usize).max(1) };
        self.cols = cols;
        let rows = n.div_ceil(cols);
        if std::mem::take(&mut self.reveal) {
            if let Some(c) = self.cursor {
                widgets::reveal(&mut self.scroll, (c / cols) as f32 * cell_h, (c / cols + 1) as f32 * cell_h + 8.0, list[3]);
            }
        }
        self.rects.clear();
        let (size_x, date_x, kind_x, edge) = self.columns(list);
        let mixed = self.mixed();
        let home = fsops::home();
        let area = widgets::scroll_area(ui, list, rows as f32 * cell_h + 8.0, &mut self.scroll);
        let offset = area.offset;
        let square = self.prefs.selection_row && details && !dol;
        let radius = if self.prefs.selection_row && !dol { 4.0 } else { 9.0 };
        let mut on_entry = false;
        for row in area.rows(cell_h, rows) {
            for i in row * cols..((row + 1) * cols).min(n) {
                let e = self.at(i).clone();
                let c = self.cell(i, list, cols, cell_w, cell_h);
                let r = [c[0], area.y(c[1]), c[2], c[3]];
                self.rects.push((i, r));
                let hov = area.hit(ui, r);
                on_entry |= hov;
                let selected = self.sel.contains(&e.path);
                let cut = self.fm.cut && self.fm.clip.contains(&e.path);
                let a = if cut || e.hidden { 0.55 } else { 1.0 };
                let sel_r = [r[0] + 1.0, r[1], r[2] - 2.0, r[3]];
                if drop_hot == Some(&e.path) && e.is_dir {
                    ui.fill(sel_r, radius, accent, 0.4);
                } else if dol {
                    crate::dolphin::row(ui, sel_r, 30_000 + i as u64, hov, selected, self.cursor == Some(i));
                } else {
                    widgets::list_row(ui, sel_r, radius, 30_000 + i as u64, hov, selected);
                }
                if square && selected {
                    ui.fill([r[0] + 1.0, r[1] + 6.0, 3.0, r[3] - 12.0], 1.5, accent, 1.0);
                }
                let editing = self.renaming.as_ref() == Some(&e.path);
                match self.prefs.view {
                    ViewMode::Icons => {
                        let side = self.prefs.icon_size * 0.92;
                        let (cx, cy) = (r[0] + r[2] / 2.0, r[1] + 8.0 + self.prefs.icon_size / 2.0);
                        self.paint_icon(ui, &e, cx, cy, side, true, a);
                        let ty = r[1] + self.prefs.icon_size + 30.0;
                        if editing {
                            let mut buf = self.rename_buf.clone();
                            if ui.text_field([r[0] + 2.0, ty - 14.0, r[2] - 4.0, 24.0], "rename", &mut buf) {
                                renamed = Some((e.path.clone(), buf.clone()));
                            }
                            self.rename_buf = buf;
                        } else {
                            let name = fit(ui, e.title(), 13.0, false, r[2] - 10.0);
                            ui.text_raw(r[0] + r[2] / 2.0, ty, &name, 13.0, selected, ink, 0.95 * a, Align::Center);
                        }
                    }
                    ViewMode::Compact | ViewMode::Details => {
                        let cy = r[1] + r[3] / 2.0;
                        let icon_px = if details { 22.0 } else { 20.0 };
                        self.paint_icon(ui, &e, r[0] + 22.0, cy, icon_px, false, a);
                        let name_max = if details { edge - r[0] - 56.0 } else { r[2] - 56.0 };
                        if editing {
                            let mut buf = self.rename_buf.clone();
                            if ui.text_field([r[0] + 40.0, r[1] + 1.0, name_max.min(360.0), r[3] - 2.0], "rename", &mut buf) {
                                renamed = Some((e.path.clone(), buf.clone()));
                            }
                            self.rename_buf = buf;
                        } else {
                            let name = fit(ui, e.title(), 13.5, false, name_max);
                            ui.text_raw(r[0] + 44.0, cy + 4.8, &name, 13.5, selected, ink, 0.95 * a, Align::Left);
                        }
                        if details {
                            if let Some(x) = kind_x {
                                if mixed {
                                    // where it is: under home as ~/…
                                    let dir = e.path.parent().map(|p| match p.strip_prefix(&home) {
                                        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
                                        Ok(rest) => format!("~/{}", rest.display()),
                                        Err(_) => p.display().to_string(),
                                    });
                                    let dir = widgets::fit_start(ui, &dir.unwrap_or_default(), 12.0, false, 180.0);
                                    ui.text_raw(x, cy + 4.5, &dir, 12.0, false, ink, 0.55 * a, Align::Left);
                                } else {
                                    ui.text(x, cy + 4.5, e.kind.label(), 12.0, false, ink, 0.55 * a, Align::Left);
                                }
                            }
                            if let Some(x) = date_x {
                                ui.text_raw(x, cy + 4.5, &fsops::format_time(e.modified), 12.0, false, ink, 0.55 * a, Align::Left);
                            }
                            if let (Some(x), false) = (size_x, e.is_dir) {
                                ui.text_raw(x, cy + 4.5, &fsops::human_size(e.size), 12.0, false, ink, 0.55 * a, Align::Right);
                            }
                        }
                    }
                }
                if ui.click && hov && !editing {
                    if ui.mods.shift && self.multi() {
                        self.select_to(i);
                    } else if ui.mods.ctrl && self.multi() {
                        self.toggle(i);
                    } else {
                        // pressing one of several selected entries keeps them (they can be dragged together)
                        if !(selected && self.sel.len() > 1) || ui.double {
                            self.select_only(i);
                        }
                        if ui.double {
                            want_open = Some(i);
                        } else {
                            press_at = Some(ui.mouse);
                        }
                    }
                    self.cursor = Some(i);
                }
            }
        }
        // the rubber band, drawn in the list's content so it scrolls with it
        if let Some(b) = self.band {
            let my = ui.mouse[1].clamp(list[1] - 40.0, list[1] + list[3] + 40.0);
            let cur = [ui.mouse[0].clamp(list[0], list[0] + list[2]), my - list[1] + offset];
            let r = [b[0].min(cur[0]), b[1].min(cur[1]), (b[0] - cur[0]).abs(), (b[1] - cur[1]).abs()];
            let sr = [r[0], list[1] + r[1] - offset, r[2], r[3]];
            ui.fill(sr, 3.0, accent, 0.14);
            outline(ui, sr, accent, 0.65);
        }
        area.end(ui);
        let in_list = inside(ui.mouse, list) && ui.mouse[0] < list[0] + list[2] - widgets::BAR_ZONE;
        // a press on the free part of the list starts a rubber band (Ctrl or Shift adds to what is selected)
        if ui.click && in_list && !on_entry && self.renaming.is_none() && !ui.double && self.multi() {
            self.band_base = if ui.mods.ctrl || ui.mods.shift { self.sel.clone() } else { Vec::new() };
            self.sel = self.band_base.clone();
            self.band = Some([ui.mouse[0], ui.mouse[1] - list[1] + offset]);
            self.cursor = None;
        } else if ui.click && in_list && !on_entry && self.renaming.is_none() && !ui.mods.ctrl {
            self.sel.clear();
        }
        if let Some(b) = self.band {
            if !ui.held {
                self.band = None;
            } else {
                let my = ui.mouse[1];
                let cur = [ui.mouse[0].clamp(list[0], list[0] + list[2]), my.clamp(list[1] - 40.0, list[1] + list[3] + 40.0) - list[1] + offset];
                let r = [b[0].min(cur[0]), b[1].min(cur[1]), (b[0] - cur[0]).abs(), (b[1] - cur[1]).abs()];
                let meets = |c: [f32; 4]| c[0] < r[0] + r[2] && c[0] + c[2] > r[0] && c[1] < r[1] + r[3] && c[1] + c[3] > r[1];
                let mut sel = self.band_base.clone();
                for i in 0..n {
                    let p = &self.entries[self.shown[i]].path;
                    if meets(self.cell(i, list, cols, cell_w, cell_h)) && !sel.contains(p) {
                        sel.push(p.clone());
                    }
                }
                self.sel = sel;
                // past the top or the bottom: the list scrolls along
                let edge = 24.0;
                if my < list[1] + edge {
                    self.scroll -= (list[1] + edge - my).min(60.0) * 0.5;
                } else if my > list[1] + list[3] - edge {
                    self.scroll += (my - (list[1] + list[3] - edge)).min(60.0) * 0.5;
                }
                host.redraw = true;
            }
        }
        if let Some(at) = press_at {
            self.pressed(at);
        }
        if let Some(i) = want_open {
            self.open(i);
        }
        if let Some((path, name)) = renamed {
            self.commit_rename(path, name);
        } else if self.renaming.is_some() && !ui.editing() {
            // Esc, or the field lost the pointer without a change
            self.renaming = None;
        }
        if n == 0 {
            let searching = self.search.as_ref().is_some_and(|s| !s.done());
            let msg = if !self.error.is_empty() {
                self.error.clone()
            } else if searching {
                chebrium::i18n::tr("Searching…").into_owned()
            } else if !self.filter.is_empty() {
                chebrium::i18n::trf("Nothing matches “{}”", &[self.filter.clone()])
            } else if self.in_trash() {
                chebrium::i18n::tr("The Trash is empty").into_owned()
            } else if self.recent() {
                chebrium::i18n::tr("No recent files").into_owned()
            } else {
                chebrium::i18n::tr("This folder is empty").into_owned()
            };
            let glyph = if !self.error.is_empty() { "info" } else if searching || !self.filter.is_empty() { "search" } else { "folder-open" };
            ui.glyph(list[0] + list[2] / 2.0, list[1] + 70.0, 40.0, glyph, ink, 0.25);
            ui.text_raw(list[0] + list[2] / 2.0, list[1] + 118.0, &msg, 14.0, false, ink, 0.55, Align::Center);
        }
    }

    fn draw_header(&mut self, ui: &mut Ui<'_>, l: &Lay) {
        let ink = ui.ink;
        let head = l.head;
        let dol = self.dolphin();
        let (size_x, date_x, kind_x, _) = self.columns(l.list);
        ui.fill([head[0] + 8.0, head[1] + head[3] - 1.0, head[2] - 16.0, 1.0], 0.0, ink, 0.10);
        let mut sort_to = None;
        let (sort, desc) = (self.prefs.sort, self.prefs.sort_desc);
        let top_h = head[1];
        let head_cell = |ui: &mut Ui<'_>, label: &str, x: f32, align: Align, by: Option<SortBy>, width: f32| -> bool {
            let rx = if align == Align::Right { x - width } else { x };
            let hov = by.is_some() && ui.hit([rx - 4.0, top_h, width + 8.0, HEAD_H]);
            let on = by.is_some() && sort == by.unwrap();
            ui.text(x, top_h + 18.5, label, 11.5, !dol || on, ink, if on { 0.9 } else if hov { 0.8 } else { 0.55 }, align);
            if on {
                let tw = ui.measure(label, 11.5, true);
                let ax = if align == Align::Right { x - tw - 10.0 } else { x + tw + 10.0 };
                ui.glyph(ax, top_h + 14.0, 12.0, if desc { "chevron-down" } else { "chevron-up" }, ink, 0.8);
            }
            // Dolphin's header has a hairline between columns
            if dol && align == Align::Left && x > head[0] + 60.0 {
                ui.fill([x - 10.0, top_h + 7.0, 1.0, HEAD_H - 14.0], 0.0, ink, 0.12);
            }
            ui.click && hov
        };
        if head_cell(ui, "Name", l.list[0] + 52.0, Align::Left, Some(SortBy::Name), 80.0) {
            sort_to = Some(SortBy::Name);
        }
        if let Some(x) = kind_x {
            if self.mixed() {
                head_cell(ui, "Location", x, Align::Left, None, 60.0);
            } else if head_cell(ui, "Type", x, Align::Left, Some(SortBy::Kind), 60.0) {
                sort_to = Some(SortBy::Kind);
            }
        }
        if let Some(x) = date_x {
            if head_cell(ui, "Modified", x, Align::Left, Some(SortBy::Modified), 100.0) {
                sort_to = Some(SortBy::Modified);
            }
        }
        if let Some(x) = size_x {
            if head_cell(ui, "Size", x, Align::Right, Some(SortBy::Size), 60.0) {
                sort_to = Some(SortBy::Size);
            }
        }
        if let Some(by) = sort_to {
            self.sort_by(by);
            self.save_prefs();
        }
    }

    /// The tabs (shown when there are two or more): a press shows one, its cross closes it, + makes a new one.
    fn draw_tabs(&mut self, ui: &mut Ui<'_>, r: [f32; 4], host: &mut Host) {
        let (ink, accent, dark) = (ui.ink, ui.accent, ui.dark);
        // (the Dolphin layout draws its tabs the toolkit's way too)
        let dol = false;
        let n = self.tabs.len();
        let plus_w = 34.0;
        let tw = ((r[2] - 16.0 - plus_w) / n as f32).min(240.0);
        self.tab_rects.clear();
        if dol {
            ui.fill([r[0], r[1] + r[3] - 1.0, r[2], 1.0], 0.0, ink, 0.12);
        }
        let mut go = None;
        let mut close = None;
        for i in 0..n {
            let t = [r[0] + 8.0 + i as f32 * tw, r[1] + 3.0, tw - 4.0, r[3] - 6.0];
            self.tab_rects.push((i, t));
            let on = i == self.tab;
            let hov = ui.hit(t);
            let wash = ui.ease(43_000 + i as u64, if hov && !on { 1.0 } else { 0.0 }, 22.0);
            if on {
                if dol {
                    let base = if dark { [0.106, 0.118, 0.125] } else { [1.0; 3] };
                    ui.fill([t[0], t[1], t[2], t[3] + 3.0], 5.0, base, 1.0);
                    ui.fill([t[0] + 4.0, t[1], t[2] - 8.0, 2.0], 1.0, accent, 1.0);
                } else {
                    ui.fill(t, 9.0, ink, 0.10);
                }
            } else {
                ui.fill(t, if dol { 5.0 } else { 9.0 }, ink, 0.06 * wash);
            }
            let title = self.tab_title(i);
            let close_r = [t[0] + t[2] - 24.0, t[1] + (t[3] - 18.0) / 2.0, 18.0, 18.0];
            let name = fit(ui, &title, 12.5, false, t[2] - 58.0);
            ui.glyph(t[0] + 16.0, t[1] + t[3] / 2.0, 14.0, "folder", accent, if on { 1.0 } else { 0.7 });
            ui.text_raw(t[0] + 30.0, t[1] + t[3] / 2.0 + 4.3, &name, 12.5, on, ink, if on { 0.95 } else { 0.65 }, Align::Left);
            if hov || on {
                let ch = ui.hit(close_r);
                if ch {
                    ui.fill(close_r, 9.0, ink, 0.12);
                }
                ui.glyph(close_r[0] + 9.0, close_r[1] + 9.0, 12.0, "x", ink, if ch { 0.9 } else { 0.5 });
                if ch && ui.click {
                    close = Some(i);
                    continue;
                }
            }
            if hov && ui.click {
                go = Some(i);
            }
        }
        let px = r[0] + 8.0 + n as f32 * tw + plus_w / 2.0;
        if round_button(ui, px, r[1] + r[3] / 2.0, "plus", Round::ink(26.0).glyph(14.0)) {
            self.new_tab(self.cwd.clone());
        }
        if let Some(i) = close {
            self.close_tab(i, host);
        } else if let Some(i) = go {
            self.switch_tab(i);
        }
    }

    // ---------------------------------------------------------------- the glass look

    fn glass_toolbar(&mut self, ui: &mut Ui<'_>, l: &Lay, host: &mut Host) {
        let (accent, ink) = (ui.accent, ui.ink);
        let w = l.w;
        let side_w = l.side[2];
        let room = chrome::button_room(host);
        let dialog = self.chooser.is_some();
        let computer = self.pseudo();
        let cy = TOP_H / 2.0;
        let left = if side_w > 0.0 { side_w } else { room[0] };
        let pill_x = left + 14.0;
        ui.fill([pill_x, cy - 16.0, 68.0, 32.0], 16.0, ink, 0.065);
        if icon_button(ui, pill_x + 18.0, cy, "chevron-left", self.can_back()) {
            self.go_back();
        }
        if icon_button(ui, pill_x + 50.0, cy, "chevron-right", self.can_forward()) {
            self.go_forward();
        }
        let sel_n = self.sel.len();
        let buttons: Vec<(&str, bool, u32)> = vec![
            ("arrow-up", self.can_up(), cmd::G_UP),
            ("refresh-cw", true, cmd::REFRESH),
            ("folder-plus", !computer, cmd::NEW_FOLDER),
            ("trash-2", sel_n > 0 && !dialog, if self.in_trash() { cmd::DELETE_FOREVER } else { cmd::TRASH }),
            ("search", true, crate::actions::FIND),
            (if self.prefs.show_hidden { "eye" } else { "eye-off" }, true, cmd::HIDDEN),
            (match self.prefs.view { ViewMode::Icons => "layout-grid", ViewMode::Details => "list", ViewMode::Compact => "columns-2" }, true, 0),
        ];
        let actions_w = buttons.len() as f32 * 32.0 + 8.0;
        let actions_x = (w - actions_w - 14.0 - room[1]).max(pill_x + 120.0);
        ui.fill([actions_x, cy - 16.0, actions_w, 32.0], 16.0, ink, 0.065);
        let mut chosen = None;
        for (i, (icon, on, id)) in buttons.iter().enumerate() {
            if icon_button(ui, actions_x + 20.0 + i as f32 * 32.0, cy, icon, *on) {
                chosen = Some(*id);
            }
        }
        // the search field sits left of the actions
        let mut title_right = actions_x - 12.0;
        if self.filter_open {
            let fw = 190.0f32.min((title_right - pill_x - 68.0 - 90.0).max(110.0));
            let fr = [actions_x - 12.0 - fw, cy - 14.0, fw, 28.0];
            title_right = fr[0] - 10.0;
            self.query_field(ui, fr);
        }
        // the name and the path (each part of the path takes you there); a click on the name types a location
        let title_x = pill_x + 68.0 + 16.0;
        let title_w = (title_right - title_x).max(60.0);
        if let Some(text) = self.want_location.take() {
            ui.begin_edit("location", &text);
            self.locating = true;
        }
        if self.locating {
            let mut buf = self.location_text();
            if let Some(t) = ui.edit_text("location") {
                buf = t.to_string();
            }
            if ui.text_field([title_x - 6.0, cy - 15.0, title_w + 6.0, 30.0], "location", &mut buf) {
                self.locating = false;
                self.go_typed(&buf);
            } else if !ui.editing() {
                self.locating = false;
            }
        } else {
            let name = fit(ui, &self.title, 15.0, true, title_w);
            ui.text_raw(title_x, cy - 1.5, &name, 15.0, true, ink, 0.95, Align::Left);
            let click = self.draw_path(ui, title_x, cy + 13.0, title_w);
            let name_r = [title_x - 4.0, 4.0, title_w, 26.0];
            if ui.hit(name_r) {
                ui.want_cursor(Cursor::Text);
                if ui.click {
                    self.want_location = Some(self.location_text());
                    host.redraw = true;
                }
            }
            if let Some(p) = click {
                self.go(p);
            }
        }
        match chosen {
            Some(0) => {
                self.prefs.view = match self.prefs.view {
                    ViewMode::Icons => ViewMode::Details,
                    ViewMode::Details => ViewMode::Compact,
                    ViewMode::Compact => ViewMode::Icons,
                };
                self.save_prefs();
            }
            Some(id) => self.command(id, host),
            None => {}
        }
        self.editing = ui.editing();
        // a press on the bare toolbar moves the window, twice maximises it
        let on_widget = ui.hit([pill_x, 0.0, 68.0, TOP_H]) || ui.hit([actions_x, 0.0, actions_w, TOP_H]) || ui.hit([title_x - 4.0, 0.0, title_w, TOP_H]);
        chrome::integrated(ui, host, [l.w, l.h], TOP_H, on_widget);

        // ---- the labelled action row: New, Cut, Copy, Paste, Rename, Delete (the Explorer look)
        let act = l.top - TOP_H;
        if act > 0.0 {
            ui.fill([side_w + 8.0, TOP_H + act - 1.0, w - side_w - 16.0, 1.0], 0.0, ink, 0.07);
            let (one, some) = (sel_n == 1, sel_n > 0);
            let acts: [(&str, &str, bool, u32); 6] = [
                ("folder-plus", "New", !computer, cmd::NEW_FOLDER),
                ("scissors", "Cut", some, cmd::CUT),
                ("copy", "Copy", some, cmd::COPY),
                ("clipboard-paste", "Paste", !computer && self.fm.can_paste(), cmd::PASTE),
                ("pencil", "Rename", one, cmd::RENAME),
                ("trash-2", "Delete", some, if self.in_trash() { cmd::DELETE_FOREVER } else { cmd::TRASH }),
            ];
            let mut x = side_w + 14.0;
            for (i, (icon, label, on, id)) in acts.into_iter().enumerate() {
                let tw = ui.measure(label, 12.5, false);
                let r = [x, TOP_H + 4.0, 38.0 + tw, act - 9.0];
                let hov = on && ui.hit(r);
                let wash = ui.ease(42_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
                ui.fill(r, 9.0, ink, 0.09 * wash);
                ui.glyph(r[0] + 18.0, r[1] + r[3] / 2.0, 16.0, icon, if on { accent } else { ink }, if on { 1.0 } else { 0.28 });
                ui.text(r[0] + 32.0, r[1] + r[3] / 2.0 + 4.5, label, 12.5, false, ink, if on { 0.92 } else { 0.32 }, Align::Left);
                if ui.click && hov {
                    self.command(id, host);
                }
                x += r[2] + 4.0;
            }
        }
    }

    /// What the location field starts with: the folder's path (a place that is not a folder gives an empty field).
    pub fn location_text(&self) -> String {
        if self.pseudo() {
            String::new()
        } else {
            self.cwd.to_string_lossy().to_string()
        }
    }

    /// The search field (in the toolbar of the glass look, in the search bar of Dolphin's): what is typed searches from
    /// the folder down.
    pub fn query_field(&mut self, ui: &mut Ui<'_>, fr: [f32; 4]) {
        let ink = ui.ink;
        if std::mem::take(&mut self.want_filter) {
            ui.begin_edit("filter", &self.filter);
        }
        let mut buf = self.filter.clone();
        ui.text_field(fr, "filter", &mut buf);
        let live = ui.edit_text("filter").map(str::to_string).unwrap_or(buf);
        if live != self.filter {
            self.set_query(live);
        }
        if self.filter.is_empty() && !ui.editing() {
            self.filter_open = false;
        }
        if self.filter.is_empty() && ui.edit_text("filter").is_some_and(str::is_empty) {
            let hint = if self.pseudo() { "Filter" } else { "Search from here" };
            ui.text(fr[0] + 10.0, fr[1] + fr[3] / 2.0 + 4.5, hint, 12.5, false, ink, 0.4, Align::Left);
        }
    }

    fn glass_status(&mut self, ui: &mut Ui<'_>, l: &Lay, host: &mut Host) {
        let ink = ui.ink;
        let foot_y = l.status[1] + l.status[3];
        let mut status = self.status();
        if let Some(c) = self.chooser.as_ref().filter(|c| c.replace) {
            status = chebrium::i18n::trf("“{}” already exists. Replace it?", &[c.name.trim().to_string()]);
        }
        let status = fit(ui, &status, 12.0, false, l.status[2] - 130.0);
        ui.text_raw(l.status[0] + 18.0, foot_y - 9.0, &status, 12.0, false, ink, 0.6, Align::Left);
        if self.prefs.view == ViewMode::Icons {
            for (i, (icon, id)) in [("minus", crate::actions::ZOOM_OUT), ("plus", crate::actions::ZOOM_IN)].into_iter().enumerate() {
                if icon_button(ui, l.w - 56.0 + i as f32 * 30.0, foot_y - 14.0, icon, true) {
                    self.command(id, host);
                }
            }
        }
    }

    /// The icon of an entry: the theme's picture (or the thumbnail), else the Lucide glyph of its kind.
    pub fn paint_icon(&mut self, ui: &mut Ui<'_>, e: &fsops::Entry, cx: f32, cy: f32, side: f32, thumb: bool, a: f32) {
        match self.icons.get(e, thumb) {
            Some(i) => {
                let (pw, ph) = (i.pic.w as f32, i.pic.h as f32);
                ui.picture([cx - side / 2.0, cy - side / 2.0, side, side], &i.pic, [0.0, 0.0, pw, ph], a);
            }
            None => ui.glyph(cx, cy, (side * 0.8).max(16.0), e.kind.icon(), e.kind.tint(ui.accent), a),
        }
    }

    /// The parts of the folder's path from home (or the root), the last one the folder itself.
    pub fn crumbs(&self) -> Vec<(String, PathBuf)> {
        let home = fsops::home();
        let (root, rest) = match self.cwd.strip_prefix(&home) {
            Ok(r) => (home.clone(), r.to_path_buf()),
            Err(_) => (PathBuf::from("/"), self.cwd.strip_prefix("/").unwrap_or(&self.cwd).to_path_buf()),
        };
        let mut parts: Vec<(String, PathBuf)> = vec![(if root == home { chebrium::i18n::tr("Home").into_owned() } else { "/".into() }, root.clone())];
        let mut acc = root;
        for c in rest.components() {
            acc.push(c);
            parts.push((c.as_os_str().to_string_lossy().to_string(), acc.clone()));
        }
        parts
    }

    /// The path under the folder's name: each part takes you there (None unless one was pressed).
    fn draw_path(&mut self, ui: &mut Ui<'_>, x: f32, base: f32, max_w: f32) -> Option<PathBuf> {
        let ink = ui.ink;
        if self.pseudo() {
            return None;
        }
        if self.prefs.path_style == PathStyle::Address {
            let t = fit(ui, &self.cwd.to_string_lossy(), 11.0, false, max_w);
            ui.text_raw(x, base, &t, 11.0, false, ink, 0.5, Align::Left);
            return None;
        }
        // Home › Documents › …, dropping the leftmost parts when too long
        let parts = self.crumbs();
        let widths: Vec<f32> = parts.iter().map(|(n, _)| ui.measure_raw(n, 11.0, false) + 14.0).collect();
        let mut start = 0;
        while start + 1 < parts.len() && widths[start..].iter().sum::<f32>() + 10.0 * (parts.len() - start) as f32 > max_w {
            start += 1;
        }
        let mut px = x;
        let mut go = None;
        if start > 0 {
            ui.text_raw(px, base, "…", 11.0, false, ink, 0.5, Align::Left);
            px += 16.0;
        }
        for (i, (name, path)) in parts.iter().enumerate().skip(start) {
            let r = [px - 3.0, base - 12.0, widths[i], 16.0];
            let hov = ui.hit(r);
            if hov {
                ui.fill(r, 5.0, ink, 0.1);
            }
            let last = i + 1 == parts.len();
            ui.text_raw(px + 4.0, base, name, 11.0, false, ink, if hov || last { 0.8 } else { 0.5 }, Align::Left);
            if ui.click && hov && !last {
                go = Some(path.clone());
            }
            px += widths[i];
            if !last {
                ui.glyph(px - 3.0, base - 4.0, 10.0, "chevron-right", ink, 0.35);
                px += 6.0;
            }
        }
        go
    }

    pub fn status(&self) -> String {
        if !self.error.is_empty() {
            return self.error.clone();
        }
        if !self.fm.note.is_empty() {
            return self.fm.note.clone();
        }
        let sel = self.selected_entries();
        if sel.is_empty() {
            if let Some(s) = &self.search {
                return if s.done() {
                    chebrium::i18n::trf("{} found", &[self.shown_len().to_string()])
                } else {
                    chebrium::i18n::trf("{} found so far, {} folders looked into…", &[self.shown_len().to_string(), s.scanned().to_string()])
                };
            }
            let dirs = self.shown.iter().filter(|&&i| self.entries[i].is_dir).count();
            let files = self.shown_len() - dirs;
            let size: u64 = self.shown.iter().map(|&i| &self.entries[i]).filter(|e| !e.is_dir).map(|e| e.size).sum();
            return if size > 0 && self.dolphin() {
                chebrium::i18n::trf("{} folders, {} files ({})", &[dirs.to_string(), files.to_string(), fsops::human_size(size)])
            } else {
                chebrium::i18n::trf("{} folders, {} files", &[dirs.to_string(), files.to_string()])
            };
        }
        let size: u64 = sel.iter().filter(|e| !e.is_dir).map(|e| e.size).sum();
        if size > 0 {
            chebrium::i18n::trf("{} selected, {}", &[sel.len().to_string(), fsops::human_size(size)])
        } else {
            chebrium::i18n::trf("{} selected", &[sel.len().to_string()])
        }
    }

    fn dialog_bar(&mut self, ui: &mut Ui<'_>, bar: [f32; 4], host: &mut Host) {
        let (ink, foot) = (ui.ink, bar[3]);
        let can = self.can_accept();
        ui.fill([bar[0], bar[1], bar[2], 1.0], 0.0, ink, 0.10);
        let by = bar[1] + (foot - 32.0) / 2.0;
        let mut x = bar[0] + bar[2] - 16.0;
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

    /// The groups of the sidebar: (header, rows of (name, Lucide glyph, path)). The home folders and the bookmarks are
    /// the Places, then Recent, then the system's.
    pub fn side_groups(&self) -> Vec<(&'static str, Vec<(String, &'static str, PathBuf)>)> {
        let trash = fsops::trash_path();
        let mut places: Vec<(String, &'static str, PathBuf)> = self.places.iter().map(|p| (chebrium::i18n::tr(p.name).into_owned(), p.icon, p.path.clone())).collect();
        places.extend(self.bookmarks.iter().filter(|b| !self.places.iter().any(|p| p.path == b.path)).map(|b| (b.label.clone(), "bookmark", b.path.clone())));
        let sys = |want: &dyn Fn(&PathBuf) -> bool| -> Vec<(String, &'static str, PathBuf)> { self.system.iter().filter(|p| want(&p.path)).map(|p| (chebrium::i18n::tr(p.name).into_owned(), p.icon, p.path.clone())).collect() };
        places.extend(sys(&|p| *p == trash));
        vec![("Places", places), ("Recent", sys(&|p| crate::places::is_recent(p))), ("System", sys(&|p| fsops::is_computer(p) || p.as_os_str() == "/"))]
    }

    fn draw_sidebar(&mut self, ui: &mut Ui<'_>, h: f32, side_w: f32, drop_hot: Option<&PathBuf>, host: &mut Host) {
        let (ink, accent, dark) = (ui.ink, ui.accent, ui.dark);
        let paper = if dark { [0.0; 3] } else { [1.0; 3] };
        let panel = [8.0, 8.0, side_w - 16.0, h - 16.0];
        ui.fill(panel, 18.0, paper, if dark { 0.06 } else { 0.42 });
        ui.fill([panel[0], panel[1], panel[2], 1.0], 0.0, [1.0; 3], if dark { 0.05 } else { 0.5 });
        // the rows: (name, icon, path, rectangle), the groups with their small headers
        let mut rows: Vec<(String, &'static str, PathBuf, [f32; 4])> = Vec::new();
        let mut headers: Vec<(String, f32)> = Vec::new();
        let mut y = 56.0;
        let sidebar_headers = self.prefs.sidebar_headers;
        for (title, group) in self.side_groups() {
            if group.is_empty() {
                continue;
            }
            if sidebar_headers {
                headers.push((chebrium::i18n::tr(title).to_uppercase(), y + 8.0));
                y += 24.0;
            }
            for (name, icon, path) in group {
                rows.push((name, icon, path, [16.0, y, side_w - 32.0, 32.0]));
                y += 34.0;
            }
            y += 6.0;
        }
        self.place_rects = rows.iter().map(|(_, _, p, r)| (p.clone(), *r)).collect();
        // one highlight glides to the place the window is in (the way the shell's menus do)
        match rows.iter().find(|(_, _, p, _)| *p == self.cwd) {
            Some((_, _, _, r)) => {
                for (s, v) in self.side_hl.iter_mut().zip(*r) {
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
        for (t, ty) in headers {
            ui.text_raw(26.0, ty + 4.0, &t, 10.5, true, ink, 0.4, Align::Left);
        }
        let mut go = None;
        for (i, (name, icon, path, r)) in rows.iter().enumerate() {
            let here = self.cwd == *path;
            let hov = ui.hit(*r);
            let wash = ui.ease(40_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
            if drop_hot == Some(path) {
                ui.fill(*r, 10.0, accent, 0.4);
            } else {
                ui.fill(*r, 10.0, ink, 0.07 * wash * if here { 0.0 } else { 1.0 });
            }
            ui.glyph(r[0] + 18.0, r[1] + r[3] / 2.0, 16.0, icon, accent, 1.0);
            let label = fit(ui, name, 13.5, here, r[2] - 50.0);
            ui.text_raw(r[0] + 40.0, r[1] + r[3] / 2.0 + 4.8, &label, 13.5, here, ink, 0.95, Align::Left);
            if ui.click && hov {
                go = Some(path.clone());
            }
        }
        // the mounted disks at the bottom, each with a bar of how full it is
        const DEV_H: f32 = 46.0;
        let floor = y + 8.0;
        let fits = (((h - 16.0 - 8.0 - 20.0 - floor) / DEV_H).floor().max(0.0) as usize).min(self.devices.len());
        let mut dev_go = None;
        if fits > 0 {
            let top = h - 16.0 - 8.0 - fits as f32 * DEV_H;
            ui.text(24.0, top - 6.0, "DEVICES", 10.5, false, ink, 0.45, Align::Left);
            for (i, d) in self.devices.iter().take(fits).enumerate() {
                let r = [16.0, top + i as f32 * DEV_H, side_w - 32.0, DEV_H - 4.0];
                let hov = ui.hit(r);
                let wash = ui.ease(41_000 + i as u64, if hov { 1.0 } else { 0.0 }, 22.0);
                let here = self.cwd == d.path;
                ui.fill(r, 10.0, ink, if here { 0.09 } else { 0.07 * wash });
                ui.glyph(r[0] + 18.0, r[1] + 14.0, 16.0, "hard-drive", accent, 1.0);
                let label = fit(ui, &d.label, 13.0, here, r[2] - 50.0);
                ui.text_raw(r[0] + 40.0, r[1] + 18.8, &label, 13.0, here, ink, 0.95, Align::Left);
                let used = if d.total > 0 { (1.0 - d.free as f32 / d.total as f32).clamp(0.0, 1.0) } else { 0.0 };
                let bar = [r[0] + 12.0, r[1] + r[3] - 11.0, r[2] - 24.0, 5.0];
                ui.fill(bar, 2.5, ink, 0.12);
                if used > 0.0 {
                    let col = if used > 0.9 { [0.92, 0.3, 0.27] } else { accent };
                    ui.fill([bar[0], bar[1], (bar[2] * used).max(5.0), bar[3]], 2.5, col, 1.0);
                }
                if ui.click && hov {
                    dev_go = Some(d.path.clone());
                }
            }
        }
        if let Some(p) = go.or(dev_go) {
            self.go(p);
        }
    }

    // ---------------------------------------------------------------- the cards

    /// A plain card over the window: its rectangle (a shadow and the paper drawn).
    fn card(&mut self, ui: &mut Ui<'_>, size: [f32; 2], cw: f32, ch: f32, key: u64) -> [f32; 4] {
        let [w, h] = size;
        let t = ui.ease(key, 1.0, 18.0);
        ui.fill([0.0, 0.0, w, h], 0.0, [0.0; 3], 0.22 * t);
        let s = 0.95 + 0.05 * t;
        let (cw, ch) = (cw.min(w - 32.0), ch.min(h - 32.0));
        let r = [(w - cw * s) / 2.0, (h - ch * s) / 2.0, cw * s, ch * s];
        let paper = if ui.dark { [0.13, 0.135, 0.15] } else { [0.99, 0.99, 0.995] };
        ui.fill([r[0] - 1.0, r[1] - 1.0, r[2] + 2.0, r[3] + 2.0], 15.0, [0.0; 3], 0.16);
        ui.fill(r, 14.0, paper, 0.98);
        r
    }

    /// The progress of copies and moves, bottom right of the list (a job that is over in a moment shows nothing).
    fn draw_jobs(&mut self, ui: &mut Ui<'_>, list: [f32; 4], _host: &mut Host) {
        let (ink, accent, dark) = (ui.ink, ui.accent, ui.dark);
        let mut cancel = None;
        let cw = 340.0f32.min(list[2] - 24.0);
        let mut y = list[1] + list[3] - 12.0;
        let shown: Vec<usize> = (0..self.jobs.len()).filter(|&i| self.jobs[i].started.elapsed().as_millis() > 250).collect();
        for i in shown.into_iter().rev() {
            let job = &mut self.jobs[i];
            let p = job.progress();
            let ch = 66.0;
            y -= ch;
            let r = [list[0] + list[2] - cw - 12.0 - widgets::BAR_ZONE, y, cw, ch];
            y -= 8.0;
            let paper = if dark { [0.16, 0.165, 0.18] } else { [1.0; 3] };
            ui.fill([r[0] - 1.0, r[1] - 1.0, r[2] + 2.0, r[3] + 2.0], 13.0, [0.0; 3], 0.14);
            ui.fill(r, 12.0, paper, 0.97);
            let dir = job.dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| "/".into());
            let verb = if job.op == crate::jobs::Op::Move { "Moving {} item(s) to {}" } else { "Copying {} item(s) to {}" };
            let title = chebrium::i18n::trf(verb, &[job.items.to_string(), dir]);
            let title = fit(ui, &title, 12.5, true, r[2] - 70.0);
            ui.glyph(r[0] + 22.0, r[1] + 22.0, 16.0, if job.op == crate::jobs::Op::Move { "move" } else { "copy" }, accent, 1.0);
            ui.text_raw(r[0] + 40.0, r[1] + 26.0, &title, 12.5, true, ink, 0.92, Align::Left);
            let frac = if p.total > 0 { (p.done as f32 / p.total as f32).clamp(0.0, 1.0) } else { 0.0 };
            let bar = [r[0] + 16.0, r[1] + 38.0, r[2] - 32.0, 5.0];
            ui.fill(bar, 2.5, ink, 0.12);
            let shown_frac = ui.ease(44_000 + i as u64, frac, 10.0);
            ui.fill([bar[0], bar[1], (bar[2] * shown_frac).max(5.0), bar[3]], 2.5, accent, 1.0);
            let detail = if p.counting {
                chebrium::i18n::tr("Counting…").into_owned()
            } else {
                let mut s = format!("{} / {}", fsops::human_size(p.done), fsops::human_size(p.total));
                if job.items > 1 && !p.current.is_empty() {
                    s = format!("{}  ·  {s}", p.current);
                }
                if p.speed > 1.0 {
                    let left = (p.total.saturating_sub(p.done)) as f64 / p.speed;
                    let left = if left < 60.0 { format!("{} s", left.ceil() as u64) } else { format!("{} min", (left / 60.0).ceil() as u64) };
                    s.push_str(&format!("  ·  {}/s  ·  {}", fsops::human_size(p.speed as u64), chebrium::i18n::trf("{} left", &[left])));
                }
                s
            };
            let detail = fit(ui, &detail, 11.0, false, r[2] - 32.0);
            ui.text_raw(r[0] + 16.0, r[1] + 58.0, &detail, 11.0, false, ink, 0.55, Align::Left);
            if round_button(ui, r[0] + r[2] - 22.0, r[1] + 21.0, "x", Round::ink(24.0).glyph(13.0)) {
                cancel = Some(i);
            }
        }
        if let Some(i) = cancel {
            self.jobs[i].cancel();
        }
    }

    /// A name is taken where files are going: skip it, keep both, or replace (the old one goes to the Trash).
    fn draw_ask(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        let ink = ui.ink;
        let r = self.card(ui, size, 520.0, 200.0, 45_001);
        let Some(a) = self.ask.as_ref() else { return };
        let (name, dir, is_dir) = (a.name.clone(), a.dir.clone(), a.is_dir);
        ui.glyph(r[0] + 34.0, r[1] + 38.0, 26.0, if is_dir { "folder" } else { "file" }, ui.accent, 1.0);
        let title = fit(ui, &chebrium::i18n::trf("“{}” is already there", &[name]), 14.5, true, r[2] - 80.0);
        ui.text_raw(r[0] + 62.0, r[1] + 36.0, &title, 14.5, true, ink, 0.95, Align::Left);
        let sub = fit(ui, &chebrium::i18n::trf("in “{}”. What should happen to the one coming in?", &[place_title(&dir)]), 12.0, false, r[2] - 80.0);
        ui.text_raw(r[0] + 62.0, r[1] + 56.0, &sub, 12.0, false, ink, 0.6, Align::Left);
        let note = if is_dir { "Replace puts the folder that is there in the Trash." } else { "Replace puts the file that is there in the Trash." };
        ui.text(r[0] + 62.0, r[1] + 76.0, note, 11.5, false, ink, 0.45, Align::Left);
        let mut all = self.ask_all;
        ui.toggle(r[0] + 62.0, r[1] + 98.0, &mut all);
        self.ask_all = all;
        ui.text(r[0] + 116.0, r[1] + 114.0, "Do this for all the rest", 12.5, false, ink, 0.8, Align::Left);
        let by = r[1] + r[3] - 52.0;
        let mut answer = None;
        let bx = r[0] + r[2] - 16.0;
        if ui.button([bx - 100.0, by, 100.0, 34.0], "Replace", true) {
            answer = Some(crate::jobs::Answer::Replace);
        }
        if ui.button([bx - 210.0, by, 102.0, 34.0], "Keep Both", false) {
            answer = Some(crate::jobs::Answer::KeepBoth);
        }
        if ui.button([bx - 300.0, by, 82.0, 34.0], "Skip", false) {
            answer = Some(crate::jobs::Answer::Skip);
        }
        if let Some(ans) = answer {
            if let Some(a) = self.ask.take() {
                a.answer(ans, self.ask_all);
            }
            host.redraw = true;
        }
    }

    /// Open With: the programs for the file's type (or all of them), the one picked, and "always use it".
    fn draw_picker(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        let (ink, accent) = (ui.ink, ui.accent);
        let r = self.card(ui, size, 460.0, 540.0, 45_002);
        let Some(p) = self.picker.as_ref() else { return };
        let first = p.files.first().and_then(|f| f.file_name()).map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let title = fit(ui, &chebrium::i18n::trf("Open “{}” with", &[first]), 15.0, true, r[2] - 40.0);
        ui.text_raw(r[0] + 20.0, r[1] + 32.0, &title, 15.0, true, ink, 0.95, Align::Left);
        let kind = memrium_de::mimeapps::describe(&p.mime);
        let kind = fit(ui, &kind, 12.0, false, r[2] - 40.0);
        ui.text_raw(r[0] + 20.0, r[1] + 52.0, &kind, 12.0, false, ink, 0.55, Align::Left);
        let list = [r[0] + 10.0, r[1] + 66.0, r[2] - 20.0, r[3] - 66.0 - 104.0];
        ui.fill([list[0], list[1] - 1.0, list[2], 1.0], 0.0, ink, 0.08);
        let n = p.list.len();
        let row_h = 42.0;
        let mut scroll = p.scroll;
        let area = widgets::scroll_area(ui, list, n as f32 * row_h + 8.0, &mut scroll);
        let mut chosen = None;
        let mut open = false;
        let items: Vec<(String, String, bool)> = self.picker.as_ref().unwrap().list.iter().map(|h| (h.name.clone(), h.icon.clone(), h.hidden)).collect();
        let picked = self.picker.as_ref().unwrap().chosen;
        for i in area.rows(row_h, n) {
            let rr = [list[0] + 4.0, area.y(4.0 + i as f32 * row_h), list[2] - 8.0 - widgets::BAR_ZONE, row_h - 2.0];
            let hov = area.hit(ui, rr);
            widgets::list_row(ui, rr, 9.0, 46_000 + i as u64, hov, i == picked);
            let (name, icon, _) = &items[i];
            match self.icons.app(icon) {
                Some(ic) => ui.picture([rr[0] + 10.0, rr[1] + 7.0, 26.0, 26.0], &ic.pic, [0.0, 0.0, ic.pic.w as f32, ic.pic.h as f32], 1.0),
                None => ui.glyph(rr[0] + 23.0, rr[1] + rr[3] / 2.0, 18.0, "app-window", accent, 1.0),
            }
            let label = fit(ui, name, 13.5, false, rr[2] - 60.0);
            ui.text_raw(rr[0] + 46.0, rr[1] + rr[3] / 2.0 + 4.8, &label, 13.5, i == picked, ink, 0.95, Align::Left);
            if hov && ui.click {
                chosen = Some(i);
                open = ui.double;
            }
        }
        area.end(ui);
        if n == 0 {
            ui.text(list[0] + list[2] / 2.0, list[1] + 60.0, "No program says it opens this", 13.0, false, ink, 0.5, Align::Center);
        }
        let p = self.picker.as_mut().unwrap();
        p.scroll = scroll;
        if let Some(i) = chosen {
            p.chosen = i;
        }
        let ty = r[1] + r[3] - 98.0;
        let mut always = p.always;
        ui.toggle(r[0] + 20.0, ty, &mut always);
        p.always = always;
        ui.text(r[0] + 74.0, ty + 16.0, "Always use it for this type", 12.5, false, ink, 0.8, Align::Left);
        let mut show_all = false;
        if !p.all {
            let lr = [r[0] + r[2] - 160.0, ty + 2.0, 140.0, 20.0];
            let hov = ui.hit(lr);
            ui.text(lr[0] + lr[2], ty + 16.0, "Show all programs", 12.5, hov, accent, 1.0, Align::Right);
            show_all = hov && ui.click;
        }
        let by = r[1] + r[3] - 52.0;
        let bx = r[0] + r[2] - 16.0;
        open |= ui.button([bx - 100.0, by, 100.0, 34.0], "Open", true) && n > 0;
        let cancel = ui.button([bx - 200.0, by, 92.0, 34.0], "Cancel", false);
        if show_all {
            let p = self.picker.as_mut().unwrap();
            let have: Vec<String> = p.list.iter().map(|h| h.id.clone()).collect();
            p.list.extend(crate::actions::all_programs().into_iter().filter(|h| !have.contains(&h.id)));
            p.all = true;
        }
        if cancel {
            self.picker = None;
        } else if open {
            self.picker_open();
        }
        host.redraw |= chosen.is_some() || show_all || cancel || open;
    }

    fn draw_quick(&mut self, ui: &mut Ui<'_>, size: [f32; 2], host: &mut Host) {
        let Some(e) = self.quick_entry() else {
            self.quick = false;
            return;
        };
        let preview = self.previews.get(&e);
        let icon = self.icons.get(&e, true).map(|i| i.pic.clone());
        let mut scroll = self.quick_scroll;
        let act = crate::quicklook::draw(ui, size, &e, preview.as_ref(), icon.as_ref(), &mut scroll);
        self.quick_scroll = scroll;
        let act_done = !matches!(act, crate::quicklook::Action::None);
        match act {
            crate::quicklook::Action::Close => self.quick = false,
            crate::quicklook::Action::Open => {
                self.quick = false;
                if let Some(i) = self.cursor {
                    self.open(i);
                }
            }
            crate::quicklook::Action::None => {}
        }
        host.redraw |= act_done;
    }
}
